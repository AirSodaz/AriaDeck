use thiserror::Error;
use url::{Host, Url};

/// Canonical validation errors for aria2 JSON-RPC WebSocket endpoints.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RpcEndpointValidationError {
    #[error(
        "HTTP JSON-RPC fallback is disabled because it loses aria2 WebSocket notifications; use ws:// or wss:// explicitly"
    )]
    HttpFallbackDisabled,
    #[error("RPC endpoint must use ws:// or wss://")]
    UnsupportedScheme,
    #[error("RPC endpoint must include a host")]
    MissingHost,
    #[error("RPC endpoint credentials are forbidden; supply ARIADECK_RPC_SECRET separately")]
    EmbeddedCredentials,
    #[error("aria2 RPC endpoint path must be exactly /jsonrpc")]
    InvalidPath,
    #[error("RPC endpoint query and fragment components are forbidden")]
    QueryOrFragmentForbidden,
    #[error(
        "remote aria2 RPC requires wss://; set ARIADECK_RPC_ALLOW_INSECURE_REMOTE=true only for an explicitly trusted network"
    )]
    InsecureRemoteForbidden,
}

/// Validates endpoint shape (scheme, host, no userinfo/query/fragment, `/jsonrpc` path)
/// without applying the runtime remote-TLS policy.
pub fn validate_rpc_endpoint_shape(endpoint: &Url) -> Result<(), RpcEndpointValidationError> {
    match endpoint.scheme() {
        "ws" | "wss" => {}
        "http" | "https" => return Err(RpcEndpointValidationError::HttpFallbackDisabled),
        _ => return Err(RpcEndpointValidationError::UnsupportedScheme),
    }
    if endpoint.host().is_none() {
        return Err(RpcEndpointValidationError::MissingHost);
    }
    if !endpoint.username().is_empty() || endpoint.password().is_some() {
        return Err(RpcEndpointValidationError::EmbeddedCredentials);
    }
    if endpoint.path() != "/jsonrpc" {
        return Err(RpcEndpointValidationError::InvalidPath);
    }
    if endpoint.query().is_some() || endpoint.fragment().is_some() {
        return Err(RpcEndpointValidationError::QueryOrFragmentForbidden);
    }
    Ok(())
}

/// Validates endpoint shape plus the transport TLS policy for non-loopback hosts.
pub fn validate_rpc_endpoint(
    endpoint: &Url,
    allow_insecure_remote: bool,
) -> Result<(), RpcEndpointValidationError> {
    validate_rpc_endpoint_shape(endpoint)?;
    if endpoint.scheme() == "ws" && !allow_insecure_remote && !is_loopback_endpoint(endpoint) {
        return Err(RpcEndpointValidationError::InsecureRemoteForbidden);
    }
    Ok(())
}

#[must_use]
pub fn is_loopback_endpoint(endpoint: &Url) -> bool {
    match endpoint.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_validation_rejects_credentials_query_fragment_and_wrong_path() {
        for (raw, expected) in [
            (
                "http://127.0.0.1:6800/jsonrpc",
                RpcEndpointValidationError::HttpFallbackDisabled,
            ),
            (
                "ftp://127.0.0.1:6800/jsonrpc",
                RpcEndpointValidationError::UnsupportedScheme,
            ),
            (
                "ws://user:pass@127.0.0.1:6800/jsonrpc",
                RpcEndpointValidationError::EmbeddedCredentials,
            ),
            (
                "wss://example.com:6800/rpc",
                RpcEndpointValidationError::InvalidPath,
            ),
            (
                "wss://example.com:6800/jsonrpc?token=secret",
                RpcEndpointValidationError::QueryOrFragmentForbidden,
            ),
            (
                "wss://example.com:6800/jsonrpc#frag",
                RpcEndpointValidationError::QueryOrFragmentForbidden,
            ),
        ] {
            let url = Url::parse(raw).expect("url should parse");
            assert_eq!(validate_rpc_endpoint_shape(&url), Err(expected), "{raw}");
        }
    }

    #[test]
    fn transport_validation_enforces_tls_on_non_loopback() {
        let local = Url::parse("ws://127.0.0.1:6800/jsonrpc").expect("local url");
        assert_eq!(validate_rpc_endpoint(&local, false), Ok(()));

        let remote_ws = Url::parse("ws://192.0.2.10:6800/jsonrpc").expect("remote ws");
        assert_eq!(
            validate_rpc_endpoint(&remote_ws, false),
            Err(RpcEndpointValidationError::InsecureRemoteForbidden)
        );
        assert_eq!(validate_rpc_endpoint(&remote_ws, true), Ok(()));

        let remote_wss = Url::parse("wss://192.0.2.10:6800/jsonrpc").expect("remote wss");
        assert_eq!(validate_rpc_endpoint(&remote_wss, false), Ok(()));
    }
}
