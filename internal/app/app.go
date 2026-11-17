// Package app wires the mygo window, tray, engine supervision, downloads,
// hotkeys and capture together: the UI shows state and fires events, the
// logic lives in internal/core.
package app

import (
	"context"
	"fmt"
	"strings"
	"sync"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"offline-translator/internal/core"
	"offline-translator/internal/winapi"
)

// CopyTranslation copies the current output without changing its whitespace.
func (a *App) CopyTranslation() {
	if strings.TrimSpace(a.OutputText) == "" {
		a.Mu.Lock()
		a.Notice = "복사할 번역 결과가 없습니다."
		a.Mu.Unlock()
		return
	}
	err := winapi.CopyText(a.OutputText)
	a.Mu.Lock()
	defer a.Mu.Unlock()
	if err != nil {
		a.Notice = fmt.Sprintf("번역 결과를 복사하지 못했습니다: %v", err)
		return
	}
	a.Notice = "번역 결과를 복사했습니다."
}

// UI tab indexes.
const (
	tabTranslate = 0
	tabModels    = 1
	tabSettings  = 2
	tabHistory   = 3
)

// autoSourceLabel is the source select's detect-language entry.
const autoSourceLabel = "자동"

// App is the shared state the UI reads and mutates. Fields written from
// goroutines are guarded by Mu; the view runs on the main thread.
type App struct {
	paths               core.AppPaths
	settings            core.Settings
	history             []core.HistoryEntry
	historyFailed       bool
	HistoryRetentionSel string
	ServerForm          core.ServerForm
	ModelSel            string

	Mu                sync.Mutex
	Tab               int
	SourceText        string
	OutputText        string
	OutputStats       string
	Streaming         bool
	EngineState       core.EngineState
	EngineLinesRaw    []string
	ModelFiles        []string
	DownloadID        string
	Download          core.DownloadProgress
	Notice            string
	SourceSelName     string
	TargetSelName     string
	AppearanceSel     string
	PromptLanguageSel string
	HotkeyRecording   bool

	winMu      sync.Mutex
	win        *mygo.Window
	invalidate func()

	supervisor    *core.Supervisor
	hotkeyReq     string
	appearanceIdx int
	cancelMu      sync.Mutex
	translateC    context.CancelFunc
	downloadC     context.CancelFunc
}

// New creates the application state and loads settings.
func New() *App {
	a := &App{}
	a.paths = core.DetectPaths()
	_ = a.paths.Ensure()
	a.settings, a.Notice = core.LoadOrDefault(a.paths.SettingsFile())
	var historyErr error
	a.history, historyErr = core.LoadHistory(a.historyPath())
	if historyErr != nil {
		a.historyFailed = true
		a.Notice = fmt.Sprintf("히스토리를 읽지 못했습니다: %v", historyErr)
	} else {
		a.pruneHistoryLocked()
	}
	a.HistoryRetentionSel = historyRetentionName(a.settings.History.RetentionDays)
	// AutoSource keeps the source select on 자동 across restarts; its stored
	// concrete language stays as the detect-failure fallback.
	if a.settings.AutoSource {
		a.SourceSelName = autoSourceLabel
	} else {
		a.SourceSelName = languageName(a.settings.SourceLang)
	}
	a.TargetSelName = languageName(a.settings.TargetLang)
	a.AppearanceSel = appearanceName(a.settings.Appearance)
	a.PromptLanguageSel = promptLanguageName(a.settings.Prompt.Language)
	a.ServerForm = core.NewServerForm(a.settings.Server)
	a.ModelSel = deref(a.settings.Server.ModelPath)
	if a.ModelSel == "" {
		a.ModelSel = autoModelLabel
	}
	return a
}

// Invalidate refreshes the window from any goroutine.
func (a *App) Invalidate() {
	a.winMu.Lock()
	invalidate := a.invalidate
	a.winMu.Unlock()
	if invalidate != nil {
		invalidate()
	}
}

// noticeLock is a compile-time assertion the notice lives behind Mu.
func (a *App) noticeLock() { a.Mu.Lock(); a.Mu.Unlock() }

// themeSource maps the stored appearance onto mygo's theme source.
func (a *App) themeSource() mygo.ThemeSource {
	switch a.settings.Appearance {
	case core.AppearanceLight:
		return mygo.ThemeLight
	case core.AppearanceDark:
		return mygo.ThemeDark
	default:
		return mygo.ThemeSystem
	}
}

// languageIndexOf resolves a code into the language table index.
func languageIndexOf(code string) int {
	for i, lang := range core.Languages {
		if lang.Code == code {
			return i
		}
	}
	return 1 // en
}

// languageName maps a settings code onto the select label used in the UI.
func languageName(code string) string {
	return core.Languages[languageIndexOf(code)].NameKo
}

