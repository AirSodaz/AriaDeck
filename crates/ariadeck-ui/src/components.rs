use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, App, AppContext as _, ClickEvent, CursorStyle, ElementId, FocusHandle,
    Hsla, IntoElement, Render, RenderOnce, Role, SharedString, Stateful, Window, div, prelude::*,
    px, svg,
};

use crate::{Theme, ThemeColors};

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type SelectionHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// The supported embedded Lucide icons.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IconName {
    Search,
    Settings,
    Plus,
    Minus,
    Square,
    SquareCheckBig,
    SquareMinus,
    X,
    Pause,
    Play,
    RotateCcw,
    Pencil,
    Trash2,
    Copy,
    Sun,
    Moon,
    Link,
    FolderDown,
    Download,
    CircleCheck,
    CircleAlert,
    CircleX,
    Info,
    LoaderCircle,
    MoreHorizontal,
    PanelRight,
    Wifi,
    WifiOff,
    ArrowDown,
    ArrowUp,
    TriangleAlert,
    CloudOff,
    SearchX,
    Inbox,
    Activity,
    CircleHelp,
    Clock3,
    List,
    RefreshCw,
    ScanSearch,
    ArrowUpDown,
    ChevronUp,
    ChevronDown,
    ChevronsUp,
    ChevronsDown,
    Check,
    /// Windows caption glyphs (thin Fluent-style strokes).
    WindowMinimize,
    WindowMaximize,
    WindowRestore,
    WindowClose,
}

impl IconName {
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Search => "icons/search.svg",
            Self::Settings => "icons/settings.svg",
            Self::Plus => "icons/plus.svg",
            Self::Minus => "icons/minus.svg",
            Self::Square => "icons/square.svg",
            Self::SquareCheckBig => "icons/square-check-big.svg",
            Self::SquareMinus => "icons/square-minus.svg",
            Self::X => "icons/x.svg",
            Self::Pause => "icons/pause.svg",
            Self::Play => "icons/play.svg",
            Self::RotateCcw => "icons/rotate-ccw.svg",
            Self::Pencil => "icons/pencil.svg",
            Self::Trash2 => "icons/trash-2.svg",
            Self::Copy => "icons/copy.svg",
            Self::Sun => "icons/sun.svg",
            Self::Moon => "icons/moon.svg",
            Self::Link => "icons/link.svg",
            Self::FolderDown => "icons/folder-down.svg",
            Self::Download => "icons/download.svg",
            Self::CircleCheck => "icons/circle-check.svg",
            Self::CircleAlert => "icons/circle-alert.svg",
            Self::CircleX => "icons/circle-x.svg",
            Self::Info => "icons/info.svg",
            Self::LoaderCircle => "icons/loader-circle.svg",
            Self::MoreHorizontal => "icons/ellipsis.svg",
            Self::PanelRight => "icons/panel-right.svg",
            Self::Wifi => "icons/wifi.svg",
            Self::WifiOff => "icons/wifi-off.svg",
            Self::ArrowDown => "icons/arrow-down.svg",
            Self::ArrowUp => "icons/arrow-up.svg",
            Self::TriangleAlert => "icons/triangle-alert.svg",
            Self::CloudOff => "icons/cloud-off.svg",
            Self::SearchX => "icons/search-x.svg",
            Self::Inbox => "icons/inbox.svg",
            Self::Activity => "icons/activity.svg",
            Self::CircleHelp => "icons/circle-help.svg",
            Self::Clock3 => "icons/clock-3.svg",
            Self::List => "icons/list.svg",
            Self::RefreshCw => "icons/refresh-cw.svg",
            Self::ScanSearch => "icons/scan-search.svg",
            Self::ArrowUpDown => "icons/arrow-up-down.svg",
            Self::ChevronUp => "icons/chevron-up.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ChevronsUp => "icons/chevrons-up.svg",
            Self::ChevronsDown => "icons/chevrons-down.svg",
            Self::Check => "icons/check.svg",
            Self::WindowMinimize => "icons/window-minimize.svg",
            Self::WindowMaximize => "icons/window-maximize.svg",
            Self::WindowRestore => "icons/window-restore.svg",
            Self::WindowClose => "icons/window-close.svg",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IconSize {
    XSmall,
    #[default]
    Small,
    Medium,
    Large,
}

impl IconSize {
    const fn pixels(self) -> f32 {
        match self {
            Self::XSmall => 12.0,
            Self::Small => 14.0,
            Self::Medium => 16.0,
            Self::Large => 24.0,
        }
    }
}

/// A consistently sized, theme-tintable SVG icon.
#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: IconSize,
    color: Option<Hsla>,
    label: Option<SharedString>,
}

