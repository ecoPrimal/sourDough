//! Transport-aware JSON-RPC 2.0 client for inter-primal communication.

use super::Capability;
use super::error::{IpcError, IpcErrorKind};
use super::protocol::{JsonRpcRequest, JsonRpcRequestExt, JsonRpcResponse, MESH_RELAY_TIMEOUT};
use crate::transport::{self, TransportEndpoint};
use crate::{env_keys, methods};

/// A transport-aware JSON-RPC 2.0 client that connects via [`TransportEndpoint`].
///
/// Uses `connect_transport()` under the hood — the caller never needs to know
/// whether the remote primal is on UDS, TCP, or relay.
pub struct IpcClient {
    endpoint: TransportEndpoint,
}

impl IpcClient {
    /// Create a new client targeting the given endpoint.
    #[must_use]
    pub const fn new(endpoint: TransportEndpoint) -> Self {
        Self { endpoint }
    }

    /// Resolve a primal by name using ecosystem socket conventions, then build a client.
    #[must_use]
    pub fn from_primal(primal_name: &str, family_id: Option<&str>) -> Self {
        Self {
            endpoint: TransportEndpoint::from_primal_name(primal_name, family_id),
        }
    }

    /// The endpoint this client targets.
    #[must_use]
    pub const fn endpoint(&self) -> &TransportEndpoint {
        &self.endpoint
    }

    /// Derive the tarpc socket path from this client's JSON-RPC endpoint.
    ///
    /// Convention: `{name}.sock` → `{name}.tarpc.sock`
    /// Returns `None` for TCP or `MeshRelay` endpoints (tarpc over TCP not yet supported).
    #[must_use]
    pub fn tarpc_path(&self) -> Option<String> {
        self.endpoint
            .tarpc_endpoint()
            .and_then(|ep| ep.uds_path().map(String::from))
    }

    /// Send a JSON-RPC request and return the response.
    ///
    /// For UDS/TCP endpoints: opens a connection, writes the request as a
    /// newline-delimited JSON line, reads the response, and closes.
    ///
    /// For `MeshRelay` endpoints: transparently routes through the local `songBird`
    /// instance via `capability.call`, which forwards to the remote peer.
    ///
    /// # Errors
    ///
    /// Returns `IpcError` on transport or protocol failure.
    pub async fn call(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse, IpcError> {
        if let TransportEndpoint::MeshRelay {
            peer_id,
            capability,
        } = &self.endpoint
        {
            return self.call_via_mesh_relay(request, peer_id, capability).await;
        }
        self.call_direct(request).await
    }

    /// Direct connection call (UDS/TCP).
    async fn call_direct(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse, IpcError> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let stream = transport::connect_transport(&self.endpoint)
            .await
            .map_err(|e| {
                IpcError::new(
                    IpcErrorKind::Transport,
                    format!("connect to {}: {e}", self.endpoint),
                )
            })?;

        let mut buf_stream = BufReader::new(stream);

        let req_json = serde_json::to_string(request).map_err(|e| {
            IpcError::new(IpcErrorKind::Internal, format!("serialize request: {e}"))
        })?;

        let writer = buf_stream.get_mut();
        writer
            .write_all(req_json.as_bytes())
            .await
            .map_err(|e| IpcError::new(IpcErrorKind::Transport, format!("write: {e}")))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|e| IpcError::new(IpcErrorKind::Transport, format!("write newline: {e}")))?;

        let mut response_line = String::new();
        buf_stream
            .read_line(&mut response_line)
            .await
            .map_err(|e| IpcError::new(IpcErrorKind::Transport, format!("read response: {e}")))?;

        serde_json::from_str(response_line.trim()).map_err(|e| {
            IpcError::new(IpcErrorKind::Internal, format!("deserialize response: {e}"))
        })
    }

    /// Route through local `songBird` mesh relay (uses `call_direct` to avoid recursion).
    async fn call_via_mesh_relay(
        &self,
        request: &JsonRpcRequest,
        peer_id: &str,
        capability: &str,
    ) -> Result<JsonRpcResponse, IpcError> {
        let relay_hub = std::env::var(env_keys::MESH_RELAY_HUB)
            .unwrap_or_else(|_| env_keys::DEFAULT_MESH_RELAY_HUB.to_owned());
        let songbird = Self::from_primal(&relay_hub, None);
        let envelope =
            JsonRpcRequest::new(methods::capability::CALL, None, 1).with_params(serde_json::json!({
                "peer_id": peer_id,
                "capability": capability,
                "request": request,
            }));
        tokio::time::timeout(MESH_RELAY_TIMEOUT, songbird.call_direct(&envelope))
            .await
            .map_err(|_| {
                IpcError::new(
                    IpcErrorKind::Timeout,
                    format!("mesh relay to {peer_id}/{capability} timed out"),
                )
            })?
    }

