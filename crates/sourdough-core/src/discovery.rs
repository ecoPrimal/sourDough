//! Discovery traits for universal adapter integration.
//!
//! Every primal needs to be discoverable. This module provides traits for
//! registering with discovery services via the universal adapter and broadcasting
//! presence to the network.
//!
//! ## Dual-Protocol Discovery (G64 Cephalization)
//!
//! Primals may advertise both JSON-RPC and tarpc endpoints. Callers choose
//! the protocol based on locality and performance needs:
//! - **JSON-RPC** (`endpoint`): universal, browser-compatible, cross-gate
//! - **tarpc** (`tarpc_endpoint`): binary framing, intra-gate, sub-ms

use crate::error::PrimalError;
use serde::{Deserialize, Serialize};

/// Which protocols a primal supports for IPC.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolSupport {
    /// JSON-RPC only (legacy or browser-facing primals).
    JsonRpcOnly,
    /// tarpc only (internal-only high-throughput primals).
    TarpcOnly,
    /// Both JSON-RPC and tarpc (cephalization-era default).
    #[default]
    DualProtocol,
}

impl std::fmt::Display for ProtocolSupport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JsonRpcOnly => f.write_str("jsonrpc"),
            Self::TarpcOnly => f.write_str("tarpc"),
            Self::DualProtocol => f.write_str("dual"),
        }
    }
}

impl std::str::FromStr for ProtocolSupport {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "jsonrpc" | "json-rpc" | "json_rpc" => Ok(Self::JsonRpcOnly),
            "tarpc" | "binary" => Ok(Self::TarpcOnly),
            "dual" | "dual-protocol" | "dual_protocol" => Ok(Self::DualProtocol),
            other => Err(format!("unknown protocol: {other}")),
        }
    }
}

/// Service registration for discovery services.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceRegistration {
    /// Service name (e.g., "my-primal", "another-primal").
    pub name: String,
    /// Service version (semver).
    pub version: String,
    /// JSON-RPC endpoint (bootstrap, discovery, diagnostics).
    pub endpoint: String,
    /// tarpc binary endpoint (intra-gate composition, sub-ms).
    /// `None` for primals that only expose JSON-RPC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tarpc_endpoint: Option<String>,
    /// Which protocols this primal supports.
    #[serde(default)]
    pub protocol_support: ProtocolSupport,
    /// Service capabilities.
    pub capabilities: Vec<UpaCapability>,
    /// Service metadata.
    pub metadata: std::collections::HashMap<String, String>,
    /// Health check endpoint (optional).
    pub health_endpoint: Option<String>,
}

impl ServiceRegistration {
    /// Create a new service registration (JSON-RPC endpoint).
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            endpoint: endpoint.into(),
            tarpc_endpoint: None,
            protocol_support: ProtocolSupport::JsonRpcOnly,
            capabilities: Vec::new(),
            metadata: std::collections::HashMap::new(),
            health_endpoint: None,
        }
    }

    /// Set the tarpc binary endpoint (enables dual-protocol).
    #[must_use]
    pub fn with_tarpc_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.tarpc_endpoint = Some(endpoint.into());
        self.protocol_support = ProtocolSupport::DualProtocol;
        self
    }

    /// Add a capability.
    #[must_use]
    pub fn with_capability(mut self, cap: UpaCapability) -> Self {
        self.capabilities.push(cap);
        self
    }

    /// Add metadata.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Set health endpoint.
    #[must_use]
    pub fn with_health_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.health_endpoint = Some(endpoint.into());
        self
    }

    /// Whether this registration includes a tarpc endpoint.
    #[must_use]
    pub const fn has_tarpc(&self) -> bool {
        self.tarpc_endpoint.is_some()
    }
}

/// UPA capability declaration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpaCapability {
    /// Capability name (e.g., "storage", "compute", "security").
    pub name: String,
    /// Capability version.
    pub version: String,
    /// Protocol (e.g., "grpc", "rest", "websocket").
    pub protocol: String,
    /// Additional capability metadata.
    pub metadata: std::collections::HashMap<String, String>,
}