impl Icon {
    #[must_use]
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: IconSize::Medium,
            color: None,
            label: None,
        }
    }

    #[must_use]
    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    #[must_use]
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    #[must_use]
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        svg()
            .path(self.name.path())
            .size(px(self.size.pixels()))
            .flex_none()
            .when_some(self.color, |icon, color| icon.text_color(color))
            .when_some(self.label, |icon, _label| icon)
    }
}

/// Compact tooltip content suitable for GPUI's `.tooltip` modifier.
#[derive(Clone, Debug)]
pub struct Tooltip {
    title: SharedString,
    meta: Option<SharedString>,
    colors: ThemeColors,
}

impl Tooltip {
    #[must_use]
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            meta: None,
            colors: Theme::dark().colors,
        }
    }

    #[must_use]
    pub fn meta(mut self, meta: impl Into<SharedString>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    pub fn text(
        title: impl Into<SharedString>,
        meta: Option<SharedString>,
        colors: ThemeColors,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView {
        let title = title.into();
        move |_, cx| {
            let title = title.clone();
            let meta = meta.clone();
            cx.new(|_| Self {
                title,
                meta,
                colors,
            })
            .into()
        }
    }
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let colors = self.colors;
        div()
            .flex()
            .items_center()
            .gap_3()
            .max_w(px(280.0))
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(colors.border_strong)
            .bg(colors.elevated_surface)
            .text_xs()
            .text_color(colors.text_primary)
            .child(self.title.clone())
            .when_some(self.meta.clone(), |tooltip, shortcut| {
                tooltip.child(
                    div()
                        .text_color(colors.text_muted)
                        .font_family("monospace")
                        .child(shortcut),
                )
            })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ButtonVariant {
    Primary,
    #[default]
    Secondary,
    Ghost,
    Danger,
}

pub type ButtonStyle = ButtonVariant;

/// Standard labeled button with unified states and optional icon.
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    variant: ButtonVariant,
    disabled: bool,
    loading: bool,
    aria_label: Option<SharedString>,
    tooltip: Option<Tooltip>,
    focus: Option<FocusHandle>,
    on_click: Option<ClickHandler>,
}

impl Button {
    #[must_use]
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            variant: ButtonVariant::Secondary,
            disabled: false,
            loading: false,
            aria_label: None,
            tooltip: None,
            focus: None,
            on_click: None,
        }
    }

    #[must_use]
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    #[must_use]
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    #[must_use]
    pub fn style(mut self, variant: ButtonStyle) -> Self {
        self.variant = variant;
        self
    }

    #[must_use]
    pub fn variant(self, variant: ButtonVariant) -> Self {
        self.style(variant)
    }

    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[must_use]
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    #[must_use]
    pub fn tooltip(mut self, tooltip: Tooltip) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    #[must_use]
    pub fn track_focus(mut self, focus: FocusHandle) -> Self {
        self.focus = Some(focus);
        self
    }

    #[must_use]
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    #[must_use]
    pub fn render(self, colors: ThemeColors) -> Stateful<gpui::Div> {
        let enabled = !self.disabled && !self.loading;
        let (background, foreground, border, hover, active) = match self.variant {
            ButtonVariant::Primary => (
                colors.accent,
                colors.text_inverse,
                colors.accent,
                colors.accent_hover,
                colors.accent_active,
            ),
            ButtonVariant::Secondary => (
                colors.elevated_surface,
                colors.text_primary,
                colors.border,
                colors.surface_hover,
                colors.surface_active,
            ),
            ButtonVariant::Ghost => (
                transparent(),
                colors.text_secondary,
                transparent(),
                colors.surface_hover,
                colors.surface_active,
            ),
            ButtonVariant::Danger => (
                colors.danger,
                colors.text_inverse,
                colors.danger,
                with_alpha(colors.danger, 0.86),
                with_alpha(colors.danger, 0.72),
            ),
        };
        let tooltip = self.tooltip;
        let aria_label = self.aria_label.unwrap_or_else(|| self.label.clone());
        let mut button = div()
            .id(self.id)
            .role(Role::Button)
            .aria_label(aria_label)
            .focusable()
            .tab_stop(enabled)
            .h(px(32.0))
            .min_w(px(32.0))
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .px_3()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(background)
            .text_sm()
            .text_color(foreground)
            .when(enabled, |button| {
                button
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(hover))
                    .active(move |style| style.bg(active))
            })
            .when(!enabled, |button| button.opacity(0.45));
        if let Some(focus) = self.focus {
            button = button.track_focus(&focus);
        }
        if enabled && let Some(handler) = self.on_click {
            button = button.on_click(move |event, window, cx| handler(event, window, cx));
        }
        if let Some(tooltip) = tooltip {
            button = button.tooltip(Tooltip::text(tooltip.title, tooltip.meta, colors));
        }
        button
            .when(self.loading, |button| {
                button.child(LoadingIndicator::new(foreground).size(IconSize::Medium))
            })
            .when(!self.loading, |button| {
                button.when_some(self.icon, |button, icon| {
                    button.child(Icon::new(icon).size(IconSize::Medium).color(foreground))
                })
            })
            .child(self.label)
    }
}

