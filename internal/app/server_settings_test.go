package app

import (
	"testing"

	"offline-translator/internal/core"
)

func TestEngineSettingsBlockedWhileActive(t *testing.T) {
	for _, kind := range []core.EngineStateKind{core.EngineStarting, core.EngineReady} {
		a := &App{settings: core.DefaultSettings(), EngineState: core.EngineState{Kind: kind}, ModelSel: "/missing.gguf"}
		a.ServerForm = core.NewServerForm(a.settings.Server)
		a.ServerForm.Port = "9999"
		a.applyServerForm()
		a.applyModelSelection()
		if a.settings.Server.Port == 9999 || a.settings.Server.ModelPath != nil {
			t.Fatal("active engine configuration changed")
		}
	}
}

func TestInvalidEngineSettingsLeaveConfigurationUnchanged(t *testing.T) {
	a := &App{settings: core.DefaultSettings(), ModelSel: "/missing.gguf"}
	a.ServerForm = core.NewServerForm(a.settings.Server)
	a.ServerForm.Port = "9"
	a.applyServerForm()
	if a.settings.Server.Port != 8788 || a.Notice == "" {
		t.Fatal("invalid port was not rejected")
	}
	a.Notice = ""
	a.applyModelSelection()
	if a.settings.Server.ModelPath != nil || a.Notice == "" {
		t.Fatal("missing model was not rejected")
	}
}
