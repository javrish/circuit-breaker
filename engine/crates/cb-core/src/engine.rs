//! # Engine Mode Configuration
//!
//! This module defines the engine execution modes for Circuit Breaker's hybrid
//! cloud architecture. It enables workflows to run locally (using a local Dagger
//! installation), in the cloud (using the Engine Service), or automatically
//! selecting the best option based on requirements.
//!
//! ## Design Philosophy
//!
//! The hybrid execution model allows Circuit Breaker to be used in multiple scenarios:
//!
//! | Scenario | Engine Mode | Rationale |
//! |----------|-------------|-----------|
//! | Local development | `Local` | Fast iteration, no network dependency |
//! | CI/CD with self-hosted runner | `Local` | Runner has Dagger installed |
//! | Production workflows | `Cloud` | Managed scaling, billing, security |
//! | Resource-intensive jobs | `Cloud` | GPU, high memory, specialized hardware |
//! | Mixed workflows | `Auto` | Simple steps local, heavy steps cloud |
//!
//! ## Usage
//!
//! Engine mode can be configured at three levels, with lower levels overriding higher:
//!
//! 1. **Runner configuration** - Default for all workflows on this runner
//! 2. **Workflow configuration** - Default for all transitions in workflow
//! 3. **Transition configuration** - Specific to a single transition
//!
//! ```rust
//! use cb_core::engine::{EngineMode, EngineRequirements, EngineConfig};
//!
//! // Simple local-only configuration
//! let config = EngineConfig::local();
//!
//! // Cloud configuration with specific requirements
//! let config = EngineConfig::cloud()
//!     .with_requirements(EngineRequirements {
//!         gpu: true,
//!         memory_gb: Some(32),
//!         ..Default::default()
//!     });
//!
//! // Auto mode (default) - prefers local, falls back to cloud
//! let config = EngineConfig::auto();
//! ```
//!
//! ## Auto Mode Selection Algorithm
//!
//! When `EngineMode::Auto` is selected, the system uses the following logic:
//!
//! 1. **Force cloud** if any of these are true:
//!    - GPU is required (`requirements.gpu == true`)
//!    - Audit trail is required (`requirements.require_audit == true`)
//!    - Memory exceeds threshold (default: 16GB)
//!    - CPU exceeds threshold (default: 8 cores)
//!
//! 2. **Try local** if:
//!    - Dagger is installed and detected
//!    - Container runtime (Docker/Podman) is running
//!
//! 3. **Fallback to cloud** if:
//!    - Local is unavailable and cloud is configured
//!    - `auto_config.fallback_to_cloud == true`
//!
//! ## Container Runtime Support
//!
//! The local engine supports multiple container runtimes:
//!
//! - **Docker** - Most common, widely supported
//! - **Podman** - Rootless alternative, common on RHEL/Fedora
//! - **Colima** - Docker Desktop alternative for macOS
//! - **Rancher Desktop** - Kubernetes-focused alternative
//! - **OrbStack** - Fast alternative for macOS
//!
//! Runtime detection is automatic but can be overridden via configuration.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// How to provision Dagger engines for execution.
///
/// This enum determines whether workflow transitions execute using a local
/// Dagger installation or a remote engine provisioned by the Engine Service.
///
/// # Examples
///
/// ```rust
/// use cb_core::engine::EngineMode;
///
/// // Parse from configuration
/// let mode: EngineMode = serde_json::from_str(r#""local""#).unwrap();
/// assert_eq!(mode, EngineMode::Local);
///
/// // Default is Auto
/// let mode = EngineMode::default();
/// assert_eq!(mode, EngineMode::Auto);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EngineMode {
    /// Use local Dagger installation (Docker/Podman).
    ///
    /// This is the fastest option for development as it:
    /// - Requires no network access to the Engine Service
    /// - Uses local container runtime for caching
    /// - Provides immediate feedback
    ///
    /// Requirements:
    /// - Dagger CLI installed (`dagger` in PATH or configured path)
    /// - Container runtime running (Docker, Podman, etc.)
    Local,

    /// Use Circuit Breaker Engine Service for managed execution.
    ///
    /// This is required for:
    /// - GPU workloads
    /// - High memory requirements (>16GB typical)
    /// - Production workflows requiring audit trails
    /// - Multi-tenant isolation
    /// - Usage tracking and billing
    ///
    /// The runner connects to the Engine Service, which provisions
    /// a Dagger engine pod and returns mTLS credentials for connection.
    Cloud,

    /// Automatically select based on requirements and availability.
    ///
    /// This is the default mode. It provides the best of both worlds:
    /// - Uses local execution when possible (faster, simpler)
    /// - Falls back to cloud for resource-intensive workloads
    /// - Handles unavailability gracefully
    ///
    /// See the module documentation for the selection algorithm.
    #[default]
    Auto,
}



impl fmt::Display for EngineMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local => write!(f, "local"),
            Self::Cloud => write!(f, "cloud"),
            Self::Auto => write!(f, "auto"),
        }
    }
}

impl EngineMode {
    /// Returns `true` if this mode allows local execution.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::EngineMode;
    ///
    /// assert!(EngineMode::Local.allows_local());
    /// assert!(EngineMode::Auto.allows_local());
    /// assert!(!EngineMode::Cloud.allows_local());
    /// ```
    #[inline]
    pub const fn allows_local(&self) -> bool {
        matches!(self, Self::Local | Self::Auto)
    }

    /// Returns `true` if this mode allows cloud execution.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::EngineMode;
    ///
    /// assert!(EngineMode::Cloud.allows_cloud());
    /// assert!(EngineMode::Auto.allows_cloud());
    /// assert!(!EngineMode::Local.allows_cloud());
    /// ```
    #[inline]
    pub const fn allows_cloud(&self) -> bool {
        matches!(self, Self::Cloud | Self::Auto)
    }

