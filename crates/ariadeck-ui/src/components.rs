use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnimationExt as _, AnyElement, AnyView, App, AppContext as _, ClickEvent, ElementId,
    FocusHandle, Hsla, IntoElement, Render, RenderOnce, Role, SharedString, Stateful, Window, div,
    prelude::*, px, svg,
};

pub use gpui_kit::component::badge::Badge;
pub use gpui_kit::component::button::{
    Button as KitButton, ButtonVariant as KitButtonVariant, ButtonVariants,
};
pub use gpui_kit::component::checkbox::Checkbox;
pub use gpui_kit::component::kbd::Kbd;
pub use gpui_kit::component::notification::Notification;
pub use gpui_kit::component::progress::{Progress, ProgressCircle};
pub use gpui_kit::component::spinner::Spinner;
pub use gpui_kit::component::switch::Switch;
pub use gpui_kit::component::tab::{Tab, TabBar, TabVariant};
pub use gpui_kit::component::tag::{Tag, TagVariant};
pub use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _};

use crate::{Theme, ThemeColors};
type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type SelectionHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

const CONTROL_RADIUS: f32 = 4.0;
const OVERLAY_RADIUS: f32 = 6.0;
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
            .gap_2()
            .max_w(px(280.0))
            .px_2p5()
            .py_1()
            .rounded(px(OVERLAY_RADIUS))
            .border_1()
            .border_color(colors.border_strong)
            .bg(colors.elevated_surface)
            .shadow_md()
            .text_xs()
            .text_color(colors.text_primary)
            .child(self.title.clone())
            .when_some(self.meta.clone(), |tooltip, shortcut| {
                tooltip.child(
                    div()
                        .px_1()
                        .py_0p5()
                        .rounded(px(2.0))
                        .bg(colors.surface)
                        .border_1()
                        .border_color(colors.border)
                        .text_color(colors.text_muted)
                        .font_family("monospace")
                        .child(shortcut),
                )
            })
            .with_animation(
                "tooltip-enter",
                gpui::Animation::new(Duration::from_millis(150)).with_easing(
                    gpui_kit::component::animation::cubic_bezier(0.16, 1.0, 0.3, 1.0),
                ),
                |this, delta| this.opacity(delta),
            )
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
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<IconName>,
    pub variant: ButtonVariant,
    pub disabled: bool,
    pub loading: bool,
    pub compact: bool,
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
    pub fn render(self, _colors: ThemeColors) -> Stateful<gpui::Div> {
        let wrapper_id = ElementId::NamedChild(self.id.clone().into(), "render-wrapper".into());
        div().id(wrapper_id).child(self)
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut btn = KitButton::new(self.id);

        let kit_variant = match self.variant {
            ButtonVariant::Primary => KitButtonVariant::Primary,
            ButtonVariant::Secondary => KitButtonVariant::Secondary,
            ButtonVariant::Danger => KitButtonVariant::Danger,
            ButtonVariant::Ghost => KitButtonVariant::Ghost,
        };
        btn = btn.with_variant(kit_variant);

        if let Some(label) = self.label {
            btn = btn.label(label);
        }
        if let Some(icon) = self.icon {
            btn = btn.icon(gpui_kit::component::Icon::new(icon));
        } else if self.loading {
            btn = btn.icon(Spinner::new());
        }
        if self.compact {
            btn = btn.compact();
        }
        if self.disabled {
            btn = btn.disabled(true);
        }
        if self.loading {
            btn = btn.loading(true);
        }
        if let Some(aria_label) = self.aria_label {
            btn = btn.accessibility_label(aria_label);
        }
        if let Some(tooltip) = self.tooltip {
            let text = if let Some(meta) = tooltip.meta {
                SharedString::from(format!("{} ({})", tooltip.title, meta))
            } else {
                tooltip.title
            };
            btn = btn.tooltip(text);
        }
        let click_handler = self.on_click;
        if let Some(handler) = click_handler.clone() {
            btn = btn.on_click(move |event, window, cx| {
                handler(event, window, cx);
            });
        }

        if self.focus.is_some() || self.disabled || self.loading {
            btn = btn.tab_stop(false);
        }

        if let Some(focus) = self.focus {
            let is_focused = focus.is_focused(window);
            let mut wrapper = div().track_focus(&focus).when(is_focused, |this| {
                this.rounded(px(CONTROL_RADIUS))
                    .border_2()
                    .border_color(cx.theme().ring)
            });
            let disabled = self.disabled;
            let loading = self.loading;
            if let Some(handler) = click_handler {
                wrapper = wrapper.on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
                    if disabled || loading || event.is_held {
                        return;
                    }
                    if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                        cx.stop_propagation();
                        let click = ClickEvent::default();
                        handler(&click, window, cx);
                    }
                });
            }
            wrapper.child(btn).into_any_element()
        } else {
            btn.into_any_element()
        }
    }
}

