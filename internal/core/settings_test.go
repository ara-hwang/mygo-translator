package core

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestSettingsDefaultsFollowModelCard(t *testing.T) {
	s := DefaultSettings()
	if s.SourceLang != "ko" || s.TargetLang != "en" {
		t.Fatalf("langs = %s/%s", s.SourceLang, s.TargetLang)
	}
	g := s.Generation
	if g.Temperature != 0.7 || g.TopP != 0.6 || g.TopK != 20 || g.RepeatPenalty != 1.05 || g.MaxTokens != 4096 {
		t.Fatalf("generation = %+v", g)
	}
	if s.Server.Port <= 1024 {
		t.Fatalf("port = %d", s.Server.Port)
	}
	if s.Appearance != AppearanceSystem {
		t.Fatalf("appearance = %v", s.Appearance)
	}
	if s.Desktop.KeepInTray || s.Desktop.StartWithWindows {
		t.Fatalf("desktop = %+v", s.Desktop)
	}
}

func TestSettingsRoundTripsThroughDisk(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "settings.json")
	s := DefaultSettings()
	s.TargetLang = "ja"
	s.Server.CtxSize = 8192
	s.Prompt.Glossary = "GPUI=>지피유아이"
	s.Appearance = AppearanceLight
	s.Desktop.KeepInTray = true
	s.Desktop.StartWithWindows = true
	if err := s.Save(path); err != nil {
		t.Fatal(err)
	}
	loaded, warning := LoadOrDefault(path)
	if warning != "" {
		t.Fatalf("warning = %q", warning)
	}
	if loaded.SourceLang != s.SourceLang || loaded.TargetLang != "ja" ||
		loaded.Server.CtxSize != 8192 || loaded.Prompt.Glossary != s.Prompt.Glossary ||
		loaded.Appearance != AppearanceLight || !loaded.Desktop.KeepInTray {
		t.Fatalf("round trip mismatch: %+v", loaded)
	}
}

func TestSettingsMissingAndCorruptFallBackToDefaults(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "settings.json")
	loaded, warning := LoadOrDefault(path)
	if warning != "" {
		t.Fatalf("missing file should be silent, got %q", warning)
	}
	if !reflect.DeepEqual(loaded, DefaultSettings()) {
		t.Fatalf("missing file should load defaults: %+v", loaded)
	}
	os.WriteFile(path, []byte("{not json"), 0o644)
	loaded, warning = LoadOrDefault(path)
	if warning == "" {
		t.Fatal("corrupt file must warn")
	}
	if !reflect.DeepEqual(loaded, DefaultSettings()) {
		t.Fatalf("corrupt file must load defaults: %+v", loaded)
	}
}

func TestSettingsSanitizeClampsOutOfRangeValues(t *testing.T) {
	s := DefaultSettings()
	s.Server.Port = 0
	s.Server.CtxSize = 1
	s.Server.Threads = 0
	s.Server.GPULayers = -99
	s.Generation.Temperature = nan()
	s.Generation.TopP = 42.0
	s.Generation.MaxTokens = 0
	s.SourceLang = "xx"
	s.TargetLang = "xx"

	fixed := s.Sanitized()
	if fixed.Server.Port != 1024 {
		t.Fatalf("port = %d", fixed.Server.Port)
	}
	if fixed.Server.CtxSize != 512 {
		t.Fatalf("ctx = %d", fixed.Server.CtxSize)
	}
	if fixed.Server.Threads != 1 {
		t.Fatalf("threads = %d", fixed.Server.Threads)
	}
	if fixed.Server.GPULayers != -1 {
		t.Fatalf("gpu layers = %d", fixed.Server.GPULayers)
	}
	if fixed.Generation.Temperature != 0.7 {
		t.Fatalf("temperature = %v", fixed.Generation.Temperature)
	}
	if fixed.Generation.TopP != 1.0 {
		t.Fatalf("top p = %v", fixed.Generation.TopP)
	}
	if fixed.Generation.MaxTokens != 16 {
		t.Fatalf("max tokens = %d", fixed.Generation.MaxTokens)
	}
	if fixed.SourceLang != "ko" || fixed.TargetLang != "en" {
		t.Fatalf("langs = %s/%s", fixed.SourceLang, fixed.TargetLang)
	}
	if fixed.DownloadBase != "https://huggingface.co" {
		t.Fatalf("base = %q", fixed.DownloadBase)
	}
}

func TestOlderSettingsFilesGetDefaults(t *testing.T) {
	path := filepath.Join(t.TempDir(), "settings.json")
	os.WriteFile(path, []byte(`{"source_lang":"ko","target_lang":"en"}`), 0o644)
	s, warning := LoadOrDefault(path)
	if warning != "" {
		t.Fatal(warning)
	}
	if s.Appearance != AppearanceSystem {
		t.Fatalf("appearance = %v", s.Appearance)
	}
	if s.Desktop != (DesktopSettings{}) {
		t.Fatalf("desktop = %+v", s.Desktop)
	}
}

func TestTextSizeZoomStepsAndClamps(t *testing.T) {
	if ZoomTextSize(14, ZoomIn) != 16 || ZoomTextSize(14, ZoomOut) != 12 {
		t.Fatal("zoom steps wrong")
	}
	if ZoomTextSize(30, ZoomReset) != TextSizeDefault {
		t.Fatal("reset wrong")
	}
	if ZoomTextSize(TextSizeMax, ZoomIn) != TextSizeMax {
		t.Fatal("max clamp wrong")
	}
	if ZoomTextSize(TextSizeMin, ZoomOut) != TextSizeMin {
		t.Fatal("min clamp wrong")
	}
	if ZoomTextSize(31, ZoomIn) != TextSizeMax {
		t.Fatal("31 in wrong")
	}
	if ZoomTextSize(0, ZoomIn) != TextSizeMin+TextSizeStep {
		t.Fatal("0 in wrong")
	}
	if ZoomTextSize(65000, ZoomOut) != TextSizeMax-TextSizeStep {
		t.Fatal("oversize out wrong")
	}
}

