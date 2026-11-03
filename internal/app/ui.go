package app

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"offline-translator/internal/core"
)

// languageNames is the select option list.
func languageNames() []string {
	names := make([]string, len(core.Languages))
	for i, lang := range core.Languages {
		names[i] = lang.NameKo
	}
	return names
}

// hotkeyModsNames is the modifier-combination dropdown.
func hotkeyModsNames() []string {
	names := make([]string, len(hotkeyModsOptions))
	for i, mods := range hotkeyModsOptions {
		names[i] = mods.Label
	}
	return names
}

// hotkeyKeyNames is the key dropdown: every key the mygo backend reports
// for hotkey registration.
func hotkeyKeyNames() []string {
	names := make([]string, 0, 64)
	for c := byte('A'); c <= 'Z'; c++ {
		names = append(names, string(rune(c)))
	}
	for c := byte('0'); c <= '9'; c++ {
		names = append(names, string(rune(c)))
	}
	for i := 1; i <= 12; i++ {
		names = append(names, fmt.Sprintf("F%d", i))
	}
	return append(names, "Space", "Tab", "Enter", ",", "-", ".", "=", ";", "/", "`", "[", "\\", "]", "'")
}

// appearanceNames is the appearance select option list.
var appearanceNames = []string{"시스템", "라이트", "다크"}

// promptLanguageNames is the instruction-language select option list.
var promptLanguageNames = []string{"자동", "영어", "중국어"}

// view is the root of the whole window: the tabs, the tab's surface and
// one bottom bar with the engine state and the last notice.
func (a *App) view(c *ui.Context) {
	a.noticeLock()
	a.Mu.Lock()
	tab := a.Tab
	notice := a.Notice
	a.Mu.Unlock()

	ui.Column(c).Fill().Children(func() {
		ui.Tabs(c, &a.Tab, "번역", "모델", "설정")
		switch tab {
		case tabTranslate:
			a.translateView(c)
		case tabModels:
			a.modelsView(c)
		case tabSettings:
			a.settingsView(c)
		}
		a.statusBar(c, notice)
	})
}

// statusBar is the bottom bar: engine readiness and running tasks on the
// left, the last notice taking the rest. It has no fixed height, so a
// wrapping notice grows the bar instead of clipping.
func (a *App) statusBar(c *ui.Context, notice string) {
	ui.Row(c).Padding(6, 10).Gap(8).AlignItems(ui.Center).Children(func() {
		if a.engineReady() {
			ui.Text(c, "● 엔진 준비됨").FontSize(11)
		} else {
			ui.Text(c, "○ "+a.EngineLabel()).FontSize(11)
		}
		if a.isStreaming() {
			ui.Spinner(c)
			ui.Text(c, "번역 중").FontSize(11)
		}
		if a.isDownloading() {
			ui.Spinner(c)
			ui.Text(c, a.progress().Label()).FontSize(11)
		}
		if strings.TrimSpace(notice) != "" {
			ui.Box(c).Grow(1) // Spacer pushing the notice right.
			ui.Text(c, notice).FontSize(11)
		}
	})
}

func (a *App) engineReady() bool {
	a.Mu.Lock()
	defer a.Mu.Unlock()
	return a.EngineState.IsReady()
}

func (a *App) isStreaming() bool {
	a.Mu.Lock()
	defer a.Mu.Unlock()
	return a.Streaming
}

func (a *App) isDownloading() bool {
	a.Mu.Lock()
	defer a.Mu.Unlock()
	return a.DownloadID != ""
}

func (a *App) progress() core.DownloadProgress {
	a.Mu.Lock()
	defer a.Mu.Unlock()
	return a.Download
}

