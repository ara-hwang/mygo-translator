//! Application state and behaviour.
//!
//! All long-running work happens off the UI thread: the engine supervisor owns the llama.cpp
//! process, translations run on a worker thread and stream deltas through a channel, and model
//! downloads are a `curl.exe` child process whose progress is read from the `.part` file. A
//! single GPUI task pumps those channels into the entity on every frame that needs it.
//!
//! Text editing uses GPUI Kit's `TextareaState`, language selection uses its `ComboboxState`,
//! and transient messages are pushed as component notifications.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use gpui_kit::component::combobox::{ComboboxEvent, ComboboxState};
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::{h_flex, ActiveTheme, IndexPath, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, AnyWindowHandle, App, ClipboardItem, Context, Entity, ScrollEvent, SharedString, Window,
};
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

use crate::smooth_scroll::SmoothScroller;

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

/// One entry of the searchable language list.
#[derive(Clone)]
pub struct LanguageItem {
    /// Backing catalogue entry.
    pub language: &'static Language,
    /// Language code, used as the combobox value.
    pub code: String,
}

impl SearchableListItem for LanguageItem {
    type Value = String;

    fn title(&self) -> SharedString {
        self.language.name_ko.into()
    }

    fn value(&self) -> &Self::Value {
        &self.code
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim();
        if query.is_empty() {
            return true;
        }
        let lowered = query.to_lowercase();
        self.language.name_ko.contains(query)
            || self.language.name_en.to_lowercase().contains(&lowered)
            || self.code.to_lowercase().contains(&lowered)
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .flex_1()
            .items_center()
            .justify_between()
            .gap_3()
            .child(div().text_sm().child(self.language.name_ko))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} · {}", self.code, self.language.name_en)),
            )
    }
}

/// Every language, with the common pair pinned to the top.
fn language_items() -> Vec<LanguageItem> {
    let mut languages: Vec<&'static Language> = lang::LANGUAGES.iter().collect();
    languages.sort_by_key(|language| {
        let preferred = matches!(language.code, "ko" | "en" | "ja" | "zh");
        (!preferred, language.name_ko)
    });
    languages
        .into_iter()
        .map(|language| LanguageItem {
            language,
            code: language.code.to_string(),
        })
        .collect()
}

/// Which of the app's scrollable containers a wheel event or an eased chase
/// step belongs to. Each one is backed by a [`SmoothScroller`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollSpot {
    /// The models tab page.
    Models,
    /// The llama-server log box on the models tab.
    EngineLog,
    /// The settings tab page.
    Settings,
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
    /// Multiline source editor.
    pub source: Entity<TextareaState>,
    /// Streaming translation result, read-only.
    pub output_state: Entity<TextareaState>,
    /// Glossary editor in settings.
    pub glossary_editor: Entity<TextareaState>,
    /// Source-language picker.
    pub source_combo: Entity<ComboboxState<SearchableVec<LanguageItem>>>,
    /// Target-language picker.
    pub target_combo: Entity<ComboboxState<SearchableVec<LanguageItem>>>,
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
    /// The window this root view lives in, used to raise notifications from the pump task.
    pub window_handle: AnyWindowHandle,
    /// Eased wheel scrolling for the models tab page.
    pub models_scroll: SmoothScroller,
    /// Eased wheel scrolling for the engine log box on the models tab.
    pub engine_log_scroll: SmoothScroller,
    /// Eased wheel scrolling for the settings tab page.
    pub settings_scroll: SmoothScroller,
    /// Messages raised before the notification overlay was ready.
    pending_notices: Vec<(String, bool)>,
    /// True once the notification overlay can receive messages.
    overlay_ready: bool,
    exe_dir: PathBuf,
}

