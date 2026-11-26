use super::{
    gateway::{map_mutation_error, map_query_error},
    options::apply_file_conflict_policy,
    *,
};
use crate::{RpcError, RpcTransport, models::TaskKey};
use ariadeck_application::{
    AddDownloadRequest, AddDownloadSource, DownloadEngineGateway, DownloadProxyConfig,
    DownloadProxyMode, FileConflictPolicy, GatewayErrorKind, QueueMove, TaskRemovalTarget,
};
use ariadeck_domain::{Gid, TransferPolicyConfig};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

struct ScriptedTransport {
    responses: Mutex<VecDeque<Result<Value, RpcError>>>,
    calls: Mutex<Vec<(String, Vec<Value>)>>,
}

impl ScriptedTransport {
    fn new(responses: impl IntoIterator<Item = Result<Value, RpcError>>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
            calls: Mutex::default(),
        }
    }
}

#[async_trait]
impl RpcTransport for ScriptedTransport {
    async fn call(&self, method: &str, params: Vec<Value>) -> Result<Value, RpcError> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((method.into(), params));
        self.responses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .unwrap_or(Err(RpcError::Closed))
    }
}

#[tokio::test]
async fn typed_methods_convert_version_global_stat_and_tasks() {
    let transport = ScriptedTransport::new([
        Ok(json!({"version": "1.37.0", "enabledFeatures": ["BitTorrent", "HTTPS"]})),
        Ok(json!({
            "downloadSpeed": "123",
            "uploadSpeed": "45",
            "numActive": "2",
            "numWaiting": "3",
            "numStoppedTotal": "4"
        })),
        Ok(json!([{
            "gid": "0000000000000001",
            "status": "active",
            "files": [{"path": "/tmp/item.bin"}]
        }])),
    ]);
    let client = Aria2Client::new(transport);

    let version = match client.get_version().await {
        Ok(version) => version,
        Err(error) => panic!("getVersion failed: {error}"),
    };
    let stat = match client.get_global_stat().await {
        Ok(stat) => stat,
        Err(error) => panic!("getGlobalStat failed: {error}"),
    };
    let tasks = match client.tell_active(TaskKey::DISCOVERY_PROJECTION).await {
        Ok(tasks) => tasks,
        Err(error) => panic!("tellActive failed: {error}"),
    };

    assert_eq!(version.version, "1.37.0");
    assert_eq!(stat.download_speed, ariadeck_domain::ByteRate::new(123));
    assert_eq!(tasks[0].display_name, "item.bin");
}

#[tokio::test]
async fn add_uri_collapses_repeated_headers_into_an_array() {
    let transport = ScriptedTransport::new([Ok(json!("000000000000000a"))]);
    let client = Aria2Client::new(transport);
    let request = AddDownloadRequest {
        source: AddDownloadSource::Uris(vec!["https://example.test/file".into()]),
        destination: None,
        file_conflict: FileConflictPolicy::Reject,
        selected_file_indices: None,
        advanced: Default::default(),
        options: vec![
            ("header".into(), "X-Token: one".into()),
            ("header".into(), "Cookie: session=secret".into()),
            ("referer".into(), "https://example.test/ref".into()),
        ],
    };

    client.add_uri(&request).await.expect("addUri succeeds");
    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "aria2.addUri");
    assert_eq!(
        calls[0].1[1]["header"],
        json!(["X-Token: one", "Cookie: session=secret"])
    );
    assert_eq!(calls[0].1[1]["referer"], json!("https://example.test/ref"));
}

#[tokio::test]
async fn add_uri_builds_options_without_losing_destination() {
    let transport = ScriptedTransport::new([Ok(json!("0000000000000009"))]);
    let client = Aria2Client::new(transport);
    let request = AddDownloadRequest {
        source: AddDownloadSource::Uris(vec![
            "https://example.test/file".into(),
            "https://mirror.test/file".into(),
        ]),
        destination: Some("D:/Downloads".into()),
        file_conflict: FileConflictPolicy::AutoRename,
        selected_file_indices: None,
        advanced: Default::default(),
        options: vec![("max-download-limit".into(), "1M".into())],
    };

    let gid = match client.add_uri(&request).await {
        Ok(gid) => gid,
        Err(error) => panic!("addUri failed: {error}"),
    };
    assert_eq!(gid, Gid::from_u64(9));
    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "aria2.addUri");
    assert_eq!(
        calls[0].1[0],
        json!(["https://example.test/file", "https://mirror.test/file"])
    );
    assert_eq!(calls[0].1[1]["dir"], Value::String("D:/Downloads".into()));
    assert_eq!(
        calls[0].1[1]["max-download-limit"],
        Value::String("1M".into())
    );
    assert_eq!(
        calls[0].1[1]["allow-overwrite"],
        Value::String("false".into())
    );
    assert_eq!(
        calls[0].1[1]["auto-file-renaming"],
        Value::String("true".into())
    );
}

