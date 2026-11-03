package core

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
)

// ServerBinary is the executable name of llama.cpp's server, per platform.
func ServerBinary() string {
	if runtime.GOOS == "windows" {
		return "llama-server.exe"
	}
	return "llama-server"
}

// ReadyTimeout is the maximum time a model may take to load before the
// launch is considered failed.
const ReadyTimeout = 300 * time.Second

// TailLines is how many trailing log lines are quoted in failure messages.
const TailLines = 8

// LaunchConfig is everything needed to launch the server.
type LaunchConfig struct {
	// ServerPath is the path to llama-server(.exe).
	ServerPath string
	// ModelPath is the path to the .gguf model.
	ModelPath string
	// Port is the loopback port to bind.
	Port int
	// CtxSize is the context window.
	CtxSize int
	// Threads are worker threads.
	Threads int
	// GPULayers: 0 = CPU only, -1 = let llama.cpp decide.
	GPULayers int
	// Alias is the model alias reported by the OpenAI-compatible API.
	Alias string
}

// Args builds the llama.cpp command line.
//
// Deliberately conservative: only flags that have existed for many llama.cpp
// releases are used, so an older binary does not abort on an unknown
// argument. --jinja is required because Hy-MT2's chat template ships inside
// the GGUF.
func (c LaunchConfig) Args() []string {
	return []string{
		"-m", c.ModelPath,
		"--host", "127.0.0.1",
		"--port", strconv.Itoa(c.Port),
		"-c", strconv.Itoa(c.CtxSize),
		"-t", strconv.Itoa(c.Threads),
		"-ngl", strconv.Itoa(c.GPULayers),
		"-np", "1",
		"--jinja",
		"--alias", c.Alias,
	}
}

// Endpoint is the endpoint the server will listen on.
func (c LaunchConfig) Endpoint() Endpoint {
	return LocalhostEndpoint(c.Port)
}

// EngineState is the published state of the managed server.
type EngineState struct {
	Kind   EngineStateKind
	Detail string // failure detail, when Kind == EngineFailed
}

// EngineStateKind discriminates the engine states.
type EngineStateKind int

const (
	EngineStopped EngineStateKind = iota
	EngineStarting
	EngineReady
	EngineFailed
)

// Label is the Korean label for the status bar.
func (s EngineState) Label() string {
	switch s.Kind {
	case EngineStopped:
		return "중지됨"
	case EngineStarting:
		return "모델 불러오는 중"
	case EngineReady:
		return "준비됨"
	default:
		return "오류"
	}
}

// IsReady is true when translations can be sent.
func (s EngineState) IsReady() bool { return s.Kind == EngineReady }

// IsBusy is true when a launch is in flight.
func (s EngineState) IsBusy() bool { return s.Kind == EngineStarting }

// EngineEvent is published by the supervisor.
type EngineEvent struct {
	State *EngineState // set on state transitions
	Log   string       // set on one line of llama-server output
}

type engineCommand struct {
	start    *LaunchConfig
	stop     bool
	shutdown bool
}

// Supervisor owns the llama-server child process and publishes events. The
// UI pumps events and never blocks.
type Supervisor struct {
	commands chan engineCommand
	events   chan EngineEvent
	once     sync.Once
}

// SpawnSupervisor creates the supervisor goroutine with a drainable event
// channel.
func SpawnSupervisor(logPath string) *Supervisor {
	s := &Supervisor{
		commands: make(chan engineCommand, 16),
		events:   make(chan EngineEvent, 512),
	}
	go runSupervisor(s.commands, s.events, logPath)
	return s
}

// Start asks the supervisor to (re)start the model server.
func (s *Supervisor) Start(config LaunchConfig) {
	s.commands <- engineCommand{start: &config}
}

// Stop asks the supervisor to stop the model server.
func (s *Supervisor) Stop() {
	s.commands <- engineCommand{stop: true}
}

// Shutdown stops the server and ends the supervisor goroutine.
func (s *Supervisor) Shutdown() {
	s.once.Do(func() { s.commands <- engineCommand{shutdown: true} })
}

// Events exposes the event channel for select-based pumps.
func (s *Supervisor) Events() <-chan EngineEvent { return s.events }

// TryRecv reads one event without blocking.
func (s *Supervisor) TryRecv() (EngineEvent, bool) {
	select {
	case event := <-s.events:
		return event, true
	default:
		return EngineEvent{}, false
	}
}

// RecvTimeout blocks for up to timeout, used by tests and the UI pump.
func (s *Supervisor) RecvTimeout(timeout time.Duration) (EngineEvent, bool) {
	select {
	case event := <-s.events:
		return event, true
	case <-time.After(timeout):
		return EngineEvent{}, false
	}
}

type childExit struct{ err error }

