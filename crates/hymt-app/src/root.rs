//! Application state and behaviour.
//!
//! All long-running work happens off the UI thread: the engine supervisor owns the llama.cpp
//! process, translations run on a worker thread and stream deltas through a channel, and model
//! downloads are a `curl.exe` child process whose progress is read from the `.part` file. A
//! single GPUI task pumps those channels into the entity on every frame that needs it.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use gpui::{Context, Entity, SharedString};
use hymt_core::{
    client::{self, Endpoint, GenerationParams, TranslationResult},
    engine::{self, EngineEvent, EngineState, LaunchConfig, Supervisor},
    glossary, http,
    lang::{self, Language},
    models::{self, DownloadProgress, ModelOption},
    paths::AppPaths,
    prompt::{self, PromptLanguage, PromptMode},
    settings::Settings,
};

use crate::text_area::TextArea;

/// Top-level tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// Translation workspace.
    Translate,
    /// Model download and runtime setup.
    Models,
    /// Sampling, prompt and engine options.
    Settings,
}

/// Which side of the language pair a picker is editing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LangSlot {
    /// Source language.
    Source,
    /// Target language.
    Target,
}

/// Style presets offered in the UI. The second field is the instruction text; empty means off.
pub const STYLE_PRESETS: &[(&str, &str)] = &[
    ("기본", ""),
    ("격식체", "formal and polite"),
    ("구어체", "casual and conversational"),
    ("기술 문서", "technical documentation"),
    ("마케팅", "marketing copy"),
    ("문학", "literary and expressive"),
];

/// Prompt-language choices shown in settings.
pub const PROMPT_LANGUAGES: &[(&str, PromptLanguage)] = &[
    ("자동", PromptLanguage::Auto),
    ("영어 지시문", PromptLanguage::English),
    ("중국어 지시문", PromptLanguage::Chinese),
];

/// A transient message shown above the status bar.
#[derive(Clone, Debug)]
pub struct Notice {
    /// Message body.
    pub text: String,
    /// Whether it reports a failure.
    pub is_error: bool,
    created: Instant,
}

impl Notice {
    /// Creates an informational notice.
    pub fn info(text: impl Into<String>) -> Self {
        Self::new(text, false)
    }

    /// Creates a failure notice.
    pub fn error(text: impl Into<String>) -> Self {
        Self::new(text, true)
    }

    fn new(text: impl Into<String>, is_error: bool) -> Self {
        Self {
            text: text.into(),
            is_error,
            created: Instant::now(),
        }
    }

    fn expired(&self) -> bool {
        self.created.elapsed() > Duration::from_secs(14)
    }
}

/// Timing information for the last translation.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// Milliseconds until the first token.
    pub first_token_ms: Option<u128>,
    /// Total milliseconds.
    pub total_ms: u128,
    /// Characters in the result.
    pub chars: usize,
    /// Server-reported tokens per second.
    pub tokens_per_second: Option<f64>,
}

/// An in-flight model download.
pub struct Download {
    /// Catalogue entry being downloaded.
    pub option: &'static ModelOption,
    /// The `curl.exe` child process.
    child: Child,
    /// Partial file being written.
    pub part: PathBuf,
    /// Final destination.
    pub dest: PathBuf,
    /// Expected size in bytes.
    pub expected: u64,
    /// Bytes observed at the last poll.
    pub downloaded: u64,
    /// When the download started.
    pub started: Instant,
}

enum TranslateEvent {
    Delta(String),
    Done(Result<TranslationResult, String>),
}

