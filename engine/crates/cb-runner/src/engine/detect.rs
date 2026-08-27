//! # Runtime Detection
//!
//! This module provides detection capabilities for Dagger installations and
//! container runtimes. It enables the engine system to automatically discover
//! and validate the local execution environment.
//!
//! ## Overview
//!
//! The detection system checks for:
//!
//! 1. **Dagger CLI**: Locates the Dagger binary and extracts version information
//! 2. **Container Runtimes**: Detects Docker, Podman, Colima, and other runtimes
//! 3. **Runtime Health**: Validates that detected runtimes are running and accessible
//!
//! ## Detection Strategy
//!
//! Detection follows a prioritized search strategy:
//!
//! 1. Check explicit configuration (if provided)
//! 2. Search PATH environment variable
//! 3. Check well-known installation locations
//! 4. Validate found installations are functional
//!
//! ## Usage
//!
//! ```rust,no_run
//! use cb_runner::engine::detect::RuntimeDetector;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let detector = RuntimeDetector::new();
//!
//! // Detect Dagger
//! if let Some(dagger) = detector.detect_dagger().await? {
//!     println!("Found Dagger {} at {:?}", dagger.version, dagger.path);
//! }
//!
//! // Detect container runtime
//! if let Some(runtime) = detector.detect_container_runtime().await? {
//!     println!("Found {} at {:?}", runtime.runtime, runtime.socket_path);
//! }
//!
//! // Check full availability
//! let status = detector.check_availability().await?;
//! if status.is_ready() {
//!     println!("Ready for local execution!");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Platform Support
//!
//! The detection system supports:
//!
//! - **Linux**: Standard paths, systemd sockets, rootless Podman
//! - **macOS**: Docker Desktop, Colima, OrbStack, Rancher Desktop
//! - **Windows**: Docker Desktop (WSL2), Docker paths
//!
//! ## Caching
//!
//! Detection results can be cached to avoid repeated filesystem and process
//! checks. Use `RuntimeDetector::with_cache()` to enable caching with a
//! configurable TTL.

use cb_core::engine::ContainerRuntime;
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::RwLock;

use super::error::{EngineError, EngineResult};

/// Information about a detected Dagger installation.
///
/// Contains the path to the Dagger binary, its version, and any additional
/// capabilities or features that were detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaggerInfo {
    /// Path to the Dagger binary.
    pub path: PathBuf,

    /// Dagger version string (e.g., "0.18.0").
    pub version: String,

    /// Parsed version components for comparison.
    pub version_parts: (u32, u32, u32),

    /// Whether this is a development/debug build.
    pub is_dev_build: bool,

    /// Additional capabilities detected.
    pub capabilities: Vec<String>,
}

impl DaggerInfo {
    /// Creates a new DaggerInfo with the given path and version.
    pub fn new(path: PathBuf, version: String) -> Self {
        let version_parts = parse_version(&version);
        let is_dev_build = version.contains("dev") || version.contains("dirty");

        Self {
            path,
            version,
            version_parts,
            is_dev_build,
            capabilities: Vec::new(),
        }
    }

    /// Returns `true` if this Dagger version meets the minimum requirement.
    ///
    /// # Arguments
    ///
    /// * `required` - Version string like "0.18.0"
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cb_runner::engine::detect::DaggerInfo;
    /// use std::path::PathBuf;
    ///
    /// let info = DaggerInfo::new(PathBuf::from("/usr/bin/dagger"), "0.18.0".into());
    /// assert!(info.meets_version("0.17.0"));
    /// assert!(info.meets_version("0.18.0"));
    /// assert!(!info.meets_version("0.19.0"));
    /// ```
    pub fn meets_version(&self, required: &str) -> bool {
        let required_parts = parse_version(required);
        self.version_parts >= required_parts
    }
}

/// Information about a detected container runtime.
///
/// Contains the runtime type, socket path, and health status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerRuntimeInfo {
    /// The type of container runtime.
    pub runtime: ContainerRuntime,

    /// Path to the runtime socket.
    pub socket_path: PathBuf,

    /// Version string if available.
    pub version: Option<String>,

    /// Whether the runtime is currently running.
    pub is_running: bool,

    /// Whether the runtime is rootless.
    pub is_rootless: bool,
}

impl ContainerRuntimeInfo {
    /// Creates a new ContainerRuntimeInfo.
    pub fn new(runtime: ContainerRuntime, socket_path: PathBuf) -> Self {
        Self {
            runtime,
            socket_path,
            version: None,
            is_running: false,
            is_rootless: runtime.is_typically_rootless(),
        }
    }

    /// Sets the version.
    pub fn with_version(mut self, version: String) -> Self {
        self.version = Some(version);
        self
    }

