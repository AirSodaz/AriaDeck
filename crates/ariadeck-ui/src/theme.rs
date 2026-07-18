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
                background: color(0x151719),
                surface: color(0x1d2023),
                elevated_surface: color(0x25292d),
                surface_hover: color(0x2c3136),
                surface_active: color(0x343a40),
                text_primary: color(0xf2f4f5),
                text_secondary: color(0xb4bbc2),
                text_muted: color(0x7f8992),
                text_inverse: color(0x101214),
                border: color(0x30353a),
                border_strong: color(0x444b52),
                focus_ring: color(0x5ba7ff),
                accent: color(0x4f9cf7),
                accent_hover: color(0x70b0fa),
                accent_active: color(0x3586e6),
                success: color(0x49b67d),
                warning: color(0xe0ad4f),
                danger: color(0xe26767),
                information: color(0x63a6d8),
                progress_track: color(0x30363b),
                progress_download: color(0x4f9cf7),
                progress_upload: color(0x49b67d),
            },
        }
    }

    /// High-contrast light palette using the same semantic roles.
    #[must_use]
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            colors: ThemeColors {
                background: color(0xf5f7f8),
                surface: color(0xffffff),
                elevated_surface: color(0xffffff),
                surface_hover: color(0xedf2f4),
                surface_active: color(0xe5eaed),
                text_primary: color(0x182027),
                text_secondary: color(0x4e5b65),
                text_muted: color(0x78848d),
                text_inverse: color(0xffffff),
                border: color(0xdde2e5),
                border_strong: color(0xc6cdd2),
                focus_ring: color(0x176fc1),
                accent: color(0x176fc1),
                accent_hover: color(0x0d5fa9),
                accent_active: color(0x0a4f8e),
                success: color(0x1f8052),
                warning: color(0xa66d05),
                danger: color(0xbd3939),
                information: color(0x276f9f),
                progress_track: color(0xdde4e8),
                progress_download: color(0x176fc1),
                progress_upload: color(0x1f8052),
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
}