impl UpaCapability {
    /// Create a new capability.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        protocol: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            protocol: protocol.into(),
            metadata: std::collections::HashMap::new(),
        }
    }
}

/// `BirdSong` broadcast configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BirdSongConfig {
    /// Whether to broadcast presence.
    pub enabled: bool,
    /// Broadcast interval.
    pub interval_secs: u64,
    /// Lineage gating (only visible to family).
    pub lineage_gated: bool,
    /// Encryption enabled.
    pub encrypted: bool,
}

impl Default for BirdSongConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: 30,
            lineage_gated: true,
            encrypted: true,
        }
    }
}

/// Discovery trait for primals.
///
/// Implement this trait to integrate with discovery services via the universal adapter.
pub trait PrimalDiscovery: Send + Sync {
    /// Get the service registration for UPA.
    fn registration(&self) -> ServiceRegistration;

    /// Register with UPA.
    ///
    /// # Errors
    ///
    /// Returns an error if registration fails.
    fn register(
        &self,
    ) -> impl std::future::Future<Output = Result<RegistrationHandle, PrimalError>> + Send;

    /// Deregister from UPA.
    ///
    /// # Errors
    ///
    /// Returns an error if deregistration fails.
    fn deregister(&self) -> impl std::future::Future<Output = Result<(), PrimalError>> + Send;

    /// Get `BirdSong` configuration (optional).
    ///
    /// Returns `None` if `BirdSong` is not used.
    fn birdsong_config(&self) -> Option<BirdSongConfig> {
        None
    }

    /// Discover a service by name.
    ///
    /// # Errors
    ///
    /// Returns an error if discovery fails.
    fn discover(
        &self,
        service_name: &str,
    ) -> impl std::future::Future<Output = Result<Vec<ServiceInfo>, PrimalError>> + Send;

    /// Discover a service by capability.
    ///
    /// # Errors
    ///
    /// Returns an error if discovery fails.
    fn discover_by_capability(
        &self,
        capability: &str,
    ) -> impl std::future::Future<Output = Result<Vec<ServiceInfo>, PrimalError>> + Send;
}

/// Handle to a service registration.
#[derive(Clone, Debug)]
pub struct RegistrationHandle {
    /// Registration ID.
    pub id: String,
    /// Service name.
    pub service_name: String,
    /// Registration timestamp.
    pub registered_at: crate::types::Timestamp,
}

/// Information about a discovered service — canonical form from `cellmembrane-types`.
///
/// Organism-specific fields (did, protocol_support, is_family, endpoint,
/// tarpc_endpoint) are mapped via the `ServiceInfoExt` extension trait
/// and canonical `endpoints` / `metadata`.
pub use cellmembrane_types::ServiceInfo;

/// Extension methods for sourDough-specific discovery patterns.
pub trait ServiceInfoExt {
    /// Get the best endpoint for high-performance intra-gate calls.
    /// Returns the tarpc endpoint if available, falls back to the first endpoint.
    fn best_endpoint(&self) -> Option<&str>;
    /// Whether this service supports tarpc binary protocol.
    fn supports_tarpc(&self) -> bool;
    /// Get the DID from metadata.
    fn did(&self) -> Option<&str>;
    /// Whether this service is in our lineage.
    fn is_family(&self) -> bool;
}

impl ServiceInfoExt for ServiceInfo {
    fn best_endpoint(&self) -> Option<&str> {
        self.endpoints
            .iter()
            .find(|e| e.starts_with("tarpc:"))
            .or_else(|| self.endpoints.first())
            .map(String::as_str)
    }

    fn supports_tarpc(&self) -> bool {
        self.endpoints.iter().any(|e| e.starts_with("tarpc:"))
    }

    fn did(&self) -> Option<&str> {
        self.metadata.get("did").map(String::as_str)
    }

