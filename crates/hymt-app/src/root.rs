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
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::component::{h_flex, ActiveTheme, IndexPath, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, AnyWindowHandle, App, ClipboardItem, Context, Entity, KeyDownEvent, Keystroke,
    ScrollEvent, SharedString, Window,
};
use hymt_core::{
    client::{self, Endpoint, GenerationParams, TranslationResult},
    engine::{self, EngineEvent, EngineState, LaunchConfig, Supervisor},
    glossary, http,
    lang::{self, Language},
    models::{self, DownloadProgress, ModelOption},
    paths::AppPaths,
    prompt::{self, PromptLanguage, PromptMode},
    settings::{AcquisitionMode, Appearance, HotkeyCombo, Settings},
};

use crate::smooth_scroll::SmoothScroller;
use crate::{autostart, capture, hotkey, tray, window_ops};

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

/// Theme choices shown in the settings select.
pub const THEME_MODES: &[(&str, Appearance)] = &[
    ("시스템", Appearance::System),
    ("라이트", Appearance::Light),
    ("다크", Appearance::Dark),
];

/// One entry of the theme select.
#[derive(Clone)]
pub struct ThemeItem {
    /// Korean label shown in the dropdown.
    pub label: &'static str,
    /// Persisted appearance value.
    pub appearance: Appearance,
}

impl SearchableListItem for ThemeItem {
    type Value = Appearance;

    fn title(&self) -> SharedString {
        self.label.into()
    }

    fn value(&self) -> &Self::Value {
        &self.appearance
    }
}

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

/// Builds the theme select with the given appearance preselected.
fn build_theme_select(
    appearance: Appearance,
    window: &mut Window,
    cx: &mut Context<Root>,
) -> Entity<SelectState<SearchableVec<ThemeItem>>> {
    let items = THEME_MODES
        .iter()
        .map(|(label, appearance)| ThemeItem {
            label,
            appearance: *appearance,
        })
        .collect::<Vec<_>>();
    let index = items
        .iter()
        .position(|item| item.appearance == appearance)
        .unwrap_or(0);
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(items),
            Some(IndexPath::default().row(index)),
            window,
            cx,
        )
    })
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

/// Completed selected-text capture from the hotkey path.
struct CaptureEvent {
    /// Generation of the request; results from superseded captures are dropped.
    epoch: u64,
    /// Captured text, or the user-facing error message.
    result: Result<String, String>,
}

/// Recording session for one hotkey binding in the settings tab.
pub struct HotkeyRecorder {
    /// Which binding is being recorded.
    pub slot: hotkey::RecorderSlot,
    /// First press of the double-press pattern, with the time it happened.
    pub first: Option<(HotkeyCombo, Instant)>,
}

/// Minimum delay between two accepted hotkey triggers, so `RegisterHotKey` auto-repeat
/// (holding the chord) does not queue up capture after capture.
const TRIGGER_DEBOUNCE: Duration = Duration::from_millis(400);

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
    /// Theme picker in settings.
    pub theme_select: Entity<SelectState<SearchableVec<ThemeItem>>>,
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
    /// Notification-area icon, present while the tray option is enabled.
    pub tray: Option<tray::Tray>,
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
    /// Global hotkey thread, when it could be started.
    pub hotkey: Option<hotkey::Hotkey>,
    /// Last configuration pushed to the hotkey thread, for change detection.
    hotkey_applied: hotkey::HotkeySpecs,
    /// Latest registration failure, surfaced in the settings tab.
    pub hotkey_error: Option<String>,
    /// Receiver of the in-flight selected-text capture.
    capture_rx: Option<Receiver<CaptureEvent>>,
    /// Generation counter of issued capture jobs.
    capture_epoch: u64,
    /// When the last trigger fired, used to debounce auto-repeat.
    last_trigger: Option<Instant>,
    /// In-flight binding recording in the settings tab.
    pub hotkey_recorder: Option<HotkeyRecorder>,
}

