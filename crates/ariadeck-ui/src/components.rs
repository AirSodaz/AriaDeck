use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, App, AppContext as _, ClickEvent, CursorStyle, ElementId, FocusHandle,
    Hsla, IntoElement, Render, RenderOnce, Role, SharedString, Stateful, Window, div, prelude::*,
    px, svg,
};

pub use gpui_kit::component::button::ButtonVariants;

use crate::{Theme, ThemeColors};

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type SelectionHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

const CONTROL_RADIUS: f32 = 4.0;
const OVERLAY_RADIUS: f32 = 6.0;
const DISABLED_OPACITY: f32 = 0.48;
const LOADING_OPACITY: f32 = 0.72;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, IntoElement)]
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
    /// Windows caption glyphs.
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

impl gpui_kit::assets::IconNamed for IconName {
    fn path(self) -> SharedString {
        self.path().into()
    }
}

impl RenderOnce for IconName {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let text_style = window.text_style();
        svg()
            .path(self.path())
            .flex_shrink_0()
            .size(text_style.font_size.to_pixels(window.rem_size()))
            .text_color(text_style.color)
    }
}

impl From<IconName> for AnyElement {
    fn from(name: IconName) -> Self {
        name.into_any_element()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IconSize {
    XSmall,
    Small,
    Medium,
    Large,
}

impl IconSize {
    #[must_use]
    pub const fn pixels(self) -> f32 {
        match self {
            Self::XSmall => 12.0,
            Self::Small => 14.0,
            Self::Medium => 16.0,
            Self::Large => 20.0,
        }
    }
}

/// Standard icon rendering.
#[derive(Clone, Debug, PartialEq, IntoElement)]
pub struct Icon {
    pub name: IconName,
    pub size: IconSize,
    pub color: Option<Hsla>,
    pub label: Option<SharedString>,
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
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut element = svg()
            .path(self.name.path())
            .size(px(self.size.pixels()))
            .flex_none();
        if let Some(color) = self.color {
            element = element.text_color(color);
        }
        element
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
            .rounded(px(OVERLAY_RADIUS))
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

/// Standard button component with fluent variants and unified styling.
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<IconName>,
    variant: ButtonVariant,
    disabled: bool,
    loading: bool,
    compact: bool,
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
            label: Some(label.into()),
            icon: None,
            variant: ButtonVariant::Secondary,
            disabled: false,
            loading: false,
            compact: false,
            aria_label: None,
            tooltip: None,
            focus: None,
            on_click: None,
        }
    }

    #[must_use]
    pub fn primary(mut self) -> Self {
        self.variant = ButtonVariant::Primary;
        self
    }

    #[must_use]
    pub fn secondary(mut self) -> Self {
        self.variant = ButtonVariant::Secondary;
        self
    }

    #[must_use]
    pub fn danger(mut self) -> Self {
        self.variant = ButtonVariant::Danger;
        self
    }

    #[must_use]
    pub fn ghost(mut self) -> Self {
        self.variant = ButtonVariant::Ghost;
        self
    }

    #[must_use]
    pub fn outline(mut self) -> Self {
        self.variant = ButtonVariant::Secondary;
        self
    }

    #[must_use]
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
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
    pub fn tooltip_with_action(
        mut self,
        action: impl Into<SharedString>,
        tooltip: Tooltip,
    ) -> Self {
        self.tooltip = Some(tooltip.meta(action));
        self
    }

