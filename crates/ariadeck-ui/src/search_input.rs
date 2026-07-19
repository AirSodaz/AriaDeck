use std::ops::Range;

use gpui::{
    A11ySubtreeBuilder, AccessibleAction, App, Bounds, ClipboardItem, Context, CursorStyle,
    Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable,
    GlobalElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PaintQuad, Pixels, Point, Role, ShapedLine, SharedString, Style, TextRun,
    UTF16Selection, UnderlineStyle, Window,
    accesskit::{self, ActionData},
    div, fill, point,
    prelude::*,
    px, relative, size,
};

use crate::{
    Backspace, Copy, Cut, Delete, MoveEnd, MoveHome, MoveLeft, MoveRight, Paste, SelectAll,
    SelectLeft, SelectRight, Theme,
    components::{Icon, IconButton, IconName, IconSize, Tooltip},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextFieldEvent {
    pub text: String,
}

pub type SearchInputEvent = TextFieldEvent;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextFieldConfig {
    pub element_id: SharedString,
    pub key_context: SharedString,
    pub role: Role,
    pub accessibility_label: SharedString,
    pub placeholder: SharedString,
    pub leading_icon: Option<IconName>,
    pub clearable: bool,
}

impl TextFieldConfig {
    #[must_use]
    pub fn search(placeholder: impl Into<SharedString>) -> Self {
        Self {
            element_id: "download-search".into(),
            key_context: "SearchInput".into(),
            role: Role::SearchInput,
            accessibility_label: "Search downloads".into(),
            placeholder: placeholder.into(),
            leading_icon: Some(IconName::Search),
            clearable: true,
        }
    }
}

pub struct TextField {
    focus_handle: FocusHandle,
    content: SharedString,
    element_id: SharedString,
    key_context: SharedString,
    role: Role,
    accessibility_label: SharedString,
    placeholder: SharedString,
    leading_icon: Option<IconName>,
    clearable: bool,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    theme: Theme,
}

pub type SearchInput = TextField;

impl gpui::EventEmitter<TextFieldEvent> for TextField {}

impl TextField {
    #[must_use]
    pub fn new(placeholder: impl Into<SharedString>, theme: Theme, cx: &mut Context<Self>) -> Self {
        Self::new_with_config(TextFieldConfig::search(placeholder), theme, cx)
    }

    #[must_use]
    pub fn new_with_config(config: TextFieldConfig, theme: Theme, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            content: SharedString::default(),
            element_id: config.element_id,
            key_context: config.key_context,
            role: config.role,
            accessibility_label: config.accessibility_label,
            placeholder: config.placeholder,
            leading_icon: config.leading_icon,
            clearable: config.clearable,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            theme,
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        if self.content == text {
            return;
        }
        self.content = text;
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.emit_change(cx);
    }

    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        if self.theme != theme {
            self.theme = theme;
            cx.notify();
        }
    }

    #[cfg(test)]
    pub(crate) fn text_bounds(&self) -> Option<Bounds<Pixels>> {
        self.last_bounds
    }

    fn emit_change(&self, cx: &mut Context<Self>) {
        cx.emit(TextFieldEvent {
            text: self.content.to_string(),
        });
        cx.notify();
    }

    fn set_accessible_value(&mut self, data: Option<&ActionData>, cx: &mut Context<Self>) {
        let Some(ActionData::Value(text)) = data else {
            return;
        };
        self.set_text(text.to_string(), cx);
    }

    fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn move_home(&mut self, _: &MoveHome, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn move_end(&mut self, _: &MoveEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let previous = self.previous_boundary(self.cursor_offset());
            if previous == self.cursor_offset() {
                window.play_system_bell();
                return;
            }
            self.select_to(previous, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if next == self.cursor_offset() {
                window.play_system_bell();
                return;
            }
            self.select_to(next, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace(['\r', '\n'], " "), window, cx);
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
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        self.is_selecting = true;
        let index = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(index, cx);
        } else {
            self.move_to(index, cx);
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

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content[..offset]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content[offset..]
            .char_indices()
            .nth(1)
            .map_or(self.content.len(), |(index, _)| offset + index)
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for character in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += character.len_utf16();
            utf8_offset += character.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for character in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += character.len_utf8();
            utf16_offset += character.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}

impl EntityInputHandler for TextField {
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
        self.content = format!(
            "{}{}{}",
            &self.content[..range.start],
            new_text,
            &self.content[range.end..]
        )
        .into();
        let cursor = range.start + new_text.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.emit_change(cx);
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
        self.content = format!(
            "{}{}{}",
            &self.content[..range.start],
            new_text,
            &self.content[range.end..]
        )
        .into();
        self.marked_range =
            (!new_text.is_empty()).then_some(range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|selection| self.range_from_utf16(selection))
            .map(|selection| range.start + selection.start..range.start + selection.end)
            .unwrap_or_else(|| {
                let cursor = range.start + new_text.len();
                cursor..cursor
            });
        self.selection_reversed = false;
        self.emit_change(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start), bounds.top()),
            point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let line = self.last_layout.as_ref()?;
        let index = line.index_for_x(point.x - bounds.left())?;
        Some(self.offset_to_utf16(index))
    }
}

struct TextFieldElement {
    input: Entity<TextField>,
}

struct TextFieldPrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextFieldElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextFieldElement {
    type RequestLayoutState = ();
    type PrepaintState = TextFieldPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let theme = input.theme;
        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), theme.colors.text_muted)
        } else {
            (content, theme.colors.text_primary)
        };
        let base_run = TextRun {
            len: display_text.len(),
            font: window.text_style().font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..base_run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(base_run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..base_run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..base_run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![base_run]
        };
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);
        let cursor_x = line.x_for_index(cursor);
        let selection_color = with_alpha(theme.colors.focus_ring, 0.24);
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_x, bounds.top()),
                        size(px(1.5), bounds.size.height),
                    ),
                    theme.colors.focus_ring,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    selection_color,
                )),
                None,
            )
        };
        TextFieldPrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
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
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let Some(line) = prepaint.line.take() else {
            return;
        };
        let _ = line.paint(
            bounds.origin,
            window.line_height(),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl gpui::Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.theme.colors;
        let (selection_tail, selection_head) = if self.selection_reversed {
            (self.selected_range.end, self.selected_range.start)
        } else {
            (self.selected_range.start, self.selected_range.end)
        };
        let (a11y_value, a11y_text_runs) = text_field_a11y_state(
            self.element_id.clone(),
            self.content.to_string(),
            selection_tail,
            selection_head,
            self.focus_handle.is_focused(window),
            window,
            cx,
        );
        let weak_input = cx.entity().downgrade();
        let clear_input = weak_input.clone();
        let is_focused = self.focus_handle.is_focused(window);
        let input = div()
            .h_full()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .overflow_hidden()
            .text_sm()
            .child(TextFieldElement { input: cx.entity() });

        div()
            .id(self.element_id.clone())
            .key_context(self.key_context.as_ref())
            .role(self.role)
            .aria_label(self.accessibility_label.clone())
            .aria_placeholder(self.placeholder.clone())
            .aria_value(a11y_value)
            .a11y_synthetic_children(a11y_text_runs)
            .on_a11y_action(AccessibleAction::SetValue, move |data, _window, cx| {
                let Some(input) = weak_input.upgrade() else {
                    return;
                };
                input.update(cx, |input, cx| input.set_accessible_value(data, cx));
            })
            .focusable()
            .tab_stop(true)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::move_home))
            .on_action(cx.listener(Self::move_end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .cursor(CursorStyle::IBeam)
            .h(px(38.0))
            .w_full()
            .min_w(px(180.0))
            .max_w(px(460.0))
            .flex()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .pl_3()
            .pr_1()
            .rounded_md()
            .border_1()
            .border_color(if is_focused {
                colors.focus_ring
            } else {
                colors.border
            })
            .bg(colors.elevated_surface)
            .text_sm()
            .when_some(self.leading_icon, |field, icon| {
                field.child(
                    Icon::new(icon)
                        .size(IconSize::Small)
                        .color(colors.text_muted),
                )
            })
            .child(input)
            .when(self.clearable && !self.content.is_empty(), |field| {
                let label = format!("Clear {}", self.accessibility_label);
                field.child(
                    IconButton::new(
                        SharedString::from(format!("{}-clear", self.element_id)),
                        IconName::X,
                    )
                    .aria_label(label)
                    .tooltip(Tooltip::new("Clear"))
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        let Some(input) = clear_input.upgrade() else {
                            return;
                        };
                        let focus = input.read(cx).focus_handle.clone();
                        input.update(cx, |input, cx| input.set_text("", cx));
                        window.focus(&focus, cx);
                    })
                    .render(colors),
                )
            })
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn with_alpha(mut color: gpui::Hsla, alpha: f32) -> gpui::Hsla {
    color.a = alpha;
    color
}