impl Root {
    /// Builds the state, loads settings, scans models and starts the pump task.
    pub fn new(
        window_handle: AnyWindowHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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

        let source = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("번역할 텍스트를 입력하세요…")
                .rows(8)
        });
        let output_state = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("번역 결과가 여기에 표시됩니다.")
                .rows(8)
        });
        let glossary_editor = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("예) GPUI => 지피유아이\n(한 줄에 `원문 => 번역` 하나)")
                .rows(6)
        });

        let source_combo = Self::build_language_combo(&settings.source_lang, window, cx);
        let target_combo = Self::build_language_combo(&settings.target_lang, window, cx);

        if !settings.prompt.glossary.is_empty() {
            let glossary = settings.prompt.glossary.clone();
            glossary_editor.update(cx, |state, cx| state.set_value(glossary, window, cx));
        }

        cx.subscribe_in(
            &source,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter {
                    secondary: true, ..
                } = event
                {
                    this.translate(window, cx);
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &glossary_editor,
            window,
            |this, editor, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.settings.prompt.glossary = editor.read(cx).value().to_string();
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &source_combo,
            window,
            |this, _, event: &ComboboxEvent<SearchableVec<LanguageItem>>, window, cx| {
                this.on_language_event(LangSlot::Source, event, window, cx);
            },
        )
        .detach();
        cx.subscribe_in(
            &target_combo,
            window,
            |this, _, event: &ComboboxEvent<SearchableVec<LanguageItem>>, window, cx| {
                this.on_language_event(LangSlot::Target, event, window, cx);
            },
        )
        .detach();

        let mut root = Self {
            paths,
            settings,
            tab: Tab::Translate,
            source,
            output_state,
            glossary_editor,
            source_combo,
            target_combo,
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
            window_handle,
            models_scroll: SmoothScroller::default(),
            engine_log_scroll: SmoothScroller::default(),
            settings_scroll: SmoothScroller::default(),
            pending_notices: Vec::new(),
            overlay_ready: false,
            exe_dir,
        };

        if let Some(warning) = settings_warning {
            root.notify(warning, true, window, cx);
        }

        if root.settings.server.auto_start {
            root.start_engine(window, cx);
        }

        let window_handle = root.window_handle;
        let task = cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(80))
                .await;
            let alive = window_handle
                .update(cx, |_view, window, cx| {
                    this.update(cx, |root, cx| {
                        if root.tick(window, cx) {
                            cx.notify();
                        }
                    })
                    .is_ok()
                })
                .is_ok();
            if !alive {
                break;
            }
        });
        task.detach();

        root
    }

    fn build_language_combo(
        code: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ComboboxState<SearchableVec<LanguageItem>>> {
        let items = language_items();
        let index = items.iter().position(|item| item.code == code).unwrap_or(0);
        cx.new(|cx| {
            ComboboxState::new(
                SearchableVec::new(items),
                vec![IndexPath::default().row(index)],
                window,
                cx,
            )
        })
    }

    fn on_language_event(
        &mut self,
        slot: LangSlot,
        event: &ComboboxEvent<SearchableVec<LanguageItem>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ComboboxEvent::Change(values) => {
                if let Some(code) = values.last() {
                    self.set_language(slot, code, window, cx);
                }
            }
            ComboboxEvent::Confirm(_) => {
                self.source.update(cx, |state, cx| state.focus(window, cx));
            }
        }
    }

    /// Scroller backing one [`ScrollSpot`].
    fn scroller_mut(&mut self, spot: ScrollSpot) -> &mut SmoothScroller {
        match spot {
            ScrollSpot::Models => &mut self.models_scroll,
            ScrollSpot::EngineLog => &mut self.engine_log_scroll,
            ScrollSpot::Settings => &mut self.settings_scroll,
        }
    }

    /// Feeds a wheel event on one scroll container into its eased chase instead
    /// of letting it move the offset in one instant jump.
    pub fn smooth_scroll(
        &mut self,
        spot: ScrollSpot,
        event: &ScrollEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chasing = {
            let scroller = self.scroller_mut(spot);
            scroller.on_wheel(event, window);
            if !scroller.is_chasing() {
                scroller.set_chasing(true);
                true
            } else {
                false
            }
        };
        if chasing {
            Self::schedule_scroll_tick(cx.entity(), spot, window);
        }
    }

    /// Runs one eased chase step for `spot`, redrawing the view, and keeps
    /// scheduling itself until the chase settles.
    fn scroll_tick(&mut self, spot: ScrollSpot, window: &mut Window, cx: &mut Context<Self>) {
        let more = {
            let scroller = self.scroller_mut(spot);
            let continues = scroller.chase_step();
            scroller.set_chasing(continues);
            continues
        };
        cx.notify();
        if more {
            let entity = cx.entity();
            window.on_next_frame(move |window, cx| {
                entity.update(cx, |root, cx| root.scroll_tick(spot, window, cx));
            });
        }
    }

    /// Queues the first chase step of a container's animation on the next frame.
    fn schedule_scroll_tick(entity: Entity<Self>, spot: ScrollSpot, window: &mut Window) {
        window.on_next_frame(move |window, cx| {
            entity.update(cx, |root, cx| root.scroll_tick(spot, window, cx));
        });
    }

    /// Raises a toast notification, or buffers it until the overlay is ready.
    pub fn notify(
        &mut self,
        text: impl Into<String>,
        is_error: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.into();
        if self.overlay_ready {
            self.push_notification(text, is_error, window, cx);
        } else {
            self.pending_notices.push((text, is_error));
        }
    }

    fn push_notification(
        &mut self,
        text: String,
        is_error: bool,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        let notification = if is_error {
            Notification::error(text)
        } else {
            Notification::info(text)
        };
        window.push_notification(notification, _cx);
    }

    /// Drains background channels. Returns true when the UI needs a redraw.
    pub fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut changed = false;

        if !self.overlay_ready {
            self.overlay_ready = true;
            let pending = std::mem::take(&mut self.pending_notices);
            for (text, is_error) in pending {
                self.push_notification(text, is_error, window, cx);
            }
        }

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
                            self.notify(message.clone(), true, window, cx);
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
                        self.sync_output(window, cx);
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
                                    self.notify("번역을 중단했습니다.", false, window, cx);
                                }
                            }
                            Err(message) => {
                                self.output = message.clone();
                                self.output_is_error = true;
                                self.stats = None;
                                self.notify(message, true, window, cx);
                            }
                        }
                        self.sync_output(window, cx);
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
                                self.notify(
                                    format!(
                                        "{} 다운로드 완료. 모델을 불러옵니다.",
                                        download.option.file_name()
                                    ),
                                    false,
                                    window,
                                    cx,
                                );
                                if self.settings.server.auto_start {
                                    self.restart_engine(window, cx);
                                }
                            }
                            Err(err) => {
                                self.notify(
                                    format!("다운로드한 파일을 옮길 수 없습니다: {err}"),
                                    true,
                                    window,
                                    cx,
                                );
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
                        self.notify(
                            format!(
                                "다운로드가 완료되지 않았습니다 ({detail}). 모델 탭에서 다시 시도하세요."
                            ),
                            true,
                            window,
                            cx,
                        );
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
                    self.notify(format!("다운로드 상태 오류: {err}"), true, window, cx);
                    changed = true;
                }
            }
        }

        changed
    }

    /// Mirrors the translation text into the read-only output editor.
    fn sync_output(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.output.clone();
        self.output_state
            .update(cx, |state, cx| state.set_value(text, window, cx));
    }

    /// Updates the language on one side of the pair.
    pub fn set_language(
        &mut self,
        slot: LangSlot,
        code: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        self.sync_language_combos(window, cx);
        cx.notify();
    }

    /// Swaps source and target languages.
    pub fn swap_languages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        std::mem::swap(
            &mut self.settings.source_lang,
            &mut self.settings.target_lang,
        );
        let _ = self.save_settings();
        self.sync_language_combos(window, cx);
        cx.notify();
    }

    /// Reflects the current language pair in both comboboxes.
    fn sync_language_combos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.settings.source_lang.clone();
        let target = self.settings.target_lang.clone();
        self.source_combo.update(cx, |combo, cx| {
            combo.set_selected_values(&[source], window, cx)
        });
        self.target_combo.update(cx, |combo, cx| {
            combo.set_selected_values(&[target], window, cx)
        });
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
    pub fn translate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.translating {
            return;
        }
        let source = self.source.read(cx).value().to_string();
        if let Err(message) = prompt::validate_source(&source) {
            self.notify(message, true, window, cx);
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
            self.notify(message, true, window, cx);
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
        self.sync_output(window, cx);

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
    pub fn copy_output(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.output.trim().is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(self.output.clone()));
        self.notify("번역문을 복사했습니다.", false, window, cx);
        cx.notify();
    }

    /// Replaces the input with the translation (handy for back-and-forth work).
    pub fn output_to_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.output.trim().is_empty() {
            return;
        }
        let text = self.output.clone();
        self.source
            .update(cx, |state, cx| state.set_value(text, window, cx));
        self.swap_languages(window, cx);
    }

    /// Pastes the clipboard into the source editor.
    pub fn paste_into_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.source
                .update(cx, |state, cx| state.set_value(text, window, cx));
        }
        cx.notify();
    }

    /// Clears both editors.
    pub fn clear_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.translate_rx = None;
        self.translating = false;
        self.source.update(cx, |state, cx| state.clean(window, cx));
        self.output.clear();
        self.output_is_error = false;
        self.stats = None;
        self.sync_output(window, cx);
        cx.notify();
    }

    /// Writes settings to disk.
    pub fn save_settings(&self) -> std::io::Result<()> {
        self.settings.save(&self.paths.settings_file())
    }

    /// Persists settings and reports the result in the UI.
    pub fn save_settings_with_notice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.prompt.glossary = self.glossary_editor.read(cx).value().to_string();
        match self.save_settings() {
            Ok(()) => self.notify("설정을 저장했습니다.", false, window, cx),
            Err(err) => self.notify(
                format!("설정을 저장할 수 없습니다: {err}"),
                true,
                window,
                cx,
            ),
        }
        cx.notify();
    }

    /// Launches the managed llama.cpp server.
    pub fn start_engine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.engine_state.is_busy() {
            return;
        }
        let Some(config) = self.launch_config(window, cx) else {
            return;
        };
        if self.engine.is_none() {
            match Supervisor::spawn(self.paths.engine_log()) {
                Ok(supervisor) => self.engine = Some(supervisor),
                Err(err) => {
                    self.notify(
                        format!("엔진 스레드를 시작할 수 없습니다: {err}"),
                        true,
                        window,
                        cx,
                    );
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
    pub fn restart_engine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(supervisor) = &self.engine {
            supervisor.stop();
        }
        self.engine_state = EngineState::Stopped;
        self.start_engine(window, cx);
    }

    fn launch_config(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<LaunchConfig> {
        let Some(server_path) =
            engine::discover_server(&self.exe_dir, self.settings.server.runtime_path.as_deref())
        else {
            self.notify(
                "llama-server를 찾을 수 없습니다. 모델 탭의 설치 안내를 따라 runtime 폴더에 넣어 주세요.",
                true,
                window,
                cx,
            );
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
            self.notify(
                "사용할 GGUF 모델이 없습니다. 모델 탭에서 Hy-MT2 모델을 내려받으세요.",
                true,
                window,
                cx,
            );
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
    pub fn rescan_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        self.notify(
            format!("모델 {}개를 찾았습니다.", self.models.len()),
            false,
            window,
            cx,
        );
        cx.notify();
    }

    /// Picks a locally installed model.
    pub fn select_model(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.server.model_path = Some(path);
        let _ = self.save_settings();
        self.restart_engine(window, cx);
    }

    /// Starts downloading a catalogue entry with `curl.exe`.
    pub fn start_download(
        &mut self,
        option: &'static ModelOption,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.download.is_some() {
            self.notify("이미 다운로드가 진행 중입니다.", true, window, cx);
            cx.notify();
            return;
        }
        let dest = self.paths.models_dir().join(option.file_name());
        if dest.is_file() {
            self.settings.server.model_path = Some(dest);
            let _ = self.save_settings();
            self.notify("이미 내려받은 모델입니다.", false, window, cx);
            cx.notify();
            return;
        }
        if let Err(err) = fs::create_dir_all(self.paths.models_dir()) {
            self.notify(
                format!("모델 폴더를 만들 수 없습니다: {err}"),
                true,
                window,
                cx,
            );
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
                self.notify(
                    format!("{} 내려받는 중…", option.file_name()),
                    false,
                    window,
                    cx,
                );
            }
            Err(err) => {
                self.notify(
                    format!(
                        "curl.exe를 실행할 수 없습니다: {err}. Windows 11의 기본 curl이 필요합니다."
                    ),
                    true,
                    window,
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// Aborts the running download.
    pub fn cancel_download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut download) = self.download.take() {
            let _ = download.child.kill();
            let _ = download.child.wait();
            self.notify("다운로드를 취소했습니다.", false, window, cx);
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

    /// True when the glossary currently yields at least one pair.
    pub fn glossary_count(&self) -> usize {
        glossary::parse(&self.settings.prompt.glossary).len()
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
            _ => String::new(),
        }
    }
}

fn curl_program() -> &'static str {
    if cfg!(windows) {
        "curl.exe"
    } else {
        "curl"
    }
}