impl Root {
    /// Builds the state, loads settings, scans models and starts the pump task.
    pub fn new(
        window_handle: AnyWindowHandle,
        window: &mut Window,
        start_hidden: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        if start_hidden {
            // The window was created with its final bounds; hide it before the first frame so
            // a sign-in launch goes straight to the notification area.
            let _ = window_ops::hide(window);
        }

        let paths = AppPaths::detect();
        let _ = paths.ensure();
        let (settings, settings_warning) = Settings::load_or_default(&paths.settings_file());

        // Create the tray icon before the model scan so a hidden sign-in launch has its
        // notification-area entry as early as possible.
        let (tray, tray_error) = if settings.desktop.keep_in_tray {
            match tray::Tray::new() {
                Ok(tray) => (Some(tray), None),
                Err(err) => (None, Some(err)),
            }
        } else {
            (None, None)
        };
        let startup_error = if settings.desktop.start_with_windows {
            autostart::set_enabled(true)
                .err()
                .map(|err| format!("Windows 시작 프로그램을 갱신할 수 없습니다: {err}"))
        } else {
            None
        };

        if start_hidden && tray.is_none() {
            // The tray option is on but the icon could not be created: never leave the app
            // running with no way to reach it.
            window_ops::show(window);
        }

        let models = engine::scan_gguf(&paths.models_dir());
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| paths.root().to_path_buf());
        let server_path =
            engine::discover_server(&exe_dir, settings.server.runtime_path.as_deref());

        let hotkey_spec = hotkey::parse_specs(&settings.hotkey);
        let (hotkey, hotkey_spawn_error) = match hotkey::Hotkey::start(hotkey_spec) {
            Ok(hotkey) => (Some(hotkey), None),
            Err(error) => (
                None,
                Some(format!("단축키 스레드를 시작할 수 없습니다: {error}")),
            ),
        };

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
        let theme_select = build_theme_select(settings.appearance, window, cx);

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
        cx.subscribe_in(
            &theme_select,
            window,
            |this, _, event: &SelectEvent<SearchableVec<ThemeItem>>, _, cx| {
                if let SelectEvent::Confirm(Some(appearance)) = event {
                    this.set_appearance(*appearance, cx);
                }
            },
        )
        .detach();