fn text_field_a11y_state(
    state_key: impl Into<ElementId>,
    text: String,
    selection_tail: usize,
    selection_head: usize,
    is_focused: bool,
    window: &mut Window,
    cx: &mut App,
) -> (String, impl FnOnce(&mut A11ySubtreeBuilder) + 'static) {
    let state = window.is_a11y_active().then(|| {
        let a11y_value = window.use_keyed_state((state_key.into(), "a11y-value"), cx, {
            let text = text.clone();
            move |_, _| text
        });
        if !is_focused && *a11y_value.read(cx) != text {
            *a11y_value.as_mut(cx) = text.clone();
        }
        let frozen_value = a11y_value.read(cx).clone();

        (frozen_value, text, selection_tail, selection_head)
    });

    let (frozen_value, run_data) = match state {
        Some((frozen_value, text, selection_tail, selection_head)) => {
            (frozen_value, Some((text, selection_tail, selection_head)))
        }
        None => (String::new(), None),
    };
    let text_runs = move |builder: &mut A11ySubtreeBuilder| {
        if let Some((text, selection_tail, selection_head)) = run_data {
            push_a11y_text_runs(builder, &text, selection_tail, selection_head);
        }
    };

    (frozen_value, text_runs)
}

const MAX_CHARS_PER_TEXT_RUN: usize = 255;

