use std::ops::Range;

use gpui::{
    actions, div, fill, point, px, size, prelude::*, App, Bounds, Context, CursorStyle, Element,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle,
    GlobalElementId, InspectorElementId, IntoElement, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, PaintQuad, Pixels, Point, SharedString, StyledText, TextLayout, TextRun,
    UTF16Selection, Window,
};

use crate::theme::Theme;

actions!(search_field, [SelectAll, Backspace, Delete, Left, Right, Home, End]);

/// The key context a focused search field claims so its editing keys arrive
/// as actions rather than reaching the surrounding page.
const SEARCH_CONTEXT: &str = "SearchField";

/// Bind the field's editing keys. Called once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-a", SelectAll, Some(SEARCH_CONTEXT)),
        KeyBinding::new("backspace", Backspace, Some(SEARCH_CONTEXT)),
        KeyBinding::new("delete", Delete, Some(SEARCH_CONTEXT)),
        KeyBinding::new("left", Left, Some(SEARCH_CONTEXT)),
        KeyBinding::new("right", Right, Some(SEARCH_CONTEXT)),
        KeyBinding::new("home", Home, Some(SEARCH_CONTEXT)),
        KeyBinding::new("end", End, Some(SEARCH_CONTEXT)),
    ]);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchFieldEvent {
    Edited,
}

impl EventEmitter<SearchFieldEvent> for SearchField {}

/// A minimal single-line text field: content, caret, and enough of the macOS
/// text-input protocol (IME composition included) to be a real search box.
///
/// Mirrors Waku's `ComposerInput` in the learning project but drops the
/// composer extras (syntax highlighting, undo history, context menu, blink).
/// The text is painted by the custom [`InputElement`]; the caret slides
/// horizontally to stay in view of the clipped viewport.
pub struct SearchField {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    /// Byte range. An empty range is a caret at `selected_range.end`.
    selected_range: Range<usize>,
    /// The byte range macOS has marked as a live composition.
    marked_range: Option<Range<usize>>,
    /// How far the single line is slid left of the clipped viewport.
    scroll_offset: Pixels,
    /// Last frame's laid-out text, used to map clicks and IME ranges to bytes.
    last_layout: Option<TextLayout>,
}

impl SearchField {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: SharedString::from("Search"),
            selected_range: 0..0,
            marked_range: None,
            scroll_offset: px(0.0),
            last_layout: None,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn focus(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// The field paints its accent border and caret only while the window is
    /// active and the field actually holds focus.
    pub fn is_visually_focused(&self, window: &Window) -> bool {
        window.is_window_active() && self.focus_handle.is_focused(window)
    }

    /// Replace the whole content, dropping any live composition.
    pub fn set_content(&mut self, content: impl Into<SharedString>, cx: &mut Context<Self>) {
        let content = content.into();
        if self.content == content {
            return;
        }
        self.content = content;
        let len = self.content.len();
        self.selected_range = len..len;
        self.marked_range = None;
        self.scroll_offset = px(0.0);
        cx.emit(SearchFieldEvent::Edited);
        cx.notify();
    }

    /// Place the caret at `range.start` (used by `cmd-a` select-all and by
    /// programmatic callers such as the apps-page clear button).
    pub fn select_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.selected_range = range;
        self.marked_range = None;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        self.selected_range.end
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.marked_range = None;
        cx.notify();
    }

