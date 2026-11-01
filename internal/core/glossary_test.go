package core

import (
	"fmt"
	"strings"
	"testing"
)

func TestGlossaryParsesArrowAndTabSeparators(t *testing.T) {
	text := "API => 인터페이스\nZed\t제드\n\n# comment\n"
	got := ParseGlossary(text)
	want := [][2]string{{"API", "인터페이스"}, {"Zed", "제드"}}
	if len(got) != 2 || got[0] != want[0] || got[1] != want[1] {
		t.Fatalf("got %v, want %v", got, want)
	}
}

func TestGlossaryReportsMalformedLines(t *testing.T) {
	pairs, warnings := ParseGlossaryReport("no separator here\nen => 영어\n=>empty\n")
	if len(pairs) != 1 || pairs[0][0] != "en" || pairs[0][1] != "영어" {
		t.Fatalf("pairs = %v", pairs)
	}
	if len(warnings) != 2 {
		t.Fatalf("warnings = %v", warnings)
	}
	if !strings.Contains(warnings[0], "1번째") {
		t.Fatalf("warnings[0] = %q", warnings[0])
	}
}

func TestGlossaryLaterDuplicatesReplaceEarlierOnes(t *testing.T) {
	got := ParseGlossary("API => 에이피아이\nAPI => 인터페이스")
	if len(got) != 1 || got[0] != [2]string{"API", "인터페이스"} {
		t.Fatalf("got %v", got)
	}
}

func TestGlossaryCapsEntries(t *testing.T) {
	var lines []string
	for i := 0; i < GlossaryMaxEntries+5; i++ {
		lines = append(lines, fmt.Sprintf("term%d => 용어%d", i, i))
	}
	pairs, warnings := ParseGlossaryReport(strings.Join(lines, "\n"))
	if len(pairs) != GlossaryMaxEntries {
		t.Fatalf("got %d pairs", len(pairs))
	}
	if !strings.Contains(warnings[len(warnings)-1], "최대") {
		t.Fatalf("warnings = %v", warnings)
	}
}

func TestGlossaryFormatsAndReparsesIdentically(t *testing.T) {
	pairs := [][2]string{{"a", "b"}}
	if got := ParseGlossary(FormatGlossary(pairs)); len(got) != 1 || got[0] != pairs[0] {
		t.Fatalf("got %v", got)
	}
}
