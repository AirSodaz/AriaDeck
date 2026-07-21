use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use rust_embed::RustEmbed;

/// AriaDeck's compile-time embedded UI assets.
#[derive(Clone, Copy, Debug, Default, RustEmbed)]
#[folder = "assets/"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(Self::get(path).map(|file| file.data))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let prefix = path.trim_start_matches('/');
        Ok(Self::iter()
            .filter(|asset| asset.starts_with(prefix))
            .map(|asset| SharedString::from(asset.into_owned()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_the_lucide_icon_subset() {
        assert!(
            Assets
                .load("icons/search.svg")
                .expect("search icon should load")
                .is_some()
        );
        assert!(
            Assets
                .load("icons/settings.svg")
                .expect("settings icon should load")
                .is_some()
        );
        for icon in [
            "icons/minus.svg",
            "icons/square.svg",
            "icons/window-minimize.svg",
            "icons/window-maximize.svg",
            "icons/window-restore.svg",
            "icons/window-close.svg",
        ] {
            assert!(
                Assets
                    .load(icon)
                    .expect("window control icon should load")
                    .is_some()
            );
        }
        assert!(
            Assets
                .list("icons/")
                .expect("embedded icon directory should list")
                .len()
                >= 20
        );
    }
}
