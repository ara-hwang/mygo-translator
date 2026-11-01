package core

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func testConfig(model string) LaunchConfig {
	return LaunchConfig{
		ServerPath: "/opt/llama/llama-server",
		ModelPath:  model,
		Port:       8788,
		CtxSize:    4096,
		Threads:    6,
		GPULayers:  0,
		Alias:      "hymt",
	}
}

func TestBuildsAConservativeCommandLine(t *testing.T) {
	args := testConfig("/models/hy.gguf").Args()
	want := []string{
		"-m", "/models/hy.gguf",
		"--host", "127.0.0.1",
		"--port", "8788",
		"-c", "4096",
		"-t", "6",
		"-ngl", "0",
		"-np", "1",
		"--jinja",
		"--alias", "hymt",
	}
	if len(args) != len(want) {
		t.Fatalf("args = %v", args)
	}
	for i := range args {
		if args[i] != want[i] {
			t.Fatalf("args[%d] = %s, want %s", i, args[i], want[i])
		}
	}
}

func TestFindsServerNextToExecutableOrInRuntime(t *testing.T) {
	dir := t.TempDir()
	if got := DiscoverServer(dir, ""); got != "" {
		t.Fatalf("got %q", got)
	}

	os.MkdirAll(filepath.Join(dir, "runtime"), 0o755)
	binary := filepath.Join(dir, "runtime", ServerBinary())
	os.WriteFile(binary, []byte("stub"), 0o755)
	if got := DiscoverServer(dir, ""); got != binary {
		t.Fatalf("got %q, want %q", got, binary)
	}

	os.WriteFile(filepath.Join(dir, ServerBinary()), []byte("stub"), 0o755)
	if got := DiscoverServer(dir, ""); got != filepath.Join(dir, ServerBinary()) {
		t.Fatalf("got %q", got)
	}

	configured := filepath.Join(dir, "elsewhere.exe")
	os.WriteFile(configured, []byte("stub"), 0o755)
	if got := DiscoverServer(dir, configured); got != configured {
		t.Fatalf("configured lookup failed: %q", got)
	}
}

func TestScansModelsAndIgnoresPartialDownloads(t *testing.T) {
	dir := t.TempDir()
	os.WriteFile(filepath.Join(dir, "b.gguf"), []byte("x"), 0o644)
	os.WriteFile(filepath.Join(dir, "a.gguf"), []byte("x"), 0o644)
	os.WriteFile(filepath.Join(dir, "c.gguf.part"), []byte("x"), 0o644)
	os.WriteFile(filepath.Join(dir, "readme.txt"), []byte("x"), 0o644)
	found := ScanGGUF(dir)
	if len(found) != 2 {
		t.Fatalf("found %d: %v", len(found), found)
	}
	if !strings.HasSuffix(found[0], "a.gguf") || !strings.HasSuffix(found[1], "b.gguf") {
		t.Fatalf("sorting wrong: %v", found)
	}
}

func TestPicksTheRecommendedQuantization(t *testing.T) {
	models := []string{
		"Hy-MT2-7B-Q4_K_M.gguf",
		"Hy-MT2-1.8B-Q8_0.gguf",
		"Hy-MT2-1.8B-Q4_K_M.gguf",
	}
	if got := PickModel(models); filepath.Base(got) != "Hy-MT2-1.8B-Q4_K_M.gguf" {
		t.Fatalf("picked %s", got)
	}
	if got := PickModel(nil); got != "" {
		t.Fatalf("empty list gave %s", got)
	}
}

func TestWaitsForHealthToFlipToReady(t *testing.T) {
	server := sequenceServer(
		statusStep(503, "loading"),
		statusStep(503, "loading"),
		jsonStep(`{"status":"ok"}`),
	)
	defer server.Close()
	endpoint := mustParse(t, server.URL)
	if err := WaitUntilReady(ctxOf(t), endpoint, 5*time.Second, 20*time.Millisecond); err != nil {
		t.Fatal(err)
	}
}

func TestWaitTimesOutWithAUsefulMessage(t *testing.T) {
	server := sequenceServer(statusStep(503, "still loading"))
	defer server.Close()
	endpoint := mustParse(t, server.URL)
	err := WaitUntilReady(ctxOf(t), endpoint, 200*time.Millisecond, 20*time.Millisecond)
	if err == nil || !strings.Contains(err.Error(), "준비되지 않았습니다") {
		t.Fatalf("err = %v", err)
	}
}

func TestSupervisorReportsAMissingBinary(t *testing.T) {
	dir := t.TempDir()
	log := filepath.Join(dir, "engine.log")
	supervisor := SpawnSupervisor(log)
	defer supervisor.Shutdown()
	supervisor.Start(LaunchConfig{
		ServerPath: filepath.Join(dir, "missing-server.exe"),
		ModelPath:  filepath.Join(dir, "model.gguf"),
		Port:       8788,
		CtxSize:    2048,
		Threads:    2,
		GPULayers:  0,
		Alias:      "hymt",
	})
	var failure *string
	deadline := time.After(5 * time.Second)
	for failure == nil {
		select {
		case <-deadline:
			t.Fatal("no failure event")
		default:
		}
		if event, ok := supervisor.RecvTimeout(100 * time.Millisecond); ok && event.State != nil {
			if event.State.Kind == EngineFailed {
				s := event.State.Detail
				failure = &s
			}
		}
	}
	if !strings.Contains(*failure, "llama-server") {
		t.Fatalf("failure = %s", *failure)
	}
}