#[tokio::test]
async fn add_torrent_uploads_base64_content_and_options() {
    let transport = ScriptedTransport::new([Ok(json!("0000000000000010"))]);
    let client = Aria2Client::new(transport);
    let request = AddDownloadRequest {
        source: AddDownloadSource::Torrent(Arc::<[u8]>::from(&b"torrent-bytes"[..])),
        destination: Some("/downloads".into()),
        file_conflict: FileConflictPolicy::Reject,
        selected_file_indices: Some(vec![1, 2, 3, 5]),
        advanced: Default::default(),
        options: vec![("max-connection-per-server".into(), "4".into())],
    };

    let gid = client
        .add_torrent(&request)
        .await
        .expect("torrent upload succeeds");

    assert_eq!(gid, Gid::from_u64(16));
    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "aria2.addTorrent");
    assert_eq!(calls[0].1[0], json!(STANDARD.encode(b"torrent-bytes")));
    assert_eq!(calls[0].1[1], json!([]));
    assert_eq!(calls[0].1[2]["dir"], json!("/downloads"));
    assert_eq!(calls[0].1[2]["allow-overwrite"], json!("false"));
    assert_eq!(calls[0].1[2]["auto-file-renaming"], json!("false"));
    assert_eq!(calls[0].1[2]["max-connection-per-server"], json!("4"));
    assert_eq!(calls[0].1[2]["select-file"], json!("1-3,5"));
}

#[tokio::test]
async fn add_metalink_preserves_all_returned_gids() {
    let transport = ScriptedTransport::new([Ok(json!(["0000000000000011", "0000000000000012"]))]);
    let client = Aria2Client::new(transport);
    let request = AddDownloadRequest {
        source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b"<metalink />"[..])),
        destination: None,
        file_conflict: FileConflictPolicy::Reject,
        selected_file_indices: None,
        advanced: Default::default(),
        options: Vec::new(),
    };

    let gids = client
        .add_metalink(&request)
        .await
        .expect("metalink upload succeeds");

    assert_eq!(gids, vec![Gid::from_u64(17), Gid::from_u64(18)]);
    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "aria2.addMetalink");
    assert_eq!(calls[0].1[0], json!(STANDARD.encode(b"<metalink />")));
}

#[tokio::test]
async fn add_metalink_rejects_empty_or_malformed_gid_results() {
    for result in [json!([]), json!(["not-a-gid"])] {
        let transport = ScriptedTransport::new([Ok(result)]);
        let client = Aria2Client::new(transport);
        let request = AddDownloadRequest {
            source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b"metadata"[..])),
            destination: None,
            file_conflict: FileConflictPolicy::Reject,
            selected_file_indices: None,
            advanced: Default::default(),
            options: Vec::new(),
        };

        assert!(client.add_metalink(&request).await.is_err());
    }
}

#[tokio::test]
async fn change_position_maps_each_queue_move_to_the_authoritative_aria2_arguments() {
    // aria2.changePosition takes (gid, pos, how); zero-based POS_SET/POS_END
    // and relative POS_CUR (D-014 evidence).
    for (movement, expected_pos, expected_how) in [
        (QueueMove::Top, json!(0), "POS_SET"),
        (QueueMove::Up, json!(-1), "POS_CUR"),
        (QueueMove::Down, json!(1), "POS_CUR"),
        (QueueMove::Bottom, json!(0), "POS_END"),
    ] {
        let transport = ScriptedTransport::new([Ok(json!(0))]);
        let client = Aria2Client::new(transport);

        let position = client
            .move_in_queue(Gid::from_u64(7), movement)
            .await
            .expect("changePosition must accept a queue move");
        assert_eq!(position, 0);

        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls.len(), 1);
        let (method, params) = &calls[0];
        assert_eq!(method, "aria2.changePosition");
        assert_eq!(
            params.as_slice(),
            &[json!("0000000000000007"), expected_pos, json!(expected_how),]
        );
    }
}

