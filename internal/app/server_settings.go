package app

import (
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
)

func containsModel(options []string, model string) bool {
	for _, option := range options {
		if option == model {
			return true
		}
	}
	return false
}

// applyModelSelection only changes the launch configuration while stopped.
func (a *App) applyModelSelection() {
	a.Mu.Lock()
	if a.EngineState.IsReady() || a.EngineState.IsBusy() || a.Streaming {
		a.Mu.Unlock()
		return
	}
	if a.ModelSel == autoModelLabel {
		a.settings.Server.ModelPath = nil
	} else {
		path := a.ModelSel
		if !fileExists(path) || !strings.EqualFold(filepath.Ext(path), ".gguf") {
			a.Notice = "존재하는 GGUF 모델 파일을 선택하세요."
			a.Mu.Unlock()
			return
		}
		a.settings.Server.ModelPath = &path
	}
	a.Notice = "모델 선택을 저장했습니다. 엔진을 시작하면 적용됩니다."
	a.Mu.Unlock()
	a.SaveSettings()
}

// engineSettingsView edits a draft so invalid input cannot affect a running engine.
func (a *App) engineSettingsView(c *ui.Context) {
	theme := c.Theme()
	state := a.engineStateView()
	ui.Column(c).Padding(14).Gap(10).Radius(8).Border(1, theme.Border).Background(theme.Surface).Children(func() {
		ui.Text(c, "엔진 실행 설정").Bold()
		ui.Text(c, "엔진을 중지한 뒤 적용하세요. 다음 엔진 시작부터 사용됩니다.").FontSize(11).TextColor(theme.TextMuted)
		ui.Text(c, "llama-server 실행 파일의 절대 경로 · 비우면 자동 탐색").FontSize(11)
		ui.TextInput(c, &a.ServerForm.RuntimePath).Label("런타임 경로").FillWidth()
		for _, field := range []struct {
			label string
			value *string
		}{
			{"포트 (1024~65535)", &a.ServerForm.Port},
			{"컨텍스트 크기 (512~32768)", &a.ServerForm.CtxSize},
			{"스레드 (1~64)", &a.ServerForm.Threads},
			{"GPU 레이어 (-1 자동 / 0 CPU / 최대 256)", &a.ServerForm.GPULayers},
		} {
			ui.Row(c.Key(field.label)).Gap(12).AlignItems(ui.Center).Children(func() {
				ui.Text(c, field.label).FontSize(12).Grow(1)
				ui.TextInput(c, field.value).Label(field.label).Width(120)
			})
		}
		ui.Checkbox(c, &a.ServerForm.AutoStart, "앱 실행 시 엔진 자동 시작")
		if ui.Button(c, "엔진 설정 적용").Disabled(state.IsReady() || state.IsBusy() || a.isStreaming()).Clicked() {
			a.applyServerForm()
		}
	})
}

func (a *App) applyServerForm() {
	a.Mu.Lock()
	if a.EngineState.IsReady() || a.EngineState.IsBusy() || a.Streaming {
		a.Mu.Unlock()
		return
	}
	server, err := a.ServerForm.Apply(a.settings.Server)
	if err != nil {
		a.Notice = err.Error()
		a.Mu.Unlock()
		return
	}
	a.settings.Server = server
	a.Notice = "엔진 설정을 저장했습니다. 다음 엔진 시작부터 적용됩니다."
	a.Mu.Unlock()
	a.SaveSettings()
}