    #[must_use]
    pub fn focus(mut self, focus: FocusHandle) -> Self {
        self.focus = Some(focus);
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
                gpui::transparent_white(),
                colors.text_secondary,
                gpui::transparent_white(),
                colors.surface_hover,
                colors.surface_active,
            ),
            ButtonVariant::Danger => {
                let mut hover_danger = colors.danger;
                hover_danger.l = (hover_danger.l * 0.9).clamp(0.0, 1.0);
                let mut active_danger = colors.danger;
                active_danger.l = (active_danger.l * 0.8).clamp(0.0, 1.0);
                (
                    colors.danger,
                    colors.text_inverse,
                    colors.danger,
                    hover_danger,
                    active_danger,
                )
            }
        };
        let tooltip = self.tooltip;
        let aria_label = self
            .aria_label
            .or_else(|| self.label.clone())
            .unwrap_or_default();
        let height = if self.compact { px(26.0) } else { px(32.0) };
        let px_pad = if self.compact { px(8.0) } else { px(12.0) };
        let mut button = div()
            .id(self.id)
            .role(Role::Button)
            .aria_label(aria_label)
            .focusable()
            .tab_stop(enabled)
            .focus_visible(move |style| style.border_2().border_color(colors.focus_ring))
            .h(height)
            .min_w(height)
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .px(px_pad)
            .rounded(px(CONTROL_RADIUS))
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
            .when(self.disabled, |button| {
                button
                    .cursor(CursorStyle::OperationNotAllowed)
                    .opacity(DISABLED_OPACITY)
            })
            .when(self.loading, |button| button.opacity(LOADING_OPACITY));

        if let Some(focus) = self.focus.as_ref() {
            button = button.track_focus(focus);
        }
        if let Some(handler) = self.on_click {
            button = button.on_click(move |event, window, cx| {
                if !enabled {
                    return;
                }
                handler(event, window, cx);
            });
        }
        if let Some(mut tooltip) = tooltip {
            tooltip.colors = colors;
            button = button.tooltip(move |_window, cx| {
                let tooltip = tooltip.clone();
                cx.new(|_| tooltip).into()
            });
        }
        if self.loading {
            let indicator_size = if self.compact {
                IconSize::Small
            } else {
                IconSize::Medium
            };
            let icon_color = match self.variant {
                ButtonVariant::Primary | ButtonVariant::Danger => colors.text_inverse,
                ButtonVariant::Ghost => colors.text_secondary,
                ButtonVariant::Secondary => colors.text_primary,
            };
            button = button.child(LoadingIndicator::new(icon_color).size(indicator_size));
        } else if let Some(icon) = self.icon {
            let icon_color = match self.variant {
                ButtonVariant::Primary | ButtonVariant::Danger => colors.text_inverse,
                ButtonVariant::Ghost => colors.text_secondary,
                ButtonVariant::Secondary => colors.text_primary,
            };
            button = button.child(
                Icon::new(icon)
                    .size(if self.compact {
                        IconSize::Small
                    } else {
                        IconSize::Medium
                    })
                    .color(icon_color),
            );
        }
        if let Some(label) = self.label {
            button = button.child(label);
        }
        button
    }
}

impl IntoElement for Button {
    type Element = Stateful<gpui::Div>;
    fn into_element(self) -> Self::Element {
        let colors = Theme::dark().colors;
        self.render(colors)
    }
}

/// Icon-only button.
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    variant: ButtonVariant,
    disabled: bool,
    loading: bool,
    compact: bool,
    aria_label: Option<SharedString>,
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
            variant: ButtonVariant::Ghost,
            disabled: false,
            loading: false,
            compact: false,
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
    pub fn style(mut self, variant: ButtonStyle) -> Self {
        self.variant = variant;
        self
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
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    #[must_use]
    pub fn tooltip(mut self, tooltip: Tooltip) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    #[must_use]
    pub fn focus(mut self, focus: FocusHandle) -> Self {
        self.focus = Some(focus);
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
                gpui::transparent_white(),
                colors.text_secondary,
                gpui::transparent_white(),
                colors.surface_hover,
                colors.surface_active,
            ),
            ButtonVariant::Danger => {
                let mut hover_danger = colors.danger;
                hover_danger.l = (hover_danger.l * 0.9).clamp(0.0, 1.0);
                let mut active_danger = colors.danger;
                active_danger.l = (active_danger.l * 0.8).clamp(0.0, 1.0);
                (
                    colors.danger,
                    colors.text_inverse,
                    colors.danger,
                    hover_danger,
                    active_danger,
                )
            }
        };
        let tooltip = self.tooltip;
        let aria_label = self.aria_label.unwrap_or_default();
        let size = if self.compact { px(26.0) } else { px(32.0) };
        let mut button = div()
            .id(self.id)
            .role(Role::Button)
            .aria_label(aria_label)
            .focusable()
            .tab_stop(enabled)
            .focus_visible(move |style| style.border_2().border_color(colors.focus_ring))
            .size(size)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(CONTROL_RADIUS))
            .border_1()
            .border_color(border)
            .bg(background)
            .when(enabled, |button| {
                button
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(hover))
                    .active(move |style| style.bg(active))
            })
            .when(self.disabled, |button| {
                button
                    .cursor(CursorStyle::OperationNotAllowed)
                    .opacity(DISABLED_OPACITY)
            })
            .when(self.loading, |button| button.opacity(LOADING_OPACITY));

        if let Some(focus) = self.focus.as_ref() {
            button = button.track_focus(focus);
        }
        if let Some(handler) = self.on_click {
            button = button.on_click(move |event, window, cx| {
                if !enabled {
                    return;
                }
                handler(event, window, cx);
            });
        }
        if let Some(mut tooltip) = tooltip {
            tooltip.colors = colors;
            button = button.tooltip(move |_window, cx| {
                let tooltip = tooltip.clone();
                cx.new(|_| tooltip).into()
            });
        }
        let icon_size = if self.compact {
            IconSize::Small
        } else {
            IconSize::Medium
        };
        if self.loading {
            button = button.child(LoadingIndicator::new(foreground).size(icon_size));
        } else {
            button = button.child(Icon::new(self.icon).size(icon_size).color(foreground));
        }
        button
    }
}