    /// Send a JSON-RPC request with a timeout.
    ///
    /// Equivalent to [`call`](Self::call) but returns `IpcError` with
    /// `IpcErrorKind::Timeout` if the operation exceeds the given duration.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] on transport failure, protocol error, or timeout.
    pub async fn call_with_timeout(
        &self,
        request: &JsonRpcRequest,
        timeout: std::time::Duration,
    ) -> Result<JsonRpcResponse, IpcError> {
        tokio::time::timeout(timeout, self.call(request))
            .await
            .map_err(|_| {
                IpcError::new(
                    IpcErrorKind::Timeout,
                    format!(
                        "request to {} timed out after {}ms",
                        self.endpoint,
                        timeout.as_millis()
                    ),
                )
            })?
    }

    /// Probe liveness of the target primal.
    ///
    /// Returns `Ok(true)` if the primal responds with a result, `Ok(false)` if
    /// it responds with an error, or an `Err` if the transport fails entirely.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] if the transport connection fails.
    pub async fn health_liveness(&self) -> Result<bool, IpcError> {
        let req = JsonRpcRequest::new(methods::health::LIVENESS, None, 1);
        let resp = self.call(&req).await?;
        Ok(resp.error.is_none())
    }

    /// Register a capability set with songbird via `ipc.register`.
    ///
    /// This is the ecosystem standard for primals to announce their
    /// capabilities at startup so other primals can discover them.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] on transport or protocol failure.
    pub async fn register_capabilities(
        &self,
        primal_name: &str,
        capabilities: &[Capability],
        endpoint: &TransportEndpoint,
    ) -> Result<JsonRpcResponse, IpcError> {
        let params = serde_json::json!({
            "primal": primal_name,
            "capabilities": capabilities,
            "endpoint": endpoint,
        });
        let req = JsonRpcRequest::new(methods::ipc::REGISTER, None, 1).with_params(params);
        self.call(&req).await
    }

    /// Resolve another primal's endpoint via songbird `ipc.resolve`.
    ///
    /// Returns the structured `TransportEndpoint` for the given primal,
    /// enabling fully dynamic discovery without hardcoded paths.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] if transport fails or songbird returns an error.
    pub async fn resolve_primal(&self, primal_name: &str) -> Result<TransportEndpoint, IpcError> {
        let params = serde_json::json!({ "primal": primal_name });
        let req = JsonRpcRequest::new(methods::ipc::RESOLVE, None, 1).with_params(params);
        let resp = self.call(&req).await?;

        let result = resp.result.ok_or_else(|| {
            let msg = resp
                .error
                .map_or_else(|| "no result".to_owned(), |e| e.message);
            IpcError::new(IpcErrorKind::DependencyUnavailable, msg)
        })?;

        serde_json::from_value(result).map_err(|e| {
            IpcError::new(IpcErrorKind::Internal, format!("deserialize endpoint: {e}"))
        })
    }

    /// Resolve a primal via songBird and return a new client targeting it.
    ///
    /// This is the canonical discovery flow now that `songBird` has
    /// topology-aware routing (`ipc.resolve` → `TransportEndpoint`).
    /// The returned client may hold a UDS, TCP, or `MeshRelay` endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] if songBird cannot resolve the target primal.
    pub async fn resolve_and_connect(primal_name: &str) -> Result<Self, IpcError> {
        let relay_hub = std::env::var(env_keys::MESH_RELAY_HUB)
            .unwrap_or_else(|_| env_keys::DEFAULT_MESH_RELAY_HUB.to_owned());
        let hub_client = Self::from_primal(&relay_hub, None);
        let endpoint = hub_client.resolve_primal(primal_name).await?;
        Ok(Self::new(endpoint))
    }

    /// Announce this primal to the ecosystem via `primal.announce`.
    ///
    /// Combines `ipc.register` (capability registration) with a startup
    /// announcement that other primals can subscribe to. This is the
    /// canonical "I'm alive and serving" signal.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] on transport or protocol failure.
    pub async fn announce(
        &self,
        primal_name: &str,
        version: &str,
        capabilities: &[Capability],
        endpoint: &TransportEndpoint,
    ) -> Result<JsonRpcResponse, IpcError> {
        let params = serde_json::json!({
            "primal": primal_name,
            "version": version,
            "capabilities": capabilities,
            "endpoint": endpoint,
        });
        let req = JsonRpcRequest::new(methods::primal::ANNOUNCE, None, 1).with_params(params);
        self.call(&req).await
    }
}