    /// Sets the running status.
    pub fn with_running(mut self, running: bool) -> Self {
        self.is_running = running;
        self
    }

    /// Sets the rootless status.
    pub fn with_rootless(mut self, rootless: bool) -> Self {
        self.is_rootless = rootless;
        self
    }
}

/// Overall availability status for local execution.
///
/// Combines Dagger and container runtime detection results with health
/// status to determine if the system is ready for local execution.
#[derive(Debug, Clone)]
pub struct AvailabilityStatus {
    /// Detected Dagger installation, if any.
    pub dagger: Option<DaggerInfo>,

    /// Detected container runtime, if any.
    pub container_runtime: Option<ContainerRuntimeInfo>,

    /// Errors encountered during detection.
    pub errors: Vec<String>,

    /// Time when this status was captured.
    pub checked_at: Instant,
}

impl AvailabilityStatus {
    /// Creates a new availability status.
    pub fn new() -> Self {
        Self {
            dagger: None,
            container_runtime: None,
            errors: Vec::new(),
            checked_at: Instant::now(),
        }
    }

    /// Returns `true` if the system is ready for local execution.
    ///
    /// This requires both Dagger and a running container runtime.
    pub fn is_ready(&self) -> bool {
        self.dagger.is_some()
            && self
                .container_runtime
                .as_ref()
                .map(|r| r.is_running)
                .unwrap_or(false)
    }

    /// Returns `true` if Dagger is available (even if runtime isn't running).
    pub fn has_dagger(&self) -> bool {
        self.dagger.is_some()
    }

    /// Returns `true` if a container runtime is installed (even if not running).
    pub fn has_container_runtime(&self) -> bool {
        self.container_runtime.is_some()
    }

    /// Returns a human-readable summary of the availability status.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        match &self.dagger {
            Some(d) => parts.push(format!("Dagger {} at {:?}", d.version, d.path)),
            None => parts.push("Dagger: not found".into()),
        }

        match &self.container_runtime {
            Some(r) if r.is_running => {
                parts.push(format!("{} running at {:?}", r.runtime, r.socket_path))
            }
            Some(r) => parts.push(format!("{} installed but not running", r.runtime)),
            None => parts.push("Container runtime: not found".into()),
        }

        if !self.errors.is_empty() {
            parts.push(format!("Errors: {}", self.errors.join(", ")));
        }

        parts.join("; ")
    }
}

impl Default for AvailabilityStatus {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache entry for detection results.
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    value: T,
    expires_at: Instant,
}

impl<T> CacheEntry<T> {
    fn new(value: T, ttl: Duration) -> Self {
        Self {
            value,
            expires_at: Instant::now() + ttl,
        }
    }

    fn is_valid(&self) -> bool {
        Instant::now() < self.expires_at
    }
}

/// Detection cache for avoiding repeated checks.
#[derive(Debug, Default)]
struct DetectionCache {
    dagger: Option<CacheEntry<Option<DaggerInfo>>>,
    runtimes: HashMap<ContainerRuntime, CacheEntry<Option<ContainerRuntimeInfo>>>,
    availability: Option<CacheEntry<AvailabilityStatus>>,
}

/// Configuration for the runtime detector.
#[derive(Debug, Clone)]
pub struct DetectorConfig {
    /// Additional paths to search for Dagger.
    pub dagger_search_paths: Vec<PathBuf>,

    /// Preferred container runtime (if any).
    pub preferred_runtime: Option<ContainerRuntime>,

    /// Timeout for detection commands.
    pub command_timeout: Duration,

    /// Cache TTL for detection results.
    pub cache_ttl: Option<Duration>,

    /// Custom Docker host override.
    pub docker_host: Option<String>,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            dagger_search_paths: Vec::new(),
            preferred_runtime: None,
            command_timeout: Duration::from_secs(10),
            cache_ttl: None,
            docker_host: None,
        }
    }
}

impl DetectorConfig {
    /// Creates a new detector configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a path to search for Dagger.
    pub fn with_dagger_search_path(mut self, path: PathBuf) -> Self {
        self.dagger_search_paths.push(path);
        self
    }

    /// Sets the preferred container runtime.
    pub fn with_preferred_runtime(mut self, runtime: ContainerRuntime) -> Self {
        self.preferred_runtime = Some(runtime);
        self
    }

    /// Sets the command timeout.
    pub fn with_command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    /// Enables caching with the given TTL.
    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = Some(ttl);
        self
    }

    /// Sets the Docker host override.
    pub fn with_docker_host(mut self, host: String) -> Self {
        self.docker_host = Some(host);
        self
    }
}