/// Square icon-only button. An accessibility label is always required.
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    aria_label: Option<SharedString>,
    variant: ButtonVariant,
    disabled: bool,
    loading: bool,
    hover_background: Option<Hsla>,
    active_background: Option<Hsla>,
    tooltip: Option<Tooltip>,
    focus: Option<FocusHandle>,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    #[must_use]
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            aria_label: None,
            variant: ButtonVariant::Ghost,
            disabled: false,
            loading: false,
            hover_background: None,
            active_background: None,
            tooltip: None,
            focus: None,
            on_click: None,
        }
    }

    #[must_use]
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    #[must_use]
    pub fn style(mut self, variant: ButtonStyle) -> Self {
        self.variant = variant;
        self
    }

    #[must_use]
    pub fn variant(self, variant: ButtonVariant) -> Self {
        self.style(variant)
    }

    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[must_use]
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    #[must_use]
    pub fn hover_background(mut self, color: Hsla) -> Self {
        self.hover_background = Some(color);
        self
    }

    #[must_use]
    pub fn active_background(mut self, color: Hsla) -> Self {
        self.active_background = Some(color);
        self
    }

    #[must_use]
    pub fn tooltip(mut self, tooltip: Tooltip) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    #[must_use]
    pub fn track_focus(mut self, focus: FocusHandle) -> Self {
        self.focus = Some(focus);
        self
    }

    #[must_use]
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    #[must_use]
    pub fn render(self, colors: ThemeColors) -> Stateful<gpui::Div> {
        let enabled = !self.disabled && !self.loading;
        let (background, foreground, border, default_hover, default_active) = match self.variant {
            ButtonVariant::Primary => (
                colors.accent,
                colors.text_inverse,
                colors.accent,
                colors.accent_hover,
                colors.accent_active,
            ),
            ButtonVariant::Secondary => (
                colors.elevated_surface,
                colors.text_primary,
                colors.border,
                colors.surface_hover,
                colors.surface_active,
            ),
            ButtonVariant::Ghost => (
                transparent(),
                colors.text_secondary,
                transparent(),
                colors.surface_hover,
                colors.surface_active,
            ),
            ButtonVariant::Danger => (
                transparent(),
                colors.danger,
                transparent(),
                with_alpha(colors.danger, 0.12),
                with_alpha(colors.danger, 0.2),
            ),
        };
        let hover = self.hover_background.unwrap_or(default_hover);
        let active = self.active_background.unwrap_or(default_active);
        let label = self.aria_label.unwrap_or_else(|| "Icon button".into());
        let mut button = div()
            .id(self.id)
            .role(Role::Button)
            .aria_label(label.clone())
            .focusable()
            .tab_stop(enabled)
            .size(px(32.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(background)
            .text_color(foreground)
            .when(enabled, |button| {
                button
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(hover))
                    .active(move |style| style.bg(active))
            })
            .when(!enabled, |button| button.opacity(0.45));
        if let Some(focus) = self.focus {
            button = button.track_focus(&focus);
        }
        if let Some(tooltip) = self.tooltip {
            button = button.tooltip(Tooltip::text(tooltip.title, tooltip.meta, colors));
        }
        if enabled && let Some(handler) = self.on_click {
            button = button.on_click(move |event, window, cx| handler(event, window, cx));
        }
        // Toolbar / chrome icon buttons are 32px hit targets; keep the glyph
        // at a fixed 16px so every action reads the same optical size.
        button.child(if self.loading {
            LoadingIndicator::new(foreground)
                .size(IconSize::Medium)
                .into_any_element()
        } else {
            Icon::new(self.icon)
                .size(IconSize::Medium)
                .color(foreground)
                .into_any_element()
        })
    }
}