    /// Splice `new_text` over `range`, placing the caret after it.
    fn replace_text(&mut self, range: Range<usize>, new_text: &str, cx: &mut Context<Self>) {
        let previous = self.content.clone();
        self.content =
            (self.content[..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        let offset = range.start + new_text.len();
        self.selected_range = offset..offset;
        self.marked_range = None;
        self.scroll_offset = px(0.0);
        if previous != self.content {
            cx.emit(SearchFieldEvent::Edited);
        }
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            if self.selected_range.start == 0 {
                return;
            }
            let start = previous_char_boundary(&self.content, self.selected_range.start);
            self.replace_text(start..self.selected_range.start, "", cx);
        } else {
            let range = self.selected_range.clone();
            self.replace_text(range, "", cx);
        }
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let start = self.selected_range.start;
            if start == self.content.len() {
                return;
            }
            let end = next_char_boundary(&self.content, start);
            self.replace_text(start..end, "", cx);
        } else {
            let range = self.selected_range.clone();
            self.replace_text(range, "", cx);
        }
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(previous_char_boundary(&self.content, self.cursor_offset()), cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(next_char_boundary(&self.content, self.cursor_offset()), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select_range(0..self.content.len(), cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.selected_range = if offset < self.selected_range.start {
                offset..self.selected_range.end
            } else {
                self.selected_range.start..offset
            };
        } else {
            self.move_to(offset, cx);
        }
        cx.notify();
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let Some(layout) = self.last_layout.as_ref() else {
            return 0;
        };
        layout
            .index_for_position(position)
            .unwrap_or_else(|index| index)
            .min(self.content.len())
    }

    // ── UTF-16 helpers (the platform text-input protocol is UTF-16) ──────

    fn offset_from_utf16(&self, offset: usize) -> usize {
        offset_from_utf16(&self.content, offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        offset_to_utf16(&self.content, offset)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    /// Resolves a range whose endpoints count UTF-16 units from `base`, the
    /// form macOS uses for everything relative to the marked text.
    fn range_from_relative_utf16(&self, base: usize, range: &Range<usize>) -> Range<usize> {
        let base_utf16 = self.offset_to_utf16(base);
        self.range_from_utf16(&(base_utf16 + range.start..base_utf16 + range.end))
    }
}

/// The byte offset that `utf16` UTF-16 code units into `text` land on. Clamps
/// past-the-end to `text.len()`.
fn offset_from_utf16(text: &str, utf16: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for character in text.chars() {
        if utf16_count >= utf16 {
            break;
        }
        utf16_count += character.len_utf16();
        utf8_offset += character.len_utf8();
    }
    utf8_offset
}

/// How many UTF-16 code units precede `byte` in `text`.
fn offset_to_utf16(text: &str, byte: usize) -> usize {
    let mut utf16_offset = 0;
    let mut utf8_count = 0;
    for character in text.chars() {
        if utf8_count >= byte {
            break;
        }
        utf8_count += character.len_utf8();
        utf16_offset += character.len_utf16();
    }
    utf16_offset
}

impl EntityInputHandler for SearchField {
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
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: false,
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
        // While text is marked, macOS reports replacement ranges relative to
        // the marked text, so the marked span itself is the commit target —
        // Zed's reading of the protocol. Absolute ranges only arrive outside
        // composition.
        let range = self.marked_range.clone().unwrap_or_else(|| {
            range_utf16
                .as_ref()
                .map(|range| self.range_from_utf16(range))
                .unwrap_or(self.selected_range.clone())
        });
        self.replace_text(range, new_text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A range arriving while text is marked is relative to the marked
        // text, and clipped to it; only without marked text is it absolute.
        let range = match (range_utf16.as_ref(), self.marked_range.as_ref()) {
            (Some(range_utf16), Some(marked)) => {
                let absolute = self.range_from_relative_utf16(marked.start, range_utf16);
                absolute.start.clamp(marked.start, marked.end)
                    ..absolute.end.clamp(marked.start, marked.end)
            }
            (Some(range_utf16), None) => self.range_from_utf16(range_utf16),
            (None, Some(marked)) => marked.clone(),
            (None, None) => self.selected_range.clone(),
        };
        self.replace_text(range.clone(), new_text, cx);
        self.marked_range =
            (!new_text.is_empty()).then_some(range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|new_range| self.range_from_relative_utf16(range.start, new_range))
            .unwrap_or_else(|| {
                let offset = range.start + new_text.len();
                offset..offset
            });
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
        let start = layout.position_for_index(range.start)?;
        let end = layout.position_for_index(range.end)?;
        let line_height = layout.line_height();
        if start.y == end.y {
            Some(Bounds::from_corners(
                start,
                point(end.x, end.y + line_height),
            ))
        } else {
            Some(Bounds::from_corners(
                point(bounds.left(), start.y),
                point(bounds.right(), end.y + line_height),
            ))
        }
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let layout = self.last_layout.as_ref()?;
        let utf8_index = layout
            .index_for_position(position)
            .unwrap_or_else(|index| index)
            .min(self.content.len());
        Some(self.offset_to_utf16(utf8_index))
    }
}

/// Horizontal scroll for a focused single-line field, reconciled every frame
/// against the caret like Zed's `autoscroll_horizontally`: reveal the caret
/// plus one `em` of lookahead, moving the scroll as little as possible.
fn single_line_scroll(
    previous: Pixels,
    viewport: Pixels,
    em: Pixels,
    text_width: Pixels,
    (start_x, head_x, end_x): (Pixels, Pixels, Pixels),
) -> Pixels {
    let max_scroll = (text_width + em - viewport).max(px(0.0));
    let scroll = previous.min(max_scroll).max(px(0.0));
    let (target_left, target_right) = if end_x - start_x + em <= viewport {
        (start_x, end_x + em)
    } else {
        (head_x, head_x + em)
    };
    if target_left < scroll {
        target_left
    } else if target_right > scroll + viewport {
        target_right - viewport
    } else {
        scroll
    }
}

struct InputElement {
    input: Entity<SearchField>,
}

impl IntoElement for InputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl InputElement {
    /// The bounds the single-line text is actually laid out at: the unwrapped
    /// line anchored `scroll_offset` left of the clipped viewport so the caret
    /// stays in view. Also reconciles the scroll for this frame, so the caller
    /// must prepaint at the returned bounds for index↔position math to agree.
    fn single_line_text_bounds(
        &self,
        bounds: Bounds<Pixels>,
        layout_state: &mut InputLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Bounds<Pixels> {
        let (focused, selection, previous_scroll) = {
            let input = self.input.read(cx);
            (
                input.is_visually_focused(window),
                (
                    input.selected_range.start,
                    input.cursor_offset(),
                    input.selected_range.end,
                ),
                input.scroll_offset,
            )
        };
        // Anchor the line at the natural origin first so index → position can
        // measure it; the definitive prepaint happens at the scrolled origin
        // returned from here.
        layout_state.text.prepaint(
            None,
            None,
            bounds,
            &mut layout_state.text_layout_state,
            window,
            cx,
        );
        let layout = layout_state.text.layout().clone();
        let x_for_index = |index: usize| {
            layout
                .position_for_index(index)
                .map_or(px(0.0), |position| position.x - bounds.origin.x)
        };
        let text_width = x_for_index(layout.len());
        let scroll = if focused {
            let style = window.text_style();
            let font_id = window.text_system().resolve_font(&style.font());
            let font_size = style.font_size.to_pixels(window.rem_size());
            let em = window
                .text_system()
                .em_advance(font_id, font_size)
                .unwrap_or(px(8.0));
            single_line_scroll(
                previous_scroll,
                bounds.size.width,
                em,
                text_width,
                (
                    x_for_index(selection.0),
                    x_for_index(selection.1),
                    x_for_index(selection.2),
                ),
            )
        } else {
            px(0.0)
        };
        self.input.update(cx, |input, _| input.scroll_offset = scroll);
        Bounds::new(
            point(bounds.origin.x - scroll, bounds.origin.y),
            size(bounds.size.width.max(text_width), bounds.size.height),
        )
    }
}

struct InputLayoutState {
    text: StyledText,
    text_layout_state: (),
}

struct PrepaintState {
    cursor: Option<PaintQuad>,
}

impl Element for InputElement {
    type RequestLayoutState = InputLayoutState;
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let placeholder = input.placeholder.clone();
        let style = window.text_style();
        let theme = Theme::current(cx);
        let content_is_empty = content.is_empty();
        let (display_text, text_color) = if content_is_empty {
            (placeholder, theme.text_ghost)
        } else {
            (content, style.color)
        };
        let base_run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let mut text = StyledText::new(display_text).with_runs(vec![base_run]);
        let (layout_id, text_layout_state) = text.request_layout(id, inspector_id, window, cx);
        (
            layout_id,
            InputLayoutState {
                text,
                text_layout_state,
            },
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout_state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let text_bounds = self.single_line_text_bounds(bounds, layout_state, window, cx);
        layout_state.text.prepaint(
            None,
            None,
            text_bounds,
            &mut layout_state.text_layout_state,
            window,
            cx,
        );
        let input = self.input.read(cx);
        let focused = input.is_visually_focused(window);
        let layout = layout_state.text.layout();
        let line_height = layout.line_height();
        let cursor = (focused && input.selected_range.is_empty())
            .then(|| input.cursor_offset())
            .and_then(|offset| layout.position_for_index(offset))
            .map(|position| {
                fill(
                    Bounds::new(position, size(px(1.5), line_height)),
                    Theme::current(cx).accent,
                )
            });
        PrepaintState { cursor }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout_state: &mut Self::RequestLayoutState,
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
        layout_state.text.paint(
            None,
            None,
            bounds,
            &mut layout_state.text_layout_state,
            &mut (),
            window,
            cx,
        );
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }
        let text_layout = layout_state.text.layout().clone();
        self.input.update(cx, |input, _| input.last_layout = Some(text_layout));
    }
}

impl Render for SearchField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let input = cx.entity();
        div()
            .key_context(SEARCH_CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_all))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .w_full()
            .whitespace_nowrap()
            .overflow_hidden()
            .text_color(theme.text)
            .child(InputElement { input })
    }
}

/// The byte offset of the previous character boundary, or 0. Safe for offsets
/// that fall inside a multi-byte character — the result is the start of the
/// character containing `byte` (or of the one just before a boundary).
fn previous_char_boundary(text: &str, byte: usize) -> usize {
    text.char_indices()
        .map(|(index, _)| index)
        .rev()
        .find(|index| *index < byte)
        .unwrap_or(0)
}

/// The byte offset of the next character boundary, or `text.len()`. Safe for
/// offsets that fall inside a multi-byte character.
fn next_char_boundary(text: &str, byte: usize) -> usize {
    text.char_indices()
        .map(|(index, _)| index)
        .find(|index| *index > byte)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use gpui::{
        Context, Entity, Pixels, Render, TestAppContext, Window, div, prelude::*, px,
    };

    use super::{
        SearchField, next_char_boundary, offset_from_utf16, offset_to_utf16,
        previous_char_boundary, single_line_scroll,
    };

    struct FieldHarness {
        field: Entity<SearchField>,
        width: Pixels,
    }

    impl Render for FieldHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(self.width).child(self.field.clone())
        }
    }

    fn setup_field<'a>(
        cx: &'a mut TestAppContext,
        content: &str,
        width: Pixels,
    ) -> (Entity<SearchField>, &'a mut gpui::VisualTestContext) {
        cx.update(super::init);
        let content = content.to_owned();
        let (harness, cx) = cx.add_window_view(move |_window, cx| {
            let field = cx.new(|cx| {
                let mut field = SearchField::new(cx);
                field.set_content(content, cx);
                field
            });
            FieldHarness { field, width }
        });
        let field = cx.read_entity(&harness, |harness, _| harness.field.clone());
        cx.update(|window, cx| window.focus(&field.read(cx).focus(), cx));
        cx.run_until_parked();
        (field, cx)
    }

    #[gpui::test]
    fn typing_inserts_at_the_caret(cx: &mut TestAppContext) {
        let (field, cx) = setup_field(cx, "hello world", px(300.0));
        cx.update(|_, cx| field.update(cx, |field, cx| field.select_range(5..5, cx)));

        cx.simulate_keystrokes("a");

        cx.read_entity(&field, |field, _| {
            assert_eq!(field.content(), "helloa world");
            assert_eq!(field.selected_range.end, 6);
        });
    }

    #[gpui::test]
    fn backspace_deletes_previous_character(cx: &mut TestAppContext) {
        let (field, cx) = setup_field(cx, "hello", px(300.0));
        cx.read_entity(&field, |field, _| assert_eq!(field.selected_range.end, 5));

        cx.simulate_keystrokes("backspace");

        cx.read_entity(&field, |field, _| {
            assert_eq!(field.content(), "hell");
            assert_eq!(field.selected_range.end, 4);
        });
    }

    #[gpui::test]
    fn left_right_home_end_move_the_caret(cx: &mut TestAppContext) {
        let (field, cx) = setup_field(cx, "abc", px(300.0));
        cx.read_entity(&field, |field, _| assert_eq!(field.selected_range.end, 3));

        cx.simulate_keystrokes("home");
        cx.read_entity(&field, |field, _| assert_eq!(field.selected_range.end, 0));

        cx.simulate_keystrokes("right");
        cx.read_entity(&field, |field, _| assert_eq!(field.selected_range.end, 1));

        cx.simulate_keystrokes("end");
        cx.read_entity(&field, |field, _| assert_eq!(field.selected_range.end, 3));

        cx.simulate_keystrokes("left");
        cx.read_entity(&field, |field, _| assert_eq!(field.selected_range.end, 2));
    }

    #[gpui::test]
    fn select_all_selects_the_whole_content(cx: &mut TestAppContext) {
        let (field, cx) = setup_field(cx, "hello world", px(300.0));
        cx.read_entity(&field, |field, _| {
            assert_eq!(field.selected_range, 11..11);
        });

        cx.simulate_keystrokes("cmd-a");
        cx.read_entity(&field, |field, _| {
            assert_eq!(field.selected_range, 0.."hello world".len());
        });

        cx.simulate_keystrokes("x");
        cx.read_entity(&field, |field, _| {
            assert_eq!(field.content(), "x");
            assert_eq!(field.selected_range.end, 1);
        });
    }

    #[gpui::test]
    fn ime_composition_marks_and_commits_text(cx: &mut TestAppContext) {
        let (field, cx) = setup_field(cx, "", px(300.0));

        cx.update(|window, cx| {
            field.update(cx, |field, cx| {
                use gpui::EntityInputHandler;
                field.replace_and_mark_text_in_range(None, "こ", None, window, cx);
                field.replace_and_mark_text_in_range(None, "こん", Some(0..2), window, cx);
                field.unmark_text(window, cx);
            });
        });

        cx.read_entity(&field, |field, _| assert_eq!(field.content(), "こん"));
    }

    #[test]
    fn char_boundaries_respect_utf8() {
        let text = "aé日";
        assert_eq!(previous_char_boundary(text, 0), 0);
        assert_eq!(previous_char_boundary(text, 1), 0);
        assert_eq!(previous_char_boundary(text, 2), 1);
        assert_eq!(previous_char_boundary(text, 3), 1);
        assert_eq!(previous_char_boundary(text, 6), 3);
        assert_eq!(next_char_boundary(text, 0), 1);
        assert_eq!(next_char_boundary(text, 1), 3);
        assert_eq!(next_char_boundary(text, 3), 6);
        assert_eq!(next_char_boundary(text, 4), 6);
        assert_eq!(next_char_boundary(text, 6), 6);
    }

    #[test]
    fn utf16_offsets_round_trip() {
        let text = "aé日x";
        let mut boundary = 0;
        let boundaries = std::iter::once(0)
            .chain(text.char_indices().map(|(index, character)| {
                index + character.len_utf8()
            }))
            .collect::<Vec<_>>();
        for byte in boundaries {
            let utf16 = offset_to_utf16(text, byte);
            assert_eq!(offset_from_utf16(text, utf16), byte);
            boundary = byte;
        }
        assert_eq!(boundary, text.len());
        assert_eq!(offset_to_utf16(text, 1), 1); // 'a'
        assert_eq!(offset_to_utf16(text, 3), 2); // 'é'
        assert_eq!(offset_to_utf16(text, 6), 3); // '日'
    }

    #[test]
    fn single_line_scroll_reveals_the_caret() {
        let viewport = px(100.0);
        let em = px(8.0);
        let text_width = px(400.0);
        assert_eq!(
            single_line_scroll(px(0.0), viewport, em, text_width, (px(0.0), px(0.0), px(0.0))),
            px(0.0)
        );
        let caret = px(390.0);
        let scroll = single_line_scroll(px(0.0), viewport, em, text_width, (caret, caret, caret));
        assert!(scroll > px(0.0), "caret must be revealed");
        assert!(
            caret + em - scroll <= viewport + px(1.0),
            "caret must sit within the viewport"
        );
        let at_rest = single_line_scroll(scroll, viewport, em, text_width, (caret, caret, caret));
        assert_eq!(at_rest, scroll, "scroll must not jitter at rest");
    }
}