#[tokio::test]
async fn change_position_rejects_a_negative_result_position() {
    let transport = ScriptedTransport::new([Ok(json!(-1))]);
    let client = Aria2Client::new(transport);

    assert!(
        client
            .move_in_queue(Gid::from_u64(1), QueueMove::Top)
            .await
            .is_err(),
        "aria2 must report a non-negative queue position"
    );
}

#[test]
fn file_conflict_policy_maps_to_authoritative_aria2_options() {
    for (policy, allow_overwrite, auto_file_renaming) in [
        (FileConflictPolicy::AutoRename, "false", "true"),
        (FileConflictPolicy::Reject, "false", "false"),
        (FileConflictPolicy::Overwrite, "true", "false"),
    ] {
        let mut options = Map::from_iter([
            ("allow-overwrite".into(), Value::String("unexpected".into())),
            (
                "auto-file-renaming".into(),
                Value::String("unexpected".into()),
            ),
        ]);

        apply_file_conflict_policy(&mut options, policy);

        assert_eq!(
            options["allow-overwrite"],
            Value::String(allow_overwrite.into())
        );
        assert_eq!(
            options["auto-file-renaming"],
            Value::String(auto_file_renaming.into())
        );
    }
}

#[tokio::test]
async fn retry_replays_sources_and_safe_task_options_with_a_new_gid() {
    let transport = ScriptedTransport::new([
        Ok(json!({
            "gid": "0000000000000007",
            "pause": "true",
            "dir": "/downloads",
            "out": "renamed.iso",
            "header": ["Cookie: session=abc", "Referer: https://example.test/"],
            "load-cookies": "/cookies.txt",
            "all-proxy": "http://proxy.test:8080",
            "max-download-limit": "1M",
            "max-connection-per-server": "4",
            "checksum": "sha-256=deadbeef",
            "select-file": "1,3-4"
        })),
        Ok(json!([
            {"uri": "https://example.test/archive.iso", "status": "used"},
            {"uri": "https://mirror.test/archive.iso", "status": "waiting"}
        ])),
        Ok(json!("0000000000000009")),
    ]);
    let client = Aria2Client::new(transport);
    let fallback = AddDownloadRequest {
        source: AddDownloadSource::Uris(vec!["https://fallback.test/archive.iso".into()]),
        destination: Some("/fallback".into()),
        file_conflict: FileConflictPolicy::default(),
        selected_file_indices: None,
        advanced: Default::default(),
        options: Vec::new(),
    };

    let new_gid = DownloadEngineGateway::retry_download(&client, Gid::from_u64(7), &fallback)
        .await
        .expect("retry succeeds");

    assert_eq!(new_gid, Gid::from_u64(9));
    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(
        calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
        vec!["aria2.getOption", "aria2.getUris", "aria2.addUri"]
    );
    assert_eq!(
        calls[2].1[0],
        json!([
            "https://example.test/archive.iso",
            "https://mirror.test/archive.iso"
        ])
    );
    let replayed = calls[2].1[1].as_object().expect("option object");
    for key in [
        "dir",
        "out",
        "header",
        "load-cookies",
        "all-proxy",
        "max-download-limit",
        "max-connection-per-server",
        "checksum",
        "select-file",
    ] {
        assert!(replayed.contains_key(key), "missing replayed option {key}");
    }
    assert!(!replayed.contains_key("gid"));
    assert!(!replayed.contains_key("pause"));
}

#[tokio::test]
async fn retry_maps_only_the_add_uri_phase_to_unknown_outcome() {
    let query_failure = ScriptedTransport::new([Err(RpcError::Timeout {
        method: "aria2.getOption".into(),
    })]);
    let query_client = Aria2Client::new(query_failure);
    let fallback = AddDownloadRequest {
        source: AddDownloadSource::Uris(vec!["https://example.test/archive.iso".into()]),
        destination: None,
        file_conflict: FileConflictPolicy::default(),
        selected_file_indices: None,
        advanced: Default::default(),
        options: Vec::new(),
    };
    let query_error =
        DownloadEngineGateway::retry_download(&query_client, Gid::from_u64(7), &fallback)
            .await
            .expect_err("query timeout must fail");
    assert_eq!(query_error.kind, GatewayErrorKind::Timeout);
    assert!(query_error.retryable);

    let mutation_failure = ScriptedTransport::new([
        Ok(json!({"dir": "/downloads"})),
        Err(RpcError::Remote {
            code: 1,
            message: "No URI data is available for GID#0000000000000007".into(),
            data: None,
        }),
        Err(RpcError::Timeout {
            method: "aria2.addUri".into(),
        }),
    ]);
    let mutation_client = Aria2Client::new(mutation_failure);
    let mutation_error =
        DownloadEngineGateway::retry_download(&mutation_client, Gid::from_u64(7), &fallback)
            .await
            .expect_err("addUri timeout must be unknown");
    assert_eq!(mutation_error.kind, GatewayErrorKind::OutcomeUnknown);
    assert!(!mutation_error.retryable);
    assert_eq!(
        mutation_client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len(),
        3
    );
}

