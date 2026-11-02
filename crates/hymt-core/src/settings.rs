//! Persisted user settings.
//!
//! Settings are stored as a single JSON file. Every field has a default and is clamped by
//! [`Settings::sanitized`], so a hand-edited or truncated file can never feed invalid values
//! (a port of `0`, zero context size, negative thread counts) into the engine.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::lang::{default_source, default_target};
use crate::prompt::PromptLanguage;

/// Where the llama.cpp server binary lives and how it is started.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ServerSettings {
    /// Absolute path to a `.gguf` model. `None` means "pick the best file found in the models
    /// folder".
    pub model_path: Option<std::path::PathBuf>,
    /// Explicit path to `llama-server.exe`. `None` means auto-discovery.
    pub runtime_path: Option<std::path::PathBuf>,
    /// Loopback port used by the managed server.
    pub port: u16,
    /// Context window. Translation prompts are short; 4k keeps KV-cache memory tiny.
    pub ctx_size: u32,
    /// Worker threads for llama.cpp.
    pub threads: u32,
    /// Layers offloaded to the GPU. `0` = pure CPU, `-1` = ask llama.cpp to choose.
    pub gpu_layers: i32,
    /// Start the server when the window opens.
    pub auto_start: bool,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            model_path: None,
            runtime_path: None,
            port: 8788,
            ctx_size: 4096,
            threads: default_threads(),
            gpu_layers: 0,
            auto_start: true,
        }
    }
}

/// Number of llama.cpp threads that fits the current machine.
pub fn default_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
        .clamp(2, 8)
}

/// Sampling parameters. Defaults are Tencent's recommended values for Hy-MT2 1.8B/7B.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GenerationSettings {
    /// Sampling temperature (model card: 0.7).
    pub temperature: f64,
    /// Nucleus sampling probability (model card: 0.6).
    pub top_p: f64,
    /// Top-k sampling cutoff (model card: 20; `-1` disables).
    pub top_k: i32,
    /// Repetition penalty (model card: 1.05).
    pub repeat_penalty: f64,
    /// Hard cap on generated tokens (model card: 4096).
    pub max_tokens: u32,
}

impl Default for GenerationSettings {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            top_p: 0.6,
            top_k: 20,
            repeat_penalty: 1.05,
            max_tokens: 4096,
        }
    }
}

/// Desktop behaviour: tray icon and Windows sign-in startup.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DesktopSettings {
    /// Keep the application alive in the notification area when the window closes.
    pub keep_in_tray: bool,
    /// Register the executable in the current user's Windows `Run` key.
    pub start_with_windows: bool,
}

/// How the global hotkey reads the selected text of the focused application.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AcquisitionMode {
    /// Read the UI Automation selection first and fall back to a synthetic
    /// `Ctrl+C` when the focused element has no text selection.
    #[default]
    Auto,
    /// Always synthesize `Ctrl+C` and read the clipboard.
    ClipboardOnly,
}

/// One hotkey binding: a modifier set plus a single logical key.
///
/// Persisted as neutral text so the settings file stays portable; the app maps
/// `key` onto a Windows virtual-key code when registering.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HotkeyCombo {
    /// `Ctrl` must be held.
    pub ctrl: bool,
    /// `Shift` must be held.
    pub shift: bool,
    /// `Alt` must be held.
    pub alt: bool,
    /// A Windows key must be held.
    pub win: bool,
    /// Single key name: `a`..`z`, `0`..`9`, `f1`..`f24`, `space`, `tab`, `enter`
    /// or the punctuation character printed on the key (`-`, `=`, `,`, `.`, `/`,
    /// `` ` ``, `;`, `'`, `[`, `\`, `]`).
    pub key: String,
}

impl Default for HotkeyCombo {
    fn default() -> Self {
        Self {
            ctrl: true,
            shift: true,
            alt: false,
            win: false,
            key: "c".to_string(),
        }
    }
}

/// The individual-named keys accepted for `HotkeyCombo::key`.
const NAMED_HOTKEY_KEYS: &[(&str, char)] = &[("space", ' '), ("tab", '\t'), ("enter", '\r')];

/// Lowercases and trims a stored key name.
pub fn normalize_hotkey_key(key: &str) -> String {
    key.trim().to_ascii_lowercase()
}

/// True when `key` is a persistable single-key name.
pub fn is_supported_key(key: &str) -> bool {
    if NAMED_HOTKEY_KEYS.iter().any(|(name, _)| *name == key) {
        return true;
    }
    if let Ok(number) = key.strip_prefix('f').unwrap_or("").parse::<u32>() {
        return (1..=24).contains(&number);
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(one), None) => {
            one.is_ascii_alphanumeric()
                || matches!(
                    one,
                    '-' | '=' | ',' | '.' | '/' | '`' | ';' | '\'' | '[' | '\\' | ']'
                )
        }
        _ => false,
    }
}

