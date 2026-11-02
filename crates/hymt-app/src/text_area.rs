//! A multi-line, soft-wrapping text editor for GPUI.
//!
//! GPUI 0.2 ships no text input widget, so this module provides one. It follows the structure
//! of GPUI's own `examples/input.rs`: an `EntityInputHandler` entity plus a custom `Element`
//! that shapes and paints the text. On top of that example it adds:
//!
//! * soft wrapping (via [`crate::wrap`]) measured with `ShapedLine::x_for_index`,
//! * selection painted through `TextRun::background_color`,
//! * vertical caret movement across visual lines,
//! * internal scrolling with caret following and mouse-wheel support.
//!
//! Text entry (including Korean IME composition) is routed through `EntityInputHandler`, which
//! is what GPUI's Windows backend uses for `WM_CHAR` and `WM_IME_COMPOSITION`; this widget
//! therefore must not insert `key_char` itself.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    div, fill, point, prelude::*, px, relative, rgb, size, App, Bounds, ClipboardItem, ContentMask,
    Context, CursorStyle, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    FocusHandle, Focusable, GlobalElementId, InspectorElementId, IntoElement, KeyDownEvent,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Point, Render,
    ScrollDelta, ScrollWheelEvent, ShapedLine, SharedString, Style, TextRun, UTF16Selection,
    Window,
};

use crate::{theme, wrap};

/// Inner padding of the editor, in pixels.
const PADDING: f32 = 14.0;
/// Minimum width used when measuring, so a very narrow panel still wraps.
const MIN_WIDTH: f32 = 40.0;

/// One visual (wrapped) line of shaped text.
#[derive(Clone)]
struct VisualLine {
    /// Byte range inside the full content.
    range: Range<usize>,
    /// Shaped text for exactly that range.
    shaped: ShapedLine,
}

/// Cache key describing everything the layout depends on.
#[derive(Clone, Debug, PartialEq)]
struct LayoutKey {
    text: SharedString,
    width_bits: u32,
    font_size_bits: u32,
    selection: Range<usize>,
    marked: Option<Range<usize>>,
}

/// Everything needed to paint and hit-test the editor.
struct Layout {
    key: LayoutKey,
    lines: Vec<VisualLine>,
    origin: Point<gpui::Pixels>,
    line_height: f32,
    scroll_y: f32,
    total_height: f32,
    caret_x: f32,
    caret_y: f32,
    viewport_height: f32,
}

/// Multi-line editable text.
pub struct TextArea {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    is_selecting: bool,
    multiline: bool,
    scroll_y: f32,
    layout: Rc<RefCell<Option<Layout>>>,
}