impl IntoElement for IconButton {
    type Element = Stateful<gpui::Div>;
    fn into_element(self) -> Self::Element {
        let colors = Theme::dark().colors;
        self.render(colors)
    }
}

/// Pill-shaped switch.
pub struct Toggle {
    pub id: ElementId,
    pub on: bool,
    pub disabled: bool,
    pub aria_label: Option<SharedString>,
    pub on_click: Option<ClickHandler>,
}

impl Toggle {
    #[must_use]
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            disabled: false,
            aria_label: None,
            on_click: None,
        }
    }

    #[must_use]
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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
        let enabled = !self.disabled;
        let on = self.on;
        let (track_bg, thumb_bg, border_color) = if on {
            (colors.accent, colors.text_inverse, colors.accent)
        } else {
            (colors.surface, colors.text_muted, colors.border_strong)
        };
        let aria_label = self.aria_label.unwrap_or_else(|| {
            if on {
                "Enabled".into()
            } else {
                "Disabled".into()
            }
        });
        let mut toggle = div()
            .id(self.id)
            .role(Role::Switch)
            .aria_label(aria_label)
            .aria_toggled(if on {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
            .focusable()
            .tab_stop(enabled)
            .focus_visible(move |style| style.border_2().border_color(colors.focus_ring))
            .w(px(36.0))
            .h(px(20.0))
            .flex_none()
            .flex()
            .items_center()
            .px(px(2.0))
            .rounded_full()
            .border_1()
            .border_color(border_color)
            .bg(track_bg)
            .when(enabled, |toggle| {
                toggle
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| {
                        style.bg(if on {
                            colors.accent_hover
                        } else {
                            colors.surface_hover
                        })
                    })
            })
            .when(self.disabled, |toggle| {
                toggle
                    .cursor(CursorStyle::OperationNotAllowed)
                    .opacity(DISABLED_OPACITY)
            });

        if let Some(handler) = self.on_click {
            toggle = toggle.on_click(move |event, window, cx| {
                if !enabled {
                    return;
                }
                handler(event, window, cx);
            });
        }

        let thumb = div().size(px(14.0)).rounded_full().bg(thumb_bg).shadow_sm();

        if on {
            toggle.justify_end().child(thumb)
        } else {
            toggle.justify_start().child(thumb)
        }
    }
}

impl IntoElement for Toggle {
    type Element = Stateful<gpui::Div>;
    fn into_element(self) -> Self::Element {
        let colors = Theme::dark().colors;
        self.render(colors)
    }
}

/// One segment inside SegmentedControl.
#[derive(Clone, Debug, PartialEq)]
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

/// Segmented control.
pub struct SegmentedControl {
    pub id: ElementId,
    pub segments: Vec<Segment>,
    pub selected_index: usize,
    pub disabled: bool,
    pub aria_label: Option<SharedString>,
    pub on_select: Option<SelectionHandler>,
    pub theme: Theme,
}

impl SegmentedControl {
    #[must_use]
    pub fn new(
        id: impl Into<ElementId>,
        segments: impl IntoIterator<Item = Segment>,
        selected_index: usize,
        theme: Theme,
    ) -> Self {
        Self {
            id: id.into(),
            segments: segments.into_iter().collect(),
            selected_index,
            disabled: false,
            aria_label: None,
            on_select: None,
            theme,
        }
    }