impl HotkeyCombo {
    /// True when the binding can be registered: at least one modifier and a supported key.
    pub fn is_valid(&self) -> bool {
        (self.ctrl || self.shift || self.alt || self.win) && is_supported_key(&self.key)
    }

    /// True when at least one modifier is required.
    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.shift || self.alt || self.win
    }
}

/// Global hotkeys that translate the selected text of another application.
///
/// The master switch governs every binding; when disabled the app keeps the
/// stored bindings so toggling back on restores them.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    /// Master switch for the whole hotkey feature.
    pub enabled: bool,
    /// Modifier chord registered through `RegisterHotKey`. `None` = not configured.
    pub combo: Option<HotkeyCombo>,
    /// Modifier + key pressed twice in quick succession, DeepL style. `None` = not configured.
    pub double_tap: Option<HotkeyCombo>,
    /// How selected text is captured when a trigger fires.
    pub acquisition: AcquisitionMode,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            combo: Some(HotkeyCombo::default()),
            double_tap: Some(HotkeyCombo {
                ctrl: true,
                shift: false,
                alt: false,
                win: false,
                key: "c".to_string(),
            }),
            acquisition: AcquisitionMode::default(),
        }
    }
}

/// Smallest editor text size, in pixels.
pub const TEXT_SIZE_MIN: u16 = 10;
/// Largest editor text size, in pixels.
pub const TEXT_SIZE_MAX: u16 = 32;
/// Pixels added or removed by one zoom step.
pub const TEXT_SIZE_STEP: u16 = 2;
/// Text size used when nothing is stored or the user resets.
pub const TEXT_SIZE_DEFAULT: u16 = 14;

/// Direction of a text size change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextZoom {
    In,
    Out,
    Reset,
}

/// Forces a text size into the supported range.
pub fn clamp_text_size(size: u16) -> u16 {
    size.clamp(TEXT_SIZE_MIN, TEXT_SIZE_MAX)
}

/// Applies one zoom step to `size`, staying inside the supported range.
pub fn zoom_text_size(size: u16, zoom: TextZoom) -> u16 {
    let size = clamp_text_size(size);
    match zoom {
        TextZoom::In => clamp_text_size(size.saturating_add(TEXT_SIZE_STEP)),
        TextZoom::Out => clamp_text_size(size.saturating_sub(TEXT_SIZE_STEP)),
        TextZoom::Reset => TEXT_SIZE_DEFAULT,
    }
}

/// Line height for `size`, keeping the input's stock 20px rows at the default 14px text.
pub fn text_line_height(size: u16) -> f32 {
    (f32::from(clamp_text_size(size)) * 20.0 / f32::from(TEXT_SIZE_DEFAULT)).round()
}

/// Reads a text size, falling back to the default for anything that is not a number so
/// one malformed value cannot discard the whole settings file.
fn lenient_text_size<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
    let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
    Ok(value
        .as_u64()
        .map(|n| clamp_text_size(n.min(u64::from(u16::MAX)) as u16))
        .unwrap_or(TEXT_SIZE_DEFAULT))
}

/// UI color theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    /// Follow the Windows system appearance.
    #[default]
    System,
    Light,
    Dark,
}

/// How prompts are built.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PromptSettings {
    /// Instruction language: auto (Chinese for Chinese targets) or a forced choice.
    pub language: PromptLanguage,
    /// Optional style instruction inserted into the prompt. Empty means "no style".
    pub style: String,
    /// Raw glossary text, one `source=>target` pair per line.
    pub glossary: String,
    /// Use the glossary (terminology intervention) while translating.
    pub use_glossary: bool,
    /// Use the delimiter-preserving instruction.
    pub preserve_delimiters: bool,
}

impl Default for PromptSettings {
    fn default() -> Self {
        Self {
            language: PromptLanguage::Auto,
            style: String::new(),
            glossary: String::new(),
            use_glossary: false,
            preserve_delimiters: false,
        }
    }
}