        // Follow the Windows appearance while the theme is set to System.
        window
            .observe_window_appearance({
                let root = cx.entity();
                move |_, cx| {
                    root.update(cx, |root, cx| root.on_system_appearance_changed(cx));
                }
            })
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
            theme_select,
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
            tray,
            window_handle,
            models_scroll: SmoothScroller::default(),
            engine_log_scroll: SmoothScroller::default(),
            settings_scroll: SmoothScroller::default(),
            pending_notices: Vec::new(),
            overlay_ready: false,
            exe_dir,
            hotkey,
            hotkey_applied: hotkey_spec,
            hotkey_error: None,
            capture_rx: None,
            capture_epoch: 0,
            last_trigger: None,
            hotkey_recorder: None,
        };

        if let Some(warning) = settings_warning {
            root.notify(warning, true, window, cx);
        }
        if let Some(error) = tray_error {
            root.notify(
                format!("트레이 아이콘을 만들 수 없습니다: {error}"),
                true,
                window,
                cx,
            );
        }
        if let Some(error) = startup_error {
            root.notify(error, true, window, cx);
        }
        if let Some(error) = hotkey_spawn_error {
            root.notify(error, true, window, cx);
        }

        let entity = cx.entity();
        window.on_window_should_close(cx, move |window, cx| {
            entity.read(cx).should_close_window(window)
        });

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

        if let Some(command) = self.poll_tray(window, cx) {
            if command == tray::TrayCommand::Quit {
                return true;
            }
            changed = true;
        }

        if !self.overlay_ready {
            self.overlay_ready = true;
            let pending = std::mem::take(&mut self.pending_notices);
            for (text, is_error) in pending {
                self.push_notification(text, is_error, window, cx);
            }
        }

        // Hotkey events. The busy guard keeps a held chord (auto-repeat) or a press
        // during a running translation from queuing duplicate captures.
        let mut hotkey_events = Vec::new();
        if let Some(hotkey) = &self.hotkey {
            while let Some(event) = hotkey.poll() {
                hotkey_events.push(event);
            }
        }
        for event in hotkey_events {
            match event {
                hotkey::HotkeyEvent::Registration(outcome) => {
                    let failure = outcome.as_ref().err().cloned();
                    if failure != self.hotkey_error {
                        self.hotkey_error = failure;
                        changed = true;
                    }
                }
                hotkey::HotkeyEvent::Triggered { clipboard_seq } => {
                    let throttled = self
                        .last_trigger
                        .is_some_and(|moment| moment.elapsed() < TRIGGER_DEBOUNCE);
                    if throttled || self.capture_rx.is_some() {
                        continue;
                    }
                    self.last_trigger = Some(Instant::now());
                    // The streaming translation must be visible, whatever tab the user
                    // left the window on.
                    self.tab = Tab::Translate;
                    if window_ops::foreground_is_own(window) {
                        // Our own window holds focus; capturing "another app's selection"
                        // would read our editor. Re-translate the current text instead.
                        let text = self.source.read(cx).value().to_string();
                        self.translate_source(text, true, window, cx);
                        changed = true;
                        continue;
                    }
                    let (sender, receiver) = mpsc::channel::<CaptureEvent>();
                    self.capture_epoch += 1;
                    let epoch = self.capture_epoch;
                    let mode = self.settings.hotkey.acquisition;
                    self.capture_rx = Some(receiver);
                    thread::Builder::new()
                        .name("hymt-capture".to_string())
                        .spawn(move || {
                            let result = capture::capture_selected(mode, clipboard_seq);
                            let _ = sender.send(CaptureEvent { epoch, result });
                        })
                        .ok();
                    changed = true;
                }
            }
        }

        // Finished captures: show the window, fill the editor, start translating.
        if let Some(receiver) = self.capture_rx.take() {
            let (event, keep) = drain_latest_capture(receiver, self.capture_epoch);
            self.capture_rx = keep;
            if let Some(event) = event {
                self.on_capture_done(event.result, window, cx);
                changed = true;
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

    /// One-line status of the hotkey feature shown in its settings card.
    ///
    /// The bool marks errors, which the UI paints in the danger color.
    pub fn hotkey_status_text(&self) -> (String, bool) {
        if let Some(error) = &self.hotkey_error {
            return (error.clone(), true);
        }
        if !self.settings.hotkey.enabled {
            return (
                "전역 단축키 기능이 꺼져 있습니다. 위 스위치로 켤 수 있습니다.".to_string(),
                false,
            );
        }
        let mut triggers = Vec::new();
        if let Some(combo) = &self.settings.hotkey.combo {
            triggers.push(format!("조합 {}", hotkey::format_combo(combo)));
        }
        if let Some(tap) = &self.settings.hotkey.double_tap {
            triggers.push(hotkey::format_double_tap(tap));
        }
        if triggers.is_empty() {
            (
                "등록된 트리거가 없습니다. 기록 버튼으로 등록하세요.".to_string(),
                false,
            )
        } else {
            (format!("등록됨: {}", triggers.join(" · ")), false)
        }
    }

    /// Applies a finished hotkey capture: surface the window, load the text and start
    /// the automatic translation.
    fn on_capture_done(
        &mut self,
        result: Result<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window_ops::show(window);
        // Bring the translation result into view, whatever tab the window had open.
        self.tab = Tab::Translate;
        match result {
            Ok(text) => {
                self.source
                    .update(cx, |state, cx| state.set_value(text.clone(), window, cx));
                self.translate_source(text, true, window, cx);
            }
            Err(message) => self.notify(message, true, window, cx),
        }
        cx.notify();
    }

    /// Pushes the current hotkey settings to the thread, deduplicating no-op writes.
    pub(crate) fn sync_hotkey(&mut self) {
        let specs = hotkey::parse_specs(&self.settings.hotkey);
        if self.hotkey_applied == specs {
            return;
        }
        self.hotkey_applied = specs;
        if let Some(hotkey) = &self.hotkey {
            hotkey.update(specs);
        }
    }

    /// Turns the global-hotkey master switch on or off.
    pub fn set_hotkey_enabled(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.hotkey.enabled = enabled;
        let _ = self.save_settings();
        self.sync_hotkey();
        self.notify(
            if enabled {
                "전역 단축키를 켰습니다. 다른 앱에서 선택한 텍스트를 번역할 수 있습니다."
            } else {
                "전역 단축키를 껐습니다."
            },
            false,
            window,
            cx,
        );
        cx.notify();
    }

    /// Changes how the trigger reads the focused application's selection.
    pub fn set_hotkey_acquisition(
        &mut self,
        mode: AcquisitionMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.hotkey.acquisition == mode {
            return;
        }
        self.settings.hotkey.acquisition = mode;
        let _ = self.save_settings();
        self.notify(
            match mode {
                AcquisitionMode::Auto => {
                    "UI Automation을 먼저 읽고, 지원하지 않는 앱에서는 Ctrl+C로 대체합니다."
                }
                AcquisitionMode::ClipboardOnly => "항상 Ctrl+C로 복사한 뒤 클립보드에서 읽습니다.",
            },
            false,
            window,
            cx,
        );
        cx.notify();
    }

    /// Starts (or stops) recording a binding in the settings tab.
    pub fn set_hotkey_recorder(
        &mut self,
        slot: Option<hotkey::RecorderSlot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match slot {
            Some(slot) => {
                self.hotkey_recorder = Some(HotkeyRecorder { slot, first: None });
                self.notify("등록할 단축키 조합을 눌러 주세요.", false, window, cx);
            }
            None => {
                if self.hotkey_recorder.is_some() {
                    self.notify("단축키 기록을 취소했습니다.", false, window, cx);
                }
                self.hotkey_recorder = None;
            }
        }
        cx.notify();
    }

    /// Consumes one key press of a recording session.
    ///
    /// Chords commit on the first full combination; the double press commits when the
    /// same modifier-key pair arrives twice within [`hotkey::DOUBLE_TAP_GAP_MS`].
    pub fn record_hotkey_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.clone();
        if key == "escape" {
            self.set_hotkey_recorder(None, window, cx);
            return;
        }
        if matches!(key.as_str(), "control" | "alt" | "shift" | "meta") {
            // Modifier-only keydown: wait for the actual key.
            return;
        }
        let Some(combo) = combo_from_keystroke(&event.keystroke) else {
            self.notify(
                "지원하지 않는 키입니다. 문자·숫자·기호, F1–F24, Space·Tab·Enter 중에서 눌러 주세요.",
                true,
                window,
                cx,
            );
            cx.notify();
            return;
        };
        if !combo.has_modifier() {
            self.notify(
                "Ctrl·Shift·Alt·Win 중 하나를 함께 눌러 주세요.",
                true,
                window,
                cx,
            );
            cx.notify();
            return;
        }
        let Some(slot) = self.hotkey_recorder.as_ref().map(|recorder| recorder.slot) else {
            return;
        };
        match slot {
            hotkey::RecorderSlot::Chord => {
                self.hotkey_recorder = None;
                self.settings.hotkey.combo = Some(combo.clone());
                let _ = self.save_settings();
                self.sync_hotkey();
                self.notify(
                    format!(
                        "조합 단축키를 등록했습니다: {}",
                        hotkey::format_combo(&combo)
                    ),
                    false,
                    window,
                    cx,
                );
            }
            hotkey::RecorderSlot::DoubleTap => {
                let first = self
                    .hotkey_recorder
                    .as_mut()
                    .and_then(|recorder| recorder.first.take());
                match first {
                    Some((first, at)) if first == combo => {
                        let gap = at.elapsed().as_millis() as u64;
                        if gap <= hotkey::DOUBLE_TAP_GAP_MS + hotkey::DOUBLE_TAP_RECORD_SLACK_MS {
                            self.hotkey_recorder = None;
                            self.settings.hotkey.double_tap = Some(combo.clone());
                            let _ = self.save_settings();
                            self.sync_hotkey();
                            self.notify(
                                format!(
                                    "두 번 누르기 단축키를 등록했습니다: {}",
                                    hotkey::format_double_tap(&combo)
                                ),
                                false,
                                window,
                                cx,
                            );
                        } else {
                            self.restart_double_tap_recording(combo);
                            self.notify(
                                "간격이 너무 늦었습니다. 다시 두 번 눌러 주세요.",
                                true,
                                window,
                                cx,
                            );
                        }
                    }
                    _ => {
                        self.restart_double_tap_recording(combo);
                        self.notify("같은 키를 한 번 더 눌러 주세요.", false, window, cx);
                    }
                }
            }
        }
        cx.notify();
    }

    /// Stores the first press of a double-press recording attempt.
    fn restart_double_tap_recording(&mut self, combo: HotkeyCombo) {
        if let Some(recorder) = self.hotkey_recorder.as_mut() {
            recorder.first = Some((combo, Instant::now()));
        }
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

    /// Starts a streaming translation of the current input (manual path).
    pub fn translate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.source.read(cx).value().to_string();
        self.translate_source(source, false, window, cx);
    }

    /// Starts a streaming translation of `source`.
    ///
    /// `force` replaces an already-running translation instead of ignoring the call, which
    /// is what the hotkey path wants on repeated triggers. The manual button keeps the
    /// forgiving behavior.
    pub fn translate_source(
        &mut self,
        source: String,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.translating && !force {
            return;
        }
        if let Err(message) = prompt::validate_source(&source) {
            self.notify(message, true, window, cx);
            cx.notify();
            return;
        }
        if self.translating {
            // Replace the in-flight job: cancel its worker and drop its events.
            if let Some(cancel) = self.cancel.take() {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            self.translate_rx = None;
            self.translating = false;
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

    /// Handles the tray icon's click and menu. Returns the command that was consumed.
    fn poll_tray(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<tray::TrayCommand> {
        let command = self.tray.as_ref()?.poll()?;
        match command {
            tray::TrayCommand::Show => {
                window_ops::show(window);
                self.source.update(cx, |state, cx| state.focus(window, cx));
            }
            tray::TrayCommand::Quit => {
                self.stop_engine(cx);
                self.tray = None;
                cx.quit();
            }
        }
        Some(command)
    }

    /// True when the window should actually close instead of hiding into the tray.
    fn should_close_window(&self, window: &mut Window) -> bool {
        !(self.settings.desktop.keep_in_tray && self.tray.is_some() && window_ops::hide(window))
    }

    /// Enables or disables the notification-area icon.
    pub fn set_tray_enabled(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        if enabled {
            if self.tray.is_none() {
                match tray::Tray::new() {
                    Ok(tray) => self.tray = Some(tray),
                    Err(err) => {
                        self.notify(
                            format!("트레이 아이콘을 만들 수 없습니다: {err}"),
                            true,
                            window,
                            cx,
                        );
                        cx.notify();
                        return;
                    }
                }
            }
            self.settings.desktop.keep_in_tray = true;
            self.notify("창을 닫으면 트레이로 숨깁니다.", false, window, cx);
        } else {
            self.tray = None;
            self.settings.desktop.keep_in_tray = false;
            self.notify("트레이 아이콘을 껐습니다.", false, window, cx);
        }
        let _ = self.save_settings();
        cx.notify();
    }

    /// Adds or removes the app from the current user's Windows startup entries.
    pub fn set_start_with_windows(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match autostart::set_enabled(enabled) {
            Ok(()) => {
                self.settings.desktop.start_with_windows = enabled;
                let message = if enabled {
                    "Windows 시작 시 자동 실행을 켰습니다."
                } else {
                    "Windows 시작 시 자동 실행을 껐습니다."
                };
                self.notify(message, false, window, cx);
                let _ = self.save_settings();
            }
            Err(err) => self.notify(
                format!("시작 프로그램 설정을 변경할 수 없습니다: {err}"),
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

/// Drains finished capture events, keeping still-running channels subscribed and dropping
/// the results of superseded captures.
fn drain_latest_capture(
    receiver: Receiver<CaptureEvent>,
    epoch: u64,
) -> (Option<CaptureEvent>, Option<Receiver<CaptureEvent>>) {
    let mut result = None;
    let mut keep = None;
    loop {
        match receiver.try_recv() {
            Ok(event) => {
                if event.epoch == epoch {
                    result = Some(event);
                    break;
                }
            }
            Err(TryRecvError::Empty) => {
                keep = Some(receiver);
                break;
            }
            Err(TryRecvError::Disconnected) => break,
        }
    }
    (result, keep)
}

/// Builds a persisted binding out of a recorded key press.
///
/// Keys gpui presents with names outside of our supported set (arrows, page keys, …) never
/// resolve, so the recorder can tell the user to pick a supported key.
fn combo_from_keystroke(keystroke: &Keystroke) -> Option<HotkeyCombo> {
    let key = hymt_core::settings::normalize_hotkey_key(&keystroke.key);
    if !hymt_core::settings::is_supported_key(&key) {
        return None;
    }
    Some(HotkeyCombo {
        ctrl: keystroke.modifiers.control,
        shift: keystroke.modifiers.shift,
        alt: keystroke.modifiers.alt,
        win: keystroke.modifiers.platform,
        key,
    })
}
