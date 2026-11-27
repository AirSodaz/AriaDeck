//! Cross-category search index and per-category section metadata for Settings.

use ariadeck_i18n::Translator;

use super::SettingsCategory;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum SettingsSectionId {
    Appearance,
    Categories,
    ProfilesList,
    EngineCores,
    NetworkProxy,
    NetworkHeaderPresets,
    NetworkMirrors,
    SpeedLimits,
    TransferPolicy,
    TrackerList,
    NotificationsMain,
    Power,
    WindowTray,
    Clipboard,
    BrowserBridge,
    AboutApp,
    AboutRuntime,
    AboutUpdates,
    SettingsTransfer,
    Diagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SettingsSectionMeta {
    pub(crate) id: SettingsSectionId,
    pub(crate) category: SettingsCategory,
    pub(crate) title_key: &'static str,
    pub(crate) scroll_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SettingsSearchHit {
    pub(crate) category: SettingsCategory,
    pub(crate) section: SettingsSectionId,
    pub(crate) label: String,
    pub(crate) category_title: String,
    pub(crate) section_title: String,
    score: u32,
}

struct SearchIndexSpec {
    category: SettingsCategory,
    section: SettingsSectionId,
    section_title_key: &'static str,
    label_key: &'static str,
    desc_key: Option<&'static str>,
    keywords: &'static str,
    requires_updates: bool,
}

pub(crate) fn sections_for_category(
    category: SettingsCategory,
    update_enabled: bool,
) -> Vec<SettingsSectionMeta> {
    let raw: &[(SettingsSectionId, &'static str)] = match category {
        SettingsCategory::General => &[
            (SettingsSectionId::Appearance, "settings-appearance"),
            (SettingsSectionId::Categories, "settings-categories"),
        ],
        SettingsCategory::Profiles => &[(SettingsSectionId::ProfilesList, "settings-profiles")],
        SettingsCategory::Engine => &[(SettingsSectionId::EngineCores, "settings-engine-cores")],
        SettingsCategory::Network => &[
            (SettingsSectionId::NetworkProxy, "settings-network-proxy"),
            (
                SettingsSectionId::NetworkHeaderPresets,
                "settings-network-header-presets",
            ),
            (
                SettingsSectionId::NetworkMirrors,
                "settings-network-mirrors",
            ),
        ],
        SettingsCategory::Transfers => &[
            (SettingsSectionId::SpeedLimits, "settings-speed-limits"),
            (
                SettingsSectionId::TransferPolicy,
                "settings-transfer-policy",
            ),
            (SettingsSectionId::TrackerList, "settings-tracker-list"),
        ],
        SettingsCategory::Notifications => &[(
            SettingsSectionId::NotificationsMain,
            "settings-notifications",
        )],
        SettingsCategory::System => &[
            (SettingsSectionId::Power, "settings-power"),
            (SettingsSectionId::WindowTray, "settings-window-tray"),
            (SettingsSectionId::Clipboard, "settings-clipboard"),
            (SettingsSectionId::BrowserBridge, "settings-bridge"),
        ],
        SettingsCategory::About => {
            if update_enabled {
                &[
                    (SettingsSectionId::AboutApp, "settings-about-app"),
                    (SettingsSectionId::AboutRuntime, "settings-about-runtime"),
                    (SettingsSectionId::AboutUpdates, "settings-update-title"),
                    (
                        SettingsSectionId::SettingsTransfer,
                        "settings-transfer-title",
                    ),
                    (SettingsSectionId::Diagnostics, "settings-diagnostics-title"),
                ]
            } else {
                &[
                    (SettingsSectionId::AboutApp, "settings-about-app"),
                    (SettingsSectionId::AboutRuntime, "settings-about-runtime"),
                    (
                        SettingsSectionId::SettingsTransfer,
                        "settings-transfer-title",
                    ),
                    (SettingsSectionId::Diagnostics, "settings-diagnostics-title"),
                ]
            }
        }
    };
    raw.iter()
        .enumerate()
        .map(|(scroll_index, &(id, title_key))| SettingsSectionMeta {
            id,
            category,
            title_key,
            scroll_index,
        })
        .collect()
}

const SEARCH_SPECS: &[SearchIndexSpec] = &[
    // General
    SearchIndexSpec {
        category: SettingsCategory::General,
        section: SettingsSectionId::Appearance,
        section_title_key: "settings-appearance",
        label_key: "settings-theme",
        desc_key: None,
        keywords: "theme dark light system appearance color scheme 主题 深色 浅色 跟随系统 外观",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::General,
        section: SettingsSectionId::Appearance,
        section_title_key: "settings-appearance",
        label_key: "settings-language",
        desc_key: Some("settings-language-description"),
        keywords: "language locale english chinese i18n 语言 中文 英文",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::General,
        section: SettingsSectionId::Categories,
        section_title_key: "settings-categories",
        label_key: "settings-categories",
        desc_key: Some("settings-categories-desc"),
        keywords: "categories folder directory extension route auto download path 分类 目录 文件夹 扩展名 自动路由",
        requires_updates: false,
    },
    // Profiles
    SearchIndexSpec {
        category: SettingsCategory::Profiles,
        section: SettingsSectionId::ProfilesList,
        section_title_key: "settings-profiles",
        label_key: "settings-profiles",
        desc_key: None,
        keywords: "profile remote rpc websocket local managed endpoint secret 配置 远程 本地 端点 密钥",
        requires_updates: false,
    },
    // Engine
    SearchIndexSpec {
        category: SettingsCategory::Engine,
        section: SettingsSectionId::EngineCores,
        section_title_key: "settings-engine-cores",
        label_key: "settings-engine-cores",
        desc_key: None,
        keywords: "engine core aria2 aria2c binary import link rollback verify 引擎 内核 导入 回滚 校验",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::Engine,
        section: SettingsSectionId::EngineCores,
        section_title_key: "settings-engine-cores",
        label_key: "core-setup-section",
        desc_key: None,
        keywords: "get aria2 scan discover download binary 获取 aria2 扫描 发现 下载",
        requires_updates: false,
    },
    // Network
    SearchIndexSpec {
        category: SettingsCategory::Network,
        section: SettingsSectionId::NetworkProxy,
        section_title_key: "settings-network-proxy",
        label_key: "settings-network-proxy",
        desc_key: None,
        keywords: "proxy http https ftp socks bypass username password 代理 绕过 用户名 密码",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::Network,
        section: SettingsSectionId::NetworkProxy,
        section_title_key: "settings-network-proxy",
        label_key: "settings-proxy-check-certificate",
        desc_key: Some("settings-proxy-check-certificate-desc"),
        keywords: "ssl tls certificate check verify 证书 校验",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::Network,
        section: SettingsSectionId::NetworkHeaderPresets,
        section_title_key: "settings-network-header-presets",
        label_key: "settings-network-header-presets",
        desc_key: Some("settings-header-presets-desc"),
        keywords: "header preset referer user-agent domain 请求头 预设 域名",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::Network,
        section: SettingsSectionId::NetworkMirrors,
        section_title_key: "settings-network-mirrors",
        label_key: "settings-network-mirrors",
        desc_key: None,
        keywords: "mirror rewrite rule github prefix host 镜像 重写 规则 加速",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::Network,
        section: SettingsSectionId::NetworkMirrors,
        section_title_key: "settings-network-mirrors",
        label_key: "settings-auto-probe-on-add",
        desc_key: Some("settings-auto-probe-on-add-desc"),
        keywords: "probe head range filename category 探测 文件名 自动探测",
        requires_updates: false,
    },
    // Transfers
    SearchIndexSpec {
        category: SettingsCategory::Transfers,
        section: SettingsSectionId::SpeedLimits,
        section_title_key: "settings-speed-limits",
        label_key: "settings-speed-limits",
        desc_key: None,
        keywords: "speed limit download upload bandwidth throttle 限速 下载速度 上传速度 带宽",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::Transfers,
        section: SettingsSectionId::TransferPolicy,
        section_title_key: "settings-transfer-policy",
        label_key: "settings-max-concurrent",
        desc_key: None,
        keywords: "concurrent connections allocation conflict integrity retry 并发 连接数 预分配 冲突 完整性",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::Transfers,
        section: SettingsSectionId::TransferPolicy,
        section_title_key: "settings-transfer-policy",
        label_key: "settings-split",
        desc_key: None,
        keywords: "split min split size chunk segments 分片 最小分片",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::Transfers,
        section: SettingsSectionId::TrackerList,
        section_title_key: "settings-tracker-list",
        label_key: "settings-tracker-enabled",
        desc_key: Some("settings-tracker-enabled-desc"),
        keywords: "tracker bt bittorrent magnet auto refresh 追踪器 磁力 种子 自动刷新",
        requires_updates: false,
    },
    // Notifications
    SearchIndexSpec {
        category: SettingsCategory::Notifications,
        section: SettingsSectionId::NotificationsMain,
        section_title_key: "settings-notifications",
        label_key: "settings-volume",
        desc_key: Some("settings-volume-desc"),
        keywords: "notification volume quiet silent toast completion error 通知 音量 安静 静音",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::Notifications,
        section: SettingsSectionId::NotificationsMain,
        section_title_key: "settings-notifications",
        label_key: "settings-notify-os",
        desc_key: Some("settings-notify-os-desc"),
        keywords: "os desktop notification low disk space 系统通知 桌面通知 磁盘空间",
        requires_updates: false,
    },
    // System
    SearchIndexSpec {
        category: SettingsCategory::System,
        section: SettingsSectionId::Power,
        section_title_key: "settings-power",
        label_key: "settings-keep-awake",
        desc_key: Some("settings-keep-awake-desc"),
        keywords: "power keep awake 电源 阻止休眠",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::System,
        section: SettingsSectionId::Power,
        section_title_key: "settings-power",
        label_key: "settings-action-after-done",
        desc_key: Some("settings-action-after-done-desc"),
        keywords: "sleep hibernate shutdown after done complete 关机 睡眠 休眠 完成后",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::System,
        section: SettingsSectionId::WindowTray,
        section_title_key: "settings-window-tray",
        label_key: "settings-tray-icon",
        desc_key: Some("settings-tray-icon-desc"),
        keywords: "tray speed meter minimize close behavior start minimized 托盘 网速 最小化 关闭行为",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::System,
        section: SettingsSectionId::Clipboard,
        section_title_key: "settings-clipboard",
        label_key: "settings-clipboard-watch",
        desc_key: Some("settings-clipboard-watch-desc"),
        keywords: "clipboard watch link magnet auto capture 剪贴板 监听 捕获",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::System,
        section: SettingsSectionId::BrowserBridge,
        section_title_key: "settings-bridge",
        label_key: "settings-bridge-auto-submit",
        desc_key: Some("settings-bridge-auto-submit-desc"),
        keywords: "browser extension bridge cookies auto submit native messaging 浏览器 扩展 桥接 自动提交",
        requires_updates: false,
    },
    // About
    SearchIndexSpec {
        category: SettingsCategory::About,
        section: SettingsSectionId::AboutApp,
        section_title_key: "settings-about-app",
        label_key: "settings-about-version",
        desc_key: None,
        keywords: "about version authors 关于 版本 作者",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::About,
        section: SettingsSectionId::AboutRuntime,
        section_title_key: "settings-about-runtime",
        label_key: "settings-about-platform",
        desc_key: None,
        keywords: "runtime platform os arch aria2 version 运行环境 平台 架构",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::About,
        section: SettingsSectionId::AboutUpdates,
        section_title_key: "settings-update-title",
        label_key: "settings-update-title",
        desc_key: None,
        keywords: "updates check release install upgrade 更新 检查更新 安装 升级",
        requires_updates: true,
    },
    SearchIndexSpec {
        category: SettingsCategory::About,
        section: SettingsSectionId::SettingsTransfer,
        section_title_key: "settings-transfer-title",
        label_key: "settings-export",
        desc_key: Some("settings-export-description"),
        keywords: "backup export json settings 备份 导出 设置",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::About,
        section: SettingsSectionId::SettingsTransfer,
        section_title_key: "settings-transfer-title",
        label_key: "settings-import",
        desc_key: Some("settings-import-description"),
        keywords: "backup import json settings 备份 导入 设置",
        requires_updates: false,
    },
    SearchIndexSpec {
        category: SettingsCategory::About,
        section: SettingsSectionId::Diagnostics,
        section_title_key: "settings-diagnostics-title",
        label_key: "settings-diagnostics-export",
        desc_key: Some("settings-diagnostics-description"),
        keywords: "diagnostics zip support redacted debug log 诊断 导出 排障",
        requires_updates: false,
    },
];

pub(crate) fn search_settings(
    query: &str,
    translator: &Translator,
    update_enabled: bool,
) -> Vec<SettingsSearchHit> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let terms: Vec<String> = trimmed
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|term| !term.is_empty())
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }

    let mut hits = Vec::new();
    for spec in SEARCH_SPECS {
        if spec.requires_updates && !update_enabled {
            continue;
        }
        let label = translator.t(spec.label_key);
        let category_title = translator.t(spec.category.message_key());
        let section_title = translator.t(spec.section_title_key);
        let desc = spec
            .desc_key
            .map(|key| translator.t(key))
            .unwrap_or_default();

        let label_lower = label.to_lowercase();
        let section_lower = section_title.to_lowercase();
        let category_lower = category_title.to_lowercase();
        let haystack = format!(
            "{label_lower} {section_lower} {category_lower} {} {}",
            desc.to_lowercase(),
            spec.keywords.to_lowercase()
        );

        if !terms.iter().all(|term| haystack.contains(term)) {
            continue;
        }

        let mut score = 0_u32;
        for term in &terms {
            if label_lower.starts_with(term) {
                score += 100;
            } else if label_lower.contains(term) {
                score += 60;
            } else if section_lower.contains(term) || category_lower.contains(term) {
                score += 40;
            } else {
                score += 15;
            }
        }
        hits.push(SettingsSearchHit {
            category: spec.category,
            section: spec.section,
            label,
            category_title,
            section_title,
            score,
        });
    }

    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.label.cmp(&b.label)));
    hits
}

#[cfg(test)]
pub(crate) fn all_referenced_translation_keys() -> Vec<&'static str> {
    let mut keys = Vec::new();
    for category in SettingsCategory::ALL {
        for meta in sections_for_category(category, true) {
            keys.push(meta.title_key);
        }
    }
    for spec in SEARCH_SPECS {
        keys.push(spec.section_title_key);
        keys.push(spec.label_key);
        if let Some(desc) = spec.desc_key {
            keys.push(desc);
        }
    }
    keys
}