impl TextArea {
    /// Creates an empty editor.
    pub fn new(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        multiline: bool,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            is_selecting: false,
            multiline,
            scroll_y: 0.0,
            layout: Rc::new(RefCell::new(None)),
        }
    }

    /// Current text.
    pub fn content(&self) -> SharedString {
        self.content.clone()
    }

    /// Replaces the content and places the caret at the end.
    pub fn set_content(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.scroll_y = 0.0;
        cx.notify();
    }

    /// Empties the editor.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_content("", cx);
    }

    /// Focuses the editor.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selection_reversed = false;
        self.selected_range = 0..self.content.len();
        cx.notify();
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        let mut previous = 0;
        for (index, _) in self.content.char_indices() {
            if index >= offset {
                break;
            }
            previous = index;
        }
        previous
    }

    fn next_boundary(&self, offset: usize) -> usize {
        for (index, ch) in self.content.char_indices() {
            if index > offset {
                return index;
            }
            if index == offset {
                return index + ch.len_utf8();
            }
        }
        self.content.len()
    }

    fn replace_range(&mut self, range: Range<usize>, text: &str) -> usize {
        let start = range.start.min(self.content.len());
        let end = range.end.clamp(start, self.content.len());
        let mut content = String::with_capacity(self.content.len() - (end - start) + text.len());
        content.push_str(&self.content[..start]);
        content.push_str(text);
        content.push_str(&self.content[end..]);
        self.content = content.into();
        start + text.len()
    }

    fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let range = self
            .marked_range
            .clone()
            .unwrap_or_else(|| self.selected_range.clone());
        let cursor = self.replace_range(range, text);
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn backspace(&mut self, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let start = self.previous_boundary(self.cursor_offset());
            self.selected_range = start..self.cursor_offset();
        }
        self.insert_text("", cx);
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let end = self.next_boundary(self.cursor_offset());
            self.selected_range = self.cursor_offset()..end;
        }
        self.insert_text("", cx);
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            self.copy(cx);
            self.insert_text("", cx);
        }
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = if self.multiline {
                text.replace("\r\n", "\n")
            } else {
                text.replace(['\r', '\n'], " ")
            };
            self.insert_text(&text, cx);
        }
    }

    fn move_horizontal(&mut self, forward: bool, select: bool, cx: &mut Context<Self>) {
        let offset = if select {
            if forward {
                self.next_boundary(self.cursor_offset())
            } else {
                self.previous_boundary(self.cursor_offset())
            }
        } else if self.selected_range.is_empty() {
            if forward {
                self.next_boundary(self.cursor_offset())
            } else {
                self.previous_boundary(self.cursor_offset())
            }
        } else if forward {
            self.selected_range.end
        } else {
            self.selected_range.start
        };
        if select {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    fn move_vertical(&mut self, delta: isize, select: bool, cx: &mut Context<Self>) {
        if !self.multiline {
            return;
        }
        let target = {
            let layout_ref = self.layout.borrow();
            let Some(layout) = layout_ref.as_ref() else {
                return;
            };
            if layout.lines.is_empty() {
                return;
            }
            let cursor = self.cursor_offset();
            let ranges: Vec<Range<usize>> =
                layout.lines.iter().map(|line| line.range.clone()).collect();
            let index = wrap::visual_line_for_cursor(&ranges, cursor).min(layout.lines.len() - 1);
            let line = &layout.lines[index];
            let local = cursor
                .saturating_sub(line.range.start)
                .min(line.range.end - line.range.start);
            let x = line.shaped.x_for_index(local);
            let next = (index as isize + delta).clamp(0, layout.lines.len() as isize - 1) as usize;
            let line = &layout.lines[next];
            let within = line
                .shaped
                .closest_index_for_x(x)
                .min(line.range.end - line.range.start);
            line.range.start + within
        };
        if select {
            self.select_to(target, cx);
        } else {
            self.move_to(target, cx);
        }
    }

    fn line_boundary(&mut self, end: bool, select: bool, cx: &mut Context<Self>) {
        let offset = if self.multiline {
            wrap::logical_line_boundary(&self.content, self.cursor_offset(), end)
        } else if end {
            self.content.len()
        } else {
            0
        };
        if select {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    fn index_for_mouse_position(&self, position: Point<gpui::Pixels>) -> usize {
        let layout_ref = self.layout.borrow();
        let Some(layout) = layout_ref.as_ref() else {
            return self.content.len();
        };
        if layout.lines.is_empty() {
            return 0;
        }
        let y = f32::from(position.y - layout.origin.y) + layout.scroll_y;
        let index = ((y / layout.line_height.max(1.0)).floor().max(0.0) as usize)
            .min(layout.lines.len() - 1);
        let line = &layout.lines[index];
        let x = f32::from(position.x - layout.origin.x).max(0.0);
        let within = line
            .shaped
            .closest_index_for_x(px(x))
            .min(line.range.end - line.range.start);
        // When the content is empty the layout was built from the placeholder
        // text, so hit-tested offsets can exceed the real content length.
        (line.range.start + within).min(self.content.len())
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        let offset = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let offset = self.index_for_mouse_position(event.position);
            self.select_to(offset, cx);
        }
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line_height = self
            .layout
            .borrow()
            .as_ref()
            .map(|layout| layout.line_height)
            .unwrap_or(20.0);
        let delta = match &event.delta {
            ScrollDelta::Pixels(delta) => f32::from(delta.y),
            ScrollDelta::Lines(delta) => delta.y * line_height,
        };
        let max_scroll = self
            .layout
            .borrow()
            .as_ref()
            .map(|layout| (layout.total_height - layout.viewport_height).max(0.0))
            .unwrap_or(0.0);
        self.scroll_y = (self.scroll_y - delta).clamp(0.0, max_scroll);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.to_ascii_lowercase();
        let modifiers = event.keystroke.modifiers;
        let command = modifiers.control || modifiers.platform;

        if command {
            match key.as_str() {
                "a" => self.select_all(cx),
                "c" => self.copy(cx),
                "x" => self.cut(cx),
                "v" => self.paste(cx),
                _ => {}
            }
            return;
        }

        match key.as_str() {
            "backspace" => self.backspace(cx),
            "delete" => self.delete(cx),
            "left" => self.move_horizontal(false, modifiers.shift, cx),
            "right" => self.move_horizontal(true, modifiers.shift, cx),
            "up" => self.move_vertical(-1, modifiers.shift, cx),
            "down" => self.move_vertical(1, modifiers.shift, cx),
            "home" => self.line_boundary(false, modifiers.shift, cx),
            "end" => self.line_boundary(true, modifiers.shift, cx),
            "enter" if self.multiline => {
                self.insert_text("\n", cx);
                // Keep the viewport on the new line.
                window.refresh();
            }
            _ => {}
        }
    }
}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let cursor = self.replace_range(range, new_text);
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let start = range.start;
        let cursor = self.replace_range(range, new_text);
        self.marked_range = (!new_text.is_empty()).then(|| start..start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .map(|range| start + range.start..start + range.end)
            .unwrap_or(cursor..cursor);
        self.selection_reversed = false;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<gpui::Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let layout_ref = self.layout.borrow();
        let layout = layout_ref.as_ref()?;
        let index = wrap::visual_line_for_cursor(
            &layout
                .lines
                .iter()
                .map(|line| line.range.clone())
                .collect::<Vec<_>>(),
            range.end,
        );
        let _line = layout.lines.get(index)?;
        let y = layout.origin.y + px(index as f32 * layout.line_height) - px(layout.scroll_y);
        Some(Bounds::new(
            point(layout.origin.x, y),
            size(bounds.size.width, px(layout.line_height)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.offset_to_utf16(self.index_for_mouse_position(point)))
    }
}

impl Render for TextArea {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();
        div()
            .size_full()
            .track_focus(&focus_handle)
            .key_context("HyTextArea")
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .child(TextAreaElement { input: cx.entity() })
    }
}

struct TextAreaElement {
    input: Entity<TextArea>,
}

impl IntoElement for TextAreaElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct PrepaintState {
    lines: Vec<VisualLine>,
    origin: Point<gpui::Pixels>,
    line_height: f32,
    scroll_y: f32,
    caret: Option<PaintQuad>,
}

impl Element for TextAreaElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selection = input.selected_range.clone();
        let marked = input.marked_range.clone();
        let placeholder = input.placeholder.clone();
        let cursor = if input.selection_reversed {
            input.selected_range.start
        } else {
            input.selected_range.end
        };
        let requested_scroll = input.scroll_y;
        let layout_handle = input.layout.clone();

        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let line_height_f32 = f32::from(line_height);
        let origin = point(bounds.origin.x + px(PADDING), bounds.origin.y + px(PADDING));
        let content_width = (f32::from(bounds.size.width) - PADDING * 2.0).max(MIN_WIDTH);
        let viewport_height = (f32::from(bounds.size.height) - PADDING * 2.0).max(line_height_f32);

        let display_text: SharedString = if content.is_empty() {
            placeholder
        } else {
            content.clone()
        };
        let key = LayoutKey {
            text: display_text.clone(),
            width_bits: content_width.to_bits(),
            font_size_bits: f32::from(font_size).to_bits(),
            selection: if content.is_empty() {
                0..0
            } else {
                selection.clone()
            },
            marked: if content.is_empty() {
                None
            } else {
                marked.clone()
            },
        };

        let rebuild = layout_handle
            .borrow()
            .as_ref()
            .map(|layout| layout.key != key)
            .unwrap_or(true);

        let (lines, total_height, caret_x, caret_y) = if rebuild {
            let selection = if content.is_empty() {
                0..0
            } else {
                selection.clone()
            };
            let (lines, total_height, caret_x, caret_y) = build_layout(
                window,
                &display_text,
                &selection,
                cursor,
                content_width,
                line_height,
                if content.is_empty() {
                    theme::PLACEHOLDER
                } else {
                    theme::TEXT
                },
            );
            (lines, total_height, caret_x, caret_y)
        } else {
            let layout_ref = layout_handle.borrow();
            let layout = layout_ref.as_ref().expect("layout exists");
            (
                layout.lines.clone(),
                layout.total_height,
                layout.caret_x,
                layout.caret_y,
            )
        };

        // Caret following.
        let max_scroll = (total_height - viewport_height).max(0.0);
        let mut scroll_y = requested_scroll;
        if caret_y < scroll_y {
            scroll_y = caret_y;
        } else if caret_y + line_height_f32 > scroll_y + viewport_height {
            scroll_y = caret_y + line_height_f32 - viewport_height;
        }
        scroll_y = scroll_y.clamp(0.0, max_scroll);
        if (scroll_y - requested_scroll).abs() > 0.5 {
            self.input.update(cx, |input, _| input.scroll_y = scroll_y);
        }

        let focused = self.input.read(cx).focus_handle.is_focused(window);
        let caret_quad = focused.then(|| {
            fill(
                Bounds::new(
                    point(
                        origin.x + px(caret_x),
                        origin.y + px(caret_y) - px(scroll_y),
                    ),
                    size(px(2.0), line_height),
                ),
                rgb(theme::CARET),
            )
        });

        if rebuild {
            layout_handle.replace(Some(Layout {
                key,
                lines: lines.clone(),
                origin,
                line_height: line_height_f32,
                scroll_y,
                total_height,
                caret_x,
                caret_y,
                viewport_height,
            }));
        } else if let Some(layout) = layout_handle.borrow_mut().as_mut() {
            layout.origin = origin;
            layout.scroll_y = scroll_y;
            layout.viewport_height = viewport_height;
            layout.caret_x = caret_x;
            layout.caret_y = caret_y;
        }

        PrepaintState {
            lines,
            origin,
            line_height: line_height_f32,
            scroll_y,
            caret: caret_quad,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );

        let state = std::mem::replace(
            prepaint,
            PrepaintState {
                lines: Vec::new(),
                origin: point(px(0.0), px(0.0)),
                line_height: 0.0,
                scroll_y: 0.0,
                caret: None,
            },
        );

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (index, line) in state.lines.iter().enumerate() {
                let y = state.origin.y + px(index as f32 * state.line_height) - px(state.scroll_y);
                if line
                    .shaped
                    .paint(point(state.origin.x, y), px(state.line_height), window, cx)
                    .is_err()
                {
                    break;
                }
            }
            if let Some(caret) = state.caret {
                window.paint_quad(caret);
            }
        });
    }
}