/// The single entity that owns the whole window state.
pub struct Root {
    /// Application directories.
    pub paths: AppPaths,
    /// Persisted settings.
    pub settings: Settings,
    /// Active tab.
    pub tab: Tab,
    /// Language picker overlay, if open.
    pub picker: Option<LangSlot>,
    /// Multiline source editor.
    pub source: Entity<TextArea>,
    /// Glossary editor in settings.
    pub glossary_editor: Entity<TextArea>,
    /// Single-line filter in the language picker.
    pub picker_filter: Entity<TextArea>,
    /// Translated text.
    pub output: String,
    /// True when the last translation failed.
    pub output_is_error: bool,
    /// True while a translation is streaming.
    pub translating: bool,
    cancel: Option<http::CancelToken>,
    translate_rx: Option<Receiver<TranslateEvent>>,
    /// Stats of the last completed translation.
    pub stats: Option<Stats>,
    /// Engine supervisor, created on first launch.
    pub engine: Option<Supervisor>,
    /// Latest engine state.
    pub engine_state: EngineState,
    /// Tail of llama-server output.
    pub engine_log: VecDeque<String>,
    /// Resolved `llama-server` path.
    pub server_path: Option<PathBuf>,
    /// GGUF files found in the models folder.
    pub models: Vec<PathBuf>,
    /// Active download.
    pub download: Option<Download>,
    /// Transient message.
    pub notice: Option<Notice>,
    exe_dir: PathBuf,
}

