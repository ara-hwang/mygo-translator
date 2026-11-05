package core

import (
	"encoding/json"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

// Server location and launch options.
type ServerSettings struct {
	// ModelPath is an absolute path to a .gguf model. Nil means "pick the
	// best file found in the models folder".
	ModelPath *string `json:"model_path,omitempty"`
	// RuntimePath is an explicit path to llama-server; nil = auto-discovery.
	RuntimePath *string `json:"runtime_path,omitempty"`
	// Port is the loopback port used by the managed server.
	Port int `json:"port"`
	// CtxSize is the context window. Translation prompts are short; 4k keeps
	// KV-cache memory tiny.
	CtxSize int `json:"ctx_size"`
	// Threads are worker threads for llama.cpp.
	Threads int `json:"threads"`
	// GPULayers: 0 = pure CPU, -1 = ask llama.cpp to choose.
	GPULayers int `json:"gpu_layers"`
	// AutoStart starts the server when the window opens.
	AutoStart bool `json:"auto_start"`
}

// DefaultServerSettings fills the conservative defaults.
func DefaultServerSettings() ServerSettings {
	return ServerSettings{
		Port:      8788,
		CtxSize:   4096,
		Threads:   DefaultThreads(),
		GPULayers: 0,
		AutoStart: true,
	}
}

// DefaultThreads returns the llama.cpp thread count that fits this machine.
func DefaultThreads() int {
	n := runtime.NumCPU()
	if n > 8 {
		n = 8
	}
	if n < 2 {
		n = 2
	}
	return n
}

// GenerationSettings holds sampling parameters. Defaults are Tencent's
// recommended values for Hy-MT2 1.8B/7B (never change them lightly).
type GenerationSettings struct {
	Temperature   float64 `json:"temperature"`
	TopP          float64 `json:"top_p"`
	TopK          int     `json:"top_k"`
	RepeatPenalty float64 `json:"repeat_penalty"`
	MaxTokens     int     `json:"max_tokens"`
}

func DefaultGenerationSettings() GenerationSettings {
	return GenerationSettings{
		Temperature:   0.7,
		TopP:          0.6,
		TopK:          20,
		RepeatPenalty: 1.05,
		MaxTokens:     4096,
	}
}

// DesktopSettings: tray icon and Windows sign-in startup.
type DesktopSettings struct {
	KeepInTray       bool `json:"keep_in_tray"`
	StartWithWindows bool `json:"start_with_windows"`
}

// AcquisitionMode picks how a hotkey trigger reads the selected text of the
// focused application.
type AcquisitionMode string

const (
	// AcquisitionAuto also uses the clipboard fallback.
	AcquisitionAuto AcquisitionMode = "auto"
	// AcquisitionClipboardOnly always synthesizes Ctrl+C and reads the
	// clipboard.
	AcquisitionClipboardOnly AcquisitionMode = "clipboard-only"
)

// HotkeyCombo is one hotkey binding: a modifier set plus a single logical
// key. Persisted as neutral text so the settings file stays portable; the
// app maps Key onto an accelerator string when registering.
type HotkeyCombo struct {
	Ctrl  bool   `json:"ctrl"`
	Shift bool   `json:"shift"`
	Alt   bool   `json:"alt"`
	Win   bool   `json:"win"`
	Key   string `json:"key"`
}

// namedHotkeyKeys is the set of individually named keys accepted for Key.
var namedHotkeyKeys = map[string]bool{"space": true, "tab": true, "enter": true}

// NormalizeHotkeyKey lowercases and trims a stored key name.
func NormalizeHotkeyKey(key string) string {
	return strings.ToLower(strings.TrimSpace(key))
}

// IsSupportedKey reports whether key is a persistable single-key name.
func IsSupportedKey(key string) bool {
	t := NormalizeHotkeyKey(key)
	if namedHotkeyKeys[t] {
		return true
	}
	if len(t) >= 2 && t[0] == 'f' && digitsOnly(t[1:]) {
		return atoiLE24(t[1:])
	}
	if len([]rune(key)) != 1 {
		return false
	}
	c := key[0]
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') ||
		c == '-' || c == '=' || c == ',' || c == '.' || c == '/' || c == '`' || c == ';' ||
		c == '\'' || c == '[' || c == '\\' || c == ']'
}

func digitsOnly(s string) bool {
	if s == "" {
		return false
	}
	for _, c := range s {
		if c < '0' || c > '9' {
			return false
		}
	}
	return true
}

func atoiLE24(s string) bool {
	if len(s) > 2 {
		return false
	}
	n := 0
	for _, c := range s {
		n = n*10 + int(c-'0')
	}
	return n >= 1 && n <= 24
}

// IsValid reports whether the binding can be registered: at least one
// modifier and a supported key.
func (c HotkeyCombo) IsValid() bool {
	return (c.Ctrl || c.Shift || c.Alt || c.Win) && IsSupportedKey(c.Key)
}

// HasModifier reports whether at least one modifier is required.
func (c HotkeyCombo) HasModifier() bool {
	return c.Ctrl || c.Shift || c.Alt || c.Win
}

// DefaultHotkeyCombo is the standard trigger chord: Ctrl+Shift+C.
func DefaultHotkeyCombo() HotkeyCombo {
	return HotkeyCombo{Ctrl: true, Shift: true, Key: "c"}
}

// HotkeySettings are global hotkeys that translate the selected text of
// another application. The master switch governs every binding; when
// disabled the app keeps the stored bindings.
type HotkeySettings struct {
	Enabled bool `json:"enabled"`
	// Combo is registered as a system-wide shortcut; nil = not configured.
	Combo *HotkeyCombo `json:"combo"`
	// DoubleTap is modifier+key pressed twice in quick succession, DeepL
	// style. Kept for settings-file compatibility; the Go app registers
	// chord triggers through GlobalShortcut only.
	DoubleTap *HotkeyCombo `json:"double_tap"`
	// Acquisition picks how selected text is captured when a trigger fires.
	Acquisition AcquisitionMode `json:"acquisition"`
}

func DefaultHotkeySettings() HotkeySettings {
	double := HotkeyCombo{Ctrl: true, Key: "c"}
	return HotkeySettings{
		Enabled:     false,
		Combo:       &HotkeyCombo{Ctrl: true, Shift: true, Key: "c"},
		DoubleTap:   &double,
		Acquisition: AcquisitionAuto,
	}
}

// Text size limits, in pixels.
const (
	TextSizeMin     = 10
	TextSizeMax     = 32
	TextSizeStep    = 2
	TextSizeDefault = 14
)

// TextZoom is the direction of a text size change.
type TextZoom int

const (
	ZoomIn TextZoom = iota
	ZoomOut
	ZoomReset
)

// ClampTextSize forces a text size into the supported range.
func ClampTextSize(size int) int {
	if size < TextSizeMin {
		return TextSizeMin
	}
	if size > TextSizeMax {
		return TextSizeMax
	}
	return size
}

// ZoomTextSize applies one zoom step to size, staying inside the range.
func ZoomTextSize(size int, zoom TextZoom) int {
	switch ClampTextSize(size); zoom {
	case ZoomIn:
		return ClampTextSize(ClampTextSize(size) + TextSizeStep)
	case ZoomOut:
		return ClampTextSize(ClampTextSize(size) - TextSizeStep)
	default:
		return TextSizeDefault
	}
}

// PromptSettings: how prompts are built.
type PromptSettings struct {
	// Language is auto (Chinese for Chinese targets) or a forced choice.
	Language PromptLanguage `json:"language"`
	// Style is an optional instruction; empty means "no style".
	Style string `json:"style"`
	// Glossary is raw text, one "source=>target" pair per line.
	Glossary string `json:"glossary"`
	// UseGlossary applies terminology intervention while translating.
	UseGlossary bool `json:"use_glossary"`
	// PreserveDelimiters uses the delimiter-preserving instruction.
	PreserveDelimiters bool `json:"preserve_delimiters"`
}

// Appearance is the UI color theme.
type Appearance string

const (
	AppearanceSystem Appearance = "system"
	AppearanceLight  Appearance = "light"
	AppearanceDark   Appearance = "dark"
)

// Settings is everything the application persists.
type Settings struct {
	History    HistorySettings `json:"history"`
	SourceLang string          `json:"source_lang"`
	TargetLang string          `json:"target_lang"`
	// AutoSource marks the source select as sitting on the detect-language
	// entry. The stored source language stays as the fallback used when the
	// text is undetectable. Files from before this field keep the manual
	// selection.
	AutoSource   bool               `json:"auto_source"`
	DownloadBase string             `json:"download_base"`
	Appearance   Appearance         `json:"appearance"`
	Server       ServerSettings     `json:"server"`
	Generation   GenerationSettings `json:"generation"`
	Prompt       PromptSettings     `json:"prompt"`
	Desktop      DesktopSettings    `json:"desktop"`
	Hotkey       HotkeySettings     `json:"hotkey"`
	// TextSize is the text size in pixels shared by the editors. Its JSON
	// form is lenient: malformed values become the default.
	TextSize FlexInt `json:"text_size"`
}

// FlexInt decodes any JSON value into a clamped text size, matching the
// Rust `lenient_text_size` helper: one malformed value must not throw away
// the whole settings file.
type FlexInt int

// UnmarshalJSON clamps a number; anything that is not a number falls back
// to the documented default.
func (f *FlexInt) UnmarshalJSON(data []byte) error {
	var n float64
	if err := json.Unmarshal(data, &n); err != nil {
		*f = FlexInt(TextSizeDefault)
		return nil
	}
	if math.IsNaN(n) || math.IsInf(n, 0) {
		*f = FlexInt(TextSizeDefault)
		return nil
	}
	if n > math.MaxInt32 {
		n = math.MaxInt32
	}
	if n < 0 {
		n = 0
	}
	*f = FlexInt(ClampTextSize(int(n)))
	return nil
}

// DefaultSettings are the settings of a fresh install.
func DefaultSettings() Settings {
	return Settings{
		SourceLang:   DefaultSource().Code,
		TargetLang:   DefaultTarget().Code,
		DownloadBase: "https://huggingface.co",
		Appearance:   AppearanceSystem,
		Server:       DefaultServerSettings(),
		Generation:   DefaultGenerationSettings(),
		Prompt:       PromptSettings{}, // zero value of Language is PromptAuto
		Desktop:      DesktopSettings{},
		Hotkey:       DefaultHotkeySettings(),
		History:      HistorySettings{RetentionDays: 7},
		TextSize:     FlexInt(TextSizeDefault),
	}
}

// Sanitized forces every value into its supported range. Called after
// loading and before saving, so the engine never receives nonsense and the
// UI never has to defend against it. Missing zero values of the numeric
// launch fields (which `json` cannot always tell apart from real zeros)
// fall back to their defaults, as serde(default) did.
func (s Settings) Sanitized() Settings {
	san := DefaultSettings()

	san.SourceLang = s.SourceLang
	if _, ok := ByCode(s.SourceLang); !ok {
		san.SourceLang = DefaultSource().Code
	}
	if _, ok := ByCode(s.TargetLang); !ok || s.TargetLang == s.SourceLang {
		if s.SourceLang == "en" {
			san.TargetLang = "ko"
		} else {
			san.TargetLang = DefaultTarget().Code
		}
	} else {
		san.TargetLang = s.TargetLang
	}

	// AutoSource is a plain user preference; a bool cannot be out of range.
	san.AutoSource = s.AutoSource

	base := strings.TrimSuffix(strings.TrimSpace(s.DownloadBase), "/")
	if !(strings.HasPrefix(base, "http://") || strings.HasPrefix(base, "https://")) || len(base) > 200 {
		san.DownloadBase = "https://huggingface.co"
	} else {
		san.DownloadBase = base
	}

	san.Appearance = s.Appearance
	switch san.Appearance {
	case AppearanceLight, AppearanceDark, AppearanceSystem:
	default:
		san.Appearance = AppearanceSystem
	}

	// Numbers: defaults arrived from the pre-filled load; sanitize clamps.
	server := s.Server
	server.Port = clampInt(server.Port, 1024, 65535)
	server.CtxSize = clampInt(server.CtxSize, 512, 32768)
	server.Threads = clampInt(server.Threads, 1, 64)
	server.GPULayers = clampInt(server.GPULayers, -1, 256)
	san.Server = server

	gen := s.Generation
	gen.Temperature = clampFinite(gen.Temperature, 0.0, 2.0, 0.7)
	gen.TopP = clampFinite(gen.TopP, 0.0, 1.0, 0.6)
	gen.TopK = clampInt(gen.TopK, -1, 200)
	gen.RepeatPenalty = clampFinite(gen.RepeatPenalty, 0.5, 2.0, 1.05)
	gen.MaxTokens = clampInt(gen.MaxTokens, 16, 8192)
	san.Generation = gen

	san.TextSize = FlexInt(ClampTextSize(int(s.TextSize)))

	// Hotkey bindings must stay registrable: normalize the key name and drop
	// malformed combinations instead of trying to register them later.
	hotkey := s.Hotkey
	hotkey.Combo = sanitizeHotkeyCombo(hotkey.Combo)
	hotkey.DoubleTap = sanitizeHotkeyCombo(hotkey.DoubleTap)
	san.Hotkey = hotkey

	style := []rune(s.Prompt.Style)
	if len(style) > 200 {
		san.Prompt.Style = string(style[:200])
	} else {
		san.Prompt.Style = s.Prompt.Style
	}
	san.Prompt.Glossary = s.Prompt.Glossary
	san.Prompt.UseGlossary = s.Prompt.UseGlossary
	san.Prompt.PreserveDelimiters = s.Prompt.PreserveDelimiters
	san.Prompt.Language = s.Prompt.Language
	san.Desktop = s.Desktop
	san.History = s.History.Sanitized()
	return san
}

func sanitizeHotkeyCombo(combo *HotkeyCombo) *HotkeyCombo {
	if combo == nil {
		return nil
	}
	out := *combo
	out.Key = NormalizeHotkeyKey(combo.Key)
	if !out.IsValid() {
		return nil
	}
	return &out
}

// LoadOrDefault reads settings from path, returning defaults when the file
// does not exist, and a warning message (Korean, for the UI) when a corrupt
// file left the defaults in place.
func LoadOrDefault(path string) (Settings, string) {
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return DefaultSettings(), ""
		}
		return DefaultSettings(), fmt.Sprintf("설정 파일을 열 수 없습니다: %v", err)
	}
	settings := DefaultSettings()
	if err := json.Unmarshal(data, &settings); err != nil {
		return DefaultSettings(), fmt.Sprintf("설정 파일을 읽을 수 없어 기본값으로 시작합니다: %v", err)
	}
	return settings.Sanitized(), ""
}

// Save writes settings atomically (temp file + rename) so a crash cannot
// truncate them.
func (s Settings) Save(path string) error {
	if parent := filepath.Dir(path); parent != "" {
		if err := os.MkdirAll(parent, 0o755); err != nil {
			return err
		}
	}
	text, err := json.MarshalIndent(s.Sanitized(), "", "  ")
	if err != nil {
		return err
	}
	text = append(text, '\n')
	tmp := strings.TrimSuffix(path, filepath.Ext(path)) + ".json.tmp"
	if err := os.WriteFile(tmp, text, 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}

func clampInt(v, min, max int) int { return clamp(v, min, max) }

func clamp(v, min, max int) int {
	if v < min {
		return min
	}
	if v > max {
		return max
	}
	return v
}

func clampFinite(v, min, max, fallback float64) float64 {
	if math.IsNaN(v) || math.IsInf(v, 0) {
		return fallback
	}
	if v < min {
		return min
	}
	if v > max {
		return max
	}
	return v
}