    /// Returns `true` if this mode requires automatic selection logic.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::EngineMode;
    ///
    /// assert!(EngineMode::Auto.is_auto());
    /// assert!(!EngineMode::Local.is_auto());
    /// assert!(!EngineMode::Cloud.is_auto());
    /// ```
    #[inline]
    pub const fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }
}

/// Container runtime used for local Dagger execution.
///
/// Dagger requires a container runtime to execute pipelines. This enum
/// represents the supported runtimes that can be auto-detected or explicitly
/// configured.
///
/// # Examples
///
/// ```rust
/// use cb_core::engine::ContainerRuntime;
///
/// // Auto-detect (default)
/// let runtime = ContainerRuntime::default();
/// assert_eq!(runtime, ContainerRuntime::Auto);
///
/// // Explicit Docker
/// let runtime = ContainerRuntime::Docker;
/// assert_eq!(runtime.socket_path(), "/var/run/docker.sock");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContainerRuntime {
    /// Automatically detect the container runtime.
    ///
    /// Detection order:
    /// 1. Docker (most common)
    /// 2. Podman
    /// 3. Colima (macOS)
    /// 4. Rancher Desktop
    /// 5. OrbStack (macOS)
    #[default]
    Auto,

    /// Docker Engine (docker.io).
    ///
    /// The most widely used container runtime. Works on Linux, macOS
    /// (via Docker Desktop or alternatives), and Windows (via WSL2).
    Docker,

    /// Podman (podman.io).
    ///
    /// A daemonless, rootless container runtime. Common on RHEL, Fedora,
    /// and CentOS systems. Provides Docker-compatible CLI.
    Podman,

    /// Colima (github.com/abiosoft/colima).
    ///
    /// A lightweight container runtime for macOS that uses Lima VMs.
    /// Popular alternative to Docker Desktop.
    Colima,

    /// Rancher Desktop (rancherdesktop.io).
    ///
    /// A Kubernetes-focused container runtime that can run Docker
    /// or containerd. Works on macOS, Windows, and Linux.
    RancherDesktop,

    /// OrbStack (orbstack.dev).
    ///
    /// A fast, lightweight Docker Desktop alternative for macOS.
    /// Known for low resource usage and fast startup.
    OrbStack,
}



impl fmt::Display for ContainerRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Docker => write!(f, "docker"),
            Self::Podman => write!(f, "podman"),
            Self::Colima => write!(f, "colima"),
            Self::RancherDesktop => write!(f, "rancher-desktop"),
            Self::OrbStack => write!(f, "orbstack"),
        }
    }
}

impl ContainerRuntime {
    /// Returns the default socket path for this runtime.
    ///
    /// Note: This returns the standard path; actual paths may vary based on
    /// installation and configuration. Use `detect_socket_path()` for runtime
    /// detection.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::ContainerRuntime;
    ///
    /// assert_eq!(ContainerRuntime::Docker.socket_path(), "/var/run/docker.sock");
    /// assert_eq!(ContainerRuntime::Podman.socket_path(), "/run/user/1000/podman/podman.sock");
    /// ```
    pub const fn socket_path(&self) -> &'static str {
        match self {
            Self::Auto => "/var/run/docker.sock", // Default to Docker
            Self::Docker => "/var/run/docker.sock",
            Self::Podman => "/run/user/1000/podman/podman.sock", // Rootless default
            Self::Colima => "~/.colima/default/docker.sock",
            Self::RancherDesktop => "~/.rd/docker.sock",
            Self::OrbStack => "~/.orbstack/run/docker.sock",
        }
    }

    /// Returns the CLI command name for this runtime.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::ContainerRuntime;
    ///
    /// assert_eq!(ContainerRuntime::Docker.cli_command(), "docker");
    /// assert_eq!(ContainerRuntime::Podman.cli_command(), "podman");
    /// ```
    pub const fn cli_command(&self) -> &'static str {
        match self {
            Self::Auto => "docker", // Default to Docker
            Self::Docker => "docker",
            Self::Podman => "podman",
            Self::Colima => "colima",
            Self::RancherDesktop => "docker", // Uses Docker CLI
            Self::OrbStack => "docker",       // Uses Docker CLI
        }
    }

    /// Returns `true` if this runtime typically runs rootless.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::ContainerRuntime;
    ///
    /// assert!(ContainerRuntime::Podman.is_typically_rootless());
    /// assert!(!ContainerRuntime::Docker.is_typically_rootless());
    /// ```
    pub const fn is_typically_rootless(&self) -> bool {
        matches!(self, Self::Podman)
    }
}

/// Resource requirements that influence engine mode selection in Auto mode.
///
/// These requirements are used to determine whether a local engine is sufficient
/// or if the workload should be routed to the cloud for additional resources.
///
/// # Examples
///
/// ```rust
/// use cb_core::engine::EngineRequirements;
///
/// // Simple requirements (will likely use local)
/// let req = EngineRequirements::default();
/// assert!(!req.requires_cloud());
///
/// // GPU requirement (forces cloud)
/// let req = EngineRequirements {
///     gpu: true,
///     ..Default::default()
/// };
/// assert!(req.requires_cloud());
///
/// // High memory requirement (suggests cloud)
/// let req = EngineRequirements {
///     memory_gb: Some(32),
///     ..Default::default()
/// };
/// assert!(req.suggests_cloud(16, 8)); // threshold: 16GB, 8 cores
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EngineRequirements {
    /// Require GPU access.
    ///
    /// If `true`, the workload will always be routed to the cloud since
    /// GPU access requires specialized engine pods with GPU node affinity.
    #[serde(default)]
    pub gpu: bool,

    /// Minimum memory in gigabytes.
    ///
    /// If this exceeds the configured cloud threshold (default: 16GB),
    /// the workload will be routed to the cloud.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_gb: Option<u32>,

    /// Minimum CPU cores.
    ///
    /// If this exceeds the configured cloud threshold (default: 8 cores),
    /// the workload will be routed to the cloud.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_cores: Option<u32>,

    /// Required Dagger engine version.
    ///
    /// Specifies a minimum Dagger engine version. In local mode, the local
    /// Dagger version must meet this requirement. In cloud mode, the Engine
    /// Service will provision an engine with at least this version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,

    /// Require audit trail for compliance.
    ///
    /// If `true`, the workload will always be routed to the cloud where
    /// execution is logged, metered, and auditable.
    #[serde(default)]
    pub require_audit: bool,

    /// Custom labels for engine selection.
    ///
    /// Used for advanced routing scenarios, such as selecting engines
    /// in specific regions or with specific capabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<std::collections::HashMap<String, String>>,
}