impl Root {
    /// Builds the state, loads settings, scans models and starts the pump task.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let paths = AppPaths::detect();
        let _ = paths.ensure();
        let (settings, settings_warning) = Settings::load_or_default(&paths.settings_file());
        let models = engine::scan_gguf(&paths.models_dir());
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| paths.root().to_path_buf());
        let server_path =
            engine::discover_server(&exe_dir, settings.server.runtime_path.as_deref());

        let source = cx.new(|cx| TextArea::new(cx, "번역할 텍스트를 입력하세요…", true));
        let glossary_editor = cx.new(|cx| {
            TextArea::new(
                cx,
                "예) GPUI => 지피유아이\n(한 줄에 `원문 => 번역` 하나)",
                true,
            )
        });
        let picker_filter = cx.new(|cx| TextArea::new(cx, "언어 검색…", false));

        if !settings.prompt.glossary.is_empty() {
            glossary_editor.update(cx, |editor, cx| {
                editor.set_content(settings.prompt.glossary.clone(), cx)
            });
        }

        cx.observe(&source, |_, _, cx| cx.notify()).detach();
        cx.observe(&glossary_editor, |this, editor, cx| {
            this.settings.prompt.glossary = editor.read(cx).content().to_string();
            cx.notify();
        })
        .detach();
        cx.observe(&picker_filter, |_, _, cx| cx.notify()).detach();

        let mut root = Self {
            paths,
            settings,
            tab: Tab::Translate,
            picker: None,
            source,
            glossary_editor,
            picker_filter,
            output: String::new(),
            output_is_error: false,
            translating: false,
            cancel: None,
            translate_rx: None,
            stats: None,
            engine: None,
            engine_state: EngineState::Stopped,
            engine_log: VecDeque::new(),
            server_path,
            models,
            download: None,
            notice: settings_warning.map(|warning| Notice::new(warning, true)),
            exe_dir,
        };

        if root.settings.server.auto_start {
            root.start_engine(cx);
        }

        let task = cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(80))
                .await;
            let alive = this
                .update(cx, |root, cx| {
                    if root.tick(cx) {
                        cx.notify();
                    }
                })
                .is_ok();
            if !alive {
                break;
            }
        });
        task.detach();

        root
    }

    /// Drains background channels. Returns true when the UI needs a redraw.
    pub fn tick(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;

        // Engine events.
        let mut events = Vec::new();
        if let Some(supervisor) = &self.engine {
            while let Some(event) = supervisor.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                EngineEvent::State(state) => {
                    if state != self.engine_state {
                        if let EngineState::Failed(message) = &state {
                            self.notice = Some(Notice::new(message.clone(), true));
                        }
                        self.engine_state = state;
                        changed = true;
                    }
                }
                EngineEvent::Log(line) => {
                    self.engine_log.push_back(line);
                    while self.engine_log.len() > 300 {
                        self.engine_log.pop_front();
                    }
                    changed = true;
                }
            }
        }

        // Translation stream.
        if let Some(receiver) = self.translate_rx.take() {
            let mut keep = true;
            loop {
                match receiver.try_recv() {
                    Ok(TranslateEvent::Delta(delta)) => {
                        self.output.push_str(&delta);
                        changed = true;
                    }
                    Ok(TranslateEvent::Done(result)) => {
                        keep = false;
                        self.translating = false;
                        self.cancel = None;
                        match result {
                            Ok(result) => {
                                self.output = result.text;
                                self.output_is_error = false;
                                self.stats = Some(Stats {
                                    first_token_ms: result.first_token_ms,
                                    total_ms: result.total_ms,
                                    chars: self.output.chars().count(),
                                    tokens_per_second: result.tokens_per_second.or_else(|| {
                                        result.completion_tokens.map(|tokens| {
                                            if result.total_ms == 0 {
                                                0.0
                                            } else {
                                                tokens as f64
                                                    / (result.total_ms as f64 / 1000.0).max(0.001)
                                            }
                                        })
                                    }),
                                });
                                if result.cancelled {
                                    self.notice = Some(Notice::new("번역을 중단했습니다.", false));
                                }
                            }
                            Err(message) => {
                                self.output = message.clone();
                                self.output_is_error = true;
                                self.stats = None;
                                self.notice = Some(Notice::new(message, true));
                            }
                        }
                        changed = true;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        keep = false;
                        self.translating = false;
                        self.cancel = None;
                        changed = true;
                        break;
                    }
                }
            }
            if keep {
                self.translate_rx = Some(receiver);
            }
        }

        // Download progress.
        if let Some(mut download) = self.download.take() {
            match download.child.try_wait() {
                Ok(Some(status)) => {
                    let size = fs::metadata(&download.part).map(|m| m.len()).unwrap_or(0);
                    if status.success() && size == download.expected {
                        match fs::rename(&download.part, &download.dest) {
                            Ok(()) => {
                                self.models = engine::scan_gguf(&self.paths.models_dir());
                                self.settings.server.model_path = Some(download.dest.clone());
                                let _ = self.save_settings();
                                self.notice = Some(Notice::new(
                                    format!(
                                        "{} 다운로드 완료. 모델을 불러옵니다.",
                                        download.option.file_name()
                                    ),
                                    false,
                                ));
                                if self.settings.server.auto_start {
                                    self.restart_engine(cx);
                                }
                            }
                            Err(err) => {
                                self.notice = Some(Notice::new(
                                    format!("다운로드한 파일을 옮길 수 없습니다: {err}"),
                                    true,
                                ));
                            }
                        }
                    } else {
                        let detail = if status.success() {
                            format!(
                                "파일 크기가 맞지 않습니다 ({} / {} 바이트)",
                                size, download.expected
                            )
                        } else {
                            format!("curl 종료 코드 {:?}", status.code())
                        };
                        self.notice = Some(Notice::new(
                            format!(
                                "다운로드가 완료되지 않았습니다 ({detail}). 모델 탭에서 다시 시도하세요."
                            ),
                            true,
                        ));
                    }
                    changed = true;
                }
                Ok(None) => {
                    if let Ok(metadata) = fs::metadata(&download.part) {
                        if metadata.len() != download.downloaded {
                            download.downloaded = metadata.len();
                            changed = true;
                        }
                    }
                    self.download = Some(download);
                }
                Err(err) => {
                    self.notice = Some(Notice::new(format!("다운로드 상태 오류: {err}"), true));
                    changed = true;
                }
            }
        }

        if self.notice.as_ref().is_some_and(Notice::expired) {
            self.notice = None;
            changed = true;
        }

        changed
    }

    /// Updates the language on one side of the pair.
    pub fn set_language(&mut self, slot: LangSlot, code: &str, cx: &mut Context<Self>) {
        if lang::by_code(code).is_none() {
            return;
        }
        match slot {
            LangSlot::Source => {
                if self.settings.source_lang == code {
                    return;
                }
                if self.settings.target_lang == code {
                    self.settings.target_lang = self.settings.source_lang.clone();
                }
                self.settings.source_lang = code.to_string();
            }
            LangSlot::Target => {
                if self.settings.target_lang == code {
                    return;
                }
                if self.settings.source_lang == code {
                    self.settings.source_lang = self.settings.target_lang.clone();
                }
                self.settings.target_lang = code.to_string();
            }
        }
        let _ = self.save_settings();
        cx.notify();
    }

    /// Swaps source and target languages.
    pub fn swap_languages(&mut self, cx: &mut Context<Self>) {
        std::mem::swap(
            &mut self.settings.source_lang,
            &mut self.settings.target_lang,
        );
        let _ = self.save_settings();
        cx.notify();
    }

    /// Looks up the selected source language.
    pub fn source_language(&self) -> &'static Language {
        lang::by_code(&self.settings.source_lang).unwrap_or_else(lang::default_source)
    }

    /// Looks up the selected target language.
    pub fn target_language(&self) -> &'static Language {
        lang::by_code(&self.settings.target_lang).unwrap_or_else(lang::default_target)
    }

    /// Builds the instruction text for the current settings.
    pub fn build_prompt(&self, source: &str) -> String {
        let target = self.target_language();
        let pairs = if self.settings.prompt.use_glossary {
            glossary::parse(&self.settings.prompt.glossary)
        } else {
            Vec::new()
        };
        let style = self.settings.prompt.style.trim();
        let mode = if !pairs.is_empty() {
            PromptMode::Terminology { pairs: &pairs }
        } else if !style.is_empty() {
            PromptMode::Style { style }
        } else if self.settings.prompt.preserve_delimiters {
            PromptMode::Delimiters
        } else {
            PromptMode::Plain
        };
        prompt::build_prompt(mode, source, target, self.settings.prompt.language)
    }

    /// Starts a streaming translation of the current input.
    pub fn translate(&mut self, cx: &mut Context<Self>) {
        if self.translating {
            return;
        }
        let source = self.source.read(cx).content().to_string();
        if let Err(message) = prompt::validate_source(&source) {
            self.notice = Some(Notice::new(message, true));
            cx.notify();
            return;
        }
        if !self.engine_state.is_ready() {
            let message = match &self.engine_state {
                EngineState::Starting => "모델을 아직 불러오는 중입니다. 잠시만 기다려 주세요.",
                EngineState::Failed(_) => {
                    "모델 서버에 문제가 있습니다. 모델 탭에서 상태를 확인하세요."
                }
                _ => "모델 서버가 중지되어 있습니다. 모델 탭에서 시작하세요.",
            };
            self.notice = Some(Notice::new(message, true));
            cx.notify();
            return;
        }

        let prompt_text = self.build_prompt(&source);
        let params = GenerationParams::from(&self.settings.generation);
        let endpoint = Endpoint::localhost(self.settings.server.port);
        let cancel = http::cancel_token();
        self.cancel = Some(cancel.clone());
        self.output.clear();
        self.output_is_error = false;
        self.stats = None;
        self.translating = true;
        self.notice = None;

        let (sender, receiver) = mpsc::channel();
        self.translate_rx = Some(receiver);
        thread::Builder::new()
            .name("hymt-translate".to_string())
            .spawn(move || {
                let stream_sender = sender.clone();
                let result = client::translate_stream(
                    &endpoint,
                    "hymt",
                    &params,
                    &prompt_text,
                    cancel,
                    &mut |delta| {
                        let _ = stream_sender.send(TranslateEvent::Delta(delta.to_string()));
                    },
                );
                let _ = sender.send(TranslateEvent::Done(result.map_err(|err| err.to_string())));
            })
            .ok();

        cx.notify();
    }

    /// Cancels an in-flight translation.
    pub fn cancel_translation(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Copies the translation to the clipboard.
    pub fn copy_output(&mut self, cx: &mut Context<Self>) {
        if self.output.trim().is_empty() {
            return;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(self.output.clone()));
        self.notice = Some(Notice::new("번역문을 복사했습니다.", false));
        cx.notify();
    }

    /// Replaces the input with the translation (handy for back-and-forth work).
    pub fn output_to_input(&mut self, cx: &mut Context<Self>) {
        if self.output.trim().is_empty() {
            return;
        }
        let text = self.output.clone();
        self.source
            .update(cx, |editor, cx| editor.set_content(text, cx));
        self.swap_languages(cx);
    }

    /// Pastes the clipboard into the source editor.
    pub fn paste_into_source(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.source
                .update(cx, |editor, cx| editor.set_content(text, cx));
        }
        cx.notify();
    }

    /// Clears both editors.
    pub fn clear_all(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.translate_rx = None;
        self.translating = false;
        self.source.update(cx, |editor, cx| editor.clear(cx));
        self.output.clear();
        self.output_is_error = false;
        self.stats = None;
        self.notice = None;
        cx.notify();
    }

    /// Writes settings to disk.
    pub fn save_settings(&self) -> std::io::Result<()> {
        self.settings.save(&self.paths.settings_file())
    }

    /// Persists settings and reports the result in the UI.
    pub fn save_settings_with_notice(&mut self, cx: &mut Context<Self>) {
        self.settings.prompt.glossary = self.glossary_editor.read(cx).content().to_string();
        match self.save_settings() {
            Ok(()) => self.notice = Some(Notice::new("설정을 저장했습니다.", false)),
            Err(err) => {
                self.notice = Some(Notice::new(
                    format!("설정을 저장할 수 없습니다: {err}"),
                    true,
                ))
            }
        }
        cx.notify();
    }

    /// Launches the managed llama.cpp server.
    pub fn start_engine(&mut self, cx: &mut Context<Self>) {
        if self.engine_state.is_busy() {
            return;
        }
        let Some(config) = self.launch_config() else {
            return;
        };
        if self.engine.is_none() {
            match Supervisor::spawn(self.paths.engine_log()) {
                Ok(supervisor) => self.engine = Some(supervisor),
                Err(err) => {
                    self.notice = Some(Notice::new(
                        format!("엔진 스레드를 시작할 수 없습니다: {err}"),
                        true,
                    ));
                    cx.notify();
                    return;
                }
            }
        }
        self.engine_state = EngineState::Starting;
        if let Some(supervisor) = &self.engine {
            supervisor.start(config);
        }
        cx.notify();
    }

    /// Stops the managed server.
    pub fn stop_engine(&mut self, cx: &mut Context<Self>) {
        if let Some(supervisor) = &self.engine {
            supervisor.stop();
        }
        self.engine_state = EngineState::Stopped;
        cx.notify();
    }

    /// Restarts with the current settings.
    pub fn restart_engine(&mut self, cx: &mut Context<Self>) {
        if let Some(supervisor) = &self.engine {
            supervisor.stop();
        }
        self.engine_state = EngineState::Stopped;
        self.start_engine(cx);
    }

    fn launch_config(&mut self) -> Option<LaunchConfig> {
        let Some(server_path) =
            engine::discover_server(&self.exe_dir, self.settings.server.runtime_path.as_deref())
        else {
            self.notice = Some(Notice::new(
                "llama-server를 찾을 수 없습니다. 모델 탭의 설치 안내를 따라 runtime 폴더에 넣어 주세요.",
                true,
            ));
            return None;
        };
        self.server_path = Some(server_path.clone());

        let configured = self
            .settings
            .server
            .model_path
            .clone()
            .filter(|path| path.is_file());
        let Some(model_path) = configured.or_else(|| engine::pick_model(&self.models)) else {
            self.notice = Some(Notice::new(
                "사용할 GGUF 모델이 없습니다. 모델 탭에서 Hy-MT2 모델을 내려받으세요.",
                true,
            ));
            return None;
        };
        self.settings.server.model_path = Some(model_path.clone());

        Some(LaunchConfig {
            server_path,
            model_path,
            port: self.settings.server.port,
            ctx_size: self.settings.server.ctx_size,
            threads: self.settings.server.threads,
            gpu_layers: self.settings.server.gpu_layers,
            alias: "hymt".to_string(),
        })
    }

    /// Re-scans the models folder.
    pub fn rescan_models(&mut self, cx: &mut Context<Self>) {
        self.models = engine::scan_gguf(&self.paths.models_dir());
        let missing = self
            .settings
            .server
            .model_path
            .as_ref()
            .is_some_and(|path| !path.is_file());
        if missing || self.settings.server.model_path.is_none() {
            self.settings.server.model_path = engine::pick_model(&self.models);
        }
        self.notice = Some(Notice::new(
            format!("모델 {}개를 찾았습니다.", self.models.len()),
            false,
        ));
        cx.notify();
    }

    /// Picks a locally installed model.
    pub fn select_model(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.settings.server.model_path = Some(path);
        let _ = self.save_settings();
        self.restart_engine(cx);
    }

    /// Starts downloading a catalogue entry with `curl.exe`.
    pub fn start_download(&mut self, option: &'static ModelOption, cx: &mut Context<Self>) {
        if self.download.is_some() {
            self.notice = Some(Notice::new("이미 다운로드가 진행 중입니다.", true));
            cx.notify();
            return;
        }
        let dest = self.paths.models_dir().join(option.file_name());
        if dest.is_file() {
            self.settings.server.model_path = Some(dest);
            let _ = self.save_settings();
            self.notice = Some(Notice::new("이미 내려받은 모델입니다.", false));
            cx.notify();
            return;
        }
        if let Err(err) = fs::create_dir_all(self.paths.models_dir()) {
            self.notice = Some(Notice::new(
                format!("모델 폴더를 만들 수 없습니다: {err}"),
                true,
            ));
            cx.notify();
            return;
        }

        let part = models::part_path(&dest);
        let url = option.download_url(&self.settings.download_base);
        let mut command = Command::new(curl_program());
        command
            .args(models::curl_args(&url, &part))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }

        match command.spawn() {
            Ok(child) => {
                self.download = Some(Download {
                    option,
                    child,
                    part,
                    dest,
                    expected: option.size_bytes,
                    downloaded: 0,
                    started: Instant::now(),
                });
                self.notice = Some(Notice::new(
                    format!("{} 내려받는 중…", option.file_name()),
                    false,
                ));
            }
            Err(err) => {
                self.notice = Some(Notice::new(
                    format!(
                        "curl.exe를 실행할 수 없습니다: {err}. Windows 11의 기본 curl이 필요합니다."
                    ),
                    true,
                ));
            }
        }
        cx.notify();
    }

    /// Aborts the running download.
    pub fn cancel_download(&mut self, cx: &mut Context<Self>) {
        if let Some(mut download) = self.download.take() {
            let _ = download.child.kill();
            let _ = download.child.wait();
            self.notice = Some(Notice::new("다운로드를 취소했습니다.", false));
        }
        cx.notify();
    }

    /// Progress of the running download.
    pub fn download_progress(&self) -> Option<DownloadProgress> {
        self.download
            .as_ref()
            .map(|download| DownloadProgress::new(download.downloaded, download.expected))
    }

    /// Downloads-per-second estimate for the running download.
    pub fn download_rate(&self) -> Option<f64> {
        let download = self.download.as_ref()?;
        let seconds = download.started.elapsed().as_secs_f64();
        if seconds < 1.0 {
            return None;
        }
        Some(download.downloaded as f64 / seconds)
    }

    /// Opens a folder in the system file manager.
    pub fn open_folder(&mut self, path: &Path) {
        #[cfg(windows)]
        let _ = Command::new("explorer").arg(path).spawn();
        #[cfg(not(windows))]
        let _ = Command::new("xdg-open").arg(path).spawn();
    }

    /// Opens a URL in the default browser.
    pub fn open_url(&mut self, url: &str) {
        #[cfg(windows)]
        let _ = Command::new("explorer").arg(url).spawn();
        #[cfg(not(windows))]
        let _ = Command::new("xdg-open").arg(url).spawn();
    }

    /// Where the user should drop `llama-server.exe`.
    pub fn runtime_dir(&self) -> PathBuf {
        self.paths.runtime_dir()
    }

    /// Filename of the currently selected model, if any.
    pub fn current_model_name(&self) -> Option<String> {
        self.settings
            .server
            .model_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
    }

    /// Short description of the engine state for the status bar.
    pub fn engine_summary(&self) -> String {
        match &self.engine_state {
            EngineState::Ready => "모델 준비됨".to_string(),
            EngineState::Starting => "모델 불러오는 중".to_string(),
            EngineState::Stopped => "엔진 중지됨".to_string(),
            EngineState::Failed(_) => "엔진 오류".to_string(),
        }
    }

    /// Status color for the engine dot.
    pub fn engine_color(&self) -> u32 {
        match &self.engine_state {
            EngineState::Ready => crate::theme::OK,
            EngineState::Starting => crate::theme::WARN,
            EngineState::Stopped => crate::theme::DISABLED,
            EngineState::Failed(_) => crate::theme::ERROR,
        }
    }

    /// True when the glossary currently yields at least one pair.
    pub fn glossary_count(&self) -> usize {
        glossary::parse(&self.settings.prompt.glossary).len()
    }

    /// Text of the language filter.
    pub fn filter_text(&self, cx: &Context<Self>) -> String {
        self.picker_filter.read(cx).content().to_lowercase()
    }

    /// Languages matching the picker filter.
    pub fn filtered_languages(&self, cx: &Context<Self>) -> Vec<&'static Language> {
        let query = self.filter_text(cx);
        let query = query.trim();
        let mut languages: Vec<&'static Language> = lang::LANGUAGES
            .iter()
            .filter(|language| {
                query.is_empty()
                    || language.name_ko.contains(query)
                    || language.name_en.to_lowercase().contains(query)
                    || language.code.to_lowercase().contains(query)
            })
            .collect();
        // Keep Korean, English and the current pair at the top for quick access.
        languages.sort_by_key(|language| {
            let preferred = matches!(language.code, "ko" | "en" | "ja" | "zh")
                || language.code == self.settings.source_lang
                || language.code == self.settings.target_lang;
            (!preferred, language.name_ko)
        });
        languages
    }

    /// Opens the picker and focuses its filter.
    pub fn open_picker(
        &mut self,
        slot: LangSlot,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.picker = Some(slot);
        self.picker_filter.update(cx, |editor, cx| editor.clear(cx));
        self.picker_filter
            .update(cx, |editor, cx| editor.focus(window, cx));
        cx.notify();
    }

    /// Closes the picker.
    pub fn close_picker(&mut self, cx: &mut Context<Self>) {
        self.picker = None;
        cx.notify();
    }

    /// Summary line for the status bar.
    pub fn stats_line(&self) -> String {
        match &self.stats {
            Some(stats) if !self.translating => {
                let mut parts = Vec::new();
                parts.push(format!("{}자", stats.chars));
                if let Some(first) = stats.first_token_ms {
                    parts.push(format!("첫 토큰 {:.1}s", first as f64 / 1000.0));
                }
                parts.push(format!("{:.1}s", stats.total_ms as f64 / 1000.0));
                if let Some(speed) = stats.tokens_per_second {
                    parts.push(format!("{speed:.1} tok/s"));
                }
                parts.join(" · ")
            }
            _ if self.translating => "번역 중…".to_string(),
            _ => "Ctrl+Enter로 번역".to_string(),
        }
    }

    /// Label to show on the source/target chip.
    pub fn language_label(&self, slot: LangSlot) -> SharedString {
        let language = match slot {
            LangSlot::Source => self.source_language(),
            LangSlot::Target => self.target_language(),
        };
        language.name_ko.into()
    }
}

fn curl_program() -> &'static str {
    if cfg!(windows) {
        "curl.exe"
    } else {
        "curl"
    }
}