/// Compact status dot with an optional label.
#[derive(IntoElement)]
pub struct StatusIndicator {
    color: Hsla,
    label: Option<SharedString>,
}

impl StatusIndicator {
    #[must_use]
    pub fn new(color: Hsla) -> Self {
        Self { color, label: None }
    }

    #[must_use]
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl RenderOnce for StatusIndicator {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let label = self.label;
        div()
            .id("status-indicator")
            .role(Role::Status)
            .when_some(label.clone(), |indicator, label| {
                indicator.aria_label(label)
            })
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .child(
                div()
                    .size(px(7.0))
                    .rounded_full()
                    .bg(self.color)
                    .flex_none(),
            )
            .when_some(label, |indicator, label| indicator.child(label))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Segment {
    pub label: SharedString,
    pub icon: Option<IconName>,
}

impl Segment {
    #[must_use]
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            icon: None,
        }
    }

    #[must_use]
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// Mutually exclusive selection control for short option sets.
#[derive(IntoElement)]
pub struct SegmentedControl {
    id: SharedString,
    segments: Vec<Segment>,
    selected: usize,
    disabled: bool,
    theme: Theme,
    on_select: Option<SelectionHandler>,
}

impl SegmentedControl {
    #[must_use]
    pub fn new(
        id: impl Into<SharedString>,
        segments: impl IntoIterator<Item = Segment>,
        selected: usize,
        theme: Theme,
    ) -> Self {
        Self {
            id: id.into(),
            segments: segments.into_iter().collect(),
            selected,
            disabled: false,
            theme,
            on_select: None,
        }
    }

    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[must_use]
    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SegmentedControl {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let colors = self.theme.colors;
        let disabled = self.disabled;
        let selected = self.selected;
        let handler = self.on_select;
        let id = self.id.clone();
        div()
            .id(id.clone())
            .role(Role::Group)
            .flex_none()
            .flex()
            .items_center()
            .gap_0p5()
            .p_0p5()
            .rounded_lg()
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .children(
                self.segments
                    .into_iter()
                    .enumerate()
                    .map(move |(index, segment)| {
                        let is_selected = index == selected;
                        let handler = handler.clone();
                        let label = segment.label;
                        let foreground = if is_selected {
                            colors.text_primary
                        } else {
                            colors.text_secondary
                        };
                        let mut button = div()
                            .id((id.clone(), index))
                            .role(Role::RadioButton)
                            .aria_label(label.clone())
                            .aria_selected(is_selected)
                            .focusable()
                            .tab_stop(!disabled)
                            .h(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_1p5()
                            .px_2()
                            .rounded_md()
                            .text_xs()
                            .text_color(foreground)
                            .when(is_selected, |button| button.bg(colors.elevated_surface))
                            .when(!disabled, |button| {
                                button
                                    .cursor(CursorStyle::PointingHand)
                                    .hover(move |style| style.bg(colors.surface_hover))
                            })
                            .when(disabled, |button| button.opacity(0.45))
                            .when_some(segment.icon, |button, icon| {
                                button
                                    .child(Icon::new(icon).size(IconSize::Small).color(foreground))
                            })
                            .child(label);
                        if !disabled && let Some(handler) = handler {
                            button =
                                button.on_click(move |_, window, cx| handler(index, window, cx));
                        }
                        button
                    }),
            )
    }
}

/// Shared modal dialog surface and scrim.
#[derive(IntoElement)]
pub struct Dialog {
    id: SharedString,
    title: SharedString,
    description: Option<SharedString>,
    body: Vec<AnyElement>,
    actions: Vec<AnyElement>,
    width: f32,
    theme: Theme,
    key_context: Option<&'static str>,
    focus: Option<FocusHandle>,
}

impl Dialog {
    #[must_use]
    pub fn new(id: impl Into<SharedString>, title: impl Into<SharedString>, theme: Theme) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            body: Vec::new(),
            actions: Vec::new(),
            width: 440.0,
            theme,
            key_context: None,
            focus: None,
        }
    }