// translateView is the main working surface: the language pair and the
// text size on one bar, the source and translation editors side by side,
// and the action row.
func (a *App) translateView(c *ui.Context) {
	ui.Column(c).Fill().Padding(12).Gap(8).Children(func() {
		ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Select(c, &a.SourceSelName, languageNames()).Width(150).OnChange(a.syncLanguageSelects)
			if ui.Button(c, "→").Clicked() {
				a.SwapLanguages()
			}
			ui.Select(c, &a.TargetSelName, languageNames()).Width(190).OnChange(a.syncLanguageSelects)
			ui.Box(c).Grow(1) // Spacer pushing the text size controls right.
			ui.Text(c, "글자 크기").FontSize(11)
			if ui.Button(c, "A−").Clicked() {
				a.zoomText(core.ZoomOut)
			}
			ui.Text(c, fmt.Sprintf("%d px", a.textSize())).FontSize(12)
			if ui.Button(c, "A+").Clicked() {
				a.zoomText(core.ZoomIn)
			}
			if ui.Button(c, "초기화").Clicked() {
				a.zoomText(core.ZoomReset)
			}
		})
		// The two editors sit side by side: Grow(1) on both of their columns
		// splits the row evenly whatever the window width is. Rows center
		// their children vertically by default, so stretch the columns to
		// make both editors fill the height.
		ui.Row(c).Fill().Grow(1).Gap(8).AlignItems(ui.Stretch).Children(func() {
			ui.Column(c).Grow(1).Gap(4).Children(func() {
				ui.Text(c, "원문").Bold().FontSize(11)
				ui.TextArea(c, &a.SourceText).
					Placeholder("번역할 텍스트를 입력하세요…").
					FontSize(float32(a.textSize())).
					Grow(1)
			})
			ui.Column(c).Grow(1).Gap(4).Children(func() {
				ui.Text(c, "번역").Bold().FontSize(11)
				ui.TextArea(c, &a.OutputText).FontSize(float32(a.textSize())).Grow(1)
			})
		})
		// The primary action sits at the right end of the row: the spacer
		// takes all free width and pushes the stats and button there.
		ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Box(c).Grow(1)
			if stats := a.statsText(); stats != "" {
				ui.Text(c, stats).FontSize(11)
			}
			if a.isStreaming() {
				if ui.Button(c, "중지").Clicked() {
					a.CancelTranslate()
				}
			} else if ui.PrimaryButton(c, "번역").Clicked() {
				a.StartTranslate()
			}
		})
	})
}

// modelsView lists the catalog, local files and the engine controls.
func (a *App) modelsView(c *ui.Context) {
	ui.Column(c).Fill().Padding(12).Gap(6).Children(func() {
		ui.Text(c, core.ModelFamily+" GGUF 공식 카탈로그").Bold()
		for _, option := range core.Catalog {
			installed := a.installedOption(option)
			a.Mu.Lock()
			downloading := a.DownloadID == option.ID
			a.Mu.Unlock()
			ui.Row(c).Gap(10).AlignItems(ui.Center).Children(func() {
				ui.Column(c).Width(330).Children(func() {
					ui.Text(c, fmt.Sprintf("%s · %s", option.Family, option.File)).FontSize(11)
				})
				switch {
				case downloading:
					progress := a.progress()
					ui.Progress(c, progress.Fraction()).Width(180)
					ui.Text(c, progress.Label()).FontSize(10)
					if ui.Button(c, "취소").Clicked() {
						a.CancelDownload()
					}
				case installed:
					ui.Text(c, "설치됨 · "+option.SizeLabel()).FontSize(11)
				case a.isDownloading():
					ui.Text(c, "다른 파일 내려받는 중…").FontSize(11)
				default:
					ui.Text(c, option.SizeLabel()).FontSize(11)
					if ui.Button(c, "내려받기").Clicked() {
						a.StartDownload(option)
					}
				}
			})
		}

		state := a.engineStateView()
		ui.Text(c, "엔진 (llama.cpp server)").Bold()
		ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
			if state.IsReady() || state.IsBusy() {
				if ui.Button(c, "엔진 중지").Clicked() {
					a.StopEngine()
				}
			} else if ui.PrimaryButton(c, "엔진 시작").Clicked() {
				a.StartEngine()
			}
			if model := a.selectedModelShort(); model != "" {
				ui.Text(c, "모델: "+model).FontSize(11)
			}
		})
		if detail, ok := a.engineFailure(); ok {
			ui.Text(c, detail).FontSize(10)
		}
		if tail := a.engineLogTail(); tail != "" {
			log := tail
			ui.TextArea(c, &log).Height(120)
		}
	})
}