func runSupervisor(commands <-chan engineCommand, events chan<- EngineEvent, logPath string) {
	var cmd *exec.Cmd
	var exited chan childExit
	var pending *LaunchConfig
	var startedAt time.Time
	var tail struct {
		mu sync.Mutex
		s  []string
	}

	publish := func(event EngineEvent) {
		select {
		case events <- event:
		default:
			// The UI must keep draining; a dropped log line beats a deadlock.
		}
	}
	publishState := func(state EngineState) { publish(EngineEvent{State: &state}) }
	publishState(EngineState{Kind: EngineStopped})

	stopChild := func() {
		if cmd != nil {
			_ = cmd.Process.Kill()
			cmd = nil
			exited = nil
		}
		pending = nil
	}

	ticker := time.NewTicker(150 * time.Millisecond)
	defer ticker.Stop()
	probeCtx, probeCancel := context.WithCancel(context.Background())
	defer probeCancel()

	for {
		select {
		case command := <-commands:
			switch {
			case command.start != nil:
				stopChild()
				tail.mu.Lock()
				tail.s = tail.s[:0]
				tail.mu.Unlock()
				config := *command.start
				appendEngineLog(logPath, fmt.Sprintf(
					"=== starting %s (%d ctx, %d threads, ngl %d) ===",
					config.ModelPath, config.CtxSize, config.Threads, config.GPULayers))
				child, exitCh, err := spawnEngine(&config, logPath, events, &tail)
				if err != nil {
					publishState(EngineState{Kind: EngineFailed,
						Detail: fmt.Sprintf("llama-server를 실행할 수 없습니다: %v", err)})
					continue
				}
				cmd = child
				exited = exitCh
				pending = &config
				startedAt = time.Now()
				publishState(EngineState{Kind: EngineStarting})
			case command.stop:
				stopChild()
				publishState(EngineState{Kind: EngineStopped})
			case command.shutdown:
				stopChild()
				return
			}
		case exit := <-exited:
			// The child died on its own. Cancellation by our own Stop has
			// already reset cmd/exited, so this is always an unexpected death.
			detail := "비정상 종료"
			if exit.err != nil {
				detail = exit.err.Error()
			}
			cmd = nil
			exited = nil
			pending = nil
			message := "모델 서버가 종료되었습니다"
			if detail != "" {
				message += " (" + detail + ")"
			}
			message += ". " + tailSummary(&tail)
			appendEngineLog(logPath, "[fatal] "+message)
			publishState(EngineState{Kind: EngineFailed, Detail: message})
			continue
		case <-ticker.C:
		}

		// Still loading? Probe /health.
		if pending == nil || cmd == nil {
			continue
		}
		endpoint := pending.Endpoint()
		health, err := ProbeHealth(probeCtx, endpoint, 2*time.Second)
		switch {
		case err == nil && health.Ready:
			pending = nil
			appendEngineLog(logPath, "=== ready ===")
			publishState(EngineState{Kind: EngineReady})
		case err == nil && !health.Ready:
			if time.Since(startedAt) > ReadyTimeout {
				stopChild()
				detail := strings.TrimSpace(health.Detail)
				if detail == "" {
					detail = "응답 없음"
				}
				message := fmt.Sprintf("모델을 %d초 안에 불러오지 못했습니다. (%s)", int(ReadyTimeout.Seconds()), detail)
				appendEngineLog(logPath, "[fatal] "+message)
				publishState(EngineState{Kind: EngineFailed, Detail: message})
			}
		default:
			if time.Since(startedAt) > ReadyTimeout {
				stopChild()
				message := fmt.Sprintf("모델을 %d초 안에 불러오지 못했습니다: %v", int(ReadyTimeout.Seconds()), err)
				appendEngineLog(logPath, "[fatal] "+message)
				publishState(EngineState{Kind: EngineFailed, Detail: message})
			}
		}
	}
}

// spawnEngine starts llama-server and returns the command plus a channel
// that fires once when the process exits (the monitor goroutine is the only
// waiter, as the process package forbids a second Wait).
func spawnEngine(
	config *LaunchConfig,
	logPath string,
	events chan<- EngineEvent,
	tail *struct {
		mu sync.Mutex
		s  []string
	},
) (*exec.Cmd, chan childExit, error) {
	command := exec.Command(config.ServerPath, config.Args()...)
	command.Stdin = nil
	stdout, err := command.StdoutPipe()
	if err != nil {
		return nil, nil, err
	}
	stderr, err := command.StderrPipe()
	if err != nil {
		return nil, nil, err
	}
	if runtime.GOOS == "windows" {
		hideWindow(command)
	}
	if err := command.Start(); err != nil {
		return nil, nil, err
	}
	go pumpEngineStream(stdout, logPath, events, tail)
	go pumpEngineStream(stderr, logPath, events, tail)
	exited := make(chan childExit, 1)
	go func() { exited <- childExit{err: command.Wait()} }()
	return command, exited, nil
}

