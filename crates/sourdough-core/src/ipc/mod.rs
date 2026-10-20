//! JSON-RPC 2.0 IPC layer for inter-primal communication.
//!
//! This is the **primary** IPC mechanism for ecoPrimals. All primals expose
//! JSON-RPC 2.0 endpoints following the semantic method naming standard
//! (`domain.verb` pattern).
//!
//! The binary RPC in [`crate::rpc`] is the optional high-throughput
//! binary path for performance-critical communication.

#![expect(
    clippy::module_name_repetitions,
    reason = "IPC types like JsonRpcRequest are clearer with full prefix"
)]

mod capability;
mod client;
mod error;
mod protocol;

pub use capability::Capability;
pub use client::IpcClient;
pub use error::{IpcError, IpcErrorKind};
pub use protocol::{
    CIRCUIT_BREAKER_OPEN, DEFAULT_IPC_TIMEOUT, DEPENDENCY_FAILURE, INTERNAL_ERROR, INVALID_PARAMS,
    INVALID_REQUEST, JSONRPC_VERSION, JsonRpcError, JsonRpcErrorExt, JsonRpcRequest,
    JsonRpcRequestExt, JsonRpcResponse, JsonRpcResponseExt, MESH_RELAY_TIMEOUT, METHOD_NOT_FOUND,
    NOT_READY, PARSE_ERROR, RATE_LIMITED, SERVICE_UNAVAILABLE,
};