#[tokio::test]
async fn change_options_sends_output_name_to_the_exact_gid() {
    let transport = ScriptedTransport::new([Ok(json!("OK"))]);
    let client = Aria2Client::new(transport);
    let gid = Gid::from_u64(10);

    client
        .change_options(gid, &[("out".into(), "renamed.iso".into())])
        .await
        .expect("changeOption succeeds");

    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "aria2.changeOption");
    assert_eq!(calls[0].1[0], Value::String(gid.to_string()));
    assert_eq!(calls[0].1[1]["out"], Value::String("renamed.iso".into()));
}

#[tokio::test]
async fn download_proxy_uses_global_options_and_clears_unspecified_values() {
    let transport = ScriptedTransport::new([Ok(json!("OK"))]);
    let client = Aria2Client::new(transport);
    let config = DownloadProxyConfig {
        mode: DownloadProxyMode::Manual,
        all_proxy: Some("http://proxy.example:8080".into()),
        https_proxy: Some("secure-proxy.example:8443".into()),
        no_proxy: vec!["localhost".into(), "10.0.0.0/8".into()],
        username: Some("proxy-user".into()),
        password: Some(secrecy::SecretString::from("secret-value")),
        ..DownloadProxyConfig::default()
    };

    DownloadEngineGateway::apply_download_proxy(&client, &config)
        .await
        .expect("changeGlobalOption succeeds");

    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "aria2.changeGlobalOption");
    let options = calls[0].1[0].as_object().expect("global option object");
    assert_eq!(options["all-proxy"], "http://proxy.example:8080");
    assert_eq!(options["http-proxy"], "");
    assert_eq!(options["https-proxy"], "secure-proxy.example:8443");
    assert_eq!(options["no-proxy"], "localhost,10.0.0.0/8");
    assert_eq!(options["all-proxy-user"], "proxy-user");
    assert_eq!(options["all-proxy-passwd"], "secret-value");
    assert_eq!(options["ftp-proxy-passwd"], "secret-value");
}

#[tokio::test]
async fn transfer_policy_uses_global_options_with_aria2_names() {
    let transport = ScriptedTransport::new([Ok(json!("OK"))]);
    let client = Aria2Client::new(transport);
    let config = TransferPolicyConfig {
        max_concurrent_downloads: 3,
        max_connection_per_server: 8,
        split: 16,
        min_split_size: 1024 * 1024,
        file_allocation: ariadeck_domain::FileAllocationMethod::Falloc,
        check_integrity: true,
    };

    DownloadEngineGateway::apply_transfer_policy(&client, &config)
        .await
        .expect("changeGlobalOption succeeds");

    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "aria2.changeGlobalOption");
    let options = calls[0].1[0].as_object().expect("global option object");
    assert_eq!(options["max-concurrent-downloads"], "3");
    assert_eq!(options["max-connection-per-server"], "8");
    assert_eq!(options["split"], "16");
    assert_eq!(options["min-split-size"], (1024 * 1024).to_string());
    assert_eq!(options["file-allocation"], "falloc");
    assert_eq!(options["check-integrity"], "true");
}

#[tokio::test]
async fn system_download_proxy_applies_resolved_endpoints_like_manual() {
    let transport = ScriptedTransport::new([Ok(json!("OK"))]);
    let client = Aria2Client::new(transport);
    let config = DownloadProxyConfig {
        mode: DownloadProxyMode::System,
        all_proxy: Some("http://system-proxy.example:3128".into()),
        no_proxy: vec!["localhost".into()],
        ..DownloadProxyConfig::default()
    };

    DownloadEngineGateway::apply_download_proxy(&client, &config)
        .await
        .expect("system proxy is applied");

    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let options = calls[0].1[0].as_object().expect("global option object");
    assert_eq!(options["all-proxy"], "http://system-proxy.example:3128");
    assert_eq!(options["no-proxy"], "localhost");
    assert_eq!(options["all-proxy-user"], "");
    assert_eq!(options["all-proxy-passwd"], "");
    assert_eq!(options["check-certificate"], "true");
}

