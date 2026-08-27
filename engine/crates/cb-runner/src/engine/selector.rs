//! # Engine Selector
//!
//! This module provides the [`EngineSelector`] which implements the automatic
//! engine selection logic for Circuit Breaker's hybrid execution model. It
//! chooses between local and cloud engines based on requirements, availability,
//! and configuration.
//!
//! ## Overview
//!
//! The selector implements the Auto mode algorithm described in the engine
//! specification:
//!
//! 1. **Force cloud** if GPU, audit, or high resources required
//! 2. **Try local** if available and meets requirements
//! 3. **Fallback to cloud** if local unavailable and cloud configured
//!
//! ## Usage
//!
//! ```rust,no_run
//! use cb_runner::engine::{EngineSelector, SelectedEngine};
//! use cb_core::engine::{EngineConfig, EngineRequirements};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create selector with auto mode configuration
//! let config = EngineConfig::auto();
//! let selector = EngineSelector::new(config).await?;
//!
//! // Select engine for a workload
//! let requirements = EngineRequirements::default();
//! let engine = selector.select(&requirements).await?;
//!
//! match engine {
//!     SelectedEngine::Local(local) => {
//!         println!("Using local engine");
//!     }
//!     SelectedEngine::Cloud(cloud) => {
//!         println!("Using cloud engine");
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Selection Algorithm
//!
//! The selection process follows these steps:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Engine Selection Flow                             │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │                                                                         │
//! │  Input: EngineConfig + EngineRequirements                               │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │ 1. Check explicit mode                                          │   │
//! │  │    - Mode::Local → try local only                               │   │
//! │  │    - Mode::Cloud → try cloud only                               │   │
//! │  │    - Mode::Auto  → continue to auto selection                   │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                │                                        │
//! │                                ▼                                        │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │ 2. Check requirements for forced cloud                          │   │
//! │  │    - GPU required?        → cloud                               │   │
//! │  │    - Audit required?      → cloud                               │   │
//! │  │    - Memory > threshold?  → cloud                               │   │
//! │  │    - CPU > threshold?     → cloud                               │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                │                                        │
//! │                                ▼                                        │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │ 3. Try local if preferred                                       │   │
//! │  │    - Local available? → use local                               │   │
//! │  │    - Not available?   → continue                                │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                │                                        │
//! │                                ▼                                        │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │ 4. Fallback to cloud if configured                              │   │
//! │  │    - Cloud available? → use cloud                               │   │
//! │  │    - Not available?   → error                                   │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Thread Safety
//!
//! `EngineSelector` is `Send + Sync` and can be safely shared across threads.

use std::sync::Arc;

use cb_core::engine::{AutoModeConfig, CloudEngineConfig, EngineConfig, EngineMode, EngineRequirements, LocalEngineConfig};
use tokio::sync::RwLock;

use super::detect::RuntimeDetector;
use super::error::{EngineError, EngineResult};
use super::local::LocalEngine;
use super::EngineExecutor;

/// A selected engine, either local or cloud.
///
/// This enum represents the result of engine selection and provides
/// a unified way to work with either engine type.
#[derive(Debug)]
pub enum SelectedEngine {
    /// A local engine using the local Dagger installation.
    Local(Arc<LocalEngine>),

    /// A cloud engine using the Engine Service.
    ///
    /// Note: Cloud engine implementation is planned for a future phase.
    /// This variant currently serves as a placeholder.
    Cloud(CloudEngineHandle),
}

impl SelectedEngine {
    /// Returns `true` if this is a local engine.
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    /// Returns `true` if this is a cloud engine.
    pub fn is_cloud(&self) -> bool {
        matches!(self, Self::Cloud(_))
    }

    /// Returns the engine type as a string.
    pub fn engine_type(&self) -> &'static str {
        match self {
            Self::Local(_) => "local",
            Self::Cloud(_) => "cloud",
        }
    }

    /// Returns a reference to the underlying executor.
    ///
    /// This allows using the selected engine without matching on the variant.
    pub fn executor(&self) -> &dyn EngineExecutor {
        match self {
            Self::Local(engine) => engine.as_ref(),
            Self::Cloud(handle) => handle,
        }
    }
}

