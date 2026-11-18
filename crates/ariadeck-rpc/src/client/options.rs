use ariadeck_application::{
    AddDownloadRequest, DownloadProxyConfig, DownloadProxyMode, FileConflictPolicy,
};
use ariadeck_domain::{SpeedLimitConfig, TransferPolicyConfig, format_bt_tracker_option};
use secrecy::ExposeSecret;
use serde_json::{Map, Value};

pub(super) fn apply_file_conflict_policy(
    options: &mut Map<String, Value>,
    policy: FileConflictPolicy,
) {
    let (allow_overwrite, auto_file_renaming) = match policy {
        FileConflictPolicy::AutoRename => (false, true),
        FileConflictPolicy::Reject => (false, false),
        FileConflictPolicy::Overwrite => (true, false),
    };
    options.insert(
        "allow-overwrite".into(),
        Value::String(allow_overwrite.to_string()),
    );
    options.insert(
        "auto-file-renaming".into(),
        Value::String(auto_file_renaming.to_string()),
    );
}

pub(super) fn request_options(request: &AddDownloadRequest) -> Map<String, Value> {
    let mut options = Map::new();
    // aria2 accepts repeated option names for multi-value keys such as
    // `header`. Collapse same-key pairs into a string array so Cookie/Referer
    // and custom headers can travel together (ADD-005).
    for (key, value) in &request.options {
        match options.get_mut(key) {
            Some(slot @ Value::String(_)) => {
                let previous = std::mem::replace(slot, Value::Null);
                *slot = Value::Array(vec![previous, Value::String(value.clone())]);
            }
            Some(Value::Array(values)) => values.push(Value::String(value.clone())),
            Some(_) | None => {
                options.insert(key.clone(), Value::String(value.clone()));
            }
        }
    }
    apply_file_conflict_policy(&mut options, request.file_conflict);
    if let Some(destination) = &request.destination {
        options.insert("dir".into(), Value::String(destination.as_str().to_owned()));
    }
    if let Some(indices) = &request.selected_file_indices {
        options.insert(
            "select-file".into(),
            Value::String(format_selected_file_indices(indices)),
        );
    }
    options
}

fn format_selected_file_indices(indices: &[u32]) -> String {
    let mut ranges = Vec::new();
    let Some(&first) = indices.first() else {
        return String::new();
    };
    let mut start = first;
    let mut end = first;
    for &index in &indices[1..] {
        if index == end.saturating_add(1) {
            end = index;
        } else {
            push_file_index_range(&mut ranges, start, end);
            start = index;
            end = index;
        }
    }
    push_file_index_range(&mut ranges, start, end);
    ranges.join(",")
}

fn push_file_index_range(ranges: &mut Vec<String>, start: u32, end: u32) {
    if start == end {
        ranges.push(start.to_string());
    } else {
        ranges.push(format!("{start}-{end}"));
    }
}

pub(super) fn speed_limit_options(config: &SpeedLimitConfig) -> Vec<(String, String)> {
    vec![
        (
            "max-overall-download-limit".into(),
            config.download_limit.get().to_string(),
        ),
        (
            "max-overall-upload-limit".into(),
            config.upload_limit.get().to_string(),
        ),
    ]
}

pub(super) fn transfer_policy_options(config: &TransferPolicyConfig) -> Vec<(String, String)> {
    vec![
        (
            "max-concurrent-downloads".into(),
            config.max_concurrent_downloads.to_string(),
        ),
        (
            "max-connection-per-server".into(),
            config.max_connection_per_server.to_string(),
        ),
        ("split".into(), config.split.to_string()),
        ("min-split-size".into(), config.min_split_size.to_string()),
        (
            "file-allocation".into(),
            config.file_allocation.as_aria2().to_owned(),
        ),
        (
            "check-integrity".into(),
            if config.check_integrity {
                "true".into()
            } else {
                "false".into()
            },
        ),
    ]
}

pub(super) fn bt_tracker_options(trackers: &[String]) -> Vec<(String, String)> {
    vec![("bt-tracker".into(), format_bt_tracker_option(trackers))]
}

pub(super) fn download_proxy_options(config: &DownloadProxyConfig) -> Vec<(String, String)> {
    // Manual and System both carry concrete endpoints (System is resolved by
    // the desktop layer before this call). Disabled clears every field.
    let active = matches!(
        config.mode,
        DownloadProxyMode::Manual | DownloadProxyMode::System
    );
    let endpoint = |value: &Option<String>| {
        if active {
            value.clone().unwrap_or_default()
        } else {
            String::new()
        }
    };
    let username = active
        .then(|| config.username.clone())
        .flatten()
        .unwrap_or_default();
    let password = if active {
        config
            .password
            .as_ref()
            .map(ExposeSecret::expose_secret)
            .cloned()
            .unwrap_or_default()
    } else {
        String::new()
    };
    let no_proxy = if active {
        config.no_proxy.join(",")
    } else {
        String::new()
    };
    let mut options = vec![
        ("all-proxy".into(), endpoint(&config.all_proxy)),
        ("http-proxy".into(), endpoint(&config.http_proxy)),
        ("https-proxy".into(), endpoint(&config.https_proxy)),
        ("ftp-proxy".into(), endpoint(&config.ftp_proxy)),
        ("no-proxy".into(), no_proxy),
        // Always push: independent of proxy mode (TLS policy for all HTTPS peers).
        (
            "check-certificate".into(),
            if config.check_certificate {
                "true".into()
            } else {
                "false".into()
            },
        ),
    ];
    for prefix in ["all", "http", "https", "ftp"] {
        options.push((format!("{prefix}-proxy-user"), username.clone()));
        options.push((format!("{prefix}-proxy-passwd"), password.clone()));
    }
    options
}