#[tokio::test]
async fn download_proxy_can_disable_certificate_verification() {
    let transport = ScriptedTransport::new([Ok(json!("OK"))]);
    let client = Aria2Client::new(transport);
    let config = DownloadProxyConfig {
        mode: DownloadProxyMode::Manual,
        all_proxy: Some("http://127.0.0.1:7897".into()),
        check_certificate: false,
        ..DownloadProxyConfig::default()
    };

    DownloadEngineGateway::apply_download_proxy(&client, &config)
        .await
        .expect("proxy apply succeeds");

    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let options = calls[0].1[0].as_object().expect("global option object");
    assert_eq!(options["all-proxy"], "http://127.0.0.1:7897");
    assert_eq!(options["check-certificate"], "false");
}

#[tokio::test]
async fn disabled_download_proxy_explicitly_clears_endpoints_and_credentials() {
    let transport = ScriptedTransport::new([Ok(json!("OK"))]);
    let client = Aria2Client::new(transport);
    let config = DownloadProxyConfig {
        mode: DownloadProxyMode::Disabled,
        all_proxy: Some("http://stale.example:8080".into()),
        username: Some("stale-user".into()),
        password: Some(secrecy::SecretString::from("stale-secret")),
        ..DownloadProxyConfig::default()
    };

    DownloadEngineGateway::apply_download_proxy(&client, &config)
        .await
        .expect("disabled proxy is applied");

    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let options = calls[0].1[0].as_object().expect("global option object");
    for key in [
        "all-proxy",
        "http-proxy",
        "https-proxy",
        "ftp-proxy",
        "no-proxy",
        "all-proxy-user",
        "all-proxy-passwd",
        "http-proxy-user",
        "http-proxy-passwd",
        "https-proxy-user",
        "https-proxy-passwd",
        "ftp-proxy-user",
        "ftp-proxy-passwd",
    ] {
        assert_eq!(options[key], "", "{key} was not cleared");
    }
}

#[tokio::test]
async fn details_projection_decodes_engine_paths_and_files() {
    let transport = ScriptedTransport::new([Ok(json!({
        "gid": "0000000000000009",
        "dir": "/srv/downloads",
        "infoHash": "abc123",
        "pieceLength": "1048576",
        "numPieces": "4",
        "bittorrent": {
            "announceList": [
                ["https://tracker-one.example/announce"],
                ["https://tracker-two.example/announce", ""]
            ]
        },
        "files": [{
            "index": "1",
            "path": "/srv/downloads/archive.iso",
            "length": "4194304",
            "completedLength": "1048576",
            "selected": "true",
            "uris": [{"uri": "https://secret.example/item"}]
        }]
    }))]);
    let client = Aria2Client::new(transport);

    let details = match client.task_details(Gid::from_u64(9)).await {
        Ok(details) => details,
        Err(error) => panic!("tellStatus details failed: {error}"),
    };

    assert_eq!(details.gid, Gid::from_u64(9));
    assert_eq!(
        details
            .directory
            .as_ref()
            .map(ariadeck_domain::EnginePath::as_str),
        Some("/srv/downloads")
    );
    assert_eq!(details.files.len(), 1);
    assert_eq!(details.files[0].path.as_str(), "/srv/downloads/archive.iso");
    assert!(details.files[0].selected);
    assert_eq!(details.trackers.len(), 2);
    assert_eq!(details.trackers[0].tier, 1);
    assert_eq!(details.trackers[1].tier, 2);
    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "aria2.tellStatus");
    assert_eq!(
        calls[0].1[1],
        json!([
            "gid",
            "files",
            "dir",
            "infoHash",
            "pieceLength",
            "numPieces",
            "bittorrent"
        ])
    );
}