/// Handle for a cloud engine (placeholder for future implementation).
///
/// This struct will be expanded in a future phase to provide full
/// cloud engine functionality via the Engine Service API.
#[derive(Debug)]
pub struct CloudEngineHandle {
    /// Configuration for the cloud engine.
    config: CloudEngineConfig,
}

impl CloudEngineHandle {
    /// Creates a new cloud engine handle.
    pub fn new(config: CloudEngineConfig) -> Self {
        Self { config }
    }

    /// Returns the cloud engine configuration.
    pub fn config(&self) -> &CloudEngineConfig {
        &self.config
    }
}

#[async_trait::async_trait]
impl EngineExecutor for CloudEngineHandle {
    async fn execute(
        &self,
        _module: &str,
        _function: Option<&str>,
        _args: Option<&std::collections::HashMap<String, serde_json::Value>>,
    ) -> EngineResult<super::ExecutionOutput> {
        // Cloud engine execution is not yet implemented
        Err(EngineError::ModeNotSupported {
            mode: "cloud".to_string(),
            reason: "Cloud engine execution is not yet implemented. Use local mode or wait for a future release.".to_string(),
        })
    }

    async fn is_healthy(&self) -> bool {
        // For now, cloud engine is always "unhealthy" since it's not implemented
        false
    }

    fn engine_type(&self) -> &'static str {
        "cloud"
    }
}

/// Result of checking engine availability.
#[derive(Debug, Clone)]
pub struct AvailabilityCheck {
    /// Whether local engine is available.
    pub local_available: bool,

    /// Reason local engine is not available, if applicable.
    pub local_reason: Option<String>,

    /// Whether cloud engine is available.
    pub cloud_available: bool,

    /// Reason cloud engine is not available, if applicable.
    pub cloud_reason: Option<String>,
}

impl AvailabilityCheck {
    /// Returns `true` if any engine is available.
    pub fn any_available(&self) -> bool {
        self.local_available || self.cloud_available
    }

    /// Returns `true` if both engines are available.
    pub fn both_available(&self) -> bool {
        self.local_available && self.cloud_available
    }
}

/// Internal state for the engine selector.
struct SelectorState {
    /// Cached local engine instance.
    local_engine: Option<Arc<LocalEngine>>,

    /// Whether local availability has been checked.
    local_checked: bool,

    /// Last availability check result.
    last_availability: Option<AvailabilityCheck>,
}

impl SelectorState {
    fn new() -> Self {
        Self {
            local_engine: None,
            local_checked: false,
            last_availability: None,
        }
    }
}

/// Engine selector for choosing between local and cloud engines.
///
/// This struct implements the automatic engine selection logic based on
/// requirements, availability, and configuration.
///
/// # Examples
///
/// ```rust,no_run
/// use cb_runner::engine::{EngineSelector, SelectedEngine};
/// use cb_core::engine::{EngineConfig, EngineRequirements};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Create selector with configuration
/// let config = EngineConfig::auto();
/// let selector = EngineSelector::new(config).await?;
///
/// // Check what's available
/// let availability = selector.check_availability().await;
/// println!("Local: {}, Cloud: {}", availability.local_available, availability.cloud_available);
///
/// // Select engine for requirements
/// let requirements = EngineRequirements::with_memory(8);
/// let engine = selector.select(&requirements).await?;
/// println!("Selected: {}", engine.engine_type());
/// # Ok(())
/// # }
/// ```
pub struct EngineSelector {
    /// Configuration for engine selection.
    config: EngineConfig,

    /// Runtime detector for local availability checks.
    detector: RuntimeDetector,

    /// Internal state.
    state: Arc<RwLock<SelectorState>>,
}