    #[must_use]
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
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

    #[must_use]
    pub fn render(self, colors: ThemeColors) -> Stateful<gpui::Div> {
        let id = self.id;
        let selected = self.selected_index;
        let disabled = self.disabled;
        let handler = self.on_select;
        let segment_count = self.segments.len();
        let aria_label = self.aria_label.unwrap_or_else(|| "Options".into());

        div()
            .id(id.clone())
            .role(Role::RadioGroup)
            .aria_label(aria_label)
            .flex()
            .items_center()
            .gap_1()
            .p(px(2.0))
            .rounded(px(CONTROL_RADIUS))
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .min_h(px(32.0))
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
                        let segment_aria = if segment_count > 1 {
                            format!("{}, {} of {}", label.as_ref(), index + 1, segment_count)
                        } else {
                            label.to_string()
                        };
                        let mut button = div()
                            .id(ElementId::NamedChild(
                                id.clone().into(),
                                SharedString::from(format!("{index}")),
                            ))
                            .role(Role::RadioButton)
                            .aria_label(segment_aria)
                            .aria_selected(is_selected)
                            .focusable()
                            .tab_stop(!disabled)
                            .focus_visible(move |style| {
                                style.border_2().border_color(colors.focus_ring)
                            })
                            .h(px(28.0))
                            .min_w(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_1p5()
                            .px_2()
                            .rounded(px(CONTROL_RADIUS))
                            .text_xs()
                            .text_color(foreground)
                            .when(is_selected, |button| {
                                button.bg(colors.elevated_surface).shadow_sm()
                            })
                            .when(!disabled, |button| {
                                button
                                    .cursor(CursorStyle::PointingHand)
                                    .hover(move |style| style.bg(colors.surface_hover))
                            })
                            .when(disabled, |button| {
                                button
                                    .cursor(CursorStyle::OperationNotAllowed)
                                    .opacity(DISABLED_OPACITY)
                            })
                            .when_some(segment.icon, |button, icon| {
                                button
                                    .child(Icon::new(icon).size(IconSize::Small).color(foreground))
                            })
                            .child(label);

                        if let Some(handler) = handler {
                            button = button.on_click(move |_event, window, cx| {
                                if disabled {
                                    return;
                                }
                                handler(index, window, cx);
                            });
                        }
                        button
                    }),
            )
    }
}

impl IntoElement for SegmentedControl {
    type Element = Stateful<gpui::Div>;
    fn into_element(self) -> Self::Element {
        let colors = self.theme.colors;
        self.render(colors)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToastKind {
    Information,
    Success,
    Error,
}

#[derive(IntoElement)]
pub struct Toast {
    id: ElementId,
    message: SharedString,
    kind: ToastKind,
    theme: Theme,
    close_label: SharedString,
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
            close_label: "Dismiss notification".into(),
            on_close: None,
        }
    }

    #[must_use]
    pub fn close_label(mut self, label: impl Into<SharedString>) -> Self {
        self.close_label = label.into();
        self
    }

    #[must_use]
    pub fn on_close(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }

    #[must_use]
    pub fn render(self, _colors: ThemeColors) -> Self {
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
            .aria_label(self.close_label)
            .focusable()
            .tab_stop(true)
            .size(px(26.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(CONTROL_RADIUS))
            .cursor(CursorStyle::PointingHand)
            .hover(move |style| style.bg(colors.surface_hover))
            .active(move |style| style.bg(colors.surface_active))
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
            .rounded(px(OVERLAY_RADIUS))
            .border_1()
            .border_color(colors.border_strong)
            .bg(colors.elevated_surface)
            .shadow_lg()
            .text_sm()
            .text_color(colors.text_primary)
            .child(Icon::new(icon).color(color))
            .child(div().min_w_0().flex_1().child(self.message))
            .child(close)
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
            width: 480.0,
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

    #[must_use]
    pub fn render(self, _colors: ThemeColors) -> Self {
        self
    }
}

impl RenderOnce for Dialog {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let colors = self.theme.colors;
        let mut card = div()
            .id(self.id)
            .role(Role::Dialog)
            .aria_label(self.title.clone())
            .w(px(self.width))
            .max_w(px(720.0))
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .rounded(px(OVERLAY_RADIUS))
            .border_1()
            .border_color(colors.border_strong)
            .bg(colors.elevated_surface)
            .shadow_lg()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(colors.text_primary)
                            .child(self.title),
                    )
                    .when_some(self.description, |header, desc| {
                        header.child(
                            div()
                                .text_sm()
                                .text_color(colors.text_secondary)
                                .child(desc),
                        )
                    }),
            )
            .children(self.body)
            .when(!self.actions.is_empty(), |card| {
                card.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_3()
                        .pt_2()
                        .children(self.actions),
                )
            });

        if let Some(key_context) = self.key_context {
            card = card.key_context(key_context);
        }
        if let Some(focus) = self.focus {
            card = card.track_focus(&focus);
        }

        div()
            .id("dialog-scrim")
            .occlude()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00_00_00_aa))
            .child(card)
    }
}