impl EngineRequirements {
    /// Creates a new empty requirements specification.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::EngineRequirements;
    ///
    /// let req = EngineRequirements::new();
    /// assert!(!req.gpu);
    /// assert!(req.memory_gb.is_none());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates requirements that explicitly request GPU access.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::EngineRequirements;
    ///
    /// let req = EngineRequirements::with_gpu();
    /// assert!(req.gpu);
    /// assert!(req.requires_cloud());
    /// ```
    pub fn with_gpu() -> Self {
        Self {
            gpu: true,
            ..Default::default()
        }
    }

    /// Creates requirements with specified memory.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::EngineRequirements;
    ///
    /// let req = EngineRequirements::with_memory(32);
    /// assert_eq!(req.memory_gb, Some(32));
    /// ```
    pub fn with_memory(memory_gb: u32) -> Self {
        Self {
            memory_gb: Some(memory_gb),
            ..Default::default()
        }
    }

    /// Returns `true` if these requirements absolutely require cloud execution.
    ///
    /// Currently, this is only the case when:
    /// - GPU is required
    /// - Audit trail is required
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::EngineRequirements;
    ///
    /// let req = EngineRequirements::default();
    /// assert!(!req.requires_cloud());
    ///
    /// let req = EngineRequirements { gpu: true, ..Default::default() };
    /// assert!(req.requires_cloud());
    ///
    /// let req = EngineRequirements { require_audit: true, ..Default::default() };
    /// assert!(req.requires_cloud());
    /// ```
    pub const fn requires_cloud(&self) -> bool {
        self.gpu || self.require_audit
    }

    /// Returns `true` if these requirements suggest cloud execution based on thresholds.
    ///
    /// This is a softer check than `requires_cloud()` - it indicates that cloud
    /// execution would be beneficial but local execution is still possible.
    ///
    /// # Arguments
    ///
    /// * `memory_threshold_gb` - Memory threshold above which cloud is suggested
    /// * `cpu_threshold_cores` - CPU threshold above which cloud is suggested
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::EngineRequirements;
    ///
    /// let req = EngineRequirements { memory_gb: Some(32), ..Default::default() };
    /// assert!(req.suggests_cloud(16, 8));  // 32 > 16
    /// assert!(!req.suggests_cloud(64, 8)); // 32 < 64
    ///
    /// let req = EngineRequirements { cpu_cores: Some(16), ..Default::default() };
    /// assert!(req.suggests_cloud(16, 8));  // 16 > 8
    /// assert!(!req.suggests_cloud(16, 32)); // 16 < 32
    /// ```
    pub fn suggests_cloud(&self, memory_threshold_gb: u32, cpu_threshold_cores: u32) -> bool {
        if self.requires_cloud() {
            return true;
        }

        if let Some(memory) = self.memory_gb {
            if memory > memory_threshold_gb {
                return true;
            }
        }

        if let Some(cpu) = self.cpu_cores {
            if cpu > cpu_threshold_cores {
                return true;
            }
        }

        false
    }

