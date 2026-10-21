//! Text field that wraps its text and grows to a number of lines, then
//! scrolls inside itself. It registers an IME input handler while focused,
//! which enables Wayland text-input-v3 and so Denial's on-screen keyboard.
//!
//! Adapted from zed's crates/gpui/examples/input.rs (Apache-2.0).

use std::ops::Range;

use denial_look::Look;
use denial_look_gpui::{color, dp, solid};
use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, Context, CursorStyle, ElementId,
    ElementInputHandler, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId,
    KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    Pixels, Point, ScrollHandle, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle,
    Window, WrappedLine, actions, div, fill, point, prelude::*, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Enter,
        Submit,
    ]
);

/// Height of one line of text, in dp.
pub const LINE: f32 = 22.0;
/// Size of the text, in dp.
const TEXT: f32 = 16.0;

pub fn bind_keys(cx: &mut App) {
    let context = Some("TextInput");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("secondary-a", SelectAll, context),
        KeyBinding::new("secondary-v", Paste, context),
        KeyBinding::new("secondary-c", Copy, context),
        KeyBinding::new("secondary-x", Cut, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("enter", Enter, context),
        KeyBinding::new("secondary-enter", Submit, context),
    ]);
}

/// Emitted on Enter in a one-line field, and on Ctrl+Enter in any.
pub struct Submitted;

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    /// Cursor and selection.
    accent: u32,
    placeholder_color: u32,
    /// Lines shown before the field scrolls. Past one, Enter starts a new
    /// line and pasted line breaks stay.
    lines: usize,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<Shaped>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    scroll: ScrollHandle,
    /// The text or the cursor changed: the next paint scrolls the cursor into
    /// view.
    reveal: bool,
}

impl EventEmitter<Submitted> for TextInput {}

impl TextInput {
    pub fn new(
        placeholder: impl Into<SharedString>,
        look: &Look,
        lines: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: placeholder.into(),
            accent: look.accent,
            placeholder_color: look.palette.secondary,
            lines: lines.max(1),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            scroll: ScrollHandle::new(),
            reveal: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Takes the colors of a changed look.
    pub fn restyle(&mut self, look: &Look, cx: &mut Context<Self>) {
        self.accent = look.accent;
        self.placeholder_color = look.palette.secondary;
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_text(String::new(), cx);
    }

    /// Replaces the text, with the cursor at its end.
    pub fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.selected_range = text.len()..text.len();
        self.content = text.into();
        self.selection_reversed = false;
        self.marked_range = None;
        self.reveal = true;
        cx.notify();
    }

    fn multiline(&self) -> bool {
        self.lines > 1
    }

    fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline() {
            self.replace_text_in_range(None, "\n", window, cx);
        } else {
            cx.emit(Submitted);
        }
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Submitted);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_line(-1.0, cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_line(1.0, cx);
    }

    /// Moves the cursor to the line `lines` above or below, keeping its x.
    fn move_line(&mut self, lines: f32, cx: &mut Context<Self>) {
        let Some(layout) = &self.last_layout else {
            return;
        };
        let at = layout.position_for_index(self.cursor_offset());
        let target = point(at.x, at.y + layout.line_height * (0.5 + lines));
        let offset = if target.y < px(0.) {
            0
        } else if target.y >= layout.height() {
            self.content.len()
        } else {
            layout.index_for_position(target)
        };
        self.move_to(offset, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let previous = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == previous {
                return;
            }
            self.select_to(previous, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next {
                return;
            }
            self.select_to(next, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = if self.multiline() {
                text
            } else {
                text.replace('\n', " ")
            };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.reveal = true;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(layout)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        layout.index_for_position(position - bounds.origin)
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.reveal = true;
        cx.notify()
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

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    /// What the field shows, the placeholder when empty, and its runs: the
    /// text being composed underlined.
    fn display(&self, window: &Window) -> (SharedString, Vec<TextRun>) {
        let style = window.text_style();
        let (text, text_color) = if self.content.is_empty() {
            (self.placeholder.clone(), solid(self.placeholder_color))
        } else {
            (self.content.clone(), style.color)
        };
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let Some(marked) = self.marked_range.clone() else {
            return (text, vec![run]);
        };
        let runs = vec![
            TextRun {
                len: marked.start,
                ..run.clone()
            },
            TextRun {
                len: marked.end - marked.start,
                underline: Some(UnderlineStyle {
                    color: Some(run.color),
                    thickness: px(1.0),
                    wavy: false,
                }),
                ..run.clone()
            },
            TextRun {
                len: text.len() - marked.end,
                ..run
            },
        ];
        (text, runs.into_iter().filter(|run| run.len > 0).collect())
    }
}

/// Text shaped for the field's width: one wrapped line per line of the text.
struct Shaped {
    lines: Vec<WrappedLine>,
    line_height: Pixels,
}

/// The field's font size and line height, taken while its style applies.
#[derive(Clone, Copy)]
struct Metrics {
    font_size: Pixels,
    line_height: Pixels,
}

impl Metrics {
    fn of(window: &Window) -> Self {
        Self {
            font_size: window.text_style().font_size.to_pixels(window.rem_size()),
            line_height: window.line_height(),
        }
    }
}

impl Shaped {
    fn new(
        text: SharedString,
        runs: &[TextRun],
        width: Option<Pixels>,
        metrics: Metrics,
        window: &Window,
    ) -> Self {
        let lines = window
            .text_system()
            .shape_text(text, metrics.font_size, runs, width, None)
            .map(|lines| lines.into_vec())
            .unwrap_or_default();
        Self {
            lines,
            line_height: metrics.line_height,
        }
    }

    fn height(&self) -> Pixels {
        let rows: usize = self
            .lines
            .iter()
            .map(|line| line.wrap_boundaries().len() + 1)
            .sum();
        self.line_height * rows.max(1) as f32
    }

    /// Where the character at byte `index` starts, from the text's top left.
    fn position_for_index(&self, index: usize) -> Point<Pixels> {
        let (mut start, mut top) = (0, px(0.));
        for line in &self.lines {
            let end = start + line.len();
            if index <= end {
                let at = line
                    .position_for_index(index - start, self.line_height)
                    .unwrap_or_default();
                return point(at.x, top + at.y);
            }
            top += line.size(self.line_height).height;
            start = end + 1;
        }
        point(px(0.), top)
    }

    /// The byte index closest to `position`, from the text's top left.
    fn index_for_position(&self, position: Point<Pixels>) -> usize {
        let (mut start, mut top) = (0, px(0.));
        for (ix, line) in self.lines.iter().enumerate() {
            let height = line.size(self.line_height).height;
            if position.y < top + height || ix + 1 == self.lines.len() {
                let y = (position.y - top).clamp(px(0.), height - px(1.));
                let (Ok(index) | Err(index)) =
                    line.closest_index_for_position(point(position.x, y), self.line_height);
                return start + index;
            }
            top += height;
            start += line.len() + 1;
        }
        0
    }

    /// Highlights for the bytes in `range`: one per row it covers.
    fn selection(&self, range: Range<usize>, bounds: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
        let (from, to) = (
            self.position_for_index(range.start),
            self.position_for_index(range.end),
        );
        let row = |top: Pixels, left: Pixels, right: Pixels| {
            Bounds::from_corners(
                bounds.origin + point(left, top),
                bounds.origin + point(right, top + self.line_height),
            )
        };
        let width = bounds.size.width;
        if from.y == to.y {
            return vec![row(from.y, from.x, to.x)];
        }
        let mut rows = vec![row(from.y, from.x, width)];
        if to.y > from.y + self.line_height {
            rows.push(Bounds::from_corners(
                bounds.origin + point(px(0.), from.y + self.line_height),
                bounds.origin + point(width, to.y),
            ));
        }
        rows.push(row(to.y, px(0.), to.x));
        rows
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        self.reveal = true;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.marked_range =
            (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.start)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.reveal = true;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = layout.position_for_index(range.start);
        let end = layout.position_for_index(range.end);
        let right = if end.y == start.y { end.x } else { start.x };
        Some(Bounds::from_corners(
            bounds.origin + start,
            bounds.origin + point(right, start.y + layout.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        let utf8_index = layout.index_for_position(point - bounds.origin);
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: gpui::Entity<TextInput>,
}

struct PrepaintState {
    layout: Option<Shaped>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    /// As tall as the text wrapped at the width it is given.
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let (text, runs) = self.input.read(cx).display(window);
        let metrics = Metrics::of(window);
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let layout = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let shaped = Shaped::new(text.clone(), &runs, width, metrics, window);
            size(width.unwrap_or_default(), shaped.height())
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let (text, runs) = input.display(window);
        let layout = Shaped::new(
            text,
            &runs,
            Some(bounds.size.width),
            Metrics::of(window),
            window,
        );
        let accent = input.accent;
        let selected_range = input.selected_range.clone();
        let cursor_at = if input.content.is_empty() {
            point(px(0.), px(0.))
        } else {
            layout.position_for_index(input.cursor_offset())
        };
        let cursor_top = bounds.origin + cursor_at;
        // Scrolls the field so the cursor shows, from the next frame on.
        if input.reveal && input.focus_handle.is_focused(window) {
            let view = input.scroll.bounds();
            let bottom = cursor_top.y + layout.line_height;
            let shift = if cursor_top.y < view.top() {
                view.top() - cursor_top.y
            } else if bottom > view.bottom() {
                view.bottom() - bottom
            } else {
                px(0.)
            };
            if shift != px(0.) {
                let offset = input.scroll.offset();
                input.scroll.set_offset(point(offset.x, offset.y + shift));
                window.request_animation_frame();
            }
        }
        let (selection, cursor) = if selected_range.is_empty() {
            let cursor = fill(
                Bounds::new(cursor_top, size(px(2.), layout.line_height)),
                color(accent, 1.0),
            );
            (Vec::new(), Some(cursor))
        } else {
            let rows = layout.selection(selected_range, bounds);
            let rows = rows.into_iter().map(|row| fill(row, color(accent, 0.35)));
            (rows.collect(), None)
        };
        PrepaintState {
            layout: Some(layout),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
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
        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection)
        }
        let layout = prepaint.layout.take().unwrap();
        let mut origin = bounds.origin;
        for line in &layout.lines {
            line.paint(
                origin,
                layout.line_height,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            )
            .unwrap();
            origin.y += line.size(layout.line_height).height;
        }
        let focused = focus_handle.is_focused(window);
        if focused && let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |input, _| {
            input.last_layout = Some(layout);
            input.last_bounds = Some(bounds);
            input.reveal &= !focused;
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(("text-input", cx.entity_id().as_u64() as usize))
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .max_h(dp(LINE) * self.lines as f32)
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::submit))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .line_height(dp(LINE))
            .text_size(dp(TEXT))
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