/// Visual status indicator dot with optional label and icon.
#[derive(IntoElement)]
pub struct StatusIndicator {
    color: Hsla,
    label: Option<SharedString>,
    icon: Option<IconName>,
}

impl StatusIndicator {
    #[must_use]
    pub fn new(color: Hsla) -> Self {
        Self {
            color,
            label: None,
            icon: None,
        }
    }

    #[must_use]
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    #[must_use]
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
}

impl RenderOnce for StatusIndicator {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .when_some(self.icon, |el, icon| {
                el.child(Icon::new(icon).size(IconSize::XSmall).color(self.color))
            })
            .when(self.icon.is_none(), |el| {
                el.child(
                    div()
                        .size(px(8.0))
                        .rounded_full()
                        .bg(self.color)
                        .flex_none(),
                )
            })
            .when_some(self.label, |el, label| {
                el.child(div().text_xs().text_color(self.color).child(label))
            })
    }
}

/// Loading spinner glyph.
#[derive(IntoElement)]
pub struct LoadingIndicator {
    color: Hsla,
    size: IconSize,
    label: SharedString,
    reduced_motion: bool,
}

impl LoadingIndicator {
    #[must_use]
    pub fn new(color: Hsla) -> Self {
        Self {
            color,
            size: IconSize::Medium,
            label: "Loading".into(),
            reduced_motion: crate::accessibility::prefers_reduced_motion(),
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

    #[must_use]
    pub fn reduced_motion(mut self, reduced: bool) -> Self {
        self.reduced_motion = reduced;
        self
    }
}

impl RenderOnce for LoadingIndicator {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let icon = if self.reduced_motion {
            IconName::Clock3
        } else {
            IconName::LoaderCircle
        };
        Icon::new(icon)
            .size(self.size)
            .color(self.color)
            .label(self.label)
    }
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

    #[test]
    fn toggle_reflects_on_off_state() {
        let on = Toggle::new("t1", true);
        assert!(on.on);
        assert!(!on.disabled);
        let off = Toggle::new("t2", false).disabled(true);
        assert!(!off.on);
        assert!(off.disabled);
    }

    #[test]
    fn button_chainable_variants() {
        let primary = Button::new("b1", "Primary").primary().compact();
        assert_eq!(primary.variant, ButtonVariant::Primary);
        assert!(primary.compact);

        let danger = Button::new("b2", "Delete").danger().disabled(true);
        assert_eq!(danger.variant, ButtonVariant::Danger);
        assert!(danger.disabled);

        let ghost = Button::new("b3", "Ghost").ghost().loading(true);
        assert_eq!(ghost.variant, ButtonVariant::Ghost);
        assert!(ghost.loading);
    }

    #[test]
    fn status_indicator_accepts_label_and_icon() {
        let indicator = StatusIndicator::new(gpui::rgb(0x00_ff_00).into())
            .label("Connected")
            .icon(IconName::Wifi);
        assert_eq!(indicator.label.as_deref(), Some("Connected"));
        assert_eq!(indicator.icon, Some(IconName::Wifi));
    }

    #[test]
    fn loading_indicator_respects_reduced_motion_flag() {
        let reduced = LoadingIndicator::new(gpui::rgb(0xff_ff_ff).into()).reduced_motion(true);
        assert!(reduced.reduced_motion);
        let normal = LoadingIndicator::new(gpui::rgb(0xff_ff_ff).into()).reduced_motion(false);
        assert!(!normal.reduced_motion);
    }
}
