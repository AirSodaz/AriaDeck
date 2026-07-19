use gpui::{Hsla, rgb};

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
                background: color(0x12161b),
                surface: color(0x181d23),
                toolbar_surface: color(0x1b2128),
                elevated_surface: color(0x222a33),
                surface_hover: color(0x27303a),
                surface_active: color(0x2d3742),
                text_primary: color(0xf2f5f7),
                text_secondary: color(0xb7c0c9),
                text_muted: color(0x828d99),
                text_inverse: color(0x0f1317),
                border: color(0x2c353f),
                border_strong: color(0x414d59),
                focus_ring: color(0x69a9ff),
                accent: color(0x5b9dff),
                accent_hover: color(0x79afff),
                accent_active: color(0x3d86ec),
                success: color(0x4fbd83),
                warning: color(0xe4ae50),
                danger: color(0xe46d72),
                information: color(0x70b3df),
                progress_track: color(0x303a45),
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
                background: color(0xf7f9fb),
                surface: color(0xf3f6f8),
                toolbar_surface: color(0xfdfefe),
                elevated_surface: color(0xffffff),
                surface_hover: color(0xeef2f7),
                surface_active: color(0xe6ebf0),
                text_primary: color(0x17202b),
                text_secondary: color(0x505d6c),
                text_muted: color(0x7b8795),
                text_inverse: color(0xffffff),
                border: color(0xdfe5eb),
                border_strong: color(0xcbd3dc),
                focus_ring: color(0x2569d8),
                accent: color(0x2569d8),
                accent_hover: color(0x195bc0),
                accent_active: color(0x124ca4),
                success: color(0x208255),
                warning: color(0xa86c05),
                danger: color(0xbd3e45),
                information: color(0x2875a6),
                progress_track: color(0xdfe6ec),
                progress_download: color(0x2569d8),
                progress_upload: color(0x208255),
            },
        }
    }
}

fn color(value: u32) -> Hsla {
    rgb(value).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_text_colors_have_usable_contrast_in_both_palettes() {
        for theme in [Theme::dark(), Theme::light()] {
            assert_ne!(theme.colors.background, theme.colors.text_primary);
            assert_ne!(theme.colors.surface, theme.colors.text_secondary);
            assert_ne!(theme.colors.accent, theme.colors.text_primary);
        }
    }

    #[test]
    fn surface_roles_are_distinct_in_both_palettes() {
        for theme in [Theme::dark(), Theme::light()] {
            assert_ne!(theme.colors.background, theme.colors.surface);
            assert_ne!(theme.colors.surface, theme.colors.toolbar_surface);
            assert_ne!(theme.colors.toolbar_surface, theme.colors.elevated_surface);
        }
    }
}