/// Everything the application persists.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    /// BCP-47-ish code of the source language.
    pub source_lang: String,
    /// BCP-47-ish code of the target language.
    pub target_lang: String,
    /// Base URL used to download GGUF files (Hugging Face or a mirror).
    pub download_base: String,
    /// UI theme (light or dark).
    pub appearance: Appearance,
    /// Engine location and launch options.
    pub server: ServerSettings,
    /// Sampling options.
    pub generation: GenerationSettings,
    /// Prompt options.
    pub prompt: PromptSettings,
    /// Tray and Windows startup options.
    pub desktop: DesktopSettings,
    /// Global hotkey triggers for selected text.
    pub hotkey: HotkeySettings,
    /// Text size in pixels shared by the source and translation editors.
    #[serde(deserialize_with = "lenient_text_size")]
    pub text_size: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            source_lang: default_source().code.to_string(),
            target_lang: default_target().code.to_string(),
            download_base: "https://huggingface.co".to_string(),
            appearance: Appearance::default(),
            server: ServerSettings::default(),
            generation: GenerationSettings::default(),
            prompt: PromptSettings::default(),
            desktop: DesktopSettings::default(),
            hotkey: HotkeySettings::default(),
            text_size: TEXT_SIZE_DEFAULT,
        }
    }
}

impl Settings {
    /// Forces every value into its supported range.
    ///
    /// Called after loading and before saving, so the engine never receives nonsense and the
    /// UI never has to defend against it.
    pub fn sanitized(mut self) -> Self {
        let valid = |code: &str| crate::lang::by_code(code).is_some();
        if !valid(&self.source_lang) {
            self.source_lang = default_source().code.to_string();
        }
        if !valid(&self.target_lang) || self.target_lang == self.source_lang {
            self.target_lang = if self.source_lang == "en" {
                "ko".to_string()
            } else {
                default_target().code.to_string()
            };
        }

        let base = self.download_base.trim().trim_end_matches('/').to_string();
        if !(base.starts_with("http://") || base.starts_with("https://")) || base.len() > 200 {
            self.download_base = "https://huggingface.co".to_string();
        } else {
            self.download_base = base;
        }

        self.server.port = self.server.port.clamp(1024, 65535);
        self.server.ctx_size = self.server.ctx_size.clamp(512, 32_768);
        self.server.threads = self.server.threads.clamp(1, 64);
        self.server.gpu_layers = self.server.gpu_layers.clamp(-1, 256);

        self.generation.temperature = clamp_finite(self.generation.temperature, 0.0, 2.0, 0.7);
        self.generation.top_p = clamp_finite(self.generation.top_p, 0.0, 1.0, 0.6);
        self.generation.repeat_penalty =
            clamp_finite(self.generation.repeat_penalty, 0.5, 2.0, 1.05);
        self.generation.top_k = self.generation.top_k.clamp(-1, 200);
        self.generation.max_tokens = self.generation.max_tokens.clamp(16, 8192);

        self.text_size = clamp_text_size(self.text_size);

        // Hotkey bindings must stay registrable: normalize the key name and drop
        // malformed combinations instead of trying to register them later.
        if let Some(combo) = self.hotkey.combo.take() {
            let combo = HotkeyCombo {
                key: normalize_hotkey_key(&combo.key),
                ..combo
            };
            self.hotkey.combo = combo.is_valid().then_some(combo);
        }
        if let Some(tap) = self.hotkey.double_tap.take() {
            let tap = HotkeyCombo {
                key: normalize_hotkey_key(&tap.key),
                ..tap
            };
            self.hotkey.double_tap = tap.is_valid().then_some(tap);
        }

        if self.prompt.style.chars().count() > 200 {
            self.prompt.style = self.prompt.style.chars().take(200).collect();
        }
        self
    }

    /// Reads settings from `path`, returning defaults when the file does not exist.
    ///
    /// A corrupted file is reported instead of silently discarded, so the UI can warn the
    /// user; the defaults are still returned.
    pub fn load_or_default(path: &Path) -> (Self, Option<String>) {
        match fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(settings) => (settings.sanitized(), None),
                Err(err) => (
                    Settings::default(),
                    Some(format!(
                        "설정 파일을 읽을 수 없어 기본값으로 시작합니다: {err}"
                    )),
                ),
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => (Settings::default(), None),
            Err(err) => (
                Settings::default(),
                Some(format!("설정 파일을 열 수 없습니다: {err}")),
            ),
        }
    }

    /// Writes settings atomically (temp file + rename) so a crash cannot truncate them.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.clone().sanitized())
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        let tmp = path.with_extension("json.tmp");
        {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(text.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
        }
        fs::rename(&tmp, path)
    }
}