    /// Merges another requirements specification into this one.
    ///
    /// The merge takes the maximum/union of all values:
    /// - Boolean flags are OR'd
    /// - Numeric values take the maximum
    /// - Optional values prefer Some over None
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::EngineRequirements;
    ///
    /// let mut base = EngineRequirements {
    ///     memory_gb: Some(8),
    ///     cpu_cores: Some(4),
    ///     ..Default::default()
    /// };
    ///
    /// let override_req = EngineRequirements {
    ///     gpu: true,
    ///     memory_gb: Some(16),
    ///     ..Default::default()
    /// };
    ///
    /// base.merge(&override_req);
    ///
    /// assert!(base.gpu);              // From override
    /// assert_eq!(base.memory_gb, Some(16)); // Max of 8 and 16
    /// assert_eq!(base.cpu_cores, Some(4));  // From base (override was None)
    /// ```
    pub fn merge(&mut self, other: &Self) {
        self.gpu = self.gpu || other.gpu;
        self.require_audit = self.require_audit || other.require_audit;

        self.memory_gb = match (self.memory_gb, other.memory_gb) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        self.cpu_cores = match (self.cpu_cores, other.cpu_cores) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        // Prefer the more specific version
        if other.engine_version.is_some() {
            self.engine_version = other.engine_version.clone();
        }

        // Merge labels
        if let Some(other_labels) = &other.labels {
            let labels = self.labels.get_or_insert_with(Default::default);
            for (k, v) in other_labels {
                labels.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Configuration for local engine execution.
///
/// Specifies how to connect to a local Dagger installation, including
/// the path to the Dagger binary and the container runtime to use.
///
/// # Examples
///
/// ```rust
/// use cb_core::engine::{LocalEngineConfig, ContainerRuntime};
/// use std::path::PathBuf;
///
/// // Auto-detect everything
/// let config = LocalEngineConfig::default();
///
/// // Explicit configuration
/// let config = LocalEngineConfig {
///     dagger_path: Some(PathBuf::from("/usr/local/bin/dagger")),
///     runtime: ContainerRuntime::Podman,
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LocalEngineConfig {
    /// Path to the Dagger binary.
    ///
    /// If `None`, the system will search for `dagger` in PATH.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dagger_path: Option<PathBuf>,

    /// Container runtime to use.
    ///
    /// If `Auto`, the system will detect the available runtime.
    #[serde(default)]
    pub runtime: ContainerRuntime,

    /// Custom Docker host (e.g., `unix:///var/run/docker.sock`).
    ///
    /// Overrides the default socket path for the selected runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker_host: Option<String>,

    /// Connection timeout for the local Dagger engine.
    ///
    /// Defaults to 30 seconds.
    #[serde(default = "default_local_connect_timeout")]
    #[serde(with = "humantime_serde")]
    pub connect_timeout: Duration,

    /// Whether to allow running without a container runtime.
    ///
    /// If `true` and no container runtime is found, the system will
    /// attempt to use Dagger's built-in provisioning. This is mainly
    /// for testing and development.
    #[serde(default)]
    pub allow_no_runtime: bool,
}

fn default_local_connect_timeout() -> Duration {
    Duration::from_secs(30)
}

impl Default for LocalEngineConfig {
    fn default() -> Self {
        Self {
            dagger_path: None,
            runtime: ContainerRuntime::Auto,
            docker_host: None,
            connect_timeout: default_local_connect_timeout(),
            allow_no_runtime: false,
        }
    }
}

impl LocalEngineConfig {
    /// Creates a new default local engine configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the path to the Dagger binary.
    pub fn with_dagger_path(mut self, path: PathBuf) -> Self {
        self.dagger_path = Some(path);
        self
    }

    /// Sets the container runtime.
    pub fn with_runtime(mut self, runtime: ContainerRuntime) -> Self {
        self.runtime = runtime;
        self
    }

    /// Sets the Docker host.
    pub fn with_docker_host(mut self, host: String) -> Self {
        self.docker_host = Some(host);
        self
    }

    /// Sets the connection timeout.
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }
}

/// Configuration for cloud engine execution via the Engine Service.
///
/// Specifies the credentials and connection parameters for the
/// Circuit Breaker Engine Service.
///
/// # Examples
///
/// ```rust
/// use cb_core::engine::CloudEngineConfig;
/// use std::time::Duration;
///
/// let config = CloudEngineConfig {
///     url: "https://engines.circuitbreaker.io".to_string(),
///     api_key: "cb_xxx".to_string(),
///     organization_id: "org_123".to_string(),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CloudEngineConfig {
    /// URL of the Engine Service.
    ///
    /// Defaults to the production Engine Service URL.
    #[serde(default = "default_engine_service_url")]
    pub url: String,

    /// API key for authentication.
    ///
    /// Can be set via the `CB_API_KEY` environment variable.
    #[serde(default)]
    pub api_key: String,

    /// Organization ID for multi-tenancy.
    ///
    /// Can be set via the `CB_ORG_ID` environment variable.
    #[serde(default)]
    pub organization_id: String,

    /// Connection timeout for the Engine Service.
    ///
    /// Defaults to 30 seconds.
    #[serde(default = "default_cloud_connect_timeout")]
    #[serde(with = "humantime_serde")]
    pub connect_timeout: Duration,

    /// Request timeout for engine provisioning.
    ///
    /// Defaults to 120 seconds to allow for cold start provisioning.
    #[serde(default = "default_cloud_request_timeout")]
    #[serde(with = "humantime_serde")]
    pub request_timeout: Duration,

    /// Maximum number of retries for failed requests.
    ///
    /// Defaults to 3.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Base delay for exponential backoff between retries.
    ///
    /// Defaults to 1 second.
    #[serde(default = "default_retry_backoff")]
    #[serde(with = "humantime_serde")]
    pub retry_backoff: Duration,

    /// Preferred engine pool for execution.
    ///
    /// If specified, the Engine Service will attempt to provision
    /// an engine from this pool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_pool: Option<String>,

    /// Preferred region for execution.
    ///
    /// If specified, the Engine Service will attempt to provision
    /// an engine in this region.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_region: Option<String>,
}

fn default_engine_service_url() -> String {
    "https://engines.circuitbreaker.io".to_string()
}

fn default_cloud_connect_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_cloud_request_timeout() -> Duration {
    Duration::from_secs(120)
}

fn default_max_retries() -> u32 {
    3
}

fn default_retry_backoff() -> Duration {
    Duration::from_secs(1)
}

impl Default for CloudEngineConfig {
    fn default() -> Self {
        Self {
            url: default_engine_service_url(),
            api_key: String::new(),
            organization_id: String::new(),
            connect_timeout: default_cloud_connect_timeout(),
            request_timeout: default_cloud_request_timeout(),
            max_retries: default_max_retries(),
            retry_backoff: default_retry_backoff(),
            preferred_pool: None,
            preferred_region: None,
        }
    }
}

impl CloudEngineConfig {
    /// Creates a new cloud engine configuration with required parameters.
    pub fn new(url: String, api_key: String, organization_id: String) -> Self {
        Self {
            url,
            api_key,
            organization_id,
            ..Default::default()
        }
    }

    /// Sets the connection timeout.
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the request timeout.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Sets the preferred pool.
    pub fn with_preferred_pool(mut self, pool: String) -> Self {
        self.preferred_pool = Some(pool);
        self
    }

    /// Sets the preferred region.
    pub fn with_preferred_region(mut self, region: String) -> Self {
        self.preferred_region = Some(region);
        self
    }

    /// Returns `true` if this configuration has valid credentials.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::CloudEngineConfig;
    ///
    /// let config = CloudEngineConfig::default();
    /// assert!(!config.has_credentials());
    ///
    /// let config = CloudEngineConfig::new(
    ///     "https://engines.circuitbreaker.io".to_string(),
    ///     "cb_xxx".to_string(),
    ///     "org_123".to_string(),
    /// );
    /// assert!(config.has_credentials());
    /// ```
    pub fn has_credentials(&self) -> bool {
        !self.api_key.is_empty() && !self.organization_id.is_empty()
    }
}

/// Configuration for automatic engine mode selection.
///
/// Controls the behavior when `EngineMode::Auto` is selected, including
/// thresholds for cloud routing and fallback behavior.
///
/// # Examples
///
/// ```rust
/// use cb_core::engine::AutoModeConfig;
///
/// // Default configuration
/// let config = AutoModeConfig::default();
/// assert!(config.prefer_local);
/// assert!(config.fallback_to_cloud);
///
/// // Custom thresholds
/// let config = AutoModeConfig {
///     cloud_memory_threshold_gb: 32,
///     cloud_cpu_threshold_cores: 16,
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AutoModeConfig {
    /// Prefer local execution when possible.
    ///
    /// If `true` (default), the system will try local execution first
    /// when requirements don't force cloud execution.
    #[serde(default = "default_prefer_local")]
    pub prefer_local: bool,

    /// Fall back to cloud if local is unavailable.
    ///
    /// If `true` (default) and local execution fails or is unavailable,
    /// the system will attempt cloud execution instead.
    #[serde(default = "default_fallback_to_cloud")]
    pub fallback_to_cloud: bool,

    /// Memory threshold (GB) above which cloud is preferred.
    ///
    /// If requirements specify memory above this threshold, the system
    /// will route to cloud execution even if local is available.
    /// Defaults to 16 GB.
    #[serde(default = "default_memory_threshold")]
    pub cloud_memory_threshold_gb: u32,

    /// CPU threshold (cores) above which cloud is preferred.
    ///
    /// If requirements specify CPU cores above this threshold, the system
    /// will route to cloud execution even if local is available.
    /// Defaults to 8 cores.
    #[serde(default = "default_cpu_threshold")]
    pub cloud_cpu_threshold_cores: u32,

    /// Timeout for local availability checks.
    ///
    /// How long to wait when checking if local Dagger is available.
    /// Defaults to 5 seconds.
    #[serde(default = "default_availability_timeout")]
    #[serde(with = "humantime_serde")]
    pub availability_check_timeout: Duration,
}

fn default_prefer_local() -> bool {
    true
}

fn default_fallback_to_cloud() -> bool {
    true
}

fn default_memory_threshold() -> u32 {
    16
}

fn default_cpu_threshold() -> u32 {
    8
}

fn default_availability_timeout() -> Duration {
    Duration::from_secs(5)
}

impl Default for AutoModeConfig {
    fn default() -> Self {
        Self {
            prefer_local: default_prefer_local(),
            fallback_to_cloud: default_fallback_to_cloud(),
            cloud_memory_threshold_gb: default_memory_threshold(),
            cloud_cpu_threshold_cores: default_cpu_threshold(),
            availability_check_timeout: default_availability_timeout(),
        }
    }
}

impl AutoModeConfig {
    /// Creates a new default auto mode configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a configuration that only uses local execution.
    ///
    /// This disables cloud fallback, effectively making auto mode
    /// behave like local mode.
    pub fn local_only() -> Self {
        Self {
            prefer_local: true,
            fallback_to_cloud: false,
            ..Default::default()
        }
    }

    /// Creates a configuration that prefers cloud execution.
    ///
    /// This sets very low thresholds so most workloads will be
    /// routed to cloud.
    pub fn prefer_cloud() -> Self {
        Self {
            prefer_local: false,
            fallback_to_cloud: true,
            cloud_memory_threshold_gb: 0,
            cloud_cpu_threshold_cores: 0,
            ..Default::default()
        }
    }

    /// Sets the memory threshold.
    pub fn with_memory_threshold(mut self, gb: u32) -> Self {
        self.cloud_memory_threshold_gb = gb;
        self
    }

    /// Sets the CPU threshold.
    pub fn with_cpu_threshold(mut self, cores: u32) -> Self {
        self.cloud_cpu_threshold_cores = cores;
        self
    }

    /// Disables cloud fallback.
    pub fn without_cloud_fallback(mut self) -> Self {
        self.fallback_to_cloud = false;
        self
    }
}

/// Complete engine configuration combining mode, requirements, and settings.
///
/// This is the top-level configuration that specifies how Dagger engines
/// should be provisioned for workflow execution.
///
/// # Examples
///
/// ```rust
/// use cb_core::engine::{EngineConfig, EngineMode, EngineRequirements};
///
/// // Simple local configuration
/// let config = EngineConfig::local();
///
/// // Cloud configuration with GPU
/// let config = EngineConfig::cloud()
///     .with_requirements(EngineRequirements::with_gpu());
///
/// // Auto mode with custom thresholds
/// let config = EngineConfig::auto()
///     .with_requirements(EngineRequirements::with_memory(8));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EngineConfig {
    /// The execution mode (local, cloud, or auto).
    #[serde(default)]
    pub mode: EngineMode,

    /// Resource and capability requirements.
    #[serde(default)]
    pub requirements: EngineRequirements,

    /// Configuration for local engine execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<LocalEngineConfig>,

    /// Configuration for cloud engine execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud: Option<CloudEngineConfig>,

    /// Configuration for auto mode selection.
    #[serde(default)]
    pub auto: AutoModeConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            mode: EngineMode::Auto,
            requirements: EngineRequirements::default(),
            local: None,
            cloud: None,
            auto: AutoModeConfig::default(),
        }
    }
}

