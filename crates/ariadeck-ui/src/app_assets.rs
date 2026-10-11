use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use gpui_kit::assets::AllAssets as KitAssets;
use rust_embed::RustEmbed;

/// AriaDeck's compile-time embedded UI assets.
#[derive(Clone, Copy, Debug, Default, RustEmbed)]
#[folder = "assets/"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let clean = path.trim_start_matches('/');
        let stripped = clean.strip_prefix("assets/").unwrap_or(clean);
        if let Some(file) = Self::get(stripped) {
            return Ok(Some(file.data));
        }
        if let Ok(Some(data)) = KitAssets.load(stripped) {
            return Ok(Some(data));
        }
        if let Some(file) = Self::get(clean) {
            return Ok(Some(file.data));
        }
        match KitAssets.load(clean) {
            Ok(Some(data)) => Ok(Some(data)),
            _ => Ok(None),
        }
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let prefix = path.trim_start_matches('/');
        let mut items = Vec::new();
        for asset in Self::iter() {
            if asset.starts_with(prefix) {
                items.push(SharedString::from(asset.into_owned()));
            }
        }
        if let Ok(kit_items) = KitAssets.list(prefix) {
            for item in kit_items {
                if !items.contains(&item) {
                    items.push(item);
                }
            }
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
        // Local assets not present in gpui-kit-assets
        assert!(
            Assets
                .load("icons/trash-2.svg")
                .expect("trash-2 icon should load from local assets")
                .is_some()
        );
        assert!(
            Assets
                .load("icons/circle-help.svg")
                .expect("circle-help icon should load from local assets")
                .is_some()
        );
        // Kit assets
        for icon in [
            crate::IconName::Search,
            crate::IconName::Settings,
            crate::IconName::Plus,
            crate::IconName::Minus,
            crate::IconName::Square,
            crate::IconName::SquareCheckBig,
            crate::IconName::SquareMinus,
            crate::IconName::X,
            crate::IconName::Pause,
            crate::IconName::Play,
            crate::IconName::RotateCcw,
            crate::IconName::Pencil,
            crate::IconName::Trash2,
            crate::IconName::Copy,
            crate::IconName::Sun,
            crate::IconName::Moon,
            crate::IconName::Link,
            crate::IconName::FolderDown,
            crate::IconName::Download,
            crate::IconName::CircleCheck,
            crate::IconName::CircleAlert,
            crate::IconName::CircleX,
            crate::IconName::Info,
            crate::IconName::LoaderCircle,
            crate::IconName::MoreHorizontal,
            crate::IconName::PanelRight,
            crate::IconName::Wifi,
            crate::IconName::WifiOff,
            crate::IconName::ArrowDown,
            crate::IconName::ArrowUp,
            crate::IconName::TriangleAlert,
            crate::IconName::CloudOff,
            crate::IconName::SearchX,
            crate::IconName::Inbox,
            crate::IconName::Activity,
            crate::IconName::CircleHelp,
            crate::IconName::Clock3,
            crate::IconName::List,
            crate::IconName::RefreshCw,
            crate::IconName::ScanSearch,
            crate::IconName::ArrowUpDown,
            crate::IconName::ChevronUp,
            crate::IconName::ChevronDown,
            crate::IconName::ChevronsUp,
            crate::IconName::ChevronsDown,
            crate::IconName::Check,
            crate::IconName::Bell,
            crate::IconName::FolderArchive,
            crate::IconName::FileText,
            crate::IconName::Music,
            crate::IconName::Video,
            crate::IconName::AppWindow,
            crate::IconName::Image,
            crate::IconName::File,
            crate::IconName::Cpu,
            crate::IconName::Server,
            crate::IconName::Users,
            crate::IconName::HardDrive,
            crate::IconName::WindowMinimize,
            crate::IconName::WindowMaximize,
            crate::IconName::WindowRestore,
            crate::IconName::WindowClose,
        ] {
            assert!(
                Assets.load(icon.path()).expect("load result").is_some(),
                "Icon {} failed to load!",
                icon.path()
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
