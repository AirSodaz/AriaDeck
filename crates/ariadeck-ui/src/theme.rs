use gpui::{App, Hsla, Window, WindowAppearance, rgb};

/// User-facing color scheme selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ThemeMode {
    /// Follow the operating-system preference.
    #[default]
    System,
    /// Use the light palette.
    Light,
    /// Use the dark palette.
    Dark,
}

impl ThemeMode {
    /// Resolve to a concrete gpui-kit ThemeMode based on window appearance.
    #[must_use]
    pub fn resolve(self, appearance: WindowAppearance) -> gpui_kit::component::ThemeMode {
        match self {
            Self::System => appearance.into(),
            Self::Light => gpui_kit::component::ThemeMode::Light,
            Self::Dark => gpui_kit::component::ThemeMode::Dark,
        }
    }
}

impl From<gpui_kit::component::ThemeMode> for ThemeMode {
    fn from(mode: gpui_kit::component::ThemeMode) -> Self {
        match mode {
            gpui_kit::component::ThemeMode::Light => Self::Light,
            gpui_kit::component::ThemeMode::Dark => Self::Dark,
        }
    }
}
/// Semantic colors consumed by AriaDeck components.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeColors {
    pub background: Hsla,
    pub surface: Hsla,
    pub toolbar_surface: Hsla,
    pub elevated_surface: Hsla,
    pub surface_hover: Hsla,
    pub surface_active: Hsla,
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub text_muted: Hsla,
    pub text_inverse: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub focus_ring: Hsla,
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_active: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub information: Hsla,
    pub progress_track: Hsla,
    pub progress_download: Hsla,
    pub progress_upload: Hsla,
}

impl ThemeColors {
    /// Construct ThemeColors mapped from gpui-kit's Theme token set.
    #[must_use]
    pub fn from_kit(kit: &gpui_kit::component::Theme) -> Self {
        let is_dark = kit.mode.is_dark();
        let fallback = if is_dark {
            Theme::dark().colors
        } else {
            Theme::light().colors
        };
        Self {
            background: kit.background,
            surface: if is_dark {
                color(0x181b1f)
            } else {
                color(0xf1f3f5)
            },
            toolbar_surface: kit.background,
            elevated_surface: kit.popover,
            surface_hover: kit.secondary_hover,
            surface_active: kit.secondary_active,
            text_primary: kit.foreground,
            text_secondary: kit.secondary_foreground,
            text_muted: kit.muted_foreground,
            text_inverse: fallback.text_inverse,
            border: kit.border,
            border_strong: kit.border,
            focus_ring: fallback.focus_ring,
            accent: fallback.accent,
            accent_hover: fallback.accent_hover,
            accent_active: fallback.accent_active,
            success: kit.success,
            warning: kit.warning,
            danger: kit.danger,
            information: kit.info,
            progress_track: fallback.progress_track,
            progress_download: fallback.progress_download,
            progress_upload: fallback.progress_upload,
        }
    }
}

/// Complete semantic theme used by the component layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub mode: ThemeMode,
    pub colors: ThemeColors,
}

impl Theme {
    /// Restrained dark palette for dense operational views.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            colors: ThemeColors {
                background: color(0x121416),
                surface: color(0x181b1f),
                toolbar_surface: color(0x181b1f),
                elevated_surface: color(0x23282d),
                surface_hover: color(0x292f35),
                surface_active: color(0x30373e),
                text_primary: color(0xf2f5f7),
                text_secondary: color(0xb8c0c8),
                text_muted: color(0x88929c),
                text_inverse: color(0x121416),
                border: color(0x2d3339),
                border_strong: color(0x434b53),
                focus_ring: color(0x5b9dff),
                accent: color(0x5b9dff),
                accent_hover: color(0x75adff),
                accent_active: color(0x438ceb),
                success: color(0x4fbd83),
                warning: color(0xe4ae50),
                danger: color(0xe46d72),
                information: color(0x70b3df),
                progress_track: color(0x30373e),
                progress_download: color(0x5b9dff),
                progress_upload: color(0x4fbd83),
            },
        }
    }

    /// High-contrast light palette using the same semantic roles.
    #[must_use]
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            colors: ThemeColors {
                background: color(0xf7f8fa),
                surface: color(0xf1f3f5),
                toolbar_surface: color(0xffffff),
                elevated_surface: color(0xffffff),
                surface_hover: color(0xe9edf1),
                surface_active: color(0xe1e6eb),
                text_primary: color(0x18212b),
                text_secondary: color(0x52606e),
                text_muted: color(0x7a8694),
                text_inverse: color(0xffffff),
                border: color(0xdce2e8),
                border_strong: color(0xc7d0d9),
                focus_ring: color(0x2569d8),
                accent: color(0x2569d8),
                accent_hover: color(0x195bc0),
                accent_active: color(0x124ca4),
                success: color(0x208255),
                warning: color(0xa86c05),
                danger: color(0xbd3e45),
                information: color(0x2875a6),
                progress_track: color(0xdce2e8),
                progress_download: color(0x2569d8),
                progress_upload: color(0x208255),
            },
        }
    }
    /// Construct a Theme from a gpui-kit Theme.
    #[must_use]
    pub fn from_kit(kit: &gpui_kit::component::Theme) -> Self {
        Self {
            mode: if kit.mode.is_dark() {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            colors: ThemeColors::from_kit(kit),
        }
    }

    /// Sync the current Theme to the global gpui-kit Theme.
    pub fn sync_to_kit(&self, window: Option<&mut Window>, cx: &mut App) {
        let mode = match self.mode {
            ThemeMode::Light => gpui_kit::component::ThemeMode::Light,
            ThemeMode::Dark => gpui_kit::component::ThemeMode::Dark,
            ThemeMode::System => match window.as_ref().map(|w| w.appearance()) {
                Some(WindowAppearance::Dark | WindowAppearance::VibrantDark) => {
                    gpui_kit::component::ThemeMode::Dark
                }
                _ => gpui_kit::component::ThemeMode::Light,
            },
        };
        gpui_kit::component::Theme::change(mode, window, cx);
        let kit = gpui_kit::component::Theme::global_mut(cx);
        kit.primary = self.colors.accent;
        kit.primary_hover = self.colors.accent_hover;
        kit.primary_active = self.colors.accent_active;
        kit.primary_foreground = self.colors.text_inverse;
        kit.button_primary = self.colors.accent;
        kit.button_primary_hover = self.colors.accent_hover;
        kit.button_primary_active = self.colors.accent_active;
        kit.button_primary_foreground = self.colors.text_inverse;
        kit.tokens.button_primary = self.colors.accent.into();
        kit.tokens.button_primary_hover = self.colors.accent_hover.into();
        kit.tokens.button_primary_active = self.colors.accent_active.into();
        kit.tokens.button_primary_foreground = self.colors.text_inverse.into();
        kit.ring = self.colors.focus_ring;
    }
}

