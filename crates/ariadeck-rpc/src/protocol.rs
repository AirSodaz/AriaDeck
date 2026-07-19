use ariadeck_domain::Gid;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::{Aria2Notification, Aria2NotificationKind, RpcError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpcCall {
    pub method: String,
    pub params: Vec<Value>,
}

impl RpcCall {
    #[must_use]
    pub fn new(method: impl Into<String>, params: Vec<Value>) -> Self {
        Self {
            method: method.into(),
            params,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub(crate) struct RpcId(pub(crate) u64);

#[derive(Serialize)]
pub(crate) struct RpcRequest<'a> {
    jsonrpc: &'static str,
    id: RpcId,
    method: &'a str,
    params: &'a [Value],
}

impl<'a> RpcRequest<'a> {
    pub(crate) fn new(id: RpcId, call: &'a RpcCall) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: &call.method,
            params: &call.params,
        }
    }
}

pub(crate) enum DecodedMessage {
    Response {
        id: RpcId,
        result: Result<Value, RpcError>,
    },
    Notification(Aria2Notification),
}

pub(crate) fn decode_payload(payload: &[u8]) -> Result<Vec<DecodedMessage>, RpcError> {
    let value: Value =
        serde_json::from_slice(payload).map_err(|error| RpcError::Protocol(error.to_string()))?;
    match value {
        Value::Array(values) => {
            if values.is_empty() {
                return Err(RpcError::Protocol(
                    "a JSON-RPC batch response cannot be empty".into(),
                ));
            }
            values.into_iter().map(decode_value).collect()
        }
        value => Ok(vec![decode_value(value)?]),
    }
}

fn decode_value(value: Value) -> Result<DecodedMessage, RpcError> {
    let Value::Object(mut object) = value else {
        return Err(RpcError::Protocol(
            "a JSON-RPC message must be an object".into(),
        ));
    };
    validate_jsonrpc_version(&object)?;

    if object.contains_key("id") {
        return decode_response(&mut object);
    }
    if object.contains_key("method") {
        return decode_notification(&mut object);
    }
    Err(RpcError::Protocol(
        "a JSON-RPC message has neither id nor method".into(),
    ))
}

fn validate_jsonrpc_version(object: &Map<String, Value>) -> Result<(), RpcError> {
    match object.get("jsonrpc").and_then(Value::as_str) {
        Some("2.0") => Ok(()),
        _ => Err(RpcError::Protocol(
            "JSON-RPC version must be exactly 2.0".into(),
        )),
    }
}

fn decode_response(object: &mut Map<String, Value>) -> Result<DecodedMessage, RpcError> {
    let id = parse_id(
        object
            .remove("id")
            .ok_or_else(|| RpcError::Protocol("response is missing id".into()))?,
    )?;
    let result = match (object.remove("result"), object.remove("error")) {
        (Some(result), None) => Ok(result),
        (None, Some(error)) => Err(parse_remote_error(error)?),
        (Some(_), Some(_)) => {
            return Err(RpcError::Protocol(
                "response contains both result and error".into(),
            ));
        }
        (None, None) => {
            return Err(RpcError::Protocol(
                "response contains neither result nor error".into(),
            ));
        }
    };
    Ok(DecodedMessage::Response { id, result })
}

fn parse_id(value: Value) -> Result<RpcId, RpcError> {
    if let Some(id) = value.as_u64() {
        return Ok(RpcId(id));
    }
    if let Some(id) = value.as_str() {
        return id
            .parse::<u64>()
            .map(RpcId)
            .map_err(|_| RpcError::Protocol("response id is not an unsigned integer".into()));
    }
    Err(RpcError::Protocol(
        "response id is not an unsigned integer".into(),
    ))
}

fn parse_remote_error(value: Value) -> Result<RpcError, RpcError> {
    let Value::Object(mut object) = value else {
        return Err(RpcError::Protocol(
            "response error must be an object".into(),
        ));
    };
    let code = object
        .remove("code")
        .and_then(|value| value.as_i64())
        .ok_or_else(|| RpcError::Protocol("response error is missing numeric code".into()))?;
    let message = object
        .remove("message")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| RpcError::Protocol("response error is missing message".into()))?;
    Ok(RpcError::Remote {
        code,
        message,
        data: object.remove("data"),
    })
}

fn decode_notification(object: &mut Map<String, Value>) -> Result<DecodedMessage, RpcError> {
    let method = object
        .remove("method")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| RpcError::Protocol("notification is missing method".into()))?;
    let kind = match method.as_str() {
        "aria2.onDownloadStart" => Aria2NotificationKind::DownloadStarted,
        "aria2.onDownloadPause" => Aria2NotificationKind::DownloadPaused,
        "aria2.onDownloadStop" => Aria2NotificationKind::DownloadStopped,
        "aria2.onDownloadComplete" => Aria2NotificationKind::DownloadCompleted,
        "aria2.onDownloadError" => Aria2NotificationKind::DownloadErrored,
        "aria2.onBtDownloadComplete" => Aria2NotificationKind::BitTorrentDownloadCompleted,
        _ => Aria2NotificationKind::Unknown(method),
    };
    let gid = extract_notification_gid(object.remove("params"))?;
    Ok(DecodedMessage::Notification(Aria2Notification {
        kind,
        gid,
    }))
}

fn extract_notification_gid(params: Option<Value>) -> Result<Option<Gid>, RpcError> {
    let Some(Value::Array(params)) = params else {
        return Ok(None);
    };
    let Some(gid) = params
        .first()
        .and_then(Value::as_object)
        .and_then(|object| object.get("gid"))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    gid.parse::<Gid>()
        .map(Some)
        .map_err(|error| RpcError::Protocol(format!("notification contains invalid GID: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_remote_error_and_string_id() {
        let messages = match decode_payload(
            br#"{"jsonrpc":"2.0","id":"7","error":{"code":1,"message":"Unauthorized"}}"#,
        ) {
            Ok(messages) => messages,
            Err(error) => panic!("valid error response rejected: {error}"),
        };

        let DecodedMessage::Response { id, result } = &messages[0] else {
            panic!("expected response");
        };
        assert_eq!(*id, RpcId(7));
        assert_eq!(
            *result,
            Err(RpcError::Remote {
                code: 1,
                message: "Unauthorized".into(),
                data: None,
            })
        );
    }

    #[test]
    fn notification_is_a_refresh_hint_with_optional_gid() {
        let messages = match decode_payload(
            br#"{"jsonrpc":"2.0","method":"aria2.onDownloadComplete","params":[{"gid":"000000000000000a"}]}"#,
        ) {
            Ok(messages) => messages,
            Err(error) => panic!("valid notification rejected: {error}"),
        };

        let DecodedMessage::Notification(notification) = &messages[0] else {
            panic!("expected notification");
        };
        assert_eq!(notification.kind, Aria2NotificationKind::DownloadCompleted);
        assert_eq!(notification.gid, Some(Gid::from_u64(10)));
    }

    #[test]
    fn rejects_ambiguous_or_versionless_messages() {
        assert!(decode_payload(br#"{"id":1,"result":true}"#).is_err());
        assert!(
            decode_payload(
                br#"{"jsonrpc":"2.0","id":1,"result":true,"error":{"code":1,"message":"x"}}"#
            )
            .is_err()
        );
    }
}
