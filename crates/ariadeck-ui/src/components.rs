use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, Hsla, IntoElement, RenderOnce, Role, SharedString, Window, div,
    prelude::*, px, svg,
};

pub use gpui_kit::component::badge::Badge;
pub use gpui_kit::component::checkbox::Checkbox;
pub use gpui_kit::component::kbd::Kbd;
pub use gpui_kit::component::notification::Notification;
pub use gpui_kit::component::progress::{Progress, ProgressCircle};
pub use gpui_kit::component::spinner::Spinner;
pub use gpui_kit::component::switch::Switch;
pub use gpui_kit::component::tab::{Tab, TabBar, TabVariant};
pub use gpui_kit::component::tag::{Tag, TagVariant};
pub use gpui_kit::component::{Disableable as _, Sizable as _};

use crate::Theme;

type SelectionHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

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
    Bell,
    FolderArchive,
    FileText,
    Music,
    Video,
    AppWindow,
    Image,
    File,
    Cpu,
    Server,
    Users,
    HardDrive,
    /// Resize grip corner for resizable panels and inputs.
    ResizeCorner,
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
            Self::Bell => "icons/bell.svg",
            Self::FolderArchive => "icons/folder-archive.svg",
            Self::FileText => "icons/file-text.svg",
            Self::Music => "icons/music.svg",
            Self::Video => "icons/video.svg",
            Self::AppWindow => "icons/app-window.svg",
            Self::Image => "icons/image.svg",
            Self::File => "icons/file.svg",
            Self::Cpu => "icons/cpu.svg",
            Self::Server => "icons/server.svg",
            Self::Users => "icons/users.svg",
            Self::HardDrive => "icons/hard-drive.svg",
            Self::ResizeCorner => "icons/resize-corner.svg",
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
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or_else(|| window.text_style().color);
        let glyph = svg()
            .path(self.name.path())
            .size(px(self.size.pixels()))
            .flex_none()
            .text_color(color);
        if let Some(label) = self.label {
            div()
                .id(ElementId::NamedChild(
                    ElementId::Name("icon".into()).into(),
                    label.clone(),
                ))
                .flex_none()
                .role(Role::Image)
                .aria_label(label)
                .child(glyph)
                .into_any_element()
        } else {
            glyph.into_any_element()
        }
    }
}

/// One segment in a segmented `TabBar`.
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

/// Segmented control backed directly by `gpui_kit::component::tab::TabBar::segmented`.
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

        // Preserve TabBar's native TabList/Tab ARIA semantics without wrapping in a
        // conflicting Role::RadioGroup container.
        div()
            .id(ElementId::NamedChild(self.id.into(), "wrap".into()))
            .when_some(self.aria_label, |this, label| this.aria_label(label))
            .child(tab_bar)
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

/// Loading spinner glyph respecting both explicit reduced-motion and GPUI's
/// `cx.reduce_motion()` preference.
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
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.reduced_motion || cx.reduce_motion() {
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

    #[gpui::test]
    fn icon_label_renders_with_image_accessibility_wrapper(cx: &mut gpui::TestAppContext) {
        let labeled = Icon::new(IconName::Wifi).label("Connected");
        assert_eq!(labeled.label.as_deref(), Some("Connected"));

        let (_view, visual_cx) = cx.add_window_view(|_window, _cx| {
            struct IconHost;
            impl gpui::Render for IconHost {
                fn render(
                    &mut self,
                    _window: &mut Window,
                    _cx: &mut gpui::Context<Self>,
                ) -> impl IntoElement {
                    div()
                        .child(Icon::new(IconName::Wifi).label("Connected"))
                        .child(Icon::new(IconName::Moon))
                }
            }
            IconHost
        });
        visual_cx.run_until_parked();
        let _ = visual_cx.update(|window, cx| window.draw(cx));
    }
}