/// Icon-only button.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    pub variant: ButtonVariant,
    pub disabled: bool,
    pub loading: bool,
    pub compact: bool,
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
    pub fn render(self, _colors: ThemeColors) -> Stateful<gpui::Div> {
        let wrapper_id = ElementId::NamedChild(self.id.clone().into(), "render-wrapper".into());
        div().id(wrapper_id).child(self)
    }
}

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let size = if self.compact {
            gpui_kit::component::Size::Small
        } else {
            gpui_kit::component::Size::Medium
        };
        let mut btn = KitButton::new(self.id)
            .icon(gpui_kit::component::Icon::new(self.icon))
            .with_size(size);

        let kit_variant = match self.variant {
            ButtonVariant::Primary => KitButtonVariant::Primary,
            ButtonVariant::Secondary => KitButtonVariant::Secondary,
            ButtonVariant::Danger => KitButtonVariant::Danger,
            ButtonVariant::Ghost => KitButtonVariant::Ghost,
        };
        btn = btn.with_variant(kit_variant);
        if self.compact {
            btn = btn.compact();
        }

        if self.disabled {
            btn = btn.disabled(true);
        }
        if self.loading {
            btn = btn.loading(true);
        }
        if let Some(aria_label) = self.aria_label {
            btn = btn.accessibility_label(aria_label);
        }
        if let Some(tooltip) = self.tooltip {
            let text = if let Some(meta) = tooltip.meta {
                SharedString::from(format!("{} ({})", tooltip.title, meta))
            } else {
                tooltip.title
            };
            btn = btn.tooltip(text);
        }
        let click_handler = self.on_click;
        if let Some(handler) = click_handler.clone() {
            btn = btn.on_click(move |event, window, cx| {
                handler(event, window, cx);
            });
        }

        if self.focus.is_some() || self.disabled || self.loading {
            btn = btn.tab_stop(false);
        }

        if let Some(focus) = self.focus {
            let is_focused = focus.is_focused(window);
            let mut wrapper = div().track_focus(&focus).when(is_focused, |this| {
                this.rounded(px(CONTROL_RADIUS))
                    .border_2()
                    .border_color(cx.theme().ring)
            });
            let disabled = self.disabled;
            let loading = self.loading;
            if let Some(handler) = click_handler {
                wrapper = wrapper.on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
                    if disabled || loading || event.is_held {
                        return;
                    }
                    if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                        cx.stop_propagation();
                        let click = ClickEvent::default();
                        handler(&click, window, cx);
                    }
                });
            }
            wrapper.child(btn).into_any_element()
        } else {
            btn.into_any_element()
        }
    }
}

/// Pill-shaped switch.
#[derive(IntoElement)]
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
    pub fn render(self, _colors: ThemeColors) -> Self {
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut switch = Switch::new(self.id)
            .checked(self.on)
            .disabled(self.disabled);

        if let Some(aria_label) = self.aria_label {
            switch = switch.accessibility_label(aria_label);
        }

        if let Some(handler) = self.on_click {
            switch = switch.on_change(move |_checked, window, cx| {
                let event = ClickEvent::default();
                handler(&event, window, cx);
            });
        }

        switch
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
#[derive(IntoElement)]
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
    pub fn render(self, _colors: ThemeColors) -> Self {
        self
    }
}

impl RenderOnce for SegmentedControl {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut tab_bar = TabBar::new(self.id.clone())
            .segmented()
            .selected_index(self.selected_index);

        if let Some(on_select) = self.on_select {
            tab_bar = tab_bar.on_click(move |ix, window, cx| {
                on_select(*ix, window, cx);
            });
        }

        for segment in self.segments {
            let mut tab = Tab::new().disabled(self.disabled);

            if let Some(icon) = segment.icon {
                if segment.label.is_empty() {
                    tab = tab.icon(gpui_kit::component::Icon::new(icon));
                } else {
                    let content = div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(Icon::new(icon).size(IconSize::Small))
                        .child(segment.label.clone());
                    tab = tab.aria_label(segment.label).child(content);
                }
            } else {
                tab = tab.label(segment.label);
            }

            tab_bar = tab_bar.child(tab);
        }