pub use crate::health::HealthProbe;
pub use crate::methods;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::PrimalError;
    use crate::methods::{
        capabilities, capability, health, identity, ipc, lifecycle, primal, system,
    };

    #[test]
    fn jsonrpc_request_roundtrip() {
        let req = JsonRpcRequest::new("health.check", None, serde_json::json!(1))
            .with_params(serde_json::json!({"deep": true}));
        let json = serde_json::to_string(&req).expect("serialize");
        let back: JsonRpcRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.jsonrpc, JSONRPC_VERSION);
        assert_eq!(back.method, "health.check");
        assert_eq!(back.id, Some(serde_json::json!(1)));
        assert_eq!(back.params, Some(serde_json::json!({"deep": true})));
    }

    #[test]
    fn jsonrpc_notification_serializes_null_id() {
        let n = JsonRpcRequest::notification("system.ping", None);
        let v = serde_json::to_value(&n).expect("to_value");
        assert!(v.get("id").is_none() || v["id"].is_null());
    }

    #[test]
    fn jsonrpc_response_success_roundtrip() {
        let res =
            JsonRpcResponse::success(serde_json::json!("req-1"), serde_json::json!({"ok": true}));
        let json = serde_json::to_string(&res).expect("serialize");
        let back: JsonRpcResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.error, None);
        assert_eq!(back.result, Some(serde_json::json!({"ok": true})));
        assert_eq!(back.id, serde_json::json!("req-1"));
    }

    #[test]
    fn jsonrpc_response_error_roundtrip() {
        let err = JsonRpcError::method_not_found("foo.bar");
        let res = JsonRpcResponse::error_from(Some(serde_json::json!(42)), err);
        let json = serde_json::to_string(&res).expect("serialize");
        let back: JsonRpcResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(back.result.is_none());
        assert_eq!(back.id, serde_json::json!(42));
        let e = back.error.expect("error field");
        assert_eq!(e.code, METHOD_NOT_FOUND);
        assert!(e.message.contains("foo.bar"));
    }

    #[test]
    fn jsonrpc_error_standard_codes() {
        assert_eq!(JsonRpcError::parse_error("bad").code, PARSE_ERROR);
        assert_eq!(JsonRpcError::internal("x").code, INTERNAL_ERROR);
        assert_eq!(
            JsonRpcError::circuit_breaker_open("upstream").code,
            CIRCUIT_BREAKER_OPEN
        );
    }

    #[test]
    fn jsonrpc_error_retryable_classification() {
        assert!(JsonRpcError::new(SERVICE_UNAVAILABLE, "x").is_retryable());
        assert!(JsonRpcError::new(DEPENDENCY_FAILURE, "x").is_retryable());
        assert!(JsonRpcError::new(RATE_LIMITED, "x").is_retryable());
        assert!(JsonRpcError::new(NOT_READY, "x").is_retryable());
        assert!(!JsonRpcError::new(CIRCUIT_BREAKER_OPEN, "x").is_retryable());
        assert!(!JsonRpcError::new(METHOD_NOT_FOUND, "x").is_retryable());
    }

    #[test]
    fn primal_error_maps_to_jsonrpc() {
        let e: JsonRpcError = PrimalError::Network("down".into()).into();
        assert_eq!(e.code, SERVICE_UNAVAILABLE);
        let e: JsonRpcError = PrimalError::dependency("db", "no").into();
        assert_eq!(e.code, DEPENDENCY_FAILURE);
        let e: JsonRpcError = PrimalError::InvalidInput("bad".into()).into();
        assert_eq!(e.code, INVALID_PARAMS);
        let e: JsonRpcError = PrimalError::NotFound("x".into()).into();
        assert_eq!(e.code, METHOD_NOT_FOUND);
        let e: JsonRpcError = PrimalError::config("c").into();
        assert_eq!(e.code, INTERNAL_ERROR);
    }

    #[test]
    fn capability_builder() {
        let cap = Capability::new("health", "1.0.0")
            .with_method("check")
            .with_method("liveness");
        assert_eq!(cap.domain, "health");
        assert_eq!(cap.version, "1.0.0");
        assert_eq!(cap.methods, vec!["check", "liveness"]);
    }

    #[test]
    fn ipc_error_retryable_by_kind() {
        assert!(IpcError::new(IpcErrorKind::Transport, "t").retryable);
        assert!(IpcError::new(IpcErrorKind::Timeout, "t").retryable);
        assert!(IpcError::new(IpcErrorKind::NotReady, "n").retryable);
        assert!(!IpcError::new(IpcErrorKind::CircuitBreakerOpen, "c").retryable);
        assert!(!IpcError::new(IpcErrorKind::Internal, "i").retryable);
    }

    #[tokio::test]
    async fn call_with_timeout_returns_timeout_error() {
        let client = IpcClient::new(crate::transport::TransportEndpoint::uds(
            "/tmp/nonexistent-primal-for-timeout-test.sock",
        ));
        let req = JsonRpcRequest::new("health.liveness", None, 1);
        let result = client
            .call_with_timeout(&req, std::time::Duration::from_millis(50))
            .await;
        let err = result.unwrap_err();
        assert!(err.kind == IpcErrorKind::Transport || err.kind == IpcErrorKind::Timeout);
    }

    #[test]
    fn default_timeout_is_reasonable() {
        assert_eq!(DEFAULT_IPC_TIMEOUT, std::time::Duration::from_secs(5));
    }

    #[test]
    fn ipc_error_from_primal_sets_source() {
        let e = IpcError::new(IpcErrorKind::Internal, "msg").from_primal("p1");
        assert_eq!(e.source_primal.as_deref(), Some("p1"));
    }

    #[test]
    fn mesh_relay_timeout_is_longer_than_default() {
        assert!(MESH_RELAY_TIMEOUT > DEFAULT_IPC_TIMEOUT);
        assert_eq!(MESH_RELAY_TIMEOUT, std::time::Duration::from_secs(15));
    }

    #[test]
    fn method_name_constants() {
        assert_eq!(health::CHECK, "health.check");
        assert_eq!(health::LIVENESS, "health.liveness");
        assert_eq!(health::READINESS, "health.readiness");
        assert_eq!(lifecycle::STATE, "lifecycle.state");
        assert_eq!(lifecycle::RELOAD, "lifecycle.reload");
        assert_eq!(capabilities::LIST, "capabilities.list");
        assert_eq!(identity::DID, "identity.did");
        assert_eq!(system::PING, "system.ping");
        assert_eq!(system::VERSION, "system.version");
        assert_eq!(ipc::RESOLVE, "ipc.resolve");
        assert_eq!(ipc::REGISTER, "ipc.register");
        assert_eq!(primal::ANNOUNCE, "primal.announce");
        assert_eq!(primal::SHUTDOWN, "primal.shutdown");
        assert_eq!(capability::CALL, "capability.call");
    }

    #[test]
    fn ipc_client_from_primal_constructs_uds() {
        let client = IpcClient::from_primal("songbird", None);
        let ep = client.endpoint();
        assert!(matches!(
            ep,
            crate::transport::TransportEndpoint::Uds { .. }
        ));
        assert!(ep.uds_path().unwrap().contains("songbird"));
    }

    #[tokio::test]
    async fn ipc_client_call_roundtrip_over_uds() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");
        let listener = tokio::net::UnixListener::bind(&sock_path).unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut buf = BufReader::new(stream);
            let mut line = String::new();
            buf.read_line(&mut line).await.unwrap();

            let req: JsonRpcRequest = serde_json::from_str(line.trim()).unwrap();
            let resp = JsonRpcResponse::success(req.id.unwrap(), serde_json::json!({"pong": true}));
            let resp_json = serde_json::to_string(&resp).unwrap();
            buf.get_mut()
                .write_all(format!("{resp_json}\n").as_bytes())
                .await
                .unwrap();
        });

        let ep = crate::transport::TransportEndpoint::uds(sock_path.to_str().unwrap());
        let client = IpcClient::new(ep);
        let req = JsonRpcRequest::new("system.ping", None, 42);
        let resp = client.call(&req).await.unwrap();

        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["pong"], true);
        server.await.unwrap();
    }
}