impl std::fmt::Debug for IpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpcClient")
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stores_endpoint() {
        let ep = TransportEndpoint::tcp("127.0.0.1", 8080);
        let client = IpcClient::new(ep.clone());
        assert_eq!(*client.endpoint(), ep);
    }

    #[test]
    fn from_primal_resolves_uds() {
        let client = IpcClient::from_primal("testprimal", None);
        let path = client.endpoint().uds_path().unwrap();
        assert!(path.contains("testprimal"));
        assert_eq!(
            std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str()),
            Some("sock")
        );
    }

    #[test]
    fn from_primal_with_family_id() {
        let client = IpcClient::from_primal("testprimal", Some("family1"));
        let path = client.endpoint().uds_path().unwrap();
        assert!(path.contains("testprimal"));
        assert!(path.contains("family1"));
    }

    #[test]
    fn tarpc_path_derives_from_uds() {
        let client = IpcClient::from_primal("beardog", None);
        let tarpc = client.tarpc_path().unwrap();
        assert!(tarpc.ends_with("beardog.tarpc.sock"));
        assert!(!tarpc.ends_with("beardog.sock.tarpc.sock"));
    }

    #[test]
    fn tarpc_path_with_family_id() {
        let client = IpcClient::from_primal("beardog", Some("abc"));
        let tarpc = client.tarpc_path().unwrap();
        assert!(tarpc.contains("beardog-abc.tarpc.sock"));
    }

    #[test]
    fn tarpc_path_returns_none_for_tcp() {
        let client = IpcClient::new(TransportEndpoint::tcp("10.0.0.1", 7700));
        assert!(client.tarpc_path().is_none());
    }

    #[test]
    fn tarpc_path_returns_none_for_mesh_relay() {
        let client = IpcClient::new(TransportEndpoint::mesh_relay("peer1", "cap1"));
        assert!(client.tarpc_path().is_none());
    }

    #[test]
    fn debug_impl() {
        let client = IpcClient::new(TransportEndpoint::tcp("1.2.3.4", 9000));
        let debug = format!("{client:?}");
        assert!(debug.contains("IpcClient"));
        assert!(debug.contains("endpoint"));
    }

    #[tokio::test]
    async fn call_direct_to_nonexistent_socket_gives_transport_error() {
        let client = IpcClient::new(TransportEndpoint::uds("/tmp/nonexistent_test.sock"));
        let req = JsonRpcRequest::new("health.liveness", None, 1);
        let err = client.call(&req).await.unwrap_err();
        assert_eq!(err.kind, IpcErrorKind::Transport);
    }

    #[tokio::test]
    async fn call_mesh_relay_routes_through_hub() {
        let client = IpcClient::new(TransportEndpoint::mesh_relay("peer1", "cap1"));
        let req = JsonRpcRequest::new("some.method", None, 1);
        let result = client.call(&req).await;
        // Either transport error (no hub running) or a response (hub is running)
        match result {
            Err(e) => {
                assert!(e.kind == IpcErrorKind::Transport || e.kind == IpcErrorKind::Timeout);
            }
            Ok(resp) => {
                // If hub responded, it's a valid JSON-RPC response (possibly error)
                assert_eq!(resp.jsonrpc, "2.0");
            }
        }
    }

    #[tokio::test]
    async fn call_with_timeout_respects_duration() {
        let client = IpcClient::new(TransportEndpoint::uds("/tmp/nonexistent_timeout_test.sock"));
        let req = JsonRpcRequest::new("health.liveness", None, 1);
        let err = client
            .call_with_timeout(&req, std::time::Duration::from_millis(10))
            .await
            .unwrap_err();
        assert!(err.kind == IpcErrorKind::Transport || err.kind == IpcErrorKind::Timeout,);
    }

    #[tokio::test]
    async fn call_direct_roundtrip_with_mock_server() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixListener;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let sock_path = tmp.path().to_owned();
        drop(tmp);

        let listener = UnixListener::bind(&sock_path).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let resp = r#"{"jsonrpc":"2.0","id":1,"result":{"alive":true}}"#;
            let writer = reader.get_mut();
            writer.write_all(resp.as_bytes()).await.unwrap();
            writer.write_all(b"\n").await.unwrap();
        });

        let client = IpcClient::new(TransportEndpoint::uds(sock_path.to_string_lossy().as_ref()));
        let req = JsonRpcRequest::new("health.liveness", None, 1);
        let resp = client.call(&req).await.unwrap();

        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap(), serde_json::json!({"alive": true}));

        server.await.unwrap();
        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn health_liveness_with_mock_server() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixListener;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let sock_path = tmp.path().to_owned();
        drop(tmp);

        let listener = UnixListener::bind(&sock_path).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let resp = r#"{"jsonrpc":"2.0","id":1,"result":{"alive":true}}"#;
            let writer = reader.get_mut();
            writer.write_all(resp.as_bytes()).await.unwrap();
            writer.write_all(b"\n").await.unwrap();
        });

        let client = IpcClient::new(TransportEndpoint::uds(sock_path.to_string_lossy().as_ref()));
        let alive = client.health_liveness().await.unwrap();
        assert!(alive);

        server.await.unwrap();
        let _ = std::fs::remove_file(&sock_path);
    }
}