    fn is_family(&self) -> bool {
        self.metadata.get("is_family").is_some_and(|v| v == "true")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_registration_builder() {
        // In real usage, endpoint would be discovered dynamically
        // This test uses a placeholder for demonstration only
        let reg = ServiceRegistration::new("test-service", "1.0.0", "http://test-endpoint:0")
            .with_capability(UpaCapability::new("storage", "1.0", "grpc"))
            .with_metadata("region", "us-west")
            .with_health_endpoint("/health");

        assert_eq!(reg.name, "test-service");
        assert_eq!(reg.version, "1.0.0");
        assert_eq!(reg.endpoint, "http://test-endpoint:0");
        assert_eq!(reg.capabilities.len(), 1);
        assert_eq!(reg.metadata.get("region"), Some(&"us-west".to_string()));
        assert_eq!(reg.health_endpoint, Some("/health".to_string()));
    }

    #[test]
    fn upa_capability_creation() {
        let cap = UpaCapability::new("compute", "2.0", "rest");

        assert_eq!(cap.name, "compute");
        assert_eq!(cap.version, "2.0");
        assert_eq!(cap.protocol, "rest");
        assert!(cap.metadata.is_empty());
    }

    #[test]
    fn birdsong_config_default() {
        let config = BirdSongConfig::default();

        assert!(config.enabled);
        assert_eq!(config.interval_secs, 30);
        assert!(config.lineage_gated);
        assert!(config.encrypted);
    }

    #[test]
    fn birdsong_config_custom() {
        let config = BirdSongConfig {
            enabled: false,
            interval_secs: 60,
            lineage_gated: false,
            encrypted: false,
        };

        assert!(!config.enabled);
        assert_eq!(config.interval_secs, 60);
    }

    #[test]
    fn service_info_serialization() {
        let mut info = ServiceInfo::new("test-1", "test")
            .with_version("1.0.0")
            .with_endpoint("http://test")
            .with_capability("storage");
        info.metadata.insert("did".into(), "did:key:test123".into());
        info.metadata.insert("is_family".into(), "true".into());
        info.metadata.insert("protocol_support".into(), "jsonrpc".into());

        let json = serde_json::to_string(&info).unwrap();
        let parsed: ServiceInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(info.name, parsed.name);
        assert!(parsed.is_family());
        assert!(!parsed.supports_tarpc());
        assert_eq!(parsed.best_endpoint(), Some("http://test"));
    }

    #[test]
    fn service_info_dual_protocol() {
        let mut info = ServiceInfo::new("fast-1", "fast-primal")
            .with_version("2.0.0")
            .with_endpoint("unix:///run/user/1000/biomeos/fast-primal.sock")
            .with_endpoint("tarpc:unix:///run/user/1000/biomeos/fast-primal.tarpc.sock")
            .with_capability("compute");
        info.metadata.insert("did".into(), "did:key:fast123".into());
        info.metadata.insert("is_family".into(), "true".into());
        info.metadata.insert("protocol_support".into(), "dual".into());

        assert!(info.supports_tarpc());
        assert_eq!(
            info.best_endpoint(),
            Some("tarpc:unix:///run/user/1000/biomeos/fast-primal.tarpc.sock")
        );

        let json = serde_json::to_string(&info).unwrap();
        let parsed: ServiceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.metadata.get("protocol_support").map(String::as_str), Some("dual"));
        assert!(parsed.supports_tarpc());
    }

    #[test]
    fn service_registration_tarpc_builder() {
        let reg = ServiceRegistration::new("myprimal", "1.0.0", "unix:///tmp/myprimal.sock")
            .with_tarpc_endpoint("unix:///tmp/myprimal.tarpc.sock")
            .with_capability(UpaCapability::new("health", "1.0", "jsonrpc"));

        assert!(reg.has_tarpc());
        assert_eq!(reg.protocol_support, ProtocolSupport::DualProtocol);
        assert_eq!(
            reg.tarpc_endpoint.as_deref(),
            Some("unix:///tmp/myprimal.tarpc.sock")
        );
    }