#[tokio::test]
async fn connection_details_loads_active_bittorrent_peers_and_redacts_sensitive_options() {
    // Each successful multicall item is a one-element array around the
    // method's ordinary result value (aria2 system.multicall contract).
    let transport = ScriptedTransport::new([Ok(json!([
        [[
            {
                "uri": "https://mirror-one.example/file",
                "status": "used"
            },
            {
                "uri": "https://mirror-two.example/file",
                "status": "waiting"
            }
        ]],
        [{
            "max-download-limit": "1M",
            "http-passwd": "must-not-leave-the-adapter",
            "header": ["Cookie: session=secret"],
            "all-proxy": "http://user:password@proxy.example:8080"
        }],
        [[{
            "ip": "2001:db8::1",
            "port": "6881",
            "downloadSpeed": "1024",
            "uploadSpeed": "512",
            "seeder": "true"
        }]]
    ]))]);
    let client = Aria2Client::new(transport);

    let details = client
        .connection_details(Gid::from_u64(9), true, true)
        .await
        .expect("connection projections decode");

    assert_eq!(details.uris.len(), 2);
    assert_eq!(details.uris[0].status, ariadeck_domain::TaskUriStatus::Used);
    assert_eq!(details.peers.len(), 1);
    assert!(details.peers[0].seeder);
    assert!(details.servers.is_empty());
    assert_eq!(
        details
            .options
            .iter()
            .find(|entry| entry.key == "max-download-limit")
            .map(|entry| (entry.value.as_str(), entry.redacted)),
        Some(("1M", false))
    );
    for key in ["all-proxy", "header", "http-passwd"] {
        let option = details
            .options
            .iter()
            .find(|entry| entry.key == key)
            .unwrap_or_else(|| panic!("missing {key}"));
        assert!(option.redacted, "{key} must be redacted in the adapter");
        assert!(
            option.value.is_empty(),
            "{key} value must not leave the adapter"
        );
    }
    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "system.multicall");
    let methods = calls[0].1[0]
        .as_array()
        .expect("multicall methods array")
        .iter()
        .map(|entry| entry["methodName"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        vec!["aria2.getUris", "aria2.getOption", "aria2.getPeers"]
    );
}

#[tokio::test]
async fn connection_details_loads_servers_only_for_active_non_bittorrent_tasks() {
    let transport = ScriptedTransport::new([Ok(json!([
        [[]],
        [{"split": "4"}],
        [[{
            "index": "1",
            "servers": [{
                "uri": "https://origin.example/file",
                "currentUri": "https://cdn.example/file",
                "downloadSpeed": "2048"
            }]
        }]]
    ]))]);
    let client = Aria2Client::new(transport);

    let details = client
        .connection_details(Gid::from_u64(9), true, false)
        .await
        .expect("server projection decodes");

    assert_eq!(details.servers.len(), 1);
    assert!(details.peers.is_empty());
    assert_eq!(details.servers[0].file_index, 1);
    assert_eq!(details.servers[0].download_speed.get(), 2_048);
    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "system.multicall");
    let methods = calls[0].1[0]
        .as_array()
        .expect("multicall methods array")
        .iter()
        .map(|entry| entry["methodName"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        vec!["aria2.getUris", "aria2.getOption", "aria2.getServers"]
    );
}

#[tokio::test]
async fn connection_details_skips_active_only_projections_for_inactive_tasks() {
    let transport = ScriptedTransport::new([Ok(json!([[[]], [{}]]))]);
    let client = Aria2Client::new(transport);

    let details = client
        .connection_details(Gid::from_u64(9), false, true)
        .await
        .expect("inactive projection decodes");

    assert!(details.peers.is_empty());
    assert!(details.servers.is_empty());
    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "system.multicall");
    let methods = calls[0].1[0]
        .as_array()
        .expect("multicall methods array")
        .iter()
        .map(|entry| entry["methodName"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(methods, vec!["aria2.getUris", "aria2.getOption"]);
}

#[tokio::test]
async fn connection_details_treats_unavailable_active_only_projection_as_empty() {
    let transport = ScriptedTransport::new([Ok(json!([
        [[]],
        [{"split": "4"}],
        {"code": 1, "message": "GID is not found in active downloads"}
    ]))]);
    let client = Aria2Client::new(transport);

    let details = client
        .connection_details(Gid::from_u64(9), true, true)
        .await
        .expect("an active-state race must keep stable projections available");

    assert!(details.peers.is_empty());
    assert_eq!(details.options[0].key, "split");
}

#[tokio::test]
async fn force_pause_and_force_remove_use_the_force_rpc_methods() {
    let transport = ScriptedTransport::new([
        Ok(json!("0000000000000009")),
        Ok(json!("0000000000000009")),
        Ok(json!("OK")),
        Ok(json!("OK")),
    ]);
    let client = Aria2Client::new(transport);

    DownloadEngineGateway::force_pause(&client, Gid::from_u64(9))
        .await
        .expect("force pause");
    DownloadEngineGateway::force_remove(&client, Gid::from_u64(9), TaskRemovalTarget::LiveTask)
        .await
        .expect("force remove");
    DownloadEngineGateway::force_pause_all(&client)
        .await
        .expect("force pause all");
    Aria2Client::purge_download_result(&client)
        .await
        .expect("purge results");

    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(
        calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
        vec![
            "aria2.forcePause",
            "aria2.forceRemove",
            "aria2.forcePauseAll",
            "aria2.purgeDownloadResult",
        ]
    );
}

#[tokio::test]
async fn list_methods_returns_engine_published_names() {
    let transport = ScriptedTransport::new([Ok(json!([
        "aria2.addUri",
        "aria2.forcePause",
        "system.multicall",
        "system.listMethods"
    ]))]);
    let client = Aria2Client::new(transport);

    let methods = client.list_methods().await.expect("list methods");
    assert!(methods.iter().any(|method| method == "aria2.forcePause"));
    assert!(methods.iter().any(|method| method == "system.multicall"));
}

#[tokio::test]
async fn terminal_removal_uses_remove_download_result() {
    let transport = ScriptedTransport::new([Ok(json!("OK"))]);
    let client = Aria2Client::new(transport);

    let result =
        DownloadEngineGateway::remove(&client, Gid::from_u64(9), TaskRemovalTarget::DownloadResult)
            .await;

    assert!(result.is_ok());
    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "aria2.removeDownloadResult");
}