// settingsView is the preferences surface. Checkboxes bind settings fields
// directly; the 저장 button persists and re-applies them.
func (a *App) settingsView(c *ui.Context) {
	a.Mu.Lock()
	if a.AppearanceSel == "" {
		a.AppearanceSel = appearanceName(a.settings.Appearance)
	}
	keepInTray := &a.settings.Desktop.KeepInTray
	startWithWindows := &a.settings.Desktop.StartWithWindows
	hotkeyEnabled := &a.settings.Hotkey.Enabled
	delimiters := &a.settings.Prompt.PreserveDelimiters
	useGlossary := &a.settings.Prompt.UseGlossary
	glossary := &a.settings.Prompt.Glossary
	style := &a.settings.Prompt.Style
	promptLang := promptLanguageName(a.settings.Prompt.Language)
	promptLangSel := promptLang
	a.Mu.Unlock()

	ui.Column(c).Fill().Padding(12).Gap(8).Children(func() {
		ui.Text(c, "일반").Bold()
		ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Text(c, "테마").FontSize(12)
			ui.Select(c, &a.AppearanceSel, appearanceNames).Width(160)
		})
		ui.Checkbox(c, keepInTray, "창을 닫아도 트레이에 유지")
		ui.Checkbox(c, startWithWindows, "Windows 시작 시 자동 실행")

		ui.Text(c, "프롬프트").Bold()
		ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Text(c, "지시문 언어").FontSize(12)
			ui.Select(c, &promptLangSel, promptLanguageNames).Width(160)
		})
		ui.Checkbox(c, delimiters, "구분자(코드 등) 원형 보존 강제")
		ui.Checkbox(c, useGlossary, "용어집 사용")
		if *useGlossary {
			ui.Text(c, "용어집 (한 줄에 하나씩, `원문 => 번역`)").FontSize(11)
			ui.TextArea(c, glossary).Height(110)
			ui.Text(c, "스타일 지시문 (선택)").FontSize(11)
			ui.TextArea(c, style).Height(60)
		}

		ui.Text(c, "글로벌 단축키").Bold()
		ui.Checkbox(c, hotkeyEnabled, "선택한 텍스트를 자동 번역 (기본 Ctrl+Shift+C)")
		if *hotkeyEnabled {
			ui.Text(c, "다른 앱에서 텍스트를 선택하고 단축키를 누르면 이 창의 원문에 넣어 바로 번역합니다. 더블탭 트리거는 설정 호환성을 위해 값만 유지됩니다.").FontSize(10)
			ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Text(c, fmt.Sprintf("조합 (%s)", a.HotkeyCurrent)).FontSize(12)
				ui.Select(c, &a.HotkeyModsSel, hotkeyModsNames()).Width(170)
				ui.Select(c, &a.HotkeyKeySel, hotkeyKeyNames()).Width(90)
				if ui.Button(c, "적용").Clicked() {
					a.applyHotkeyCombo()
				}
			})
		}

		if ui.PrimaryButton(c, "설정 저장").Clicked() {
			a.applyAppearance(a.AppearanceSel)
			a.Mu.Lock()
			a.settings.Prompt.Language = promptLanguageOf(promptLangSel)
			a.Mu.Unlock()
			a.SaveSettings()
		}
	})
}

// promptLanguageName maps the stored value onto the select label.
func promptLanguageName(l core.PromptLanguage) string {
	switch l {
	case core.PromptEnglish:
		return "영어"
	case core.PromptChinese:
		return "중국어"
	default:
		return "자동"
	}
}

// promptLanguageOf maps the select label back onto the stored value.
func promptLanguageOf(name string) core.PromptLanguage {
	switch name {
	case "영어":
		return core.PromptEnglish
	case "중국어":
		return core.PromptChinese
	default:
		return core.PromptAuto
	}
}

// applyAppearance maps and stores the appearance select state.
func (a *App) applyAppearance(name string) {
	a.Mu.Lock()
	switch name {
	case "라이트":
		a.settings.Appearance = core.AppearanceLight
	case "다크":
		a.settings.Appearance = core.AppearanceDark
	default:
		a.settings.Appearance = core.AppearanceSystem
	}
	a.Mu.Unlock()
}

// installedOption checks whether a catalog file is already present.
func (a *App) installedOption(option core.ModelOption) bool {
	a.Mu.Lock()
	defer a.Mu.Unlock()
	for _, local := range a.ModelFiles {
		if filepath.Base(local) == option.FileName() {
			return true
		}
	}
	return false
}

// engineStateView returns a copy of the engine state.
func (a *App) engineStateView() core.EngineState {
	a.Mu.Lock()
	defer a.Mu.Unlock()
	return a.EngineState
}

// engineFailure renders the failure detail, if any.
func (a *App) engineFailure() (string, bool) {
	a.Mu.Lock()
	defer a.Mu.Unlock()
	if a.EngineState.Kind != core.EngineFailed || strings.TrimSpace(a.EngineState.Detail) == "" {
		return "", false
	}
	return a.EngineState.Detail, true
}

// engineLogTail renders the last engine log lines.
func (a *App) engineLogTail() string {
	a.Mu.Lock()
	lines := a.EngineLinesRaw
	a.Mu.Unlock()
	if len(lines) == 0 {
		return ""
	}
	start := len(lines) - 12
	if start < 0 {
		start = 0
	}
	return strings.Join(lines[start:], "\n")
}

// selectedModelShort is the file name of the model in use.
func (a *App) selectedModelShort() string {
	a.Mu.Lock()
	model := deref(a.settings.Server.ModelPath)
	files := a.ModelFiles
	a.Mu.Unlock()
	if model != "" {
		return filepath.Base(model)
	}
	if picked := core.PickModel(files); picked != "" {
		return filepath.Base(picked)
	}
	return ""
}

// statsText returns the last translation statistics line.
func (a *App) statsText() string {
	a.Mu.Lock()
	defer a.Mu.Unlock()
	return a.OutputStats
}