/// Runtime detector for Dagger and container runtimes.
///
/// This struct provides methods to detect and validate the local execution
/// environment. It can optionally cache results to avoid repeated checks.
///
/// # Examples
///
/// ```rust,no_run
/// use cb_runner::engine::detect::{RuntimeDetector, DetectorConfig};
/// use std::time::Duration;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Create detector with caching
/// let config = DetectorConfig::new()
///     .with_cache_ttl(Duration::from_secs(60));
/// let detector = RuntimeDetector::with_config(config);
///
/// // Check availability (results are cached)
/// let status = detector.check_availability().await?;
/// println!("Ready: {}", status.is_ready());
/// # Ok(())
/// # }
/// ```
pub struct RuntimeDetector {
    config: DetectorConfig,
    cache: Arc<RwLock<DetectionCache>>,
}

impl RuntimeDetector {
    /// Creates a new runtime detector with default configuration.
    pub fn new() -> Self {
        Self::with_config(DetectorConfig::default())
    }

    /// Creates a new runtime detector with the given configuration.
    pub fn with_config(config: DetectorConfig) -> Self {
        Self {
            config,
            cache: Arc::new(RwLock::new(DetectionCache::default())),
        }
    }

    /// Detects the Dagger installation.
    ///
    /// Searches for the Dagger binary in PATH and configured search paths,
    /// then validates the installation by checking the version.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(DaggerInfo))` if Dagger is found and functional,
    /// `Ok(None)` if not found, or an error if detection fails unexpectedly.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use cb_runner::engine::detect::RuntimeDetector;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let detector = RuntimeDetector::new();
    ///
    /// match detector.detect_dagger().await? {
    ///     Some(info) => println!("Dagger {} found", info.version),
    ///     None => println!("Dagger not found"),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn detect_dagger(&self) -> EngineResult<Option<DaggerInfo>> {
        // Check cache first
        if let Some(ttl) = self.config.cache_ttl {
            let cache = self.cache.read().await;
            if let Some(entry) = &cache.dagger {
                if entry.is_valid() {
                    return Ok(entry.value.clone());
                }
            }
            drop(cache);
        }

        let result = self.detect_dagger_uncached().await;

        // Update cache if enabled
        if let Some(ttl) = self.config.cache_ttl {
            if let Ok(ref info) = result {
                let mut cache = self.cache.write().await;
                cache.dagger = Some(CacheEntry::new(info.clone(), ttl));
            }
        }

