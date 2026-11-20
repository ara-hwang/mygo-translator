package app

import (
	"os"
	"path/filepath"
	"strings"

	"offline-translator/internal/core"
)

// StartEngine starts the managed server for the selected model.
func (a *App) StartEngine() {
	a.Mu.Lock()
	config, failure := a.resolveLaunch()
	if config != nil {
		a.EngineState = core.EngineState{Kind: core.EngineStarting}
	}
	a.Mu.Unlock()
	if config == nil {
		a.Mu.Lock()
		a.EngineState = core.EngineState{Kind: core.EngineFailed,
			Detail: failure}
		a.Mu.Unlock()
		a.Invalidate()
		return
	}
	a.supervisor.Start(*config)
}

// launchConfig resolves the model file and builds the launch config.
func (a *App) launchConfig() *core.LaunchConfig {
	config, _ := a.resolveLaunch()
	return config
}

// Failure details distinguish a missing model from a missing engine binary.
const (
	failNoModel  = "사용할 수 있는 모델 없음 — 모델 탭에서 모델을 내려받으세요"
	failNoServer = "실행 엔진(llama-server)을 찾을 수 없음 — 설정에서 엔진 경로를 지정하세요"
)

// resolveLaunch returns the launch config, or a failure detail.
func (a *App) resolveLaunch() (*core.LaunchConfig, string) {
	model := deref(a.settings.Server.ModelPath)
	if model != "" && !fileExists(model) {
		model = ""
	}
	if model == "" {
		model = core.PickModel(core.ScanGGUF(a.paths.ModelsDir()))
	}
	if model == "" {
		return nil, failNoModel
	}
	server := core.DiscoverServer(exeDir(), deref(a.settings.Server.RuntimePath))
	if server == "" {
		return nil, failNoServer
	}
	return &core.LaunchConfig{
		ServerPath: server,
		ModelPath:  model,
		Port:       a.settings.Server.Port,
		CtxSize:    a.settings.Server.CtxSize,
		Threads:    a.settings.Server.Threads,
		GPULayers:  a.settings.Server.GPULayers,
		Alias:      "hy-mt2",
	}, ""
}

func deref(s *string) string {
	if s == nil {
		return ""
	}
	return *s
}

func fileExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && !info.IsDir()
}

func exeDir() string {
	exe, err := os.Executable()
	if err != nil {
		return ""
	}
	return filepath.Dir(exe)
}

// StopEngine stops the managed server.
func (a *App) StopEngine() { a.supervisor.Stop() }

// engineEventSink publishes supervisor events into app state.
func (a *App) engineEventSink(event core.EngineEvent) {
	if event.State != nil {
		a.Mu.Lock()
		a.EngineState = *event.State
		a.Mu.Unlock()
	}
	if event.Log != "" {
		a.Mu.Lock()
		a.EngineLinesRaw = append(a.EngineLinesRaw, event.Log)
		if len(a.EngineLinesRaw) > 400 {
			a.EngineLinesRaw = a.EngineLinesRaw[len(a.EngineLinesRaw)-400:]
		}
		a.Mu.Unlock()
	}
	a.Invalidate()
}

// pumpEvents drains the supervisor channel.
func (a *App) pumpEvents() {
	for event := range a.supervisor.Events() {
		a.engineEventSink(event)
	}
}

// EngineLabel is the status bar engine text.
func (a *App) EngineLabel() string {
	a.Mu.Lock()
	defer a.Mu.Unlock()
	switch a.EngineState.Kind {
	case core.EngineStarting:
		return "모델 불러오는 중…"
	case core.EngineFailed:
		trimmed := strings.TrimSpace(a.EngineState.Detail)
		if trimmed == "" {
			return "오류"
		}
		runes := []rune(trimmed)
		if len(runes) > 60 {
			trimmed = string(runes[:60]) + "…"
		}
		return trimmed
	default:
		return a.EngineState.Label()
	}
}

// refreshLocalModels rescans the models folder.
func (a *App) refreshLocalModels() {
	files := core.ScanGGUF(a.paths.ModelsDir())
	a.Mu.Lock()
	a.ModelFiles = files
	a.Mu.Unlock()
}