// appearanceName maps the stored appearance onto the select label.
func appearanceName(a core.Appearance) string {
	switch a {
	case core.AppearanceLight:
		return "라이트"
	case core.AppearanceDark:
		return "다크"
	default:
		return "시스템"
	}
}

// syncLanguageSelects copies the select names back into the settings. The
// 자동 entry only flips AutoSource: the stored source language stays as the
// detect-failure fallback.
func (a *App) syncLanguageSelects() {
	a.Mu.Lock()
	target := languageCodeByName(a.TargetSelName)
	if a.SourceSelName == autoSourceLabel {
		a.settings.AutoSource = true
	} else if source := languageCodeByName(a.SourceSelName); source != "" {
		a.settings.AutoSource = false
		a.settings.SourceLang = source
	}
	if target != "" && target != a.settings.SourceLang {
		a.settings.TargetLang = target
	}
	settings := a.settings
	a.Mu.Unlock()
	go settings.Save(a.paths.SettingsFile())
}

// languageCodeByName resolves a select label back to a stable code.
func languageCodeByName(name string) string {
	for _, lang := range core.Languages {
		if lang.NameKo == name {
			return lang.Code
		}
	}
	return ""
}

// SaveSettings persists the current settings asynchronously and re-applies
// desktop behavior and hotkeys.
func (a *App) SaveSettings() {
	a.Mu.Lock()
	settings := a.settings
	a.Mu.Unlock()
	go func() {
		if err := settings.Save(a.paths.SettingsFile()); err != nil {
			a.Mu.Lock()
			a.settingsMsgLocked(fmt.Sprintf("설정을 저장하지 못했습니다: %v", err))
			a.Mu.Unlock()
			a.Invalidate()
		}
	}()
	a.applyDesktop()
	a.applyHotkey()
	a.Invalidate()
}

// settingsMsgLocked sets Notice; caller holds or re-locks Mu.
func (a *App) settingsMsgLocked(msg string) { a.Notice = msg }

func (a *App) applyDesktop() {
	if a.settings.Desktop.StartWithWindows != mygo.App.OpenAtLogin() {
		_ = mygo.App.SetOpenAtLogin(a.settings.Desktop.StartWithWindows)
	}
}

// applyHotkey (re)registers the configured global shortcut.
func (a *App) applyHotkey() {
	if a.hotkeyReq != "" {
		mygo.GlobalShortcut.Unregister(a.hotkeyReq)
		a.hotkeyReq = ""
	}
	if !a.settings.Hotkey.Enabled {
		return
	}
	combo := core.DefaultHotkeyCombo()
	if a.settings.Hotkey.Combo != nil && a.settings.Hotkey.Combo.IsValid() {
		combo = *a.settings.Hotkey.Combo
	} else if a.settings.Hotkey.Combo != nil {
		a.Mu.Lock()
		a.Notice = "단축키 조합이 유효하지 않습니다. 설정 파일을 확인하세요."
		a.Mu.Unlock()
		return
	}
	accel := comboToAccelerator(combo)
	if err := mygo.GlobalShortcut.Register(accel, func() {
		go a.captureSelection()
	}); err != nil {
		a.Mu.Lock()
		a.Notice = fmt.Sprintf("단축키를 등록하지 못했습니다(%s): %v", accel, err)
		a.Mu.Unlock()
		a.Invalidate()
		return
	}
	a.hotkeyReq = accel
}

// comboToAccelerator renders a combo as a mygo accelerator string.
func comboToAccelerator(combo core.HotkeyCombo) string {
	var b strings.Builder
	if combo.Ctrl {
		b.WriteString("Ctrl+")
	}
	if combo.Alt {
		b.WriteString("Alt+")
	}
	if combo.Shift {
		b.WriteString("Shift+")
	}
	if combo.Win {
		b.WriteString("Win+")
	}
	key := core.NormalizeHotkeyKey(combo.Key)
	switch key {
	case "space":
		b.WriteString("Space")
	case "tab":
		b.WriteString("Tab")
	case "enter":
		b.WriteString("Enter")
	default:
		b.WriteString(strings.ToUpper(key))
	}
	return b.String()
}

// hotkeyCombo returns the configured chord, or the default when none is
// stored yet.
func (a *App) hotkeyCombo() core.HotkeyCombo {
	if combo := a.settings.Hotkey.Combo; combo != nil && combo.IsValid() {
		return *combo
	}
	return core.DefaultHotkeyCombo()
}

// SwapLanguages exchanges the source and target languages. With the source
// select on 자동 the direction is resolved from the source text first, since
// auto cannot move to the target side: the resolved pair becomes the newly
// stored manual selection.
func (a *App) SwapLanguages() {
	a.Mu.Lock()
	sourceCode := a.settings.SourceLang
	if a.SourceSelName == autoSourceLabel {
		if lang, ok := core.Detect(a.SourceText); ok {
			sourceCode = lang.Code
		}
		a.settings.AutoSource = false
	}
	a.settings.SourceLang, a.settings.TargetLang = a.settings.TargetLang, sourceCode
	a.SourceSelName = languageName(a.settings.SourceLang)
	a.TargetSelName = languageName(a.settings.TargetLang)
	a.Mu.Unlock()
	a.SaveSettings()
}

