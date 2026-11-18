use ariadeck_domain::{TaskOptionEntry, TaskPeer, TaskServer, TaskSnapshot, TaskUri};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::{
    RpcError,
    models::{DetailUriWire, PeerWire, ServerGroupWire, TaskKey, TaskWire},
};

pub(super) fn next_batch_result(
    results: &mut impl Iterator<Item = Result<Value, RpcError>>,
    method: &str,
) -> Result<Value, RpcError> {
    results.next().ok_or_else(|| {
        RpcError::Protocol(format!("batch response is missing result for {method}"))
    })?
}

pub(super) fn next_multicall_result(
    results: &mut impl Iterator<Item = Result<Value, RpcError>>,
    method: &str,
) -> Result<Value, RpcError> {
    results.next().ok_or_else(|| {
        RpcError::Protocol(format!("multicall response is missing result for {method}"))
    })?
}

pub(super) fn decode_multicall_entry(method: String, entry: Value) -> Result<Value, RpcError> {
    // aria2 wraps a successful multicall item as a one-element array and an
    // error item as an object with code/message, matching the RPC manual.
    if let Value::Array(mut values) = entry {
        if values.len() == 1 {
            return Ok(values.remove(0));
        }
        return Err(RpcError::InvalidData {
            method: "system.multicall".into(),
            field: method,
            message: "expected a one-element success array".into(),
        });
    }
    if let Value::Object(mut object) = entry {
        let code = object
            .remove("code")
            .and_then(|value| value.as_i64())
            .unwrap_or(1);
        let message = object
            .remove("message")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("{method} failed inside multicall"));
        return Err(RpcError::Remote {
            code,
            message,
            data: object.remove("data"),
        });
    }
    Err(RpcError::InvalidData {
        method: "system.multicall".into(),
        field: method,
        message: "expected a success array or an error object".into(),
    })
}

pub(super) fn decode_detail_uris(value: Value) -> Result<Vec<TaskUri>, RpcError> {
    const METHOD: &str = "aria2.getUris";
    Ok(decode::<Vec<DetailUriWire>>(METHOD, value)?
        .into_iter()
        .map(DetailUriWire::into_domain)
        .filter(|uri| !uri.uri.is_empty())
        .collect())
}

pub(super) fn decode_servers(value: Value) -> Result<Vec<TaskServer>, RpcError> {
    const METHOD: &str = "aria2.getServers";
    let groups = decode::<Vec<ServerGroupWire>>(METHOD, value)?;
    let mut servers = Vec::new();
    for group in groups {
        servers.extend(group.into_domain(METHOD)?);
    }
    Ok(servers)
}

pub(super) fn decode_peers(value: Value) -> Result<Vec<TaskPeer>, RpcError> {
    const METHOD: &str = "aria2.getPeers";
    decode::<Vec<PeerWire>>(METHOD, value)?
        .into_iter()
        .map(|peer| peer.into_domain(METHOD))
        .collect()
}

pub(super) fn decode_validated_option_map(
    method: &str,
    value: Value,
) -> Result<Map<String, Value>, RpcError> {
    let options = decode::<Map<String, Value>>(method, value)?;
    for (key, value) in &options {
        let valid = matches!(value, Value::String(_))
            || matches!(value, Value::Array(values) if values.iter().all(Value::is_string));
        if !valid {
            return Err(RpcError::InvalidData {
                method: method.into(),
                field: key.clone(),
                message: "expected a string or an array of strings".into(),
            });
        }
    }
    Ok(options)
}

pub(super) fn decode_option_entries(value: Value) -> Result<Vec<TaskOptionEntry>, RpcError> {
    let options = decode_validated_option_map("aria2.getOption", value)?;
    let mut entries = options
        .into_iter()
        .filter_map(|(key, value)| {
            if ariadeck_domain::task_option_key_is_sensitive(&key) {
                return Some(TaskOptionEntry {
                    key,
                    value: String::new(),
                    redacted: true,
                });
            }
            value.as_str().map(|value| TaskOptionEntry {
                key,
                value: value.to_owned(),
                redacted: false,
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(entries)
}

pub(super) fn decode_tasks(method: &str, value: Value) -> Result<Vec<TaskSnapshot>, RpcError> {
    decode::<Vec<TaskWire>>(method, value)?
        .into_iter()
        .map(|task| task.into_domain(method))
        .collect()
}

pub(super) fn is_gid_not_found(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("gid") && message.contains("not found")
}

pub(super) fn is_no_uri_data(error: &RpcError) -> bool {
    matches!(
        error,
        RpcError::Remote { message, .. }
            if message.to_ascii_lowercase().contains("no uri data is available")
    )
}

pub(super) fn task_keys(keys: &[TaskKey]) -> Value {
    Value::Array(
        keys.iter()
            .map(|key| Value::String(key.as_str().into()))
            .collect(),
    )
}

pub(super) fn decode<T>(method: &str, value: Value) -> Result<T, RpcError>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value).map_err(|error| RpcError::InvalidData {
        method: method.into(),
        field: "response".into(),
        message: error.to_string(),
    })
}