    #[must_use]
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    #[must_use]
    pub fn key_context(mut self, key_context: &'static str) -> Self {
        self.key_context = Some(key_context);
        self
    }

    #[must_use]
    pub fn track_focus(mut self, focus: FocusHandle) -> Self {
        self.focus = Some(focus);
        self
    }

    #[must_use]
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.body.push(child.into_any_element());
        self
    }

    #[must_use]
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl RenderOnce for Dialog {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let colors = self.theme.colors;
        div()
            .id(SharedString::from(format!("dialog-scrim-{}", self.id)))
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(with_alpha(colors.background, 0.72))
            .occlude()
            .child(
                div()
                    .id(self.id)
                    .role(Role::Dialog)
                    .aria_label(self.title.clone())
                    .when_some(self.key_context, |dialog, key_context| {
                        dialog.key_context(key_context)
                    })
                    .when_some(self.focus, |dialog, focus| dialog.track_focus(&focus))
                    .w(px(self.width))
                    .max_w_full()
                    .flex()
                    .flex_col()
                    .rounded_lg()
                    .border_1()
                    .border_color(colors.border_strong)
                    .bg(colors.elevated_surface)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .px_5()
                            .pt_5()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(colors.text_primary)
                                    .child(self.title),
                            )
                            .when_some(self.description, |header, description| {
                                header.child(
                                    div()
                                        .text_sm()
                                        .text_color(colors.text_secondary)
                                        .child(description),
                                )
                            }),
                    )
                    .child(div().flex().flex_col().gap_3().p_5().children(self.body))
                    .when(!self.actions.is_empty(), |dialog| {
                        dialog.child(
                            div()
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap_2()
                                .px_5()
                                .pb_5()
                                .children(self.actions),
                        )
                    }),
            )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToastKind {
    Success,
    Error,
    Information,
}

/// Non-layout-shifting operation feedback surface.
#[derive(IntoElement)]
pub struct Toast {
    id: ElementId,
    message: SharedString,
    kind: ToastKind,
    theme: Theme,
    on_close: Option<ClickHandler>,
}

impl Toast {
    #[must_use]
    pub fn new(
        id: impl Into<ElementId>,
        message: impl Into<SharedString>,
        kind: ToastKind,
        theme: Theme,
    ) -> Self {
        Self {
            id: id.into(),
            message: message.into(),
            kind,
            theme,
            on_close: None,
        }
    }

