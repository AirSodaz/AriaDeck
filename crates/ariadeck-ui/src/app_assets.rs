use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use gpui_kit::assets::AllAssets as KitAssets;

/// AriaDeck's compile-time embedded UI assets.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == "icon.svg" {
            return Ok(Some(Cow::Borrowed(include_bytes!("../assets/icon.svg"))));
        }
        KitAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items = KitAssets.list(path).unwrap_or_default();
        if "icon.svg".starts_with(path) {
            items.push("icon.svg".into());
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_the_lucide_icon_subset() {
        assert!(
            Assets
                .load("icon.svg")
                .expect("application icon should load")
                .is_some()
        );
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