fn build_layout(
    window: &mut Window,
    text: &SharedString,
    selection: &Range<usize>,
    cursor: usize,
    width: f32,
    line_height: gpui::Pixels,
    color: u32,
) -> (Vec<VisualLine>, f32, f32, f32) {
    let text_style = window.text_style();
    let font_size = text_style.font_size.to_pixels(window.rem_size());
    let base_run = text_style.to_run(0);
    let selection_color = rgb(theme::SELECTION);
    let text_color = rgb(color);

    let mut logical_ranges: Vec<Range<usize>> = Vec::new();
    let mut start = 0usize;
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            logical_ranges.push(start..index);
            start = index + 1;
        }
    }
    logical_ranges.push(start..text.len());

    let mut lines: Vec<VisualLine> = Vec::new();
    for range in logical_ranges {
        let slice_text: SharedString = text[range.clone()].to_string().into();
        let probe_runs = vec![TextRun {
            len: slice_text.len(),
            ..base_run.clone()
        }];
        let slices = if width > MIN_WIDTH {
            let probe =
                window
                    .text_system()
                    .shape_line(slice_text.clone(), font_size, &probe_runs, None);
            let probe_len = slice_text.len();
            let measure = |offset: usize| {
                let local = offset.min(probe_len);
                f32::from(probe.x_for_index(local))
            };
            wrap::wrap_line(&slice_text, range.start, width, measure)
        } else {
            vec![range.clone()]
        };

        for slice in slices {
            let line_text: SharedString = text[slice.clone()].to_string().into();
            let runs = runs_for(
                &base_run,
                slice.start,
                slice.end,
                selection,
                selection_color.into(),
                text_color.into(),
            );
            let shaped = window
                .text_system()
                .shape_line(line_text, font_size, &runs, None);
            lines.push(VisualLine {
                range: slice,
                shaped,
            });
        }
    }

    if lines.is_empty() {
        let runs = vec![base_run.clone()];
        let shaped = window
            .text_system()
            .shape_line("".into(), font_size, &runs, None);
        lines.push(VisualLine {
            range: 0..0,
            shaped,
        });
    }

    let line_height_f32 = f32::from(line_height);
    let total_height = lines.len() as f32 * line_height_f32;

    let (caret_x, caret_y) = caret_position(&lines, cursor.min(text.len()), line_height_f32);

    (lines, total_height, caret_x, caret_y)
}

