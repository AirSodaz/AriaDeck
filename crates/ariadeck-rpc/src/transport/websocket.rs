use std::{
    collections::HashMap,
    fmt,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use async_trait::async_trait;
use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::Value;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error as WebSocketError, Message},
};
use url::{Host, Url};

use crate::{
    Aria2Notification, RpcCall, RpcError,
    protocol::{DecodedMessage, RpcId, RpcRequest, decode_payload},
    transport::RpcTransport,
};

#[derive(Clone)]
pub struct WebSocketConfig {
    pub endpoint: Url,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub allow_insecure_remote: bool,
}

impl WebSocketConfig {
    #[must_use]
    pub fn new(endpoint: Url) -> Self {
        Self {
            endpoint,
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(15),
            allow_insecure_remote: false,
        }
    }

    pub fn validate(&self) -> Result<(), RpcError> {
        validate_endpoint(&self.endpoint, self.allow_insecure_remote)?;
        if self.connect_timeout.is_zero() {
            return Err(RpcError::Configuration(
                "connect timeout must be greater than zero".into(),
            ));
        }
        if self.request_timeout.is_zero() {
            return Err(RpcError::Configuration(
                "request timeout must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for WebSocketConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketConfig")
            .field("endpoint", &redacted_endpoint(&self.endpoint))
            .field("connect_timeout", &self.connect_timeout)
            .field("request_timeout", &self.request_timeout)
            .field("allow_insecure_remote", &self.allow_insecure_remote)
            .finish()
    }
}

#[derive(Clone)]
pub struct WebSocketTransport {
    commands: mpsc::Sender<Command>,
    notifications: broadcast::Sender<Aria2Notification>,
    closed: watch::Receiver<Option<RpcError>>,
    next_id: Arc<AtomicU64>,
    request_timeout: Duration,
}

impl WebSocketTransport {
    pub async fn connect(config: WebSocketConfig) -> Result<Self, RpcError> {
        config.validate()?;

        let secure = config.endpoint.scheme() == "wss";
        let endpoint = config.endpoint.as_str().to_owned();
        let (stream, _) = tokio::time::timeout(config.connect_timeout, connect_async(endpoint))
            .await
            .map_err(|_| RpcError::Timeout {
                method: "websocket.connect".into(),
            })?
            .map_err(|error| map_connect_error(error, secure))?;
        let (commands, command_rx) = mpsc::channel(128);
        let (notifications, _) = broadcast::channel(128);
        let (closed_tx, closed) = watch::channel(None);
        let actor_notifications = notifications.clone();
        tokio::spawn(async move {
            let reason = run_connection(stream, command_rx, actor_notifications).await;
            let _ = closed_tx.send(Some(reason));
        });

        Ok(Self {
            commands,
            notifications,
            closed,
            next_id: Arc::new(AtomicU64::new(1)),
            request_timeout: config.request_timeout,
        })
    }

    #[must_use]
    pub fn subscribe_notifications(&self) -> broadcast::Receiver<Aria2Notification> {
        self.notifications.subscribe()
    }

    #[must_use]
    pub fn subscribe_closed(&self) -> watch::Receiver<Option<RpcError>> {
        self.closed.clone()
    }

    pub async fn close(&self) {
        let mut closed = self.closed.clone();
        if closed.borrow().is_some() {
            return;
        }
        if self.commands.send(Command::Shutdown).await.is_err() {
            return;
        }
        while closed.borrow().is_none() {
            if closed.changed().await.is_err() {
                break;
            }
        }
    }

    async fn dispatch(
        &self,
        calls: Vec<RpcCall>,
        batch: bool,
    ) -> Result<Vec<Result<Value, RpcError>>, RpcError> {
        if calls.is_empty() {
            return Ok(Vec::new());
        }

        let mut ids = Vec::with_capacity(calls.len());
        let mut waiters = Vec::with_capacity(calls.len());
        let requests = calls
            .iter()
            .map(|call| {
                let id = RpcId(self.next_id.fetch_add(1, Ordering::Relaxed));
                let (sender, receiver) = oneshot::channel();
                ids.push(id);
                waiters.push((call.method.clone(), receiver));
                (RpcRequest::new(id, call), sender)
            })
            .collect::<Vec<_>>();

        let payload = if batch {
            serde_json::to_string(
                &requests
                    .iter()
                    .map(|(request, _)| request)
                    .collect::<Vec<_>>(),
            )
        } else {
            serde_json::to_string(&requests[0].0)
        }
        .map_err(|error| RpcError::Serialization(error.to_string()))?;
        let pending = requests
            .into_iter()
            .zip(ids.iter().copied())
            .map(|((_, sender), id)| (id, sender))
            .collect();

        self.commands
            .send(Command::Send { payload, pending })
            .await
            .map_err(|_| RpcError::Closed)?;

        let receive_all = async {
            let mut results = Vec::with_capacity(waiters.len());
            for (_, receiver) in waiters {
                results.push(receiver.await.unwrap_or(Err(RpcError::Closed)));
            }
            results
        };

        match tokio::time::timeout(self.request_timeout, receive_all).await {
            Ok(results) => Ok(results),
            Err(_) => {
                let _ = self.commands.send(Command::Cancel(ids)).await;
                let method = calls
                    .iter()
                    .map(|call| call.method.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                Err(RpcError::Timeout { method })
            }
        }
    }
}

#[async_trait]
impl RpcTransport for WebSocketTransport {
    async fn call(&self, method: &str, params: Vec<Value>) -> Result<Value, RpcError> {
        let mut results = self
            .dispatch(vec![RpcCall::new(method, params)], false)
            .await?;
        results.pop().unwrap_or(Err(RpcError::Closed))
    }

    async fn batch(&self, calls: Vec<RpcCall>) -> Result<Vec<Result<Value, RpcError>>, RpcError> {
        self.dispatch(calls, true).await
    }
}

enum Command {
    Send {
        payload: String,
        pending: Vec<(RpcId, ResponseSender)>,
    },
    Cancel(Vec<RpcId>),
    Shutdown,
}

type ResponseSender = oneshot::Sender<Result<Value, RpcError>>;

async fn run_connection<S>(
    stream: tokio_tungstenite::WebSocketStream<S>,
    mut commands: mpsc::Receiver<Command>,
    notifications: broadcast::Sender<Aria2Notification>,
) -> RpcError
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut sink, mut stream) = stream.split();
    let mut pending = HashMap::<RpcId, ResponseSender>::new();

    loop {
        tokio::select! {
            command = commands.recv() => {
                match command {
                    Some(Command::Send { payload, pending: new_pending }) => {
                        pending.extend(new_pending);
                        if let Err(error) = sink.send(Message::Text(payload.into())).await {
                            let error = map_websocket_error(error);
                            fail_all(&mut pending, &error);
                            return error;
                        }
                    }
                    Some(Command::Cancel(ids)) => {
                        for id in ids {
                            pending.remove(&id);
                        }
                    }
                    Some(Command::Shutdown) | None => {
                        let _ = sink.close().await;
                        fail_all(&mut pending, &RpcError::Closed);
                        return RpcError::Closed;
                    }
                }
            }
            message = stream.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        handle_payload(text.as_bytes(), &mut pending, &notifications);
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        handle_payload(bytes.as_ref(), &mut pending, &notifications);
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if let Err(error) = sink.send(Message::Pong(payload)).await {
                            let error = map_websocket_error(error);
                            fail_all(&mut pending, &error);
                            return error;
                        }
                    }
                    Some(Ok(Message::Pong(_) | Message::Frame(_))) => {}
                    Some(Ok(Message::Close(_))) | None => {
                        fail_all(&mut pending, &RpcError::Closed);
                        return RpcError::Closed;
                    }
                    Some(Err(error)) => {
                        let error = map_websocket_error(error);
                        fail_all(&mut pending, &error);
                        return error;
                    }
                }
            }
        }
    }
}

