package app

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestRecordedHotkey(t *testing.T) {
	tests := []struct {
		event ui.InputEvent
		want  string
		valid bool
	}{
		{ui.InputEvent{Key: ui.KeyC, Mods: ui.Ctrl | ui.Shift}, "Ctrl+Shift+C", true},
		{ui.InputEvent{Key: ui.KeyUnknown, PhysicalKey: ui.KeyA, Mods: ui.Alt}, "Alt+A", true},
		{ui.InputEvent{Key: ui.KeyF12, Mods: ui.Super}, "Win+F12", true},
		{ui.InputEvent{Key: ui.KeyMinus, Mods: ui.Ctrl}, "Ctrl+-", true},
		{ui.InputEvent{Key: ui.KeyA}, "", false},
		{ui.InputEvent{Key: ui.KeyUp, Mods: ui.Ctrl}, "", false},
	}
	for _, tt := range tests {
		combo, ok := recordedHotkey(tt.event)
		if ok != tt.valid {
			t.Errorf("recordedHotkey(%+v) valid = %v, want %v", tt.event, ok, tt.valid)
		} else if ok && comboToAccelerator(combo) != tt.want {
			t.Errorf("recordedHotkey(%+v) = %s, want %s", tt.event, comboToAccelerator(combo), tt.want)
		}
	}
}