func TestTextSizeIsSanitizedAndPersisted(t *testing.T) {
	sized := func(size int) Settings { s := DefaultSettings(); s.TextSize = FlexInt(size); return s }
	if sized(2).Sanitized().TextSize != FlexInt(TextSizeMin) {
		t.Fatal("small size must clamp to min")
	}
	if sized(999).Sanitized().TextSize != FlexInt(TextSizeMax) {
		t.Fatal("big size must clamp to max")
	}

	path := filepath.Join(t.TempDir(), "settings.json")
	sized(20).Save(path)
	loaded, warning := LoadOrDefault(path)
	if warning != "" || loaded.TextSize != 20 {
		t.Fatalf("loaded %d warning %q", loaded.TextSize, warning)
	}
}

func TestMissingOrMalformedTextSizeFallsBackToDefault(t *testing.T) {
	parse := func(json string) FlexInt {
		path := filepath.Join(t.TempDir(), "settings.json")
		os.WriteFile(path, []byte(json), 0o644)
		s, warning := LoadOrDefault(path)
		if warning != "" {
			t.Fatal(warning)
		}
		return s.TextSize
	}
	if parse(`{"source_lang":"ko"}`) != FlexInt(TextSizeDefault) {
		t.Fatal("missing text size must default")
	}
	if parse(`{"text_size":"big"}`) != FlexInt(TextSizeDefault) {
		t.Fatal("string text size must default")
	}
	if parse(`{"text_size":{"source":18}}`) != FlexInt(TextSizeDefault) {
		t.Fatal("object text size must default")
	}
	if parse(`{"text_size":100000}`) != FlexInt(TextSizeMax) {
		t.Fatal("oversized text size must clamp")
	}
	if parse(`{"text_size":18}`) != 18 {
		t.Fatal("valid text size must persist")
	}
}

func TestSourceAndTargetCanNeverBeIdentical(t *testing.T) {
	s := DefaultSettings()
	s.TargetLang = "ko"
	fixed := s.Sanitized()
	if fixed.SourceLang == fixed.TargetLang {
		t.Fatal("identical langs must split")
	}
}

func TestHotkeyDefaultsAreDisabledWithStandardBindings(t *testing.T) {
	hotkey := DefaultSettings().Hotkey
	if hotkey.Enabled {
		t.Fatal("disabled by default")
	}
	chord := hotkey.Combo
	if chord == nil || !chord.Ctrl || !chord.Shift || chord.Alt || chord.Win || chord.Key != "c" || !chord.IsValid() {
		t.Fatalf("default chord = %+v", chord)
	}
	tap := hotkey.DoubleTap
	if tap == nil || !tap.Ctrl || tap.Shift || tap.Key != "c" {
		t.Fatalf("default double tap = %+v", tap)
	}
	if hotkey.Acquisition != AcquisitionAuto {
		t.Fatal("acquisition default")
	}
}

func TestHotkeySettingsPersistAndStayEnabled(t *testing.T) {
	s := DefaultSettings()
	s.Hotkey.Enabled = true
	s.Hotkey.Acquisition = AcquisitionClipboardOnly
	s.Hotkey.Combo = &HotkeyCombo{Ctrl: true, Alt: true, Key: "f4"}

	path := filepath.Join(t.TempDir(), "settings.json")
	s.Save(path)
	loaded, warning := LoadOrDefault(path)
	if warning != "" {
		t.Fatal(warning)
	}
	if loaded.Hotkey.Enabled != true || loaded.Hotkey.Acquisition != AcquisitionClipboardOnly {
		t.Fatalf("hotkey = %+v", loaded.Hotkey)
	}
	if loaded.Hotkey.Combo == nil || loaded.Hotkey.Combo.Key != "f4" || !loaded.Hotkey.Combo.Alt {
		t.Fatalf("combo = %+v", loaded.Hotkey.Combo)
	}
}

func TestHotkeySanitizeDropsUnusableBindings(t *testing.T) {
	s := DefaultSettings()
	s.Hotkey.Enabled = true
	s.Hotkey.Combo = &HotkeyCombo{Ctrl: true, Key: "ctrl", Shift: true} // "ctrl" is not a key
	s.Hotkey.DoubleTap = &HotkeyCombo{Key: "c"}                         // no modifier

	fixed := s.Sanitized()
	if fixed.Hotkey.Combo != nil || fixed.Hotkey.DoubleTap != nil {
		t.Fatalf("unusable bindings must drop, got %+v / %+v", fixed.Hotkey.Combo, fixed.Hotkey.DoubleTap)
	}
	if !fixed.Hotkey.Enabled {
		t.Fatal("master switch is independent of bindings")
	}
}

func TestHotkeyKeyNamesAreNormalized(t *testing.T) {
	if !IsSupportedKey("f12") || !IsSupportedKey("space") || !IsSupportedKey(",") || !IsSupportedKey("`") {
		t.Fatal("supported keys rejected")
	}
	if IsSupportedKey("ctrl") || IsSupportedKey("f25") || IsSupportedKey("") || IsSupportedKey("hello") {
		t.Fatal("unsupported keys accepted")
	}
	combo := HotkeyCombo{Ctrl: true, Key: NormalizeHotkeyKey("  C ")}
	if !combo.IsValid() {
		t.Fatal("normalized combo invalid")
	}
}

func nan() float64 {
	zero := 0.0
	return zero / zero
}
