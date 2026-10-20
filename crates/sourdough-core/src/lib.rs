#![forbid(unsafe_code)]

//! # `SourDough` Core
//!
//! The essential traits and patterns that all ecoPrimals share.
//!
//! `SourDough` provides the minimal, agnostic foundation for building new primals.
//! It makes no assumptions about what your primal does—only that it needs to:
//!
//! - Have an identity (identity service via universal adapter)
//! - Be discoverable (discovery service via universal adapter)
//! - Have a lifecycle (start, stop, reload)
//! - Be observable (health checks)
//! - Be configurable
//!
//! ## Lifecycle Example
//!
//! ```
//! use sourdough_core::{PrimalLifecycle, PrimalState, PrimalError};
//!
//! struct MyPrimal { state: PrimalState }
//!
//! impl PrimalLifecycle for MyPrimal {
//!     fn state(&self) -> PrimalState { self.state }
//!
//!     async fn start(&mut self) -> Result<(), PrimalError> {
//!         self.state = PrimalState::Running;
//!         Ok(())
//!     }
//!
//!     async fn stop(&mut self) -> Result<(), PrimalError> {
//!         self.state = PrimalState::Stopped;
//!         Ok(())
//!     }
//! }
//! ```
//!
//! ## Transport Adoption (5-step pattern)
//!
//! Every primal adopts transport injection with this pattern:
//!
//! ```no_run
//! use sourdough_core::transport::TransportEndpoint;
//! use sourdough_core::ipc::{IpcClient, JsonRpcRequest, DEFAULT_IPC_TIMEOUT};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Step 1: Accept TRANSPORT_ENDPOINT from launcher
//! let endpoint: TransportEndpoint = std::env::var("TRANSPORT_ENDPOINT")
//!     .ok()
//!     .and_then(|s| serde_json::from_str(&s).ok())
//!     .unwrap_or_else(|| TransportEndpoint::uds("/tmp/biomeos/myprimal.sock"));
//!
//! // Step 2: Connect to other primals via IpcClient
//! let songbird = IpcClient::from_primal("songbird", None);
//!
//! // Step 3: Make calls with timeout protection
//! let req = JsonRpcRequest::new("health.liveness", None, 1);
//! let resp = songbird.call_with_timeout(&req, DEFAULT_IPC_TIMEOUT).await?;
//!
//! // Step 4: Use connect_transport for raw stream access
//! let stream = sourdough_core::transport::connect_transport(&endpoint).await?;
//!
//! // Step 5: Never self-bind — let the launcher handle the listener
//! # Ok(())
//! # }
//! ```

pub mod bind_mode;
pub mod circuit_breaker;
pub mod config;
pub mod discovery;
pub mod env_keys;
pub mod error;
pub mod health;
pub mod identity;
pub mod ipc;
pub mod lifecycle;
pub mod methods;
pub mod protocol_negotiation;
pub mod rpc;
pub mod tarpc_service;
pub mod platform_paths;
pub mod platform_signal;
pub mod platform_substrate;
pub mod transport;
pub mod types;

// Re-exports for convenience
pub use circuit_breaker::{CircuitBreaker, CircuitState};
pub use config::{ConfigLoader, PrimalConfig};
pub use discovery::{PrimalDiscovery, ServiceRegistration, UpaCapability};
pub use error::{PrimalError, PrimalResult};
pub use health::{DependencyHealth, HealthStatus, PrimalHealth};
pub use identity::{Did, PrimalIdentity, Signature};
pub use ipc::{
    Capability, DEFAULT_IPC_TIMEOUT, HealthProbe, IpcClient, IpcError, IpcErrorKind, JsonRpcError,
    JsonRpcErrorExt, JsonRpcRequest, JsonRpcRequestExt, JsonRpcResponse, JsonRpcResponseExt,
};
pub use lifecycle::{PrimalLifecycle, PrimalState};
pub use protocol_negotiation::{
    IpcProtocol, NegotiationError, NegotiationRequest, NegotiationResponse, negotiate_client,
    negotiate_server, select_protocol,
};
pub use rpc::{PrimalRpc, RpcRequest, RpcResponse};
pub use tarpc_service::{
    HealthResponse, IdentityResponse, PrimalService, PrimalServiceClient, TarpcCapability,
    default_tarpc_socket_path,
};
#[cfg(unix)]
pub use tarpc_service::{connect_primal, connect_primal_by_name};
pub use transport::{
    PeekedStream, Protocol, TransportEndpoint, TransportStream, connect_transport, peek_protocol,
    resolve_socket_path, socket_path_in,
};
pub use platform_paths::PrimalDirs;
pub use platform_signal::{on_shutdown, shutdown_signal, shutdown_signal_named};
pub use platform_substrate::{
    PlatformAccess, ensure_dir_with_access, ensure_secure_parent, is_symlink, platform_link,
    query_access,
};
pub use types::{epoch_secs_to_utc, ContentHash, Timestamp};