fn validate_endpoint(endpoint: &Url, allow_insecure_remote: bool) -> Result<(), RpcError> {
    match endpoint.scheme() {
        "ws" | "wss" => {}
        "http" | "https" => {
            return Err(RpcError::Configuration(
                "HTTP JSON-RPC fallback is disabled because it loses aria2 WebSocket notifications; use ws:// or wss:// explicitly"
                    .into(),
            ));
        }
        _ => {
            return Err(RpcError::Configuration(
                "RPC endpoint must use ws:// or wss://".into(),
            ));
        }
    }
    if endpoint.host().is_none() {
        return Err(RpcError::Configuration(
            "RPC endpoint must include a host".into(),
        ));
    }
    if !endpoint.username().is_empty() || endpoint.password().is_some() {
        return Err(RpcError::Configuration(
            "RPC endpoint credentials are forbidden; supply ARIADECK_RPC_SECRET separately".into(),
        ));
    }
    if endpoint.path() != "/jsonrpc" {
        return Err(RpcError::Configuration(
            "aria2 RPC endpoint path must be exactly /jsonrpc".into(),
        ));
    }
    if endpoint.query().is_some() || endpoint.fragment().is_some() {
        return Err(RpcError::Configuration(
            "RPC endpoint query and fragment components are forbidden".into(),
        ));
    }
    if endpoint.scheme() == "ws" && !allow_insecure_remote && !is_loopback(endpoint) {
        return Err(RpcError::Configuration(
            "remote aria2 RPC requires wss://; set ARIADECK_RPC_ALLOW_INSECURE_REMOTE=true only for an explicitly trusted network"
                .into(),
        ));
    }
    Ok(())
}