impl EngineConfig {
    /// Creates a new engine configuration with default auto mode.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a configuration for local-only execution.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::{EngineConfig, EngineMode};
    ///
    /// let config = EngineConfig::local();
    /// assert_eq!(config.mode, EngineMode::Local);
    /// ```
    pub fn local() -> Self {
        Self {
            mode: EngineMode::Local,
            local: Some(LocalEngineConfig::default()),
            ..Default::default()
        }
    }

    /// Creates a configuration for cloud-only execution.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::{EngineConfig, EngineMode};
    ///
    /// let config = EngineConfig::cloud();
    /// assert_eq!(config.mode, EngineMode::Cloud);
    /// ```
    pub fn cloud() -> Self {
        Self {
            mode: EngineMode::Cloud,
            ..Default::default()
        }
    }

    /// Creates a configuration for automatic mode selection.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_core::engine::{EngineConfig, EngineMode};
    ///
    /// let config = EngineConfig::auto();
    /// assert_eq!(config.mode, EngineMode::Auto);
    /// ```
    pub fn auto() -> Self {
        Self {
            mode: EngineMode::Auto,
            local: Some(LocalEngineConfig::default()),
            ..Default::default()
        }
    }

    /// Sets the resource requirements.
    pub fn with_requirements(mut self, requirements: EngineRequirements) -> Self {
        self.requirements = requirements;
        self
    }