fn is_word_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn char_index_for_byte(text: &str, byte_offset: usize) -> usize {
    text.char_indices()
        .take_while(|(byte_index, _)| *byte_index < byte_offset)
        .count()
}

fn a11y_text_position(
    char_index: usize,
    synthetic_node_id: impl Fn(u64) -> accesskit::NodeId,
) -> accesskit::TextPosition {
    let chunk_index = if char_index > 0 && char_index.is_multiple_of(MAX_CHARS_PER_TEXT_RUN) {
        char_index / MAX_CHARS_PER_TEXT_RUN - 1
    } else {
        char_index / MAX_CHARS_PER_TEXT_RUN
    };
    accesskit::TextPosition {
        node: synthetic_node_id(chunk_index as u64),
        character_index: char_index - chunk_index * MAX_CHARS_PER_TEXT_RUN,
    }
}

fn build_a11y_text_runs(
    text: &str,
    selection_tail: usize,
    selection_head: usize,
    synthetic_node_id: impl Fn(u64) -> accesskit::NodeId,
) -> (
    Vec<(accesskit::NodeId, accesskit::Node)>,
    accesskit::TextSelection,
) {
    let chars: Vec<char> = text.chars().collect();
    let total_chars = chars.len();
    let num_chunks = total_chars.div_ceil(MAX_CHARS_PER_TEXT_RUN).max(1);

    let mut word_starts = Vec::new();
    let mut was_word_char = false;
    for (index, character) in chars.iter().enumerate() {
        let is_word = is_word_char(*character);
        if is_word && !was_word_char {
            word_starts.push(index);
        }
        was_word_char = is_word;
    }

    let mut runs = Vec::with_capacity(num_chunks);
    for chunk_index in 0..num_chunks {
        let char_start = chunk_index * MAX_CHARS_PER_TEXT_RUN;
        let char_end = (char_start + MAX_CHARS_PER_TEXT_RUN).min(total_chars);
        let chunk_chars = &chars[char_start..char_end];

        let mut node = accesskit::Node::new(accesskit::Role::TextRun);
        node.set_text_direction(accesskit::TextDirection::LeftToRight);
        node.set_value(chunk_chars.iter().collect::<String>());
        node.set_character_lengths(
            chunk_chars
                .iter()
                .map(|character| character.len_utf8() as u8)
                .collect::<Vec<_>>(),
        );
        node.set_word_starts(
            word_starts
                .iter()
                .filter(|&&word_start| word_start >= char_start && word_start < char_end)
                .map(|&word_start| (word_start - char_start) as u8)
                .collect::<Vec<_>>(),
        );
        if chunk_index > 0 {
            node.set_previous_on_line(synthetic_node_id(chunk_index as u64 - 1));
        }
        if chunk_index + 1 < num_chunks {
            node.set_next_on_line(synthetic_node_id(chunk_index as u64 + 1));
        }

        runs.push((synthetic_node_id(chunk_index as u64), node));
    }

    let anchor = a11y_text_position(
        char_index_for_byte(text, selection_tail),
        &synthetic_node_id,
    );
    let focus = a11y_text_position(
        char_index_for_byte(text, selection_head),
        &synthetic_node_id,
    );
    (runs, accesskit::TextSelection { anchor, focus })
}