fn is_loopback(endpoint: &Url) -> bool {
    match endpoint.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

fn redacted_endpoint(endpoint: &Url) -> String {
    let mut redacted = endpoint.clone();
    let _ = redacted.set_username("");
    let _ = redacted.set_password(None);
    redacted.set_query(None);
    redacted.set_fragment(None);
    redacted.to_string()
}

fn map_websocket_error(error: WebSocketError) -> RpcError {
    match error {
        WebSocketError::Tls(error) => RpcError::Tls(error.to_string()),
        WebSocketError::Http(response) if matches!(response.status().as_u16(), 401 | 403) => {
            RpcError::Authentication(format!(
                "WebSocket handshake returned HTTP status {}",
                response.status().as_u16()
            ))
        }
        WebSocketError::Http(response) => RpcError::Transport(format!(
            "WebSocket handshake returned HTTP status {}",
            response.status().as_u16()
        )),
        error => RpcError::Transport(error.to_string()),
    }
}

fn map_connect_error(error: WebSocketError, secure: bool) -> RpcError {
    match error {
        WebSocketError::Io(error) if secure && error.kind() == std::io::ErrorKind::InvalidData => {
            RpcError::Tls(error.to_string())
        }
        error => map_websocket_error(error),
    }
}

fn handle_payload(
    payload: &[u8],
    pending: &mut HashMap<RpcId, ResponseSender>,
    notifications: &broadcast::Sender<Aria2Notification>,
) {
    match decode_payload(payload) {
        Ok(messages) => {
            for message in messages {
                match message {
                    DecodedMessage::Response { id, result } => {
                        if let Some(sender) = pending.remove(&id) {
                            let _ = sender.send(result);
                        } else {
                            tracing::debug!(
                                request_id = id.0,
                                "ignoring late or unknown RPC response"
                            );
                        }
                    }
                    DecodedMessage::Notification(notification) => {
                        let _ = notifications.send(notification);
                    }
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "discarding malformed JSON-RPC payload");
            fail_all(pending, &error);
        }
    }
}

fn fail_all(pending: &mut HashMap<RpcId, ResponseSender>, error: &RpcError) {
    for (_, sender) in pending.drain() {
        let _ = sender.send(Err(error.clone()));
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use rcgen::generate_simple_self_signed;
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio_rustls::{
        TlsAcceptor,
        rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer},
    };
    use tokio_tungstenite::accept_async;

    use super::*;

    type TestError = Box<dyn Error + Send + Sync>;

    async fn test_endpoint() -> Result<(TcpListener, Url), TestError> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = Url::parse(&format!("ws://{}/jsonrpc", listener.local_addr()?))?;
        Ok((listener, endpoint))
    }

    #[test]
    fn endpoint_validation_enforces_transport_path_and_remote_tls_policy() -> Result<(), TestError>
    {
        for endpoint in [
            "ws://localhost:6800/jsonrpc",
            "ws://127.0.0.1:6800/jsonrpc",
            "ws://[::1]:6800/jsonrpc",
            "wss://downloads.example:443/jsonrpc",
        ] {
            WebSocketConfig::new(Url::parse(endpoint)?).validate()?;
        }

        for endpoint in [
            "http://localhost:6800/jsonrpc",
            "https://downloads.example/jsonrpc",
            "ftp://localhost/jsonrpc",
            "ws://localhost:6800/",
            "ws://localhost:6800/jsonrpc?token=secret",
            "ws://localhost:6800/jsonrpc#secret",
            "ws://user:password@localhost:6800/jsonrpc",
            "ws://downloads.example:6800/jsonrpc",
        ] {
            assert!(matches!(
                WebSocketConfig::new(Url::parse(endpoint)?).validate(),
                Err(RpcError::Configuration(_))
            ));
        }

        let mut explicitly_insecure =
            WebSocketConfig::new(Url::parse("ws://downloads.example:6800/jsonrpc")?);
        explicitly_insecure.allow_insecure_remote = true;
        explicitly_insecure.validate()?;
        Ok(())
    }

    #[test]
    fn endpoint_debug_redacts_userinfo_query_and_fragment() -> Result<(), TestError> {
        let config = WebSocketConfig::new(Url::parse(
            "wss://rpc-user:rpc-password@downloads.example/jsonrpc?authorization=bearer-token#secret",
        )?);
        let debug = format!("{config:?}");

        assert!(debug.contains("wss://downloads.example/jsonrpc"));
        for secret in [
            "rpc-user",
            "rpc-password",
            "authorization",
            "bearer-token",
            "secret",
        ] {
            assert!(!debug.contains(secret));
        }
        Ok(())
    }

    #[test]
    fn handshake_authentication_error_omits_response_headers() -> Result<(), TestError> {
        let response = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(401)
            .header("www-authenticate", "Bearer embedded-secret")
            .body(None)?;
        let error = map_websocket_error(WebSocketError::Http(Box::new(response)));
        let rendered = error.to_string();

        assert!(matches!(error, RpcError::Authentication(_)));
        assert!(rendered.contains("401"));
        assert!(!rendered.contains("embedded-secret"));
        Ok(())
    }

    #[tokio::test]
    async fn rejects_an_untrusted_wss_certificate_as_a_terminal_tls_error() -> Result<(), TestError>
    {
        let certified = generate_simple_self_signed(vec!["localhost".into()])?;
        let certificate = certified.cert.der().clone();
        let private_key = PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der());
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate], private_key.into())?;
        let acceptor = TlsAcceptor::from(Arc::new(server_config));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            let _ = acceptor.accept(stream).await;
            Ok::<_, std::io::Error>(())
        });

        let endpoint = Url::parse(&format!("wss://localhost:{port}/jsonrpc"))?;
        let error = match WebSocketTransport::connect(WebSocketConfig::new(endpoint)).await {
            Ok(transport) => {
                transport.close().await;
                return Err(std::io::Error::other(
                    "the untrusted certificate was unexpectedly accepted",
                )
                .into());
            }
            Err(error) => error,
        };

        assert!(
            matches!(error, RpcError::Tls(_)),
            "unexpected TLS failure classification: {error:?}"
        );
        assert!(
            error
                .to_string()
                .to_ascii_lowercase()
                .contains("certificate")
        );
        server.await??;
        Ok(())
    }

    fn text_json(message: Message) -> Result<Value, TestError> {
        match message {
            Message::Text(text) => Ok(serde_json::from_slice(text.as_bytes())?),
            _ => Err(std::io::Error::other("expected a text WebSocket message").into()),
        }
    }

    #[tokio::test]
    async fn matches_out_of_order_responses_to_concurrent_calls() -> Result<(), TestError> {
        let (listener, endpoint) = test_endpoint().await?;
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            let mut socket = accept_async(socket).await?;
            let first = text_json(
                socket
                    .next()
                    .await
                    .ok_or_else(|| std::io::Error::other("missing first request"))??,
            )?;
            let second = text_json(
                socket
                    .next()
                    .await
                    .ok_or_else(|| std::io::Error::other("missing second request"))??,
            )?;
            let requests = [first, second];
            for request in requests.iter().rev() {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "result": request["method"],
                });
                socket
                    .send(Message::Text(response.to_string().into()))
                    .await?;
            }
            Ok::<_, TestError>(())
        });

        let transport = WebSocketTransport::connect(WebSocketConfig::new(endpoint)).await?;
        let (first, second) = tokio::join!(
            transport.call("first", Vec::new()),
            transport.call("second", Vec::new())
        );
        assert_eq!(first?, Value::String("first".into()));
        assert_eq!(second?, Value::String("second".into()));
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn sends_real_json_batch_and_restores_call_order() -> Result<(), TestError> {
        let (listener, endpoint) = test_endpoint().await?;
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            let mut socket = accept_async(socket).await?;
            let request = text_json(
                socket
                    .next()
                    .await
                    .ok_or_else(|| std::io::Error::other("missing batch request"))??,
            )?;
            let requests = request
                .as_array()
                .ok_or_else(|| std::io::Error::other("request was not a JSON batch"))?;
            let responses = requests
                .iter()
                .rev()
                .map(|request| {
                    json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": request["method"],
                    })
                })
                .collect::<Vec<_>>();
            socket
                .send(Message::Text(serde_json::to_string(&responses)?.into()))
                .await?;
            Ok::<_, TestError>(())
        });

        let transport = WebSocketTransport::connect(WebSocketConfig::new(endpoint)).await?;
        let results = transport
            .batch(vec![
                RpcCall::new("one", Vec::new()),
                RpcCall::new("two", Vec::new()),
            ])
            .await?;
        assert_eq!(results[0], Ok(Value::String("one".into())));
        assert_eq!(results[1], Ok(Value::String("two".into())));
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn forwards_typed_notifications() -> Result<(), TestError> {
        let (listener, endpoint) = test_endpoint().await?;
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            let mut socket = accept_async(socket).await?;
            socket
                .send(Message::Text(
                    json!({
                        "jsonrpc": "2.0",
                        "method": "aria2.onDownloadStart",
                        "params": [{"gid": "0000000000000003"}],
                    })
                    .to_string()
                    .into(),
                ))
                .await?;
            Ok::<_, TestError>(())
        });

        let transport = WebSocketTransport::connect(WebSocketConfig::new(endpoint)).await?;
        let mut notifications = transport.subscribe_notifications();
        let notification = notifications.recv().await?;
        assert_eq!(
            notification.kind,
            crate::Aria2NotificationKind::DownloadStarted
        );
        assert_eq!(notification.gid, Some(ariadeck_domain::Gid::from_u64(3)));
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn times_out_and_cancels_unanswered_request() -> Result<(), TestError> {
        let (listener, endpoint) = test_endpoint().await?;
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            let mut socket = accept_async(socket).await?;
            let _ = socket.next().await;
            tokio::time::sleep(Duration::from_millis(150)).await;
            Ok::<_, TestError>(())
        });

        let mut config = WebSocketConfig::new(endpoint);
        config.request_timeout = Duration::from_millis(30);
        let transport = WebSocketTransport::connect(config).await?;
        let result = transport.call("never.responds", Vec::new()).await;
        assert_eq!(
            result,
            Err(RpcError::Timeout {
                method: "never.responds".into(),
            })
        );
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn close_waits_until_socket_actor_has_finished() -> Result<(), TestError> {
        let (listener, endpoint) = test_endpoint().await?;
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            let mut socket = accept_async(socket).await?;
            match socket.next().await {
                Some(Ok(Message::Close(_))) | None => Ok::<_, TestError>(()),
                Some(Ok(message)) => Err(std::io::Error::other(format!(
                    "expected close frame, received {message:?}"
                ))
                .into()),
                Some(Err(error)) => Err(error.into()),
            }
        });

        let transport = WebSocketTransport::connect(WebSocketConfig::new(endpoint)).await?;
        let closed = transport.subscribe_closed();
        transport.close().await;

        assert!(closed.borrow().is_some());
        server.await??;
        Ok(())
    }
}
