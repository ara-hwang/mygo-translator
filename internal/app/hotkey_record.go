package app

import (
	"fmt"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"offline-translator/internal/core"
)

// startHotkeyRecording releases the existing binding so it can be recorded again.
func (a *App) startHotkeyRecording() {
	if a.hotkeyReq != "" {
		mygo.GlobalShortcut.Unregister(a.hotkeyReq)
		a.hotkeyReq = ""
	}
	a.HotkeyRecording = true
	a.Invalidate()
}

func (a *App) cancelHotkeyRecording() {
	a.HotkeyRecording = false
	a.applyHotkey()
	a.Invalidate()
}

// recordHotkeyInput captures one key with modifiers while the recorder has focus.
func (a *App) recordHotkeyInput(event ui.InputEvent) bool {
	if !a.HotkeyRecording || event.Kind != ui.InputKeyDown {
		return false
	}
	if event.Repeat {
		return true
	}
	if event.Key == ui.KeyEscape {
		a.cancelHotkeyRecording()
		return true
	}
	combo, ok := recordedHotkey(event)
	if !ok {
		a.Mu.Lock()
		a.Notice = "Ctrl·Shift·Alt·Win과 지원되는 키를 함께 눌러 주세요."
		a.Mu.Unlock()
		a.Invalidate()
		return true
	}

	accel := comboToAccelerator(combo)
	if err := mygo.GlobalShortcut.Register(accel, func() { go a.captureSelection() }); err != nil {
		a.cancelHotkeyRecording()
		a.Mu.Lock()
		a.Notice = fmt.Sprintf("단축키를 등록하지 못했습니다(%s): %v", accel, err)
		a.Mu.Unlock()
		a.Invalidate()
		return true
	}
	a.hotkeyReq = accel
	a.HotkeyRecording = false
	a.Mu.Lock()
	a.settings.Hotkey.Combo = &combo
	settings := a.settings
	a.Mu.Unlock()
	go func() {
		if err := settings.Save(a.paths.SettingsFile()); err != nil {
			a.Mu.Lock()
			a.Notice = fmt.Sprintf("설정을 저장하지 못했습니다: %v", err)
			a.Mu.Unlock()
			a.Invalidate()
		}
	}()
	a.Invalidate()
	return true
}

func recordedHotkey(event ui.InputEvent) (core.HotkeyCombo, bool) {
	key := event.Key
	if event.PhysicalKey != ui.KeyUnknown {
		key = event.PhysicalKey
	}
	var name string
	switch {
	case key >= ui.KeyA && key <= ui.KeyZ:
		name = string(rune('a' + key - ui.KeyA))
	case key >= ui.Key0 && key <= ui.Key9:
		name = string(rune('0' + key - ui.Key0))
	case key >= ui.KeyF1 && key <= ui.KeyF12:
		name = fmt.Sprintf("f%d", key-ui.KeyF1+1)
	default:
		name = map[ui.Key]string{
			ui.KeySpace: "space", ui.KeyTab: "tab", ui.KeyEnter: "enter",
			ui.KeyMinus: "-", ui.KeyEqual: "=", ui.KeyComma: ",", ui.KeyPeriod: ".",
			ui.KeySlash: "/", ui.KeySemicolon: ";", ui.KeyQuote: "'",
			ui.KeyBracketLeft: "[", ui.KeyBracketRight: "]", ui.KeyBackslash: "\\", ui.KeyBackquote: "`",
		}[key]
	}
	combo := core.HotkeyCombo{Key: name, Ctrl: event.Mods&ui.Ctrl != 0, Shift: event.Mods&ui.Shift != 0,
		Alt: event.Mods&ui.Alt != 0, Win: event.Mods&ui.Super != 0}
	return combo, combo.IsValid()
}