func pumpEngineStream(pipe interface {
	Read([]byte) (int, error)
}, logPath string, events chan<- EngineEvent, tail *struct {
	mu sync.Mutex
	s  []string
},
) {
	buf := make([]byte, 8192)
	var line strings.Builder
	record := func(raw string) {
		raw = strings.TrimRight(raw, " \t\n")
		if raw == "" {
			return
		}
		appendEngineLog(logPath, raw)
		tail.mu.Lock()
		tail.s = append(tail.s, raw)
		if len(tail.s) > TailLines {
			tail.s = tail.s[len(tail.s)-TailLines:]
		}
		tail.mu.Unlock()
		select {
		case events <- EngineEvent{Log: raw}:
		default:
		}
	}
	for {
		n, readErr := pipe.Read(buf)
		if n > 0 {
			chunk := string(buf[:n])
			for _, r := range chunk {
				if r == '\n' {
					record(line.String())
					line.Reset()
				} else if r != '\r' {
					line.WriteRune(r)
				}
			}
		}
		if readErr != nil {
			if line.Len() > 0 {
				record(line.String())
			}
			return
		}
	}
}

func tailSummary(tail *struct {
	mu sync.Mutex
	s  []string
}) string {
	tail.mu.Lock()
	defer tail.mu.Unlock()
	if len(tail.s) == 0 {
		return ""
	}
	joined := strings.Join(tail.s[max(0, len(tail.s)-3):], " / ")
	runes := []rune(joined)
	if len(runes) > 400 {
		joined = string(runes[:400])
	}
	return joined
}

func appendEngineLog(logPath, line string) {
	file, err := os.OpenFile(logPath, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644)
	if err != nil {
		return
	}
	defer file.Close()
	fmt.Fprintln(file, line)
}

// DiscoverServer looks for llama-server next to the executable, in
// runtime/, on PATH, or at a user-configured location.
func DiscoverServer(exeDir string, configured string) string {
	if configured != "" {
		if dirExists(configured) {
			candidate := filepath.Join(configured, ServerBinary())
			if fileExists(candidate) {
				return candidate
			}
		} else if fileExists(configured) {
			return configured
		}
	}
	candidates := []string{
		filepath.Join(exeDir, ServerBinary()),
		filepath.Join(exeDir, "runtime", ServerBinary()),
	}
	if parent := filepath.Dir(exeDir); parent != exeDir && parent != "." {
		candidates = append(candidates, filepath.Join(parent, "runtime", ServerBinary()))
	}
	for _, candidate := range candidates {
		if fileExists(candidate) {
			return candidate
		}
	}
	for _, dir := range filepath.SplitList(os.Getenv("PATH")) {
		if dir == "" {
			continue
		}
		if candidate := filepath.Join(dir, ServerBinary()); fileExists(candidate) {
			return candidate
		}
	}
	return ""
}

// ScanGGUF lists .gguf files in a folder, sorted by name.
func ScanGGUF(dir string) []string {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil
	}
	var models []string
	for _, entry := range entries {
		if entry.IsDir() || !IsModelFile(entry.Name()) {
			continue
		}
		models = append(models, filepath.Join(dir, entry.Name()))
	}
	sort.Strings(models)
	return models
}

// PickModel picks the most sensible default model from a folder listing.
//
// The 1.8B Q4_K_M file is the app's recommendation, so it wins; otherwise
// smaller families and lighter quantizations are preferred over 7B/30B
// files that may not fit in RAM.
func PickModel(models []string) string {
	best := ""
	bestScore := -1000
	for _, path := range models {
		score := ModelScore(filepath.Base(path))
		if score > bestScore {
			bestScore = score
			best = path
		}
	}
	return best
}

// ModelScore ranks a model file name.
func ModelScore(name string) int {
	name = strings.ToLower(name)
	score := 0
	if strings.Contains(name, "hy-mt2") {
		score += 20
	}
	if strings.Contains(name, "1.8b") {
		score += 40
	}
	switch {
	case strings.Contains(name, "q4_k_m"):
		score += 30
	case strings.Contains(name, "q4_0"):
		score += 20
	case strings.Contains(name, "q6_k"):
		score += 15
	case strings.Contains(name, "q8_0"):
		score += 10
	}
	if strings.Contains(name, "30b") {
		score -= 40
	}
	if strings.Contains(name, "7b") {
		score -= 10
	}
	if strings.Contains(name, "1.25bit") || strings.Contains(name, "2bit") {
		score -= 5
	}
	return score
}

// WaitUntilReady polls /health until the server reports ready.
func WaitUntilReady(ctx context.Context, endpoint Endpoint, timeout, poll time.Duration) error {
	started := time.Now()
	last := "모델 서버가 응답하지 않습니다."
	for {
		select {
		case <-ctx.Done():
			return errors.New("취소되었습니다.")
		case <-time.After(poll):
		}
		health, err := ProbeHealth(ctx, endpoint, 2*time.Second)
		switch {
		case err == nil && health.Ready:
			return nil
		case err == nil && health.Detail != "":
			last = health.Detail
		case err != nil && ctx.Err() != nil:
			return errors.New("취소되었습니다.")
		case err != nil:
			last = err.Error()
		}
		if time.Since(started) >= timeout {
			return fmt.Errorf("%s (%d초 동안 준비되지 않았습니다)", last, int(timeout.Seconds()))
		}
	}
}