    #[must_use]
    pub fn on_close(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Toast {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let colors = self.theme.colors;
        let (icon, color, role) = match self.kind {
            ToastKind::Success => (IconName::CircleCheck, colors.success, Role::Status),
            ToastKind::Error => (IconName::CircleX, colors.danger, Role::Alert),
            ToastKind::Information => (IconName::Info, colors.information, Role::Status),
        };
        let close_id = ElementId::NamedChild(self.id.clone().into(), "close".into());
        let mut close = div()
            .id(close_id)
            .role(Role::Button)
            .aria_label("Dismiss notification")
            .size(px(26.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor(CursorStyle::PointingHand)
            .hover(move |style| style.bg(colors.surface_hover))
            .child(
                Icon::new(IconName::X)
                    .size(IconSize::Small)
                    .color(colors.text_muted),
            );
        if let Some(handler) = self.on_close {
            close = close.on_click(move |event, window, cx| handler(event, window, cx));
        }
        div()
            .id(self.id)
            .role(role)
            .aria_label(self.message.clone())
            .min_w(px(280.0))
            .max_w(px(440.0))
            .flex()
            .items_center()
            .gap_3()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(colors.border_strong)
            .bg(colors.elevated_surface)
            .text_sm()
            .text_color(colors.text_primary)
            .child(Icon::new(icon).color(color))
            .child(div().min_w_0().flex_1().child(self.message))
            .child(close)
    }
}

/// Consistent compact pending-state glyph.
#[derive(IntoElement)]
pub struct LoadingIndicator {
    color: Hsla,
    size: IconSize,
    label: SharedString,
}

impl LoadingIndicator {
    #[must_use]
    pub fn new(color: Hsla) -> Self {
        Self {
            color,
            size: IconSize::Medium,
            label: "Loading".into(),
        }
    }

    #[must_use]
    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    #[must_use]
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }
}

impl RenderOnce for LoadingIndicator {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        Icon::new(IconName::LoaderCircle)
            .size(self.size)
            .color(self.color)
            .label(self.label)
    }
}

fn transparent() -> Hsla {
    gpui::transparent_black()
}

fn with_alpha(mut color: Hsla, alpha: f32) -> Hsla {
    color.a = alpha;
    color
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_uses_the_embedded_icon_namespace() {
        for icon in [
            IconName::Search,
            IconName::Settings,
            IconName::Plus,
            IconName::X,
            IconName::Pause,
            IconName::Play,
            IconName::RotateCcw,
            IconName::Trash2,
            IconName::Copy,
            IconName::Sun,
            IconName::Moon,
            IconName::Link,
            IconName::FolderDown,
            IconName::Download,
            IconName::CircleCheck,
            IconName::CircleAlert,
            IconName::CircleX,
            IconName::Info,
            IconName::LoaderCircle,
            IconName::MoreHorizontal,
            IconName::PanelRight,
            IconName::Wifi,
            IconName::WifiOff,
            IconName::ArrowDown,
            IconName::ArrowUp,
            IconName::TriangleAlert,
            IconName::CloudOff,
            IconName::SearchX,
            IconName::Inbox,
            IconName::Activity,
            IconName::CircleHelp,
            IconName::Clock3,
            IconName::List,
            IconName::RefreshCw,
            IconName::ScanSearch,
            IconName::Minus,
            IconName::Square,
            IconName::ArrowUpDown,
            IconName::ChevronUp,
            IconName::ChevronDown,
            IconName::ChevronsUp,
            IconName::ChevronsDown,
            IconName::Check,
            IconName::WindowMinimize,
            IconName::WindowMaximize,
            IconName::WindowRestore,
            IconName::WindowClose,
        ] {
            assert!(icon.path().starts_with("icons/"));
            assert!(icon.path().ends_with(".svg"));
        }
    }

    #[test]
    fn segments_support_optional_icons() {
        let segment = Segment::new("Dark").icon(IconName::Moon);
        assert_eq!(segment.label.as_ref(), "Dark");
        assert_eq!(segment.icon, Some(IconName::Moon));
    }
}