    /// Sets the local engine configuration.
    pub fn with_local_config(mut self, config: LocalEngineConfig) -> Self {
        self.local = Some(config);
        self
    }

    /// Sets the cloud engine configuration.
    pub fn with_cloud_config(mut self, config: CloudEngineConfig) -> Self {
        self.cloud = Some(config);
        self
    }

    /// Sets the auto mode configuration.
    pub fn with_auto_config(mut self, config: AutoModeConfig) -> Self {
        self.auto = config;
        self
    }

    /// Returns the effective local configuration, or default if not set.
    pub fn local_config(&self) -> LocalEngineConfig {
        self.local.clone().unwrap_or_default()
    }

    /// Returns the effective cloud configuration, or default if not set.
    pub fn cloud_config(&self) -> CloudEngineConfig {
        self.cloud.clone().unwrap_or_default()
    }

    /// Determines if cloud execution is available based on configuration.
    ///
    /// Returns `true` if cloud mode is allowed and credentials are configured.
    pub fn cloud_available(&self) -> bool {
        self.mode.allows_cloud()
            && self
                .cloud
                .as_ref()
                .map(|c| c.has_credentials())
                .unwrap_or(false)
    }

    /// Merges another configuration into this one.
    ///
    /// The other configuration takes precedence for mode, but requirements
    /// are merged (taking the maximum of each).
    pub fn merge(&mut self, other: &Self) {
        // Mode from other takes precedence if explicitly set
        if other.mode != EngineMode::Auto {
            self.mode = other.mode;
        }

        // Merge requirements (takes maximum)
        self.requirements.merge(&other.requirements);

        // Override configs if provided
        if other.local.is_some() {
            self.local = other.local.clone();
        }
        if other.cloud.is_some() {
            self.cloud = other.cloud.clone();
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // EngineMode tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_engine_mode_default() {
        let mode = EngineMode::default();
        assert_eq!(mode, EngineMode::Auto);
    }

    #[test]
    fn test_engine_mode_display() {
        assert_eq!(EngineMode::Local.to_string(), "local");
        assert_eq!(EngineMode::Cloud.to_string(), "cloud");
        assert_eq!(EngineMode::Auto.to_string(), "auto");
    }

    #[test]
    fn test_engine_mode_allows_local() {
        assert!(EngineMode::Local.allows_local());
        assert!(EngineMode::Auto.allows_local());
        assert!(!EngineMode::Cloud.allows_local());
    }

    #[test]
    fn test_engine_mode_allows_cloud() {
        assert!(EngineMode::Cloud.allows_cloud());
        assert!(EngineMode::Auto.allows_cloud());
        assert!(!EngineMode::Local.allows_cloud());
    }

    #[test]
    fn test_engine_mode_is_auto() {
        assert!(EngineMode::Auto.is_auto());
        assert!(!EngineMode::Local.is_auto());
        assert!(!EngineMode::Cloud.is_auto());
    }

    #[test]
    fn test_engine_mode_serialization() {
        let mode = EngineMode::Local;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, r#""local""#);

        let parsed: EngineMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, mode);
    }