    #[test]
    fn protocol_support_default_is_dual() {
        assert_eq!(ProtocolSupport::default(), ProtocolSupport::DualProtocol);
    }

    #[test]
    fn protocol_support_display() {
        assert_eq!(ProtocolSupport::JsonRpcOnly.to_string(), "jsonrpc");
        assert_eq!(ProtocolSupport::TarpcOnly.to_string(), "tarpc");
        assert_eq!(ProtocolSupport::DualProtocol.to_string(), "dual");
    }

    #[test]
    fn protocol_support_from_str_roundtrip() {
        for variant in [
            ProtocolSupport::JsonRpcOnly,
            ProtocolSupport::TarpcOnly,
            ProtocolSupport::DualProtocol,
        ] {
            let s = variant.to_string();
            let parsed: ProtocolSupport = s.parse().unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn protocol_support_from_str_aliases() {
        assert_eq!(
            "json-rpc".parse::<ProtocolSupport>().unwrap(),
            ProtocolSupport::JsonRpcOnly
        );
        assert_eq!(
            "binary".parse::<ProtocolSupport>().unwrap(),
            ProtocolSupport::TarpcOnly
        );
        assert_eq!(
            "dual-protocol".parse::<ProtocolSupport>().unwrap(),
            ProtocolSupport::DualProtocol
        );
    }

    #[test]
    fn protocol_support_from_str_error() {
        assert!("unknown".parse::<ProtocolSupport>().is_err());
    }

    #[test]
    fn service_info_canonical_defaults() {
        let info = ServiceInfo::new("old-1", "old")
            .with_version("0.1.0")
            .with_endpoint("unix:///tmp/old.sock")
            .with_capability("health");
        assert!(!info.supports_tarpc());
        assert_eq!(info.best_endpoint(), Some("unix:///tmp/old.sock"));
    }

    #[test]
    fn registration_handle_creation() {
        let handle = RegistrationHandle {
            id: "reg-123".to_string(),
            service_name: "test-service".to_string(),
            registered_at: crate::types::Timestamp::now(),
        };

        assert_eq!(handle.id, "reg-123");
        assert_eq!(handle.service_name, "test-service");
    }

    // Mock implementation for testing
    struct MockDiscoveryPrimal {
        service_name: String,
    }

    impl MockDiscoveryPrimal {
        fn new(name: impl Into<String>) -> Self {
            Self {
                service_name: name.into(),
            }
        }
    }

    impl PrimalDiscovery for MockDiscoveryPrimal {
        fn registration(&self) -> ServiceRegistration {
            // In tests, use OS-assigned port (0) to avoid hardcoding
            ServiceRegistration::new(&self.service_name, "1.0.0", "http://test-endpoint:0")
                .with_capability(UpaCapability::new("test", "1.0", "grpc"))
        }

        async fn register(&self) -> Result<RegistrationHandle, PrimalError> {
            Ok(RegistrationHandle {
                id: format!("reg-{}", self.service_name),
                service_name: self.service_name.clone(),
                registered_at: crate::types::Timestamp::now(),
            })
        }

        async fn deregister(&self) -> Result<(), PrimalError> {
            Ok(())
        }

        fn birdsong_config(&self) -> Option<BirdSongConfig> {
            Some(BirdSongConfig::default())
        }

        async fn discover(&self, _service_name: &str) -> Result<Vec<ServiceInfo>, PrimalError> {
            Ok(vec![])
        }

        async fn discover_by_capability(
            &self,
            _capability: &str,
        ) -> Result<Vec<ServiceInfo>, PrimalError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn trait_registration() {
        let primal = MockDiscoveryPrimal::new("test-primal");

        let reg = primal.registration();
        assert_eq!(reg.name, "test-primal");
        assert_eq!(reg.capabilities.len(), 1);

        let handle = primal.register().await.unwrap();
        assert_eq!(handle.service_name, "test-primal");

        primal.deregister().await.unwrap();
    }

    #[tokio::test]
    async fn trait_birdsong_config() {
        let primal = MockDiscoveryPrimal::new("test");

        let config = primal.birdsong_config();
        assert!(config.is_some());

        let config = config.unwrap();
        assert!(config.enabled);
        assert!(config.encrypted);
    }

    #[tokio::test]
    async fn trait_discovery() {
        let primal = MockDiscoveryPrimal::new("test");

        let services = primal.discover("other-service").await.unwrap();
        assert!(services.is_empty());

        let services = primal.discover_by_capability("storage").await.unwrap();
        assert!(services.is_empty());
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_service_registration() -> impl Strategy<Value = ServiceRegistration> {
            (
                "[a-z][a-z0-9-]{2,20}",
                "[0-9]+\\.[0-9]+\\.[0-9]+",
                "unix:///tmp/[a-z]+\\.sock",
            )
                .prop_map(|(name, version, endpoint)| {
                    ServiceRegistration::new(name, version, endpoint)
                })
        }

        fn arb_service_info() -> impl Strategy<Value = ServiceInfo> {
            (
                "[a-z][a-z0-9-]{2,20}",
                "[0-9]+\\.[0-9]+\\.[0-9]+",
                "unix:///tmp/[a-z]+\\.sock",
                proptest::option::of("unix:///tmp/[a-z]+\\.tarpc\\.sock"),
                prop::collection::vec("[a-z]+", 0..5),
            )
                .prop_map(|(name, version, endpoint, tarpc_ep, caps)| {
                    let mut info = ServiceInfo::new(&name, &name)
                        .with_version(&version)
                        .with_endpoint(&endpoint);
                    if let Some(tarpc) = &tarpc_ep {
                        info = info.with_endpoint(format!("tarpc:{tarpc}"));
                    }
                    for cap in &caps {
                        info = info.with_capability(cap);
                    }
                    info
                })
        }

        proptest! {
            #[test]
            fn service_registration_json_roundtrip(reg in arb_service_registration()) {
                let json = serde_json::to_string(&reg).unwrap();
                let back: ServiceRegistration = serde_json::from_str(&json).unwrap();
                prop_assert_eq!(&reg.name, &back.name);
                prop_assert_eq!(&reg.version, &back.version);
                prop_assert_eq!(&reg.endpoint, &back.endpoint);
            }

            #[test]
            fn service_info_json_roundtrip(info in arb_service_info()) {
                let json = serde_json::to_string(&info).unwrap();
                let back: ServiceInfo = serde_json::from_str(&json).unwrap();
                prop_assert_eq!(&info.name, &back.name);
                prop_assert_eq!(&info.endpoints, &back.endpoints);
                prop_assert_eq!(&info.capabilities, &back.capabilities);
            }

            #[test]
            fn service_info_best_endpoint_prefers_tarpc(info in arb_service_info()) {
                let best = info.best_endpoint();
                let has_tarpc = info.endpoints.iter().any(|e| e.starts_with("tarpc:"));
                if has_tarpc {
                    prop_assert!(best.unwrap().starts_with("tarpc:"));
                } else {
                    prop_assert_eq!(best, info.endpoints.first().map(|s| s.as_str()));
                }
            }

            #[test]
            fn service_registration_tarpc_builder_sets_dual(
                name in "[a-z]{3,10}",
                version in "[0-9]+\\.[0-9]+\\.[0-9]+",
                endpoint in "unix:///tmp/[a-z]+\\.sock",
                tarpc_ep in "unix:///tmp/[a-z]+\\.tarpc\\.sock",
            ) {
                let reg = ServiceRegistration::new(name, version, endpoint)
                    .with_tarpc_endpoint(&tarpc_ep);
                prop_assert!(reg.has_tarpc());
                prop_assert_eq!(reg.protocol_support, ProtocolSupport::DualProtocol);
                prop_assert_eq!(reg.tarpc_endpoint.as_deref(), Some(tarpc_ep.as_str()));
            }
        }
    }
}