        div()
            .id(ElementId::NamedChild(
                self.id.clone().into(),
                "group".into(),
            ))
            .role(Role::RadioGroup)
            .when_some(self.aria_label, |this, label| this.aria_label(label))
            .child(tab_bar)
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
        let mut close = IconButton::new(close_id, IconName::X)
            .aria_label(self.close_label)
            .compact()
            .ghost();
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
            .with_animation(
                "toast-enter",
                gpui::Animation::new(Duration::from_millis(250)).with_easing(
                    gpui_kit::component::animation::cubic_bezier(0.16, 1.0, 0.3, 1.0),
                ),
                |this, delta| this.opacity(delta),
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
            .id(self.id.clone())
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
            .shadow_xl()
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

        let animated_card = card.with_animation(
            "dialog-card-enter",
            gpui::Animation::new(Duration::from_millis(250)).with_easing(
                gpui_kit::component::animation::cubic_bezier(0.16, 1.0, 0.3, 1.0),
            ),
            |this, delta| this.opacity(delta),
        );

        let overlay_id = ElementId::NamedChild(ElementId::Name(self.id).into(), "overlay".into());
        div()
            .id(overlay_id)
            .occlude()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00_00_00_aa))
            .child(animated_card)
            .with_animation(
                "dialog-scrim-fade",
                gpui::Animation::new(Duration::from_millis(200)).with_easing(
                    gpui_kit::component::animation::cubic_bezier(0.16, 1.0, 0.3, 1.0),
                ),
                |this, delta| this.opacity(delta),
            )
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
        if self.reduced_motion {
            Icon::new(IconName::Clock3)
                .size(self.size)
                .color(self.color)
                .label(self.label)
                .into_any_element()
        } else {
            let spinner_size = match self.size {
                IconSize::XSmall => gpui_kit::component::Size::XSmall,
                IconSize::Small => gpui_kit::component::Size::Small,
                IconSize::Medium => gpui_kit::component::Size::Medium,
                IconSize::Large => gpui_kit::component::Size::Large,
            };
            Spinner::new()
                .with_size(spinner_size)
                .color(self.color)
                .into_any_element()
        }
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

    #[test]
    fn segmented_control_preserves_aria_label() {
        let control =
            SegmentedControl::new("theme-mode", vec![Segment::new("Light")], 0, Theme::dark())
                .aria_label("Theme mode selector");
        assert_eq!(control.aria_label.as_deref(), Some("Theme mode selector"));
    }

    #[test]
    fn icon_button_chainable_variants_and_compact() {
        let icon_btn = IconButton::new("ib1", IconName::Search).primary().compact();
        assert!(icon_btn.compact);
    }

    #[test]
    fn button_tooltip_with_meta() {
        let tooltip = Tooltip::new("Add task").meta("Ctrl+N");
        let btn = Button::new("b4", "Add").tooltip(tooltip);
        let tooltip = btn.tooltip.expect("tooltip should be configured");
        assert_eq!(tooltip.title.as_ref(), "Add task");
        assert_eq!(tooltip.meta.as_deref(), Some("Ctrl+N"));
    }
    #[test]
    fn segmented_control_supports_icons_and_labels() {
        let control = SegmentedControl::new(
            "theme-mode",
            vec![
                Segment::new("System").icon(IconName::Settings),
                Segment::new("Light").icon(IconName::Sun),
                Segment::new("Dark").icon(IconName::Moon),
            ],
            1,
            Theme::dark(),
        );
        assert_eq!(control.segments.len(), 3);
        assert_eq!(control.segments[0].label.as_ref(), "System");
        assert_eq!(control.segments[0].icon, Some(IconName::Settings));
        assert_eq!(control.segments[1].label.as_ref(), "Light");
        assert_eq!(control.segments[1].icon, Some(IconName::Sun));
        assert_eq!(control.segments[2].label.as_ref(), "Dark");
        assert_eq!(control.segments[2].icon, Some(IconName::Moon));
    }

    #[test]
    fn button_disabled_and_loading_states() {
        let btn = Button::new("b-test", "Save").disabled(true);
        assert!(btn.disabled);
        assert!(!btn.loading);

        let loading_btn = Button::new("b-loading", "Saving").loading(true);
        assert!(loading_btn.loading);
        assert!(!loading_btn.disabled);
    }
}