fn clamp_finite(value: f64, min: f64, max: f64, fallback: f64) -> f64 {
    if !value.is_finite() {
        return fallback;
    }
    value.clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn defaults_follow_the_model_card() {
        let settings = Settings::default();
        assert_eq!(settings.source_lang, "ko");
        assert_eq!(settings.target_lang, "en");
        assert_eq!(settings.generation.temperature, 0.7);
        assert_eq!(settings.generation.top_p, 0.6);
        assert_eq!(settings.generation.top_k, 20);
        assert_eq!(settings.generation.repeat_penalty, 1.05);
        assert_eq!(settings.generation.max_tokens, 4096);
        assert!(settings.server.port > 1024);
        assert_eq!(settings.appearance, Appearance::System);
        assert!(!settings.desktop.keep_in_tray);
        assert!(!settings.desktop.start_with_windows);
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = TempDir::new("settings");
        let path = dir.path().join("settings.json");
        let mut settings = Settings::default();
        settings.target_lang = "ja".into();
        settings.server.ctx_size = 8192;
        settings.prompt.glossary = "GPUI=>지피유아이".into();
        settings.appearance = Appearance::Light;
        settings.desktop.keep_in_tray = true;
        settings.desktop.start_with_windows = true;
        settings.save(&path).unwrap();

        let (loaded, warning) = Settings::load_or_default(&path);
        assert!(warning.is_none());
        assert_eq!(loaded, settings.sanitized());
    }

    #[test]
    fn missing_file_and_corrupt_file_fall_back_to_defaults() {
        let dir = TempDir::new("settings-missing");
        let path = dir.path().join("settings.json");
        let (loaded, warning) = Settings::load_or_default(&path);
        assert_eq!(loaded, Settings::default());
        assert!(warning.is_none());

        fs::write(&path, "{not json").unwrap();
        let (loaded, warning) = Settings::load_or_default(&path);
        assert_eq!(loaded, Settings::default());
        assert!(warning.is_some());
    }

    #[test]
    fn sanitize_clamps_out_of_range_values() {
        let mut settings = Settings::default();
        settings.server.port = 0;
        settings.server.ctx_size = 1;
        settings.server.threads = 0;
        settings.server.gpu_layers = -99;
        settings.generation.temperature = f64::NAN;
        settings.generation.top_p = 42.0;
        settings.generation.max_tokens = 0;
        settings.source_lang = "xx".into();
        settings.target_lang = "xx".into();

        let fixed = settings.sanitized();
        assert_eq!(fixed.server.port, 1024);
        assert_eq!(fixed.server.ctx_size, 512);
        assert_eq!(fixed.server.threads, 1);
        assert_eq!(fixed.server.gpu_layers, -1);
        assert_eq!(fixed.generation.temperature, 0.7);
        assert_eq!(fixed.generation.top_p, 1.0);
        assert_eq!(fixed.generation.max_tokens, 16);
        assert_eq!(fixed.source_lang, "ko");
        assert_eq!(fixed.target_lang, "en");
        assert_eq!(fixed.download_base, "https://huggingface.co");
    }

    #[test]
    fn older_settings_files_get_desktop_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{"source_lang":"ko","target_lang":"en"}"#).unwrap();
        assert_eq!(settings.appearance, Appearance::System);
        assert_eq!(settings.desktop, DesktopSettings::default());
    }

    #[test]
    fn text_size_zoom_steps_and_clamps() {
        assert_eq!(zoom_text_size(14, TextZoom::In), 16);
        assert_eq!(zoom_text_size(14, TextZoom::Out), 12);
        assert_eq!(zoom_text_size(30, TextZoom::Reset), TEXT_SIZE_DEFAULT);
        assert_eq!(zoom_text_size(TEXT_SIZE_MAX, TextZoom::In), TEXT_SIZE_MAX);
        assert_eq!(zoom_text_size(TEXT_SIZE_MIN, TextZoom::Out), TEXT_SIZE_MIN);
        assert_eq!(zoom_text_size(31, TextZoom::In), TEXT_SIZE_MAX);
        assert_eq!(
            zoom_text_size(0, TextZoom::In),
            TEXT_SIZE_MIN + TEXT_SIZE_STEP
        );
        assert_eq!(
            zoom_text_size(u16::MAX, TextZoom::Out),
            TEXT_SIZE_MAX - TEXT_SIZE_STEP
        );
    }

    #[test]
    fn line_height_scales_with_text_size() {
        assert_eq!(text_line_height(TEXT_SIZE_DEFAULT), 20.0);
        assert_eq!(text_line_height(28), 40.0);
        assert_eq!(text_line_height(TEXT_SIZE_MIN), 14.0);
        assert!(text_line_height(TEXT_SIZE_MAX) > f32::from(TEXT_SIZE_MAX));
    }

    #[test]
    fn text_size_is_sanitized_and_persisted() {
        let sized = |text_size| Settings {
            text_size,
            ..Settings::default()
        };
        assert_eq!(sized(2).sanitized().text_size, TEXT_SIZE_MIN);
        assert_eq!(sized(999).sanitized().text_size, TEXT_SIZE_MAX);

        let dir = TempDir::new("settings-text-size");
        let path = dir.path().join("settings.json");
        sized(20).save(&path).unwrap();
        let (loaded, warning) = Settings::load_or_default(&path);
        assert!(warning.is_none());
        assert_eq!(loaded.text_size, 20);
    }

    #[test]
    fn missing_or_malformed_text_size_falls_back_to_default() {
        let parse = |json: &str| serde_json::from_str::<Settings>(json).unwrap().text_size;
        assert_eq!(parse(r#"{"source_lang":"ko"}"#), TEXT_SIZE_DEFAULT);
        assert_eq!(parse(r#"{"text_size":"big"}"#), TEXT_SIZE_DEFAULT);
        assert_eq!(parse(r#"{"text_size":{"source":18}}"#), TEXT_SIZE_DEFAULT);
        assert_eq!(parse(r#"{"text_size":100000}"#), TEXT_SIZE_MAX);
        assert_eq!(parse(r#"{"text_size":18}"#), 18);
    }

    #[test]
    fn source_and_target_can_never_be_identical() {
        let mut settings = Settings::default();
        settings.target_lang = "ko".into();
        let fixed = settings.sanitized();
        assert_ne!(fixed.source_lang, fixed.target_lang);
    }

    #[test]
    fn hotkey_defaults_are_disabled_with_standard_bindings() {
        let hotkey = Settings::default().hotkey;
        assert!(!hotkey.enabled);
        let chord = hotkey.combo.expect("default chord");
        assert!(chord.ctrl && chord.shift && !chord.alt && !chord.win);
        assert_eq!(chord.key, "c");
        assert!(chord.is_valid());
        let double_tap = hotkey.double_tap.expect("default double press");
        assert!(double_tap.ctrl && !double_tap.shift);
        assert_eq!(double_tap.key, "c");
        assert_eq!(hotkey.acquisition, AcquisitionMode::Auto);
    }

    #[test]
    fn hotkey_settings_persist_and_stay_enabled() {
        let mut settings = Settings {
            hotkey: HotkeySettings {
                enabled: true,
                acquisition: AcquisitionMode::ClipboardOnly,
                ..HotkeySettings::default()
            },
            ..Settings::default()
        };
        settings.hotkey.combo = Some(HotkeyCombo {
            ctrl: true,
            shift: false,
            alt: true,
            win: false,
            key: "f4".to_string(),
        });

        let dir = TempDir::new("settings-hotkey");
        let path = dir.path().join("settings.json");
        settings.save(&path).unwrap();
        let (loaded, warning) = Settings::load_or_default(&path);
        assert!(warning.is_none());
        assert_eq!(loaded.hotkey, settings.hotkey);
    }

    #[test]
    fn hotkey_sanitize_drops_unusable_bindings() {
        let unusable_combo = HotkeyCombo {
            ctrl: true,
            shift: false,
            key: "ctrl".to_string(),
            ..HotkeyCombo::default()
        };
        let unusable_tap = HotkeyCombo {
            ctrl: false,
            shift: false,
            key: "c".to_string(),
            ..HotkeyCombo::default()
        };

        let settings = Settings {
            hotkey: HotkeySettings {
                enabled: true,
                combo: Some(unusable_combo),
                double_tap: Some(unusable_tap),
                ..HotkeySettings::default()
            },
            ..Settings::default()
        };
        let fixed = settings.sanitized();
        assert!(fixed.hotkey.combo.is_none());
        assert!(fixed.hotkey.double_tap.is_none());
        // The master switch itself is not invalidated; the app reports that no
        // trigger is registered.
        assert!(fixed.hotkey.enabled);
    }

    #[test]
    fn hotkey_key_names_are_normalized() {
        let mut combo = HotkeyCombo {
            ctrl: true,
            key: "  C ".to_string(),
            ..HotkeyCombo::default()
        };
        combo.key = normalize_hotkey_key(&combo.key);
        assert!(combo.is_valid());
        assert!(is_supported_key("f12"));
        assert!(is_supported_key("space"));
        assert!(is_supported_key(","));
        assert!(is_supported_key("`"));
        assert!(!is_supported_key("ctrl"));
        assert!(!is_supported_key("f25"));
        assert!(!is_supported_key(""));
        assert!(!is_supported_key("hello"));
    }
}