fn caret_position(lines: &[VisualLine], cursor: usize, line_height: f32) -> (f32, f32) {
    if lines.is_empty() {
        return (0.0, 0.0);
    }
    let ranges: Vec<Range<usize>> = lines.iter().map(|line| line.range.clone()).collect();
    let index = wrap::visual_line_for_cursor(&ranges, cursor).min(lines.len() - 1);
    let line = &lines[index];
    let local = cursor
        .saturating_sub(line.range.start)
        .min(line.range.end - line.range.start);
    (
        f32::from(line.shaped.x_for_index(local)),
        index as f32 * line_height,
    )
}

fn runs_for(
    prototype: &TextRun,
    start: usize,
    end: usize,
    selection: &Range<usize>,
    selection_color: gpui::Hsla,
    text_color: gpui::Hsla,
) -> Vec<TextRun> {
    let len = end - start;
    let mut base = TextRun {
        len,
        ..prototype.clone()
    };
    base.color = text_color;
    if selection.is_empty() {
        return vec![base];
    }
    let selected_start = selection.start.max(start);
    let selected_end = selection.end.min(end);
    if selected_start >= selected_end {
        return vec![base];
    }
    let head = selected_start - start;
    let middle = selected_end - selected_start;
    let tail = end - selected_end;

    let mut runs = Vec::new();
    if head > 0 {
        runs.push(TextRun {
            len: head,
            ..base.clone()
        });
    }
    runs.push(TextRun {
        len: middle,
        background_color: Some(selection_color),
        ..base.clone()
    });
    if tail > 0 {
        runs.push(TextRun { len: tail, ..base });
    }
    runs
}