// captureSelection copies the selected text of the focused application into
// the source editor, shows the window and starts translating.
func (a *App) captureSelection() {
	text, err := winapi.CaptureSelected()
	if err != nil {
		a.Mu.Lock()
		a.Notice = err.Error()
		a.Mu.Unlock()
		a.Invalidate()
		return
	}
	a.Mu.Lock()
	a.SourceText = text
	a.OutputText = ""
	a.Tab = tabTranslate
	a.Mu.Unlock()
	if win := a.window(); win != nil {
		win.Show()
		win.Focus()
	}
	a.Invalidate()
	a.StartTranslate()
}

func (a *App) window() *mygo.Window {
	a.winMu.Lock()
	defer a.winMu.Unlock()
	return a.win
}

// Run is the app entry: single instance lock, theme, window and tray.
func Run() {
	a := New()

	if !mygo.App.RequestSingleInstanceLock() {
		return
	}
	mygo.App.SetName("Offline Translator")
	mygo.App.SetVersion(core.Version)
	// Register the embedded font once, before the first frame; a failure
	// keeps the system font instead. See ui.go for the family's use.
	_ = registerAppFont()
	mygo.Theme.SetSource(a.themeSource())
	if a.settings.Desktop.StartWithWindows {
		_ = mygo.App.SetOpenAtLogin(true)
	}
	// The desktop toggles the Run key; settings windows later re-apply it.
	_ = a.settings.Desktop.StartWithWindows

	quitting := false
	mygo.App.OnBeforeQuit(func(*mygo.QuitEvent) { quitting = true })
	mygo.App.OnSecondInstance(func(args []string, cwd string) {
		if win := a.window(); win != nil {
			win.Restore()
			win.Focus()
		}
	})
	mygo.App.OnWindowAllClosed(func() {
		// Keep running with the tray while the user asked to stay resident.
		if !a.settings.Desktop.KeepInTray {
			mygo.App.Quit()
		}
	})

	supervisor := core.SpawnSupervisor(a.paths.EngineLog())
	a.supervisor = supervisor
	go a.pumpEvents()
	a.refreshLocalModels()

	mygo.App.WhenReady(func() {
		hidden := startedAtLogin(a)
		win := mygo.NewWindow(mygo.WindowOptions{
			Title:     "Offline Translator",
			Page:      mygo.PageOptions{DevTools: mygo.DevToolsDisabled},
			Width:     920,
			MinWidth:  700,
			Height:    640,
			MinHeight: 480,
			StateKey:  "main",
			Content:   ui.View(a.view),
			Hidden:    hidden,
		})
		a.winMu.Lock()
		a.win = win
		a.invalidate = win.Invalidate
		a.winMu.Unlock()
		_ = win.SetIcon(windowIconPNG)
		win.OnClose(func(e *mygo.CloseEvent) {
			if !quitting && a.settings.Desktop.KeepInTray {
				e.PreventDefault()
				win.Hide()
			}
		})
		if _, err := mygo.NewTray(mygo.TrayOptions{
			Icon:    trayIconPNG,
			ToolTip: "Offline Translator",
			Menu: mygo.NewMenu([]*mygo.MenuItem{
				{Label: "열기", Click: func(*mygo.MenuItem, *mygo.Window) {
					win.Show()
					win.Focus()
				}},
				mygo.Separator(),
				{Role: mygo.RoleQuit},
			}),
		}); err != nil {
			// Without a tray the desktop app behaves like a normal one.
			mygo.App.OnWindowAllClosed(func() {})
		}
		a.applyHotkey()
		if a.settings.Server.AutoStart {
			a.StartEngine()
		}
	})

	if err := mygo.App.Run(); err != nil {
		fmt.Println(err)
	}
	a.StopEngine()
}

// startedAtLogin hides the first window when a login launch asked for it.
func startedAtLogin(a *App) bool {
	if !mygo.App.WasOpenedAtLogin() {
		return false
	}
	return true
}

// textSize is the shared editor text size.
func (a *App) textSize() int {
	a.Mu.Lock()
	defer a.Mu.Unlock()
	return int(a.settings.TextSize)
}

// zoomText applies one text zoom step.
func (a *App) zoomText(zoom core.TextZoom) {
	a.Mu.Lock()
	a.settings.TextSize = core.FlexInt(core.ZoomTextSize(int(a.settings.TextSize), zoom))
	a.Mu.Unlock()
	a.SaveSettings()
}