fn push_a11y_text_runs(
    builder: &mut A11ySubtreeBuilder,
    text: &str,
    selection_tail: usize,
    selection_head: usize,
) {
    let (runs, selection) = build_a11y_text_runs(text, selection_tail, selection_head, |chunk| {
        builder.synthetic_node_id(chunk)
    });
    for (id, node) in runs {
        builder.push_child(id, node);
    }
    builder.parent_node().set_text_selection(selection);
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, TestAppContext, accesskit::NodeId};

    use super::*;

    #[test]
    fn search_config_preserves_legacy_metadata() {
        let config = TextFieldConfig::search("Search downloads or GID");

        assert_eq!(config.element_id.as_ref(), "download-search");
        assert_eq!(config.key_context.as_ref(), "SearchInput");
        assert_eq!(config.role, Role::SearchInput);
        assert_eq!(config.accessibility_label.as_ref(), "Search downloads");
        assert_eq!(config.placeholder.as_ref(), "Search downloads or GID");
        assert_eq!(config.leading_icon, Some(IconName::Search));
        assert!(config.clearable);
    }

    #[gpui::test]
    fn field_config_preserves_decorations(cx: &mut TestAppContext) {
        let input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "download-url".into(),
                    key_context: "AddDownloadInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Download URL".into(),
                    placeholder: "https://example.com/file".into(),
                    leading_icon: Some(IconName::Link),
                    clearable: true,
                },
                Theme::dark(),
                cx,
            )
        });

        cx.read_entity(&input, |input, _| {
            assert_eq!(input.leading_icon, Some(IconName::Link));
            assert!(input.clearable);
            assert_eq!(input.placeholder.as_ref(), "https://example.com/file");
        });
    }

    #[test]
    fn search_names_are_compatibility_aliases() {
        let input: Option<SearchInput> = None;
        let _: Option<TextField> = input;

        let event = SearchInputEvent {
            text: "example".to_owned(),
        };
        let _: TextFieldEvent = event;
    }

    #[test]
    fn accessibility_text_runs_preserve_utf8_selection() {
        let text = "aé中🙂";
        let (runs, selection) = build_a11y_text_runs(text, 1, text.len(), NodeId);

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].0, NodeId(0));
        assert_eq!(runs[0].1.role(), accesskit::Role::TextRun);
        assert_eq!(selection.anchor.node, NodeId(0));
        assert_eq!(selection.anchor.character_index, 1);
        assert_eq!(selection.focus.node, NodeId(0));
        assert_eq!(selection.focus.character_index, 4);
    }

    #[test]
    fn accessibility_text_runs_split_long_values_at_valid_boundaries() {
        let text = "a".repeat(MAX_CHARS_PER_TEXT_RUN + 1);
        let (runs, selection) =
            build_a11y_text_runs(&text, MAX_CHARS_PER_TEXT_RUN, text.len(), NodeId);

        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].0, NodeId(0));
        assert_eq!(runs[1].0, NodeId(1));
        assert_eq!(selection.anchor.node, NodeId(0));
        assert_eq!(selection.anchor.character_index, MAX_CHARS_PER_TEXT_RUN);
        assert_eq!(selection.focus.node, NodeId(1));
        assert_eq!(selection.focus.character_index, 1);
    }

    #[gpui::test]
    fn accessible_set_value_uses_the_normal_text_update_path(cx: &mut TestAppContext) {
        let input = cx.new(|cx| TextField::new("URL", Theme::dark(), cx));
        let action = ActionData::Value("https://example.com/file".into());

        cx.update_entity(&input, |input, cx| {
            input.set_accessible_value(Some(&action), cx);
        });

        cx.read_entity(&input, |input, _| {
            assert_eq!(input.text(), "https://example.com/file");
            assert_eq!(
                input.selected_range,
                input.content.len()..input.content.len()
            );
            assert!(!input.selection_reversed);
            assert!(input.marked_range.is_none());
        });
    }
}