        result
    }

    /// Detects the Dagger installation without caching.
    async fn detect_dagger_uncached(&self) -> EngineResult<Option<DaggerInfo>> {
        let search_paths = self.build_dagger_search_paths();

        for path in &search_paths {
            let dagger_path = if path.is_file() {
                path.clone()
            } else {
                path.join("dagger")
            };

            if !dagger_path.exists() {
                continue;
            }

            // Try to get version
            match self.get_dagger_version(&dagger_path).await {
                Ok(version) => {
                    return Ok(Some(DaggerInfo::new(dagger_path, version)));
                }
                Err(e) => {
                    tracing::debug!(
                        path = ?dagger_path,
                        error = %e,
                        "Found Dagger binary but failed to get version"
                    );
                    continue;
                }
            }
        }

        Ok(None)
    }

    /// Builds the list of paths to search for Dagger.
    fn build_dagger_search_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // Add configured search paths first
        paths.extend(self.config.dagger_search_paths.clone());

        // Add PATH entries
        if let Ok(path_env) = env::var("PATH") {
            for p in env::split_paths(&path_env) {
                if !paths.contains(&p) {
                    paths.push(p);
                }
            }
        }

        // Add well-known locations
        let well_known = [
            "/usr/local/bin",
            "/usr/bin",
            "/opt/homebrew/bin",  // macOS Homebrew (ARM)
            "/home/linuxbrew/.linuxbrew/bin",  // Linux Homebrew
        ];

        for wk in well_known {
            let p = PathBuf::from(wk);
            if !paths.contains(&p) {
                paths.push(p);
            }
        }

        // Add user-specific locations
        if let Ok(home) = env::var("HOME") {
            let home_paths = [
                format!("{}/.local/bin", home),
                format!("{}/bin", home),
                format!("{}/.dagger/bin", home),
            ];
            for hp in home_paths {
                let p = PathBuf::from(hp);
                if !paths.contains(&p) {
                    paths.push(p);
                }
            }
        }

        paths
    }

    /// Gets the Dagger version from a binary.
    async fn get_dagger_version(&self, path: &Path) -> EngineResult<String> {
        let output = tokio::time::timeout(
            self.config.command_timeout,
            Command::new(path)
                .arg("version")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await
        .map_err(|_| EngineError::timeout("dagger version", self.config.command_timeout))?
        .map_err(|e| EngineError::DetectionFailed {
            component: "Dagger".into(),
            message: e.to_string(),
        })?;

        if !output.status.success() {
            return Err(EngineError::DetectionFailed {
                component: "Dagger".into(),
                message: format!(
                    "dagger version failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse version from output like "dagger v0.18.0 (registry.dagger.io/engine:v0.18.0) linux/amd64"
        // or "dagger version 0.18.0"
        let version = stdout
            .split_whitespace()
            .find(|s| s.starts_with('v') || s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
            .map(|s| s.trim_start_matches('v').to_string())
            .ok_or_else(|| EngineError::DetectionFailed {
                component: "Dagger".into(),
                message: format!("Could not parse version from: {}", stdout.trim()),
            })?;

        Ok(version)
    }

    /// Detects an available container runtime.
    ///
    /// Searches for Docker, Podman, and other supported runtimes in order
    /// of preference. If a preferred runtime is configured, it will be
    /// checked first.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(ContainerRuntimeInfo))` if a runtime is found,
    /// `Ok(None)` if no runtime is available.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use cb_runner::engine::detect::RuntimeDetector;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let detector = RuntimeDetector::new();
    ///
    /// match detector.detect_container_runtime().await? {
    ///     Some(info) => println!("Found {} (running: {})", info.runtime, info.is_running),
    ///     None => println!("No container runtime found"),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn detect_container_runtime(&self) -> EngineResult<Option<ContainerRuntimeInfo>> {
        // Check DOCKER_HOST environment variable first
        if let Some(docker_host) = self.get_docker_host() {
            if let Some(info) = self.check_docker_host(&docker_host).await? {
                return Ok(Some(info));
            }
        }

        // Build runtime check order
        let mut runtimes_to_check = vec![
            ContainerRuntime::Docker,
            ContainerRuntime::Podman,
            ContainerRuntime::Colima,
            ContainerRuntime::OrbStack,
            ContainerRuntime::RancherDesktop,
        ];

        // Move preferred runtime to front if specified
        if let Some(preferred) = self.config.preferred_runtime {
            runtimes_to_check.retain(|r| *r != preferred);
            runtimes_to_check.insert(0, preferred);
        }

        // Check each runtime
        for runtime in runtimes_to_check {
            if let Some(info) = self.detect_specific_runtime(runtime).await? {
                return Ok(Some(info));
            }
        }

        Ok(None)
    }

    /// Checks a specific Docker host for connectivity.
    async fn check_docker_host(&self, host: &str) -> EngineResult<Option<ContainerRuntimeInfo>> {
        let socket_path = if host.starts_with("unix://") {
            PathBuf::from(host.trim_start_matches("unix://"))
        } else {
            return Ok(None); // TCP hosts not directly supported yet
        };

        if !socket_path.exists() {
            return Ok(None);
        }

        // Check if Docker is running
        let is_running = self.check_docker_cli_running().await;

        Ok(Some(
            ContainerRuntimeInfo::new(ContainerRuntime::Docker, socket_path)
                .with_running(is_running),
        ))
    }

    /// Gets the Docker host from config or environment.
    fn get_docker_host(&self) -> Option<String> {
        self.config.docker_host.clone().or_else(|| env::var("DOCKER_HOST").ok())
    }

    /// Detects a specific container runtime.
    async fn detect_specific_runtime(
        &self,
        runtime: ContainerRuntime,
    ) -> EngineResult<Option<ContainerRuntimeInfo>> {
        // Check cache first
        if let Some(ttl) = self.config.cache_ttl {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.runtimes.get(&runtime) {
                if entry.is_valid() {
                    return Ok(entry.value.clone());
                }
            }
            drop(cache);
        }

        let result = self.detect_specific_runtime_uncached(runtime).await;

        // Update cache if enabled
        if let Some(ttl) = self.config.cache_ttl {
            if let Ok(ref info) = result {
                let mut cache = self.cache.write().await;
                cache.runtimes.insert(runtime, CacheEntry::new(info.clone(), ttl));
            }
        }

        result
    }

    /// Detects a specific container runtime without caching.
    async fn detect_specific_runtime_uncached(
        &self,
        runtime: ContainerRuntime,
    ) -> EngineResult<Option<ContainerRuntimeInfo>> {
        let socket_paths = self.get_socket_paths(runtime);

        for socket_path in socket_paths {
            if !socket_path.exists() {
                continue;
            }

            let is_running = match runtime {
                ContainerRuntime::Docker
                | ContainerRuntime::Colima
                | ContainerRuntime::OrbStack
                | ContainerRuntime::RancherDesktop => self.check_docker_cli_running().await,
                ContainerRuntime::Podman => self.check_podman_running().await,
                ContainerRuntime::Auto => continue, // Skip auto
            };

            let version = self.get_runtime_version(runtime).await.ok();

            return Ok(Some(
                ContainerRuntimeInfo::new(runtime, socket_path)
                    .with_running(is_running)
                    .with_version(version.unwrap_or_default()),
            ));
        }

        Ok(None)
    }

    /// Gets the socket paths to check for a runtime.
    fn get_socket_paths(&self, runtime: ContainerRuntime) -> Vec<PathBuf> {
        let home = env::var("HOME").unwrap_or_default();
        // Get UID from environment or use a default for rootless paths
        let uid = env::var("UID")
            .or_else(|_| env::var("EUID"))
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1000);

        match runtime {
            ContainerRuntime::Docker | ContainerRuntime::Auto => vec![
                PathBuf::from("/var/run/docker.sock"),
                PathBuf::from(format!("{}/.docker/run/docker.sock", home)),
            ],
            ContainerRuntime::Podman => vec![
                PathBuf::from(format!("/run/user/{}/podman/podman.sock", uid)),
                PathBuf::from("/var/run/podman/podman.sock"),
                PathBuf::from(format!("{}/.local/share/containers/podman/machine/podman.sock", home)),
            ],
            ContainerRuntime::Colima => vec![
                PathBuf::from(format!("{}/.colima/default/docker.sock", home)),
                PathBuf::from(format!("{}/.colima/docker.sock", home)),
            ],
            ContainerRuntime::OrbStack => vec![
                PathBuf::from(format!("{}/.orbstack/run/docker.sock", home)),
            ],
            ContainerRuntime::RancherDesktop => vec![
                PathBuf::from(format!("{}/.rd/docker.sock", home)),
            ],
        }
    }

    /// Checks if Docker CLI can connect to the daemon.
    async fn check_docker_cli_running(&self) -> bool {
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            Command::new("docker")
                .args(["info", "--format", "{{.ServerVersion}}"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
        )
        .await;

        matches!(result, Ok(Ok(status)) if status.success())
    }

    /// Checks if Podman is running.
    async fn check_podman_running(&self) -> bool {
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            Command::new("podman")
                .args(["info", "--format", "{{.Version.Version}}"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
        )
        .await;

        matches!(result, Ok(Ok(status)) if status.success())
    }

    /// Gets the version of a container runtime.
    async fn get_runtime_version(&self, runtime: ContainerRuntime) -> EngineResult<String> {
        let (cmd, args): (&str, &[&str]) = match runtime {
            ContainerRuntime::Docker
            | ContainerRuntime::Colima
            | ContainerRuntime::OrbStack
            | ContainerRuntime::RancherDesktop
            | ContainerRuntime::Auto => ("docker", &["version", "--format", "{{.Server.Version}}"]),
            ContainerRuntime::Podman => ("podman", &["version", "--format", "{{.Version}}"]),
        };

        let output = tokio::time::timeout(
            self.config.command_timeout,
            Command::new(cmd)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await
        .map_err(|_| EngineError::timeout("get runtime version", self.config.command_timeout))?
        .map_err(|e| EngineError::DetectionFailed {
            component: runtime.to_string(),
            message: e.to_string(),
        })?;

        if !output.status.success() {
            return Err(EngineError::DetectionFailed {
                component: runtime.to_string(),
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Checks overall availability for local execution.
    ///
    /// This method combines Dagger and container runtime detection into
    /// a single status report.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use cb_runner::engine::detect::RuntimeDetector;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let detector = RuntimeDetector::new();
    /// let status = detector.check_availability().await?;
    ///
    /// if status.is_ready() {
    ///     println!("System is ready for local execution");
    /// } else {
    ///     println!("Issues: {}", status.summary());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn check_availability(&self) -> EngineResult<AvailabilityStatus> {
        // Check cache first
        if let Some(ttl) = self.config.cache_ttl {
            let cache = self.cache.read().await;
            if let Some(entry) = &cache.availability {
                if entry.is_valid() {
                    return Ok(entry.value.clone());
                }
            }
            drop(cache);
        }

        let mut status = AvailabilityStatus::new();

        // Detect Dagger
        match self.detect_dagger().await {
            Ok(dagger) => status.dagger = dagger,
            Err(e) => status.errors.push(format!("Dagger detection: {}", e)),
        }

        // Detect container runtime
        match self.detect_container_runtime().await {
            Ok(runtime) => status.container_runtime = runtime,
            Err(e) => status.errors.push(format!("Runtime detection: {}", e)),
        }

        // Update cache if enabled
        if let Some(ttl) = self.config.cache_ttl {
            let mut cache = self.cache.write().await;
            cache.availability = Some(CacheEntry::new(status.clone(), ttl));
        }

        Ok(status)
    }

    /// Clears all cached detection results.
    pub async fn clear_cache(&self) {
        let mut cache = self.cache.write().await;
        *cache = DetectionCache::default();
    }

    /// Validates that a minimum Dagger version is installed.
    ///
    /// # Arguments
    ///
    /// * `required_version` - Minimum version string (e.g., "0.18.0")
    ///
    /// # Returns
    ///
    /// Returns `Ok(DaggerInfo)` if a compatible version is found, or an
    /// appropriate error if not.
    pub async fn require_dagger_version(&self, required_version: &str) -> EngineResult<DaggerInfo> {
        let dagger = self.detect_dagger().await?.ok_or_else(|| {
            EngineError::dagger_not_found(self.build_dagger_search_paths())
        })?;

        if !dagger.meets_version(required_version) {
            return Err(EngineError::DaggerVersionIncompatible {
                installed: dagger.version.clone(),
                required: required_version.to_string(),
            });
        }

        Ok(dagger)
    }

    /// Requires both Dagger and a running container runtime.
    ///
    /// This is a convenience method that combines version checking with
    /// runtime availability checking.
    pub async fn require_local_environment(
        &self,
        required_dagger_version: Option<&str>,
    ) -> EngineResult<(DaggerInfo, ContainerRuntimeInfo)> {
        // Check Dagger
        let dagger = match required_dagger_version {
            Some(version) => self.require_dagger_version(version).await?,
            None => self
                .detect_dagger()
                .await?
                .ok_or_else(|| EngineError::dagger_not_found(self.build_dagger_search_paths()))?,
        };

        // Check container runtime
        let runtime = self
            .detect_container_runtime()
            .await?
            .ok_or(EngineError::NoContainerRuntime)?;

        if !runtime.is_running {
            return Err(EngineError::ContainerRuntimeNotRunning {
                runtime: runtime.runtime.to_string(),
                start_command: get_start_command(&runtime.runtime),
            });
        }

        Ok((dagger, runtime))
    }
}

impl Default for RuntimeDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Parses a version string into (major, minor, patch) components.
///
/// Handles version strings like "0.18.0", "v0.18.0", "0.18.0-dev", etc.
fn parse_version(version: &str) -> (u32, u32, u32) {
    let version = version.trim_start_matches('v');
    let parts: Vec<&str> = version.split(|c| c == '.' || c == '-').collect();

    let major = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);

    (major, minor, patch)
}

/// Gets the start command for a container runtime.
fn get_start_command(runtime: &ContainerRuntime) -> String {
    match runtime {
        ContainerRuntime::Docker => {
            if cfg!(target_os = "macos") {
                "open -a Docker".to_string()
            } else {
                "sudo systemctl start docker".to_string()
            }
        }
        ContainerRuntime::Podman => "podman machine start".to_string(),
        ContainerRuntime::Colima => "colima start".to_string(),
        ContainerRuntime::OrbStack => "open -a OrbStack".to_string(),
        ContainerRuntime::RancherDesktop => "open -a 'Rancher Desktop'".to_string(),
        ContainerRuntime::Auto => "start your container runtime".to_string(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // Version parsing tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_parse_version_simple() {
        assert_eq!(parse_version("0.18.0"), (0, 18, 0));
        assert_eq!(parse_version("1.2.3"), (1, 2, 3));
        assert_eq!(parse_version("10.20.30"), (10, 20, 30));
    }

    #[test]
    fn test_parse_version_with_v_prefix() {
        assert_eq!(parse_version("v0.18.0"), (0, 18, 0));
        assert_eq!(parse_version("v1.0.0"), (1, 0, 0));
    }

    #[test]
    fn test_parse_version_with_suffix() {
        assert_eq!(parse_version("0.18.0-dev"), (0, 18, 0));
        assert_eq!(parse_version("0.18.0-rc1"), (0, 18, 0));
        assert_eq!(parse_version("0.18.0-dirty"), (0, 18, 0));
    }

    #[test]
    fn test_parse_version_partial() {
        assert_eq!(parse_version("0.18"), (0, 18, 0));
        assert_eq!(parse_version("1"), (1, 0, 0));
        assert_eq!(parse_version(""), (0, 0, 0));
    }

    // ------------------------------------------------------------------------
    // DaggerInfo tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_dagger_info_new() {
        let info = DaggerInfo::new(PathBuf::from("/usr/bin/dagger"), "0.18.0".into());

        assert_eq!(info.path, PathBuf::from("/usr/bin/dagger"));
        assert_eq!(info.version, "0.18.0");
        assert_eq!(info.version_parts, (0, 18, 0));
        assert!(!info.is_dev_build);
    }

    #[test]
    fn test_dagger_info_dev_build() {
        let info = DaggerInfo::new(PathBuf::from("/usr/bin/dagger"), "0.18.0-dev".into());
        assert!(info.is_dev_build);

        let info = DaggerInfo::new(PathBuf::from("/usr/bin/dagger"), "0.18.0-dirty".into());
        assert!(info.is_dev_build);
    }

    #[test]
    fn test_dagger_info_meets_version() {
        let info = DaggerInfo::new(PathBuf::from("/usr/bin/dagger"), "0.18.0".into());

        assert!(info.meets_version("0.17.0"));
        assert!(info.meets_version("0.18.0"));
        assert!(!info.meets_version("0.19.0"));
        assert!(!info.meets_version("1.0.0"));
    }

    #[test]
    fn test_dagger_info_meets_version_major() {
        let info = DaggerInfo::new(PathBuf::from("/usr/bin/dagger"), "1.0.0".into());

        assert!(info.meets_version("0.99.99"));
        assert!(info.meets_version("1.0.0"));
        assert!(!info.meets_version("1.0.1"));
        assert!(!info.meets_version("2.0.0"));
    }

    // ------------------------------------------------------------------------
    // ContainerRuntimeInfo tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_container_runtime_info_new() {
        let info = ContainerRuntimeInfo::new(
            ContainerRuntime::Docker,
            PathBuf::from("/var/run/docker.sock"),
        );

        assert_eq!(info.runtime, ContainerRuntime::Docker);
        assert_eq!(info.socket_path, PathBuf::from("/var/run/docker.sock"));
        assert!(!info.is_running);
        assert!(!info.is_rootless);
    }

    #[test]
    fn test_container_runtime_info_podman_rootless() {
        let info = ContainerRuntimeInfo::new(
            ContainerRuntime::Podman,
            PathBuf::from("/run/user/1000/podman/podman.sock"),
        );

        assert!(info.is_rootless);
    }

    #[test]
    fn test_container_runtime_info_builder() {
        let info = ContainerRuntimeInfo::new(
            ContainerRuntime::Docker,
            PathBuf::from("/var/run/docker.sock"),
        )
        .with_version("24.0.0".into())
        .with_running(true)
        .with_rootless(false);

        assert_eq!(info.version, Some("24.0.0".into()));
        assert!(info.is_running);
        assert!(!info.is_rootless);
    }

    // ------------------------------------------------------------------------
    // AvailabilityStatus tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_availability_status_new() {
        let status = AvailabilityStatus::new();

        assert!(status.dagger.is_none());
        assert!(status.container_runtime.is_none());
        assert!(status.errors.is_empty());
        assert!(!status.is_ready());
    }

    #[test]
    fn test_availability_status_is_ready() {
        let mut status = AvailabilityStatus::new();

        // Not ready without Dagger
        assert!(!status.is_ready());

        // Add Dagger
        status.dagger = Some(DaggerInfo::new(
            PathBuf::from("/usr/bin/dagger"),
            "0.18.0".into(),
        ));

        // Still not ready without runtime
        assert!(!status.is_ready());

        // Add runtime (not running)
        status.container_runtime = Some(ContainerRuntimeInfo::new(
            ContainerRuntime::Docker,
            PathBuf::from("/var/run/docker.sock"),
        ));

        // Still not ready (runtime not running)
        assert!(!status.is_ready());

        // Make runtime running
        status.container_runtime = Some(
            ContainerRuntimeInfo::new(
                ContainerRuntime::Docker,
                PathBuf::from("/var/run/docker.sock"),
            )
            .with_running(true),
        );

        // Now ready
        assert!(status.is_ready());
    }

    #[test]
    fn test_availability_status_has_dagger() {
        let mut status = AvailabilityStatus::new();
        assert!(!status.has_dagger());

        status.dagger = Some(DaggerInfo::new(
            PathBuf::from("/usr/bin/dagger"),
            "0.18.0".into(),
        ));
        assert!(status.has_dagger());
    }

    #[test]
    fn test_availability_status_has_container_runtime() {
        let mut status = AvailabilityStatus::new();
        assert!(!status.has_container_runtime());

        status.container_runtime = Some(ContainerRuntimeInfo::new(
            ContainerRuntime::Docker,
            PathBuf::from("/var/run/docker.sock"),
        ));
        assert!(status.has_container_runtime());
    }

    #[test]
    fn test_availability_status_summary() {
        let mut status = AvailabilityStatus::new();

        let summary = status.summary();
        assert!(summary.contains("not found"));

        status.dagger = Some(DaggerInfo::new(
            PathBuf::from("/usr/bin/dagger"),
            "0.18.0".into(),
        ));
        status.container_runtime = Some(
            ContainerRuntimeInfo::new(
                ContainerRuntime::Docker,
                PathBuf::from("/var/run/docker.sock"),
            )
            .with_running(true),
        );

        let summary = status.summary();
        assert!(summary.contains("0.18.0"));
        assert!(summary.contains("running"));
    }

    // ------------------------------------------------------------------------
    // DetectorConfig tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_detector_config_default() {
        let config = DetectorConfig::default();

        assert!(config.dagger_search_paths.is_empty());
        assert!(config.preferred_runtime.is_none());
        assert_eq!(config.command_timeout, Duration::from_secs(10));
        assert!(config.cache_ttl.is_none());
    }

    #[test]
    fn test_detector_config_builder() {
        let config = DetectorConfig::new()
            .with_dagger_search_path(PathBuf::from("/custom/bin"))
            .with_preferred_runtime(ContainerRuntime::Podman)
            .with_command_timeout(Duration::from_secs(30))
            .with_cache_ttl(Duration::from_secs(60))
            .with_docker_host("unix:///custom/docker.sock".into());

        assert_eq!(config.dagger_search_paths, vec![PathBuf::from("/custom/bin")]);
        assert_eq!(config.preferred_runtime, Some(ContainerRuntime::Podman));
        assert_eq!(config.command_timeout, Duration::from_secs(30));
        assert_eq!(config.cache_ttl, Some(Duration::from_secs(60)));
        assert_eq!(
            config.docker_host,
            Some("unix:///custom/docker.sock".into())
        );
    }

    // ------------------------------------------------------------------------
    // RuntimeDetector tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_runtime_detector_new() {
        let detector = RuntimeDetector::new();
        // Just ensure it can be created
        assert!(detector.config.dagger_search_paths.is_empty());
    }

    #[test]
    fn test_runtime_detector_with_config() {
        let config = DetectorConfig::new()
            .with_cache_ttl(Duration::from_secs(120));
        let detector = RuntimeDetector::with_config(config);

        assert_eq!(detector.config.cache_ttl, Some(Duration::from_secs(120)));
    }

    #[test]
    fn test_build_dagger_search_paths() {
        let detector = RuntimeDetector::new();
        let paths = detector.build_dagger_search_paths();

        // Should include well-known paths
        assert!(paths.iter().any(|p| p == Path::new("/usr/local/bin")));
        assert!(paths.iter().any(|p| p == Path::new("/usr/bin")));
    }

    #[test]
    fn test_build_dagger_search_paths_with_custom() {
        let config = DetectorConfig::new()
            .with_dagger_search_path(PathBuf::from("/my/custom/path"));
        let detector = RuntimeDetector::with_config(config);
        let paths = detector.build_dagger_search_paths();

        // Custom path should be first
        assert_eq!(paths.first(), Some(&PathBuf::from("/my/custom/path")));
    }

    #[test]
    fn test_get_socket_paths_docker() {
        let detector = RuntimeDetector::new();
        let paths = detector.get_socket_paths(ContainerRuntime::Docker);

        assert!(paths.iter().any(|p| p == Path::new("/var/run/docker.sock")));
    }

    #[test]
    fn test_get_socket_paths_podman() {
        let detector = RuntimeDetector::new();
        let paths = detector.get_socket_paths(ContainerRuntime::Podman);

        // Should include rootless paths
        assert!(paths.iter().any(|p| p.to_string_lossy().contains("podman")));
    }

    // ------------------------------------------------------------------------
    // Cache tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_cache_entry_validity() {
        let entry = CacheEntry::new("test", Duration::from_millis(100));
        assert!(entry.is_valid());

        // Simulate time passing
        std::thread::sleep(Duration::from_millis(150));
        assert!(!entry.is_valid());
    }

    #[tokio::test]
    async fn test_detector_clear_cache() {
        let config = DetectorConfig::new().with_cache_ttl(Duration::from_secs(60));
        let detector = RuntimeDetector::with_config(config);

        // Add something to cache (via detection attempt)
        let _ = detector.detect_dagger().await;

        // Clear cache
        detector.clear_cache().await;

        // Cache should be empty
        let cache = detector.cache.read().await;
        assert!(cache.dagger.is_none());
        assert!(cache.runtimes.is_empty());
        assert!(cache.availability.is_none());
    }

    // ------------------------------------------------------------------------
    // get_start_command tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_get_start_command() {
        assert!(get_start_command(&ContainerRuntime::Podman).contains("podman"));
        assert!(get_start_command(&ContainerRuntime::Colima).contains("colima"));
    }
}