    // ------------------------------------------------------------------------
    // ContainerRuntime tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_container_runtime_default() {
        let runtime = ContainerRuntime::default();
        assert_eq!(runtime, ContainerRuntime::Auto);
    }

    #[test]
    fn test_container_runtime_display() {
        assert_eq!(ContainerRuntime::Docker.to_string(), "docker");
        assert_eq!(ContainerRuntime::Podman.to_string(), "podman");
        assert_eq!(ContainerRuntime::Colima.to_string(), "colima");
        assert_eq!(ContainerRuntime::RancherDesktop.to_string(), "rancher-desktop");
        assert_eq!(ContainerRuntime::OrbStack.to_string(), "orbstack");
    }

    #[test]
    fn test_container_runtime_socket_path() {
        assert_eq!(ContainerRuntime::Docker.socket_path(), "/var/run/docker.sock");
        assert!(ContainerRuntime::Podman.socket_path().contains("podman"));
    }

    #[test]
    fn test_container_runtime_cli_command() {
        assert_eq!(ContainerRuntime::Docker.cli_command(), "docker");
        assert_eq!(ContainerRuntime::Podman.cli_command(), "podman");
        // These use Docker CLI
        assert_eq!(ContainerRuntime::RancherDesktop.cli_command(), "docker");
        assert_eq!(ContainerRuntime::OrbStack.cli_command(), "docker");
    }

    #[test]
    fn test_container_runtime_is_typically_rootless() {
        assert!(ContainerRuntime::Podman.is_typically_rootless());
        assert!(!ContainerRuntime::Docker.is_typically_rootless());
    }

    // ------------------------------------------------------------------------
    // EngineRequirements tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_engine_requirements_default() {
        let req = EngineRequirements::default();
        assert!(!req.gpu);
        assert!(!req.require_audit);
        assert!(req.memory_gb.is_none());
        assert!(req.cpu_cores.is_none());
    }

    #[test]
    fn test_engine_requirements_with_gpu() {
        let req = EngineRequirements::with_gpu();
        assert!(req.gpu);
        assert!(req.requires_cloud());
    }

    #[test]
    fn test_engine_requirements_with_memory() {
        let req = EngineRequirements::with_memory(32);
        assert_eq!(req.memory_gb, Some(32));
    }

    #[test]
    fn test_engine_requirements_requires_cloud() {
        let req = EngineRequirements::default();
        assert!(!req.requires_cloud());

        let req = EngineRequirements {
            gpu: true,
            ..Default::default()
        };
        assert!(req.requires_cloud());

        let req = EngineRequirements {
            require_audit: true,
            ..Default::default()
        };
        assert!(req.requires_cloud());
    }

    #[test]
    fn test_engine_requirements_suggests_cloud() {
        let req = EngineRequirements::default();
        assert!(!req.suggests_cloud(16, 8));

        // High memory suggests cloud
        let req = EngineRequirements {
            memory_gb: Some(32),
            ..Default::default()
        };
        assert!(req.suggests_cloud(16, 8));
        assert!(!req.suggests_cloud(64, 8));

        // High CPU suggests cloud
        let req = EngineRequirements {
            cpu_cores: Some(16),
            ..Default::default()
        };
        assert!(req.suggests_cloud(16, 8));
        assert!(!req.suggests_cloud(16, 32));
    }

    #[test]
    fn test_engine_requirements_merge() {
        let mut base = EngineRequirements {
            memory_gb: Some(8),
            cpu_cores: Some(4),
            ..Default::default()
        };

        let other = EngineRequirements {
            gpu: true,
            memory_gb: Some(16),
            ..Default::default()
        };

        base.merge(&other);

        assert!(base.gpu);
        assert_eq!(base.memory_gb, Some(16)); // Max of 8 and 16
        assert_eq!(base.cpu_cores, Some(4)); // From base
    }

    #[test]
    fn test_engine_requirements_merge_takes_max() {
        let mut req1 = EngineRequirements {
            memory_gb: Some(32),
            cpu_cores: Some(4),
            ..Default::default()
        };

        let req2 = EngineRequirements {
            memory_gb: Some(16),
            cpu_cores: Some(8),
            ..Default::default()
        };

        req1.merge(&req2);

        assert_eq!(req1.memory_gb, Some(32)); // Max
        assert_eq!(req1.cpu_cores, Some(8)); // Max
    }

    // ------------------------------------------------------------------------
    // LocalEngineConfig tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_local_engine_config_default() {
        let config = LocalEngineConfig::default();
        assert!(config.dagger_path.is_none());
        assert_eq!(config.runtime, ContainerRuntime::Auto);
        assert!(config.docker_host.is_none());
        assert_eq!(config.connect_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_local_engine_config_builder() {
        let config = LocalEngineConfig::new()
            .with_dagger_path(PathBuf::from("/usr/local/bin/dagger"))
            .with_runtime(ContainerRuntime::Podman)
            .with_docker_host("unix:///custom/socket".to_string())
            .with_connect_timeout(Duration::from_secs(60));

        assert_eq!(
            config.dagger_path,
            Some(PathBuf::from("/usr/local/bin/dagger"))
        );
        assert_eq!(config.runtime, ContainerRuntime::Podman);
        assert_eq!(config.docker_host, Some("unix:///custom/socket".to_string()));
        assert_eq!(config.connect_timeout, Duration::from_secs(60));
    }

    // ------------------------------------------------------------------------
    // CloudEngineConfig tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_cloud_engine_config_default() {
        let config = CloudEngineConfig::default();
        assert_eq!(config.url, "https://engines.circuitbreaker.io");
        assert!(config.api_key.is_empty());
        assert!(config.organization_id.is_empty());
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_cloud_engine_config_new() {
        let config = CloudEngineConfig::new(
            "https://custom.engine.io".to_string(),
            "api_key_123".to_string(),
            "org_456".to_string(),
        );

        assert_eq!(config.url, "https://custom.engine.io");
        assert_eq!(config.api_key, "api_key_123");
        assert_eq!(config.organization_id, "org_456");
    }

    #[test]
    fn test_cloud_engine_config_has_credentials() {
        let config = CloudEngineConfig::default();
        assert!(!config.has_credentials());

        let config = CloudEngineConfig::new(
            "https://engines.circuitbreaker.io".to_string(),
            "key".to_string(),
            "org".to_string(),
        );
        assert!(config.has_credentials());

        // Missing org
        let config = CloudEngineConfig {
            api_key: "key".to_string(),
            ..Default::default()
        };
        assert!(!config.has_credentials());
    }

    #[test]
    fn test_cloud_engine_config_builder() {
        let config = CloudEngineConfig::new(
            "https://engines.circuitbreaker.io".to_string(),
            "key".to_string(),
            "org".to_string(),
        )
        .with_connect_timeout(Duration::from_secs(60))
        .with_request_timeout(Duration::from_secs(300))
        .with_preferred_pool("gpu-pool".to_string())
        .with_preferred_region("us-west-2".to_string());

        assert_eq!(config.connect_timeout, Duration::from_secs(60));
        assert_eq!(config.request_timeout, Duration::from_secs(300));
        assert_eq!(config.preferred_pool, Some("gpu-pool".to_string()));
        assert_eq!(config.preferred_region, Some("us-west-2".to_string()));
    }

    // ------------------------------------------------------------------------
    // AutoModeConfig tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_auto_mode_config_default() {
        let config = AutoModeConfig::default();
        assert!(config.prefer_local);
        assert!(config.fallback_to_cloud);
        assert_eq!(config.cloud_memory_threshold_gb, 16);
        assert_eq!(config.cloud_cpu_threshold_cores, 8);
    }

    #[test]
    fn test_auto_mode_config_local_only() {
        let config = AutoModeConfig::local_only();
        assert!(config.prefer_local);
        assert!(!config.fallback_to_cloud);
    }

    #[test]
    fn test_auto_mode_config_prefer_cloud() {
        let config = AutoModeConfig::prefer_cloud();
        assert!(!config.prefer_local);
        assert!(config.fallback_to_cloud);
        assert_eq!(config.cloud_memory_threshold_gb, 0);
        assert_eq!(config.cloud_cpu_threshold_cores, 0);
    }

    #[test]
    fn test_auto_mode_config_builder() {
        let config = AutoModeConfig::new()
            .with_memory_threshold(32)
            .with_cpu_threshold(16)
            .without_cloud_fallback();

        assert_eq!(config.cloud_memory_threshold_gb, 32);
        assert_eq!(config.cloud_cpu_threshold_cores, 16);
        assert!(!config.fallback_to_cloud);
    }

    // ------------------------------------------------------------------------
    // EngineConfig tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_engine_config_default() {
        let config = EngineConfig::default();
        assert_eq!(config.mode, EngineMode::Auto);
        assert!(config.local.is_none());
        assert!(config.cloud.is_none());
    }

    #[test]
    fn test_engine_config_local() {
        let config = EngineConfig::local();
        assert_eq!(config.mode, EngineMode::Local);
        assert!(config.local.is_some());
    }

    #[test]
    fn test_engine_config_cloud() {
        let config = EngineConfig::cloud();
        assert_eq!(config.mode, EngineMode::Cloud);
    }

    #[test]
    fn test_engine_config_auto() {
        let config = EngineConfig::auto();
        assert_eq!(config.mode, EngineMode::Auto);
        assert!(config.local.is_some());
    }

    #[test]
    fn test_engine_config_with_requirements() {
        let config = EngineConfig::cloud().with_requirements(EngineRequirements::with_gpu());

        assert!(config.requirements.gpu);
    }

    #[test]
    fn test_engine_config_cloud_available() {
        // No cloud config
        let config = EngineConfig::cloud();
        assert!(!config.cloud_available());

        // Cloud config without credentials
        let config = EngineConfig::cloud().with_cloud_config(CloudEngineConfig::default());
        assert!(!config.cloud_available());

        // Cloud config with credentials
        let config = EngineConfig::cloud().with_cloud_config(CloudEngineConfig::new(
            "https://engines.circuitbreaker.io".to_string(),
            "key".to_string(),
            "org".to_string(),
        ));
        assert!(config.cloud_available());

        // Local mode doesn't allow cloud
        let config = EngineConfig::local().with_cloud_config(CloudEngineConfig::new(
            "https://engines.circuitbreaker.io".to_string(),
            "key".to_string(),
            "org".to_string(),
        ));
        assert!(!config.cloud_available());
    }

    #[test]
    fn test_engine_config_merge() {
        let mut base = EngineConfig::auto().with_requirements(EngineRequirements {
            memory_gb: Some(8),
            ..Default::default()
        });

        let override_config = EngineConfig::cloud().with_requirements(EngineRequirements {
            gpu: true,
            memory_gb: Some(16),
            ..Default::default()
        });

        base.merge(&override_config);

        assert_eq!(base.mode, EngineMode::Cloud); // Overridden
        assert!(base.requirements.gpu); // From override
        assert_eq!(base.requirements.memory_gb, Some(16)); // Max
    }

    #[test]
    fn test_engine_config_merge_keeps_auto_mode() {
        let mut base = EngineConfig::local();

        // Auto mode doesn't override explicit mode
        let override_config = EngineConfig::auto();
        base.merge(&override_config);

        assert_eq!(base.mode, EngineMode::Local);
    }

    // ------------------------------------------------------------------------
    // Serialization tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_engine_config_serialization_roundtrip() {
        let config = EngineConfig::auto()
            .with_requirements(EngineRequirements {
                gpu: true,
                memory_gb: Some(32),
                cpu_cores: Some(8),
                ..Default::default()
            })
            .with_local_config(LocalEngineConfig {
                runtime: ContainerRuntime::Podman,
                ..Default::default()
            })
            .with_cloud_config(CloudEngineConfig::new(
                "https://engines.circuitbreaker.io".to_string(),
                "key".to_string(),
                "org".to_string(),
            ));

        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: EngineConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.mode, config.mode);
        assert_eq!(parsed.requirements.gpu, config.requirements.gpu);
        assert_eq!(parsed.requirements.memory_gb, config.requirements.memory_gb);
    }

    #[test]
    fn test_engine_requirements_serialization() {
        let req = EngineRequirements {
            gpu: true,
            memory_gb: Some(32),
            cpu_cores: Some(16),
            require_audit: true,
            engine_version: Some("0.18.0".to_string()),
            labels: None,
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"gpu\":true"));
        assert!(json.contains("\"memory_gb\":32"));

        let parsed: EngineRequirements = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }
}