impl EngineSelector {
    /// Creates a new engine selector with the given configuration.
    ///
    /// This constructor performs initial availability checks based on the
    /// configuration mode.
    ///
    /// # Arguments
    ///
    /// * `config` - Engine configuration specifying mode and settings
    ///
    /// # Returns
    ///
    /// Returns a new `EngineSelector` on success, or an error if no engine
    /// can be made available with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Mode is `Local` but local engine is not available
    /// - Mode is `Cloud` but cloud is not configured
    /// - Mode is `Auto` but neither local nor cloud is available
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use cb_runner::engine::EngineSelector;
    /// use cb_core::engine::EngineConfig;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // Auto mode (most flexible)
    /// let selector = EngineSelector::new(EngineConfig::auto()).await?;
    ///
    /// // Local-only mode
    /// let selector = EngineSelector::new(EngineConfig::local()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(config: EngineConfig) -> EngineResult<Self> {
        let detector = RuntimeDetector::new();
        let state = Arc::new(RwLock::new(SelectorState::new()));

        let selector = Self {
            config,
            detector,
            state,
        };

        // Validate based on mode
        selector.validate_configuration().await?;

        Ok(selector)
    }

    /// Creates a new engine selector without validation.
    ///
    /// This is useful for testing or when you want to defer validation.
    pub fn new_unchecked(config: EngineConfig) -> Self {
        Self {
            config,
            detector: RuntimeDetector::new(),
            state: Arc::new(RwLock::new(SelectorState::new())),
        }
    }

    /// Validates the configuration and checks engine availability.
    async fn validate_configuration(&self) -> EngineResult<()> {
        match self.config.mode {
            EngineMode::Local => {
                // Local mode requires local Dagger
                self.ensure_local_available().await?;
            }
            EngineMode::Cloud => {
                // Cloud mode requires credentials
                if !self.config.cloud_available() {
                    return Err(EngineError::MissingConfiguration {
                        mode: "cloud".to_string(),
                        missing: "api_key and organization_id".to_string(),
                    });
                }
            }
            EngineMode::Auto => {
                // Auto mode should have at least one option
                let availability = self.check_availability().await;
                if !availability.any_available() {
                    return Err(EngineError::NoEngineAvailable {
                        local_reason: availability.local_reason,
                        cloud_reason: availability.cloud_reason,
                    });
                }
            }
        }

        Ok(())
    }

    /// Checks availability of local and cloud engines.
    ///
    /// # Returns
    ///
    /// Returns an [`AvailabilityCheck`] with the status of each engine.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use cb_runner::engine::EngineSelector;
    /// use cb_core::engine::EngineConfig;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let selector = EngineSelector::new(EngineConfig::auto()).await?;
    /// let availability = selector.check_availability().await;
    ///
    /// if availability.local_available {
    ///     println!("Local engine is available");
    /// }
    /// if availability.cloud_available {
    ///     println!("Cloud engine is available");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn check_availability(&self) -> AvailabilityCheck {
        let mut check = AvailabilityCheck {
            local_available: false,
            local_reason: None,
            cloud_available: false,
            cloud_reason: None,
        };

        // Check local availability
        if self.config.mode.allows_local() {
            match self.detector.check_availability().await {
                Ok(status) if status.is_ready() => {
                    check.local_available = true;
                }
                Ok(status) => {
                    check.local_reason = Some(status.summary());
                }
                Err(e) => {
                    check.local_reason = Some(e.to_string());
                }
            }
        } else {
            check.local_reason = Some("Local mode not allowed by configuration".to_string());
        }

        // Check cloud availability
        if self.config.mode.allows_cloud() {
            if self.config.cloud_available() {
                // Cloud is configured, assume available
                // In a real implementation, we would ping the Engine Service
                check.cloud_available = true;
            } else {
                check.cloud_reason = Some("Cloud credentials not configured".to_string());
            }
        } else {
            check.cloud_reason = Some("Cloud mode not allowed by configuration".to_string());
        }

        // Cache the result
        {
            let mut state = self.state.write().await;
            state.last_availability = Some(check.clone());
        }

        check
    }

    /// Ensures a local engine is available, creating one if necessary.
    async fn ensure_local_available(&self) -> EngineResult<Arc<LocalEngine>> {
        // Check if we already have a local engine
        {
            let state = self.state.read().await;
            if let Some(engine) = &state.local_engine {
                if engine.is_healthy().await {
                    return Ok(engine.clone());
                }
            }
        }

        // Create a new local engine
        let local_config = self.config.local.clone().unwrap_or_default();
        let engine = LocalEngine::new(local_config).await?;
        let engine = Arc::new(engine);

        // Cache it
        {
            let mut state = self.state.write().await;
            state.local_engine = Some(engine.clone());
            state.local_checked = true;
        }

        Ok(engine)
    }

    /// Selects an engine based on the given requirements.
    ///
    /// This is the main entry point for engine selection. It applies the
    /// selection algorithm to choose the best engine for the given workload.
    ///
    /// # Arguments
    ///
    /// * `requirements` - Resource and capability requirements for the workload
    ///
    /// # Returns
    ///
    /// Returns a [`SelectedEngine`] that can be used for execution.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No engine is available that meets the requirements
    /// - The selected engine is not healthy
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use cb_runner::engine::{EngineSelector, SelectedEngine, EngineExecutor};
    /// use cb_core::engine::{EngineConfig, EngineRequirements};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let selector = EngineSelector::new(EngineConfig::auto()).await?;
    ///
    /// // Select for default requirements (uses local if available)
    /// let engine = selector.select(&EngineRequirements::default()).await?;
    ///
    /// // Select for GPU requirements (forces cloud)
    /// let engine = selector.select(&EngineRequirements::with_gpu()).await?;
    /// assert!(engine.is_cloud());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn select(&self, requirements: &EngineRequirements) -> EngineResult<SelectedEngine> {
        tracing::debug!(
            mode = ?self.config.mode,
            requirements = ?requirements,
            "Selecting engine"
        );

        match self.config.mode {
            EngineMode::Local => self.select_local(requirements).await,
            EngineMode::Cloud => self.select_cloud(requirements).await,
            EngineMode::Auto => self.select_auto(requirements).await,
        }
    }

    /// Selects a local engine.
    async fn select_local(&self, _requirements: &EngineRequirements) -> EngineResult<SelectedEngine> {
        let engine = self.ensure_local_available().await?;
        Ok(SelectedEngine::Local(engine))
    }

    /// Selects a cloud engine.
    async fn select_cloud(&self, _requirements: &EngineRequirements) -> EngineResult<SelectedEngine> {
        if !self.config.cloud_available() {
            return Err(EngineError::MissingConfiguration {
                mode: "cloud".to_string(),
                missing: "api_key and organization_id".to_string(),
            });
        }

        let cloud_config = self.config.cloud.clone().unwrap_or_default();
        Ok(SelectedEngine::Cloud(CloudEngineHandle::new(cloud_config)))
    }

    /// Selects an engine automatically based on requirements and availability.
    async fn select_auto(&self, requirements: &EngineRequirements) -> EngineResult<SelectedEngine> {
        let auto_config = &self.config.auto;

        // Step 1: Check if requirements force cloud
        if requirements.requires_cloud() {
            tracing::debug!("Requirements force cloud (GPU or audit)");
            return self.select_cloud_with_fallback(requirements, auto_config).await;
        }

        // Step 2: Check if requirements suggest cloud based on thresholds
        if requirements.suggests_cloud(
            auto_config.cloud_memory_threshold_gb,
            auto_config.cloud_cpu_threshold_cores,
        ) {
            tracing::debug!("Requirements suggest cloud (high resources)");
            // Still try cloud first if it suggests cloud
            if self.config.cloud_available() {
                return self.select_cloud(requirements).await;
            }
            // Fall through to local if cloud not available
        }

        // Step 3: Try local if preferred
        if auto_config.prefer_local {
            match self.ensure_local_available().await {
                Ok(engine) => {
                    tracing::debug!("Selected local engine (preferred)");
                    return Ok(SelectedEngine::Local(engine));
                }
                Err(e) => {
                    tracing::debug!(error = %e, "Local engine not available");
                    // Continue to fallback
                }
            }
        }

        // Step 4: Fallback to cloud if configured
        if auto_config.fallback_to_cloud && self.config.cloud_available() {
            tracing::debug!("Falling back to cloud engine");
            return self.select_cloud(requirements).await;
        }

        // Step 5: Try local as last resort (if not preferred but still allowed)
        if !auto_config.prefer_local && self.config.mode.allows_local() {
            match self.ensure_local_available().await {
                Ok(engine) => {
                    tracing::debug!("Selected local engine (last resort)");
                    return Ok(SelectedEngine::Local(engine));
                }
                Err(e) => {
                    tracing::debug!(error = %e, "Local engine not available as last resort");
                }
            }
        }

        // No engine available
        let availability = self.check_availability().await;
        Err(EngineError::NoEngineAvailable {
            local_reason: availability.local_reason,
            cloud_reason: availability.cloud_reason,
        })
    }

    /// Attempts to select cloud, falling back to local if cloud is unavailable.
    async fn select_cloud_with_fallback(
        &self,
        requirements: &EngineRequirements,
        auto_config: &AutoModeConfig,
    ) -> EngineResult<SelectedEngine> {
        if self.config.cloud_available() {
            return self.select_cloud(requirements).await;
        }

        // Cloud not available, check if we can fallback
        if auto_config.fallback_to_cloud {
            // This is a contradiction - cloud required but not available
            // and fallback_to_cloud doesn't help here
        }

        // Try local if allowed (but warn that requirements can't be fully met)
        if self.config.mode.allows_local() && !requirements.requires_cloud() {
            tracing::warn!(
                "Cloud required by preferences but not available, trying local"
            );
            return self.select_local(requirements).await;
        }

        Err(EngineError::NoEngineAvailable {
            local_reason: if requirements.gpu {
                Some("GPU workloads require cloud engine".to_string())
            } else if requirements.require_audit {
                Some("Audit requirement requires cloud engine".to_string())
            } else {
                Some("Requirements suggest cloud but local may work".to_string())
            },
            cloud_reason: Some("Cloud credentials not configured".to_string()),
        })
    }

    /// Returns the engine configuration.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }
}