#[tokio::test]
async fn live_removal_falls_back_when_the_task_completed_before_rpc_execution() {
    let transport = ScriptedTransport::new([
        Err(RpcError::Remote {
            code: 1,
            message: "GID is not found in active downloads".into(),
            data: None,
        }),
        Ok(json!("OK")),
    ]);
    let client = Aria2Client::new(transport);

    let result =
        DownloadEngineGateway::remove(&client, Gid::from_u64(9), TaskRemovalTarget::LiveTask).await;

    assert!(result.is_ok());
    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].0, "aria2.remove");
    assert_eq!(calls[1].0, "aria2.removeDownloadResult");
}

#[tokio::test]
async fn malformed_remove_download_result_is_an_unknown_mutation_outcome() {
    let transport = ScriptedTransport::new([Ok(json!("0000000000000009"))]);
    let client = Aria2Client::new(transport);

    let error =
        DownloadEngineGateway::remove(&client, Gid::from_u64(9), TaskRemovalTarget::DownloadResult)
            .await
            .expect_err("unexpected response must not be reported as success");

    assert_eq!(error.kind, GatewayErrorKind::OutcomeUnknown);
    assert!(!error.retryable);
}

#[tokio::test]
async fn live_snapshot_fetches_every_waiting_page_before_reconcile() {
    let first_page = (1_u64..=1_000)
        .map(|value| {
            json!({
                "gid": format!("{value:016x}"),
                "status": "waiting",
                "files": [{"path": format!("/tmp/{value}.bin")}]
            })
        })
        .collect::<Vec<_>>();
    let transport = ScriptedTransport::new([
        Ok(json!({
            "numActive": "0",
            "numWaiting": "1001",
            "numStoppedTotal": "0"
        })),
        Ok(json!([])),
        Ok(Value::Array(first_page)),
        Ok(json!([{
            "gid": "00000000000003e9",
            "status": "waiting",
            "files": [{"path": "/tmp/1001.bin"}]
        }])),
    ]);
    let client = Aria2Client::new(transport);

    let snapshot = match client.refresh_live_snapshot().await {
        Ok(snapshot) => snapshot,
        Err(error) => panic!("live snapshot failed: {error}"),
    };

    assert_eq!(snapshot.waiting.len(), 1_001);
    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls.len(), 4);
    assert_eq!(
        calls[1].1[0],
        json!([
            "gid",
            "status",
            "totalLength",
            "completedLength",
            "uploadLength",
            "downloadSpeed",
            "uploadSpeed",
            "seeder",
            "connections",
            "errorCode",
            "errorMessage",
            "verifyIntegrityPending"
        ])
    );
    assert_eq!(calls[3].0, "aria2.tellWaiting");
    assert_eq!(calls[3].1[0], json!(1_000));
    assert_eq!(calls[3].1[1], json!(1));
}

