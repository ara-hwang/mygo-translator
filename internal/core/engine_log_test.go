package core

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"testing"
	"unicode/utf8"
)

func TestEngineLogTailSummary(t *testing.T) {
	for _, tt := range []struct {
		name  string
		lines []string
		want  string
	}{
		{name: "empty"},
		{name: "one", lines: []string{"first"}, want: "first"},
		{name: "three", lines: []string{"first", "second", "third"}, want: "first / second / third"},
		{name: "last three", lines: []string{"omitted", "first", "second", "third"}, want: "first / second / third"},
		{name: "400 runes", lines: []string{strings.Repeat("한", 399) + "끝"}, want: strings.Repeat("한", 399) + "끝"},
		{name: "truncate runes", lines: []string{strings.Repeat("한", 399) + "끝밖"}, want: strings.Repeat("한", 399) + "끝"},
		{name: "truncate joined", lines: []string{strings.Repeat("가", 398), "나다"}, want: strings.Repeat("가", 398) + " /"},
	} {
		t.Run(tt.name, func(t *testing.T) {
			var tail engineLogTail
			for _, line := range tt.lines {
				tail.append(line)
			}
			if got := tail.summary(); got != tt.want || !utf8.ValidString(got) {
				t.Fatalf("summary = %q, want %q (valid UTF-8)", got, tt.want)
			}
		})
	}
}

func TestEngineLogTailRetentionAndReset(t *testing.T) {
	var tail engineLogTail
	tail.reset()
	var want []string
	for line := range 12 {
		text := fmt.Sprintf("line %d", line)
		tail.append(text)
		if line >= 4 {
			want = append(want, text)
		}
	}
	if !reflect.DeepEqual(tail.s, want) {
		t.Fatalf("tail = %q, want %q", tail.s, want)
	}
	tail.reset()
	if len(tail.s) != 0 || tail.summary() != "" {
		t.Fatalf("reset retained lines: %q", tail.s)
	}
	tail.append("restarted")
	if got := tail.summary(); got != "restarted" {
		t.Fatalf("summary after reset and append = %q", got)
	}
}

func TestEngineLogTailConcurrentAccess(t *testing.T) {
	var tail engineLogTail
	var workers sync.WaitGroup
	for worker := range 3 {
		workers.Go(func() {
			for range 100 {
				switch worker {
				case 0:
					tail.append("line")
				case 1:
					tail.reset()
				case 2:
					_ = tail.summary()
				}
			}
		})
	}
	workers.Wait()
	if len(tail.s) > 8 {
		t.Fatalf("tail length = %d, want at most 8", len(tail.s))
	}
}

func TestPumpEngineStreamRecordsBeforeEvents(t *testing.T) {
	var tail engineLogTail
	logPath := filepath.Join(t.TempDir(), "engine.log")
	// Inspect recording before the next read: each previous record must
	// already have reached disk, the tail, and the event channel.
	events := make(chan EngineEvent, 32)
	want := []string{"  leading", "progressdone", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "final"}
	chunks := []string{"\n \t\n  leading \t\r\npro", "gress\rdone\nthird\nfourth\nfifth\nsixth\nseventh\neighth\nninth\nfinal \t"}
	pipe := &engineLogTestReader{chunks: chunks}
	pipe.beforeRead = func(index int) {
		if index != 1 {
			return
		}
		if got := tail.summary(); got != want[0] {
			t.Errorf("tail before next read = %q, want %q", got, want[0])
		}
		data, err := os.ReadFile(logPath)
		if err != nil || string(data) != want[0]+"\n" {
			t.Errorf("log before next read = %q, %v", data, err)
		}
		if len(events) != 1 {
			t.Errorf("events before next read = %d, want 1", len(events))
		}
	}
	pumpEngineStream(pipe, logPath, events, &tail)
	close(events)
	var got []string
	for event := range events {
		if event.State != nil {
			t.Fatal("log pump emitted a state transition")
		}
		got = append(got, event.Log)
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("events = %q, want %q", got, want)
	}
	if !reflect.DeepEqual(tail.s, want[len(want)-8:]) {
		t.Fatalf("tail = %q, want last eight lines", tail.s)
	}
	if got := tail.summary(); got != "eighth / ninth / final" {
		t.Errorf("summary = %q", got)
	}
	data, err := os.ReadFile(logPath)
	if err != nil || string(data) != strings.Join(want, "\n")+"\n" {
		t.Errorf("log = %q, %v", data, err)
	}
}

func TestPumpEngineStreamFullEvents(t *testing.T) {
	var tail engineLogTail
	events := make(chan EngineEvent, 1)
	events <- EngineEvent{Log: "existing"}
	logPath := filepath.Join(t.TempDir(), "engine.log")
	pumpEngineStream(strings.NewReader("new\n"), logPath, events, &tail)
	if got := <-events; got.Log != "existing" {
		t.Fatalf("full channel overwritten: %+v", got)
	}
	if got := tail.summary(); got != "new" {
		t.Fatalf("tail = %q, want new", got)
	}
	data, err := os.ReadFile(logPath)
	if err != nil || string(data) != "new\n" {
		t.Fatalf("log = %q, %v", data, err)
	}
}

func TestPumpEngineStreamConcurrentTail(t *testing.T) {
	var tail engineLogTail
	logPath := filepath.Join(t.TempDir(), "engine.log")
	events := make(chan EngineEvent, 200)
	var workers sync.WaitGroup
	for worker := range 2 {
		workers.Go(func() {
			var input strings.Builder
			for line := range 50 {
				fmt.Fprintf(&input, "%d:%d\n", worker, line)
			}
			pumpEngineStream(strings.NewReader(input.String()), logPath, events, &tail)
		})
	}
	for range 100 {
		_ = tail.summary()
	}
	workers.Wait()
	if len(tail.s) != 8 {
		t.Fatalf("tail length = %d, want 8", len(tail.s))
	}
	if len(events) != 100 {
		t.Fatalf("event count = %d, want 100", len(events))
	}
}

type engineLogTestReader struct {
	chunks     []string
	index      int
	beforeRead func(int)
}

func (r *engineLogTestReader) Read(p []byte) (int, error) {
	if r.beforeRead != nil {
		r.beforeRead(r.index)
	}
	if r.index == len(r.chunks) {
		return 0, io.EOF
	}
	n := copy(p, r.chunks[r.index])
	r.index++
	if r.index == len(r.chunks) {
		return n, io.EOF
	}
	return n, nil
}