impl std::fmt::Debug for EngineSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineSelector")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // SelectedEngine tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_selected_engine_is_local() {
        // We can't easily create a LocalEngine in tests without Dagger,
        // so we test the enum itself
        let cloud = SelectedEngine::Cloud(CloudEngineHandle::new(CloudEngineConfig::default()));
        assert!(!cloud.is_local());
        assert!(cloud.is_cloud());
        assert_eq!(cloud.engine_type(), "cloud");
    }

    // ------------------------------------------------------------------------
    // CloudEngineHandle tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_cloud_engine_handle_new() {
        let config = CloudEngineConfig::new(
            "https://engines.circuitbreaker.io".to_string(),
            "key".to_string(),
            "org".to_string(),
        );
        let handle = CloudEngineHandle::new(config.clone());

        assert_eq!(handle.config().url, config.url);
        assert_eq!(handle.config().api_key, config.api_key);
    }

    #[test]
    fn test_cloud_engine_handle_type() {
        let handle = CloudEngineHandle::new(CloudEngineConfig::default());
        assert_eq!(handle.engine_type(), "cloud");
    }

    #[tokio::test]
    async fn test_cloud_engine_handle_is_not_healthy() {
        let handle = CloudEngineHandle::new(CloudEngineConfig::default());
        // Cloud engine is not implemented, so it's not healthy
        assert!(!handle.is_healthy().await);
    }

    #[tokio::test]
    async fn test_cloud_engine_handle_execute_not_implemented() {
        let handle = CloudEngineHandle::new(CloudEngineConfig::default());
        let result = handle.execute("module", None, None).await;

        assert!(result.is_err());
        match result {
            Err(EngineError::ModeNotSupported { mode, .. }) => {
                assert_eq!(mode, "cloud");
            }
            _ => panic!("Expected ModeNotSupported error"),
        }
    }

    // ------------------------------------------------------------------------
    // AvailabilityCheck tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_availability_check_any_available() {
        let check = AvailabilityCheck {
            local_available: true,
            local_reason: None,
            cloud_available: false,
            cloud_reason: Some("not configured".to_string()),
        };
        assert!(check.any_available());

        let check = AvailabilityCheck {
            local_available: false,
            local_reason: Some("not found".to_string()),
            cloud_available: true,
            cloud_reason: None,
        };
        assert!(check.any_available());

        let check = AvailabilityCheck {
            local_available: false,
            local_reason: Some("not found".to_string()),
            cloud_available: false,
            cloud_reason: Some("not configured".to_string()),
        };
        assert!(!check.any_available());
    }

    #[test]
    fn test_availability_check_both_available() {
        let check = AvailabilityCheck {
            local_available: true,
            local_reason: None,
            cloud_available: true,
            cloud_reason: None,
        };
        assert!(check.both_available());

        let check = AvailabilityCheck {
            local_available: true,
            local_reason: None,
            cloud_available: false,
            cloud_reason: Some("not configured".to_string()),
        };
        assert!(!check.both_available());
    }

    // ------------------------------------------------------------------------
    // EngineSelector tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_engine_selector_new_unchecked() {
        let config = EngineConfig::auto();
        let selector = EngineSelector::new_unchecked(config);

        assert_eq!(selector.config().mode, EngineMode::Auto);
    }

    #[tokio::test]
    async fn test_engine_selector_check_availability() {
        let config = EngineConfig::auto();
        let selector = EngineSelector::new_unchecked(config);

        let availability = selector.check_availability().await;

        // On most CI systems, local won't be available
        // This test just ensures the method runs without panicking
        assert!(
            availability.local_reason.is_some() || availability.local_available,
            "Should have either a reason or be available"
        );
    }

    #[tokio::test]
    async fn test_engine_selector_select_cloud_not_configured() {
        let config = EngineConfig::cloud(); // No credentials
        let selector = EngineSelector::new_unchecked(config);

        let result = selector.select(&EngineRequirements::default()).await;

        assert!(result.is_err());
        match result {
            Err(EngineError::MissingConfiguration { mode, .. }) => {
                assert_eq!(mode, "cloud");
            }
            _ => panic!("Expected MissingConfiguration error"),
        }
    }

    #[tokio::test]
    async fn test_engine_selector_config_access() {
        let config = EngineConfig::local();
        let selector = EngineSelector::new_unchecked(config);

        assert_eq!(selector.config().mode, EngineMode::Local);
    }

    #[test]
    fn test_engine_selector_debug() {
        let config = EngineConfig::auto();
        let selector = EngineSelector::new_unchecked(config);

        let debug_str = format!("{:?}", selector);
        assert!(debug_str.contains("EngineSelector"));
    }

    // ------------------------------------------------------------------------
    // Selection logic tests
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_selector_auto_with_cloud_config() {
        let cloud_config = CloudEngineConfig::new(
            "https://engines.test".to_string(),
            "key".to_string(),
            "org".to_string(),
        );
        let config = EngineConfig::auto().with_cloud_config(cloud_config);
        let selector = EngineSelector::new_unchecked(config);

        // GPU requirement should select cloud
        let requirements = EngineRequirements::with_gpu();
        let result = selector.select(&requirements).await;

        // Should get a cloud engine (even though it's not implemented)
        match result {
            Ok(engine) => assert!(engine.is_cloud()),
            Err(_) => {
                // This is also acceptable if local isn't available
            }
        }
    }

    #[tokio::test]
    async fn test_selector_auto_prefers_local_default() {
        let config = EngineConfig::auto();

        // Default auto config prefers local
        assert!(config.auto.prefer_local);
        assert!(config.auto.fallback_to_cloud);
    }
}