#[test]
fn authentication_remote_error_maps_to_gateway_category() {
    let error = map_query_error(RpcError::Remote {
        code: 1,
        message: "Unauthorized".into(),
        data: None,
    });

    assert_eq!(error.kind, GatewayErrorKind::Authentication);
    assert!(!error.retryable);
}

#[test]
fn mutating_timeout_is_reported_as_an_unknown_outcome() {
    let error = map_mutation_error(RpcError::Timeout {
        method: "aria2.addUri".into(),
    });

    assert_eq!(error.kind, GatewayErrorKind::OutcomeUnknown);
    assert!(!error.retryable);
}

#[test]
fn malformed_mutation_responses_are_reported_as_unknown_outcomes() {
    for rpc_error in [
        RpcError::Protocol("malformed response".into()),
        RpcError::InvalidData {
            method: "aria2.addUri".into(),
            field: "result".into(),
            message: "expected GID".into(),
        },
    ] {
        let error = map_mutation_error(rpc_error);
        assert_eq!(error.kind, GatewayErrorKind::OutcomeUnknown);
        assert!(!error.retryable);
    }
}

#[tokio::test]
async fn change_options_preserves_repeated_header_keys() {
    let transport = ScriptedTransport::new([Ok(json!("OK"))]);
    let client = Aria2Client::new(transport);
    client
        .change_options(
            Gid::from_u64(1),
            &[
                ("header".into(), "X-One: 1".into()),
                ("header".into(), "X-Two: 2".into()),
            ],
        )
        .await
        .expect("change_options");

    let calls = client
        .transport()
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(calls[0].1[1], json!({ "header": ["X-One: 1", "X-Two: 2"] }));
}

#[tokio::test]
async fn retry_uri_falls_back_when_original_gid_was_purged() {
    let transport = ScriptedTransport::new([
        Err(RpcError::Remote {
            code: 1,
            message: "GID 0000000000000009 is not found".into(),
            data: None,
        }),
        Err(RpcError::Remote {
            code: 1,
            message: "GID 0000000000000009 is not found".into(),
            data: None,
        }),
        Ok(json!("00000000000000aa")),
    ]);
    let client = Aria2Client::new(transport);
    let fallback = AddDownloadRequest {
        source: AddDownloadSource::Uris(vec!["https://example.test/file.bin".into()]),
        destination: Some(ariadeck_domain::EnginePath::new("/downloads")),
        ..AddDownloadRequest::default()
    };
    let next = client
        .retry_uri(Gid::from_u64(9), &fallback)
        .await
        .expect("retry should fall back to fallback URI");
    assert_eq!(next, Gid::from_u64(0xaa));
}

#[tokio::test]
async fn change_uri_and_editable_state_decode_and_strip_secrets() {
    let transport = ScriptedTransport::new([
        Ok(json!([1, 1])),
        Ok(json!([
            [[{"uri": "https://cdn.example.test/a.bin", "status": "used"}]],
            [{
                "referer": "https://ref.example.test/",
                "user-agent": "AriaDeck/2",
                "header": ["Cookie: secret=1", "Authorization: Bearer abc", "X-Custom: ok"],
                "all-proxy": "http://user:pass@proxy.example.test:8080",
                "max-download-limit": "512K",
                "split": "8",
                "out": "a.bin",
                "dir": "/downloads"
            }]
        ])),
    ]);
    let client = Aria2Client::new(transport);
    let counts = client
        .change_uri(
            Gid::from_u64(7),
            1,
            &["https://old.example.test/a.bin".into()],
            &["https://cdn.example.test/a.bin".into()],
            Some(0),
        )
        .await
        .expect("changeUri succeeds");
    assert_eq!(counts, (1, 1));

    let state = client
        .editable_state(Gid::from_u64(7))
        .await
        .expect("editable_state succeeds");
    assert_eq!(state.uris, vec!["https://cdn.example.test/a.bin"]);
    assert_eq!(state.referer.as_deref(), Some("https://ref.example.test/"));
    assert_eq!(state.user_agent.as_deref(), Some("AriaDeck/2"));
    assert_eq!(state.headers, vec!["X-Custom: ok"]);
    assert_eq!(
        state.proxy.as_deref(),
        Some("http://proxy.example.test:8080/")
    );
    assert_eq!(state.max_download_limit.as_deref(), Some("512K"));
    assert_eq!(state.split.as_deref(), Some("8"));
    assert_eq!(state.out.as_deref(), Some("a.bin"));
    assert_eq!(state.dir.as_deref(), Some("/downloads"));
}