fn color(value: u32) -> Hsla {
    rgb(value).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relative_luminance(color: Hsla) -> f32 {
        let color = color.to_rgb();
        let linear = |channel: f32| {
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    fn contrast_ratio(first: Hsla, second: Hsla) -> f32 {
        let first = relative_luminance(first);
        let second = relative_luminance(second);
        let (lighter, darker) = if first > second {
            (first, second)
        } else {
            (second, first)
        };
        (lighter + 0.05) / (darker + 0.05)
    }

    fn assert_contrast(label: &str, foreground: Hsla, background: Hsla, minimum: f32) {
        let actual = contrast_ratio(foreground, background);
        assert!(
            actual >= minimum,
            "{label} contrast was {actual:.2}:1, expected at least {minimum:.1}:1"
        );
    }

    #[test]
    fn normal_text_meets_wcag_aa_in_both_palettes() {
        for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
            let colors = theme.colors;
            assert_contrast(
                &format!("{name} primary text"),
                colors.text_primary,
                colors.background,
                4.5,
            );
            assert_contrast(
                &format!("{name} secondary text"),
                colors.text_secondary,
                colors.surface,
                4.5,
            );
            assert_contrast(
                &format!("{name} inverse action text"),
                colors.text_inverse,
                colors.accent,
                4.5,
            );
        }
    }

    #[test]
    fn muted_text_focus_and_status_colors_remain_perceivable() {
        for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
            let colors = theme.colors;
            assert_contrast(
                &format!("{name} muted text"),
                colors.text_muted,
                colors.elevated_surface,
                3.0,
            );
            assert_contrast(
                &format!("{name} focus ring"),
                colors.focus_ring,
                colors.elevated_surface,
                3.0,
            );
            for (role, color) in [
                ("success", colors.success),
                ("warning", colors.warning),
                ("danger", colors.danger),
                ("information", colors.information),
            ] {
                assert_contrast(&format!("{name} {role}"), color, colors.background, 3.0);
            }
        }
    }

    #[test]
    fn palettes_use_the_approved_neutral_core_tokens() {
        let light = Theme::light().colors;
        assert_eq!(light.background, color(0xf7f8fa));
        assert_eq!(light.surface, color(0xf1f3f5));
        assert_eq!(light.elevated_surface, color(0xffffff));
        assert_eq!(light.text_primary, color(0x18212b));
        assert_eq!(light.text_secondary, color(0x52606e));
        assert_eq!(light.text_muted, color(0x7a8694));
        assert_eq!(light.border, color(0xdce2e8));
        assert_eq!(light.border_strong, color(0xc7d0d9));
        assert_eq!(light.accent, color(0x2569d8));

        let dark = Theme::dark().colors;
        assert_eq!(dark.background, color(0x121416));
        assert_eq!(dark.surface, color(0x181b1f));
        assert_eq!(dark.elevated_surface, color(0x23282d));
        assert_eq!(dark.text_primary, color(0xf2f5f7));
        assert_eq!(dark.text_secondary, color(0xb8c0c8));
        assert_eq!(dark.text_muted, color(0x88929c));
        assert_eq!(dark.border, color(0x2d3339));
        assert_eq!(dark.border_strong, color(0x434b53));
        assert_eq!(dark.accent, color(0x5b9dff));
    }

    #[test]
    fn theme_mode_resolution_and_conversion() {
        assert_eq!(
            ThemeMode::System.resolve(WindowAppearance::Light),
            gpui_kit::component::ThemeMode::Light
        );
        assert_eq!(
            ThemeMode::System.resolve(WindowAppearance::Dark),
            gpui_kit::component::ThemeMode::Dark
        );
        assert_eq!(
            ThemeMode::Light.resolve(WindowAppearance::Dark),
            gpui_kit::component::ThemeMode::Light
        );
        assert_eq!(
            ThemeMode::Dark.resolve(WindowAppearance::Light),
            gpui_kit::component::ThemeMode::Dark
        );
        assert_eq!(
            ThemeMode::from(gpui_kit::component::ThemeMode::Light),
            ThemeMode::Light
        );
        assert_eq!(
            ThemeMode::from(gpui_kit::component::ThemeMode::Dark),
            ThemeMode::Dark
        );
    }
}
