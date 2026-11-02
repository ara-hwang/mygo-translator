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
    /// Engine location and launch options.
    pub server: ServerSettings,
    /// Sampling options.
    pub generation: GenerationSettings,
    /// Prompt options.
    pub prompt: PromptSettings,
    /// Tray and Windows startup options.
    pub desktop: DesktopSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            source_lang: default_source().code.to_string(),
            target_lang: default_target().code.to_string(),
            download_base: "https://huggingface.co".to_string(),
            server: ServerSettings::default(),
            generation: GenerationSettings::default(),
            prompt: PromptSettings::default(),
            desktop: DesktopSettings::default(),
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
        assert_eq!(settings.desktop, DesktopSettings::default());
    }

    #[test]
    fn source_and_target_can_never_be_identical() {
        let mut settings = Settings::default();
        settings.target_lang = "ko".into();
        let fixed = settings.sanitized();
        assert_ne!(fixed.source_lang, fixed.target_lang);
    }
}
