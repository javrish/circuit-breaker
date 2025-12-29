//! Configuration module for webhook endpoints.
//!
//! This module provides types for loading and managing webhook endpoint
//! configurations from files or Kubernetes Custom Resources.
//!
//! ## Configuration Format
//!
//! Webhook endpoints can be configured via YAML files:
//!
//! ```yaml
//! apiVersion: circuitbreaker.io/v1
//! kind: WebhookEndpoint
//! metadata:
//!   name: github-webhook
//!   namespace: production
//! spec:
//!   path: /webhooks/github
//!   auth:
//!     type: hmac-sha256
//!     secretRef:
//!       name: github-webhook-secret
//!       key: secret
//!   triggers:
//!     - event: push
//!       filter: "ref == 'refs/heads/main'"
//!       workflow: build-and-deploy
//!       inputs:
//!         repository: "{{ .repository.full_name }}"
//!         commit: "{{ .head_commit.id }}"
//! ```

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::auth::{AuthConfig, AuthMethod, SecretRef};
use crate::error::{Result, WebhookError};
use crate::{RateLimitConfig, TriggerMapping, WebhookEndpoint};

/// Top-level webhook configuration containing multiple endpoints.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// API version for the configuration.
    #[serde(rename = "apiVersion", default = "default_api_version")]
    pub api_version: String,

    /// Kind of resource.
    #[serde(default = "default_kind")]
    pub kind: String,

    /// List of webhook endpoints.
    #[serde(default)]
    pub endpoints: Vec<EndpointConfig>,
}

fn default_api_version() -> String {
    "circuitbreaker.io/v1".to_string()
}

fn default_kind() -> String {
    "WebhookConfig".to_string()
}

/// Configuration for a single webhook endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointConfig {
    /// API version for the configuration.
    #[serde(rename = "apiVersion", default = "default_api_version")]
    pub api_version: String,

    /// Kind of resource (WebhookEndpoint).
    #[serde(default = "default_endpoint_kind")]
    pub kind: String,

    /// Metadata for the endpoint.
    pub metadata: EndpointMetadata,

    /// Endpoint specification.
    pub spec: EndpointSpec,
}

fn default_endpoint_kind() -> String {
    "WebhookEndpoint".to_string()
}

/// Metadata for a webhook endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointMetadata {
    /// Unique name for the endpoint.
    pub name: String,

    /// Namespace for the endpoint.
    #[serde(default = "default_namespace")]
    pub namespace: String,

    /// Labels for the endpoint.
    #[serde(default)]
    pub labels: HashMap<String, String>,

    /// Annotations for the endpoint.
    #[serde(default)]
    pub annotations: HashMap<String, String>,
}

fn default_namespace() -> String {
    "default".to_string()
}

/// Specification for a webhook endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointSpec {
    /// URL path for this endpoint.
    #[serde(default)]
    pub path: Option<String>,

    /// Source type (github, gitlab, dockerhub, generic).
    #[serde(default = "default_source_type")]
    pub source_type: String,

    /// Authentication configuration.
    #[serde(default)]
    pub auth: Option<AuthSpec>,

    /// Trigger mappings.
    #[serde(default)]
    pub triggers: Vec<TriggerSpec>,

    /// Maximum payload size in bytes.
    #[serde(rename = "maxPayloadBytes", default = "default_max_payload")]
    pub max_payload_bytes: usize,

    /// Rate limit configuration.
    #[serde(rename = "rateLimit", default)]
    pub rate_limit: Option<RateLimitSpec>,

    /// IP allowlist.
    #[serde(rename = "ipAllowlist", default)]
    pub ip_allowlist: Vec<String>,

    /// Whether the endpoint is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Validation configuration.
    #[serde(default)]
    pub validation: Option<ValidationSpec>,
}

fn default_source_type() -> String {
    "generic".to_string()
}

fn default_max_payload() -> usize {
    1_048_576 // 1MB
}

fn default_enabled() -> bool {
    true
}

/// Authentication specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSpec {
    /// Authentication type.
    #[serde(rename = "type")]
    pub auth_type: String,

    /// Inline secret value (not recommended for production).
    #[serde(default)]
    pub secret: Option<String>,

    /// Reference to a Kubernetes secret.
    #[serde(rename = "secretRef", default)]
    pub secret_ref: Option<SecretRefSpec>,

    /// Header name for the signature/token.
    #[serde(rename = "headerName", default)]
    pub header_name: Option<String>,

    /// Signature prefix to strip.
    #[serde(rename = "signaturePrefix", default)]
    pub signature_prefix: Option<String>,
}

/// Reference to a Kubernetes secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretRefSpec {
    /// Secret name.
    pub name: String,

    /// Key within the secret.
    pub key: String,

    /// Namespace (defaults to endpoint namespace).
    #[serde(default)]
    pub namespace: Option<String>,
}

/// Trigger specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerSpec {
    /// Event type to match.
    pub event: String,

    /// Optional CEL filter expression.
    #[serde(default)]
    pub filter: Option<FilterSpec>,

    /// Workflow to trigger.
    pub workflow: String,

    /// Workflow namespace.
    #[serde(rename = "workflowNamespace", default)]
    pub workflow_namespace: Option<String>,

    /// Input mappings.
    #[serde(default)]
    pub inputs: HashMap<String, String>,
}

/// Filter specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterSpec {
    /// Simple string filter (CEL expression).
    Expression(String),

    /// Structured filter with field matchers.
    Structured(StructuredFilter),
}

/// Structured filter with field matchers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredFilter {
    /// Field to match.
    #[serde(flatten)]
    pub fields: HashMap<String, serde_json::Value>,
}

/// Rate limit specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitSpec {
    /// Maximum requests allowed.
    pub requests: u32,

    /// Time period (e.g., "1m", "1h").
    pub period: String,

    /// Key for rate limiting.
    #[serde(default = "default_rate_key")]
    pub key: String,
}

fn default_rate_key() -> String {
    "source.ip".to_string()
}

/// Validation specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSpec {
    /// JSON Schema reference.
    #[serde(default)]
    pub schema: Option<SchemaRef>,

    /// Required fields.
    #[serde(default)]
    pub required: Vec<String>,

    /// Content size limit.
    #[serde(rename = "maxPayloadBytes", default)]
    pub max_payload_bytes: Option<usize>,
}

/// JSON Schema reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRef {
    /// Schema reference (e.g., "#/definitions/GitHubPushEvent").
    #[serde(rename = "$ref", default)]
    pub reference: Option<String>,

    /// Inline schema.
    #[serde(flatten)]
    pub inline: Option<serde_json::Value>,
}

impl WebhookConfig {
    /// Load configuration from a YAML file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            WebhookError::Config(format!(
                "Failed to read config file '{}': {}",
                path.as_ref().display(),
                e
            ))
        })?;

        Self::from_yaml(&content)
    }

    /// Load configuration from a YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        // Try to parse as a list of endpoints first
        if let Ok(endpoints) = serde_yaml::from_str::<Vec<EndpointConfig>>(yaml) {
            return Ok(Self {
                api_version: default_api_version(),
                kind: default_kind(),
                endpoints,
            });
        }

        // Try to parse as a single endpoint
        if let Ok(endpoint) = serde_yaml::from_str::<EndpointConfig>(yaml) {
            return Ok(Self {
                api_version: default_api_version(),
                kind: default_kind(),
                endpoints: vec![endpoint],
            });
        }

        // Try to parse as the full config structure
        serde_yaml::from_str(yaml)
            .map_err(|e| WebhookError::Config(format!("Failed to parse YAML: {}", e)))
    }

    /// Load configuration from a directory of YAML files.
    pub fn from_directory(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut endpoints = Vec::new();

        for entry in std::fs::read_dir(dir).map_err(|e| {
            WebhookError::Config(format!(
                "Failed to read config directory '{}': {}",
                dir.display(),
                e
            ))
        })? {
            let entry = entry.map_err(|e| {
                WebhookError::Config(format!("Failed to read directory entry: {}", e))
            })?;
            let path = entry.path();

            if path
                .extension()
                .map_or(false, |ext| ext == "yaml" || ext == "yml")
            {
                let config = Self::from_file(&path)?;
                endpoints.extend(config.endpoints);
            }
        }

        Ok(Self {
            api_version: default_api_version(),
            kind: default_kind(),
            endpoints,
        })
    }

    /// Convert to a list of WebhookEndpoint instances.
    pub fn to_endpoints(&self) -> Result<Vec<WebhookEndpoint>> {
        self.endpoints
            .iter()
            .map(|e| e.to_webhook_endpoint())
            .collect()
    }
}

impl EndpointConfig {
    /// Convert to a WebhookEndpoint instance.
    pub fn to_webhook_endpoint(&self) -> Result<WebhookEndpoint> {
        let now = Utc::now();

        // Generate path if not specified
        let path = self
            .spec
            .path
            .clone()
            .unwrap_or_else(|| format!("/webhooks/{}", self.metadata.name));

        // Convert auth spec
        let auth = self.spec.auth.as_ref().map(|a| a.to_auth_config());

        // Convert trigger specs
        let triggers: Vec<TriggerMapping> = self
            .spec
            .triggers
            .iter()
            .map(|t| t.to_trigger_mapping(&self.metadata.namespace))
            .collect();

        // Convert rate limit spec
        let rate_limit = self.spec.rate_limit.as_ref().map(|r| RateLimitConfig {
            requests: r.requests,
            period: r.period.clone(),
            key: r.key.clone(),
        });

        Ok(WebhookEndpoint {
            id: format!("{}/{}", self.metadata.namespace, self.metadata.name),
            name: self.metadata.name.clone(),
            namespace: self.metadata.namespace.clone(),
            path,
            auth,
            triggers,
            max_payload_bytes: self.spec.max_payload_bytes,
            rate_limit,
            ip_allowlist: self.spec.ip_allowlist.clone(),
            enabled: self.spec.enabled,
            created_at: now,
            updated_at: now,
        })
    }
}

impl AuthSpec {
    /// Convert to AuthConfig.
    pub fn to_auth_config(&self) -> AuthConfig {
        let method = match self.auth_type.to_lowercase().as_str() {
            "hmac-sha256" | "hmac_sha256" => AuthMethod::HmacSha256,
            "hmac-sha1" | "hmac_sha1" => AuthMethod::HmacSha1,
            "bearer" | "bearer-token" | "bearer_token" => AuthMethod::BearerToken,
            "basic" => AuthMethod::Basic,
            "ip-allowlist" | "ip_allowlist" => AuthMethod::IpAllowlist,
            "mtls" => AuthMethod::MTls,
            _ => AuthMethod::None,
        };

        let secret_ref = self.secret_ref.as_ref().map(|r| SecretRef {
            name: r.name.clone(),
            key: r.key.clone(),
            namespace: r.namespace.clone(),
        });

        AuthConfig {
            method,
            secret: self.secret.clone(),
            secret_ref,
            header_name: self.header_name.clone(),
            signature_prefix: self.signature_prefix.clone(),
            token: None,
            ip_allowlist: vec![],
            required: true,
        }
    }
}

impl TriggerSpec {
    /// Convert to TriggerMapping.
    pub fn to_trigger_mapping(&self, default_namespace: &str) -> TriggerMapping {
        let filter = self.filter.as_ref().map(|f| match f {
            FilterSpec::Expression(expr) => expr.clone(),
            FilterSpec::Structured(s) => {
                // Convert structured filter to CEL-like expression
                s.fields
                    .iter()
                    .map(|(k, v)| format!("{} == {}", k, v))
                    .collect::<Vec<_>>()
                    .join(" && ")
            }
        });

        TriggerMapping {
            event: self.event.clone(),
            filter,
            workflow: self.workflow.clone(),
            workflow_namespace: Some(
                self.workflow_namespace
                    .clone()
                    .unwrap_or_else(|| default_namespace.to_string()),
            ),
            inputs: self.inputs.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_endpoint() {
        let yaml = r#"
apiVersion: circuitbreaker.io/v1
kind: WebhookEndpoint
metadata:
  name: github-webhook
  namespace: production
spec:
  path: /webhooks/github
  auth:
    type: hmac-sha256
    secretRef:
      name: github-secret
      key: webhook-secret
  triggers:
    - event: push
      workflow: build-and-deploy
      inputs:
        repository: "{{ .repository.full_name }}"
"#;

        let config = WebhookConfig::from_yaml(yaml).unwrap();
        assert_eq!(config.endpoints.len(), 1);

        let endpoint = &config.endpoints[0];
        assert_eq!(endpoint.metadata.name, "github-webhook");
        assert_eq!(endpoint.metadata.namespace, "production");
        assert_eq!(endpoint.spec.path, Some("/webhooks/github".to_string()));
        assert!(endpoint.spec.auth.is_some());
        assert_eq!(endpoint.spec.triggers.len(), 1);
    }

    #[test]
    fn test_parse_multiple_endpoints() {
        let yaml = r#"
- apiVersion: circuitbreaker.io/v1
  kind: WebhookEndpoint
  metadata:
    name: github-webhook
    namespace: default
  spec:
    triggers:
      - event: push
        workflow: build

- apiVersion: circuitbreaker.io/v1
  kind: WebhookEndpoint
  metadata:
    name: gitlab-webhook
    namespace: default
  spec:
    triggers:
      - event: push
        workflow: build
"#;

        let config = WebhookConfig::from_yaml(yaml).unwrap();
        assert_eq!(config.endpoints.len(), 2);
    }

    #[test]
    fn test_to_webhook_endpoint() {
        let config = EndpointConfig {
            api_version: default_api_version(),
            kind: default_endpoint_kind(),
            metadata: EndpointMetadata {
                name: "test-webhook".to_string(),
                namespace: "test-ns".to_string(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
            },
            spec: EndpointSpec {
                path: Some("/webhooks/test".to_string()),
                source_type: "github".to_string(),
                auth: Some(AuthSpec {
                    auth_type: "hmac-sha256".to_string(),
                    secret: Some("test-secret".to_string()),
                    secret_ref: None,
                    header_name: None,
                    signature_prefix: None,
                }),
                triggers: vec![TriggerSpec {
                    event: "push".to_string(),
                    filter: None,
                    workflow: "my-workflow".to_string(),
                    workflow_namespace: None,
                    inputs: HashMap::new(),
                }],
                max_payload_bytes: 1_048_576,
                rate_limit: None,
                ip_allowlist: vec![],
                enabled: true,
                validation: None,
            },
        };

        let endpoint = config.to_webhook_endpoint().unwrap();
        assert_eq!(endpoint.id, "test-ns/test-webhook");
        assert_eq!(endpoint.name, "test-webhook");
        assert_eq!(endpoint.namespace, "test-ns");
        assert_eq!(endpoint.path, "/webhooks/test");
        assert!(endpoint.auth.is_some());
        assert_eq!(endpoint.triggers.len(), 1);
    }

    #[test]
    fn test_auth_spec_to_auth_config() {
        let spec = AuthSpec {
            auth_type: "hmac-sha256".to_string(),
            secret: Some("my-secret".to_string()),
            secret_ref: None,
            header_name: Some("X-Custom-Signature".to_string()),
            signature_prefix: Some("sha256=".to_string()),
        };

        let config = spec.to_auth_config();
        assert_eq!(config.method, AuthMethod::HmacSha256);
        assert_eq!(config.secret, Some("my-secret".to_string()));
        assert_eq!(config.header_name, Some("X-Custom-Signature".to_string()));
    }

    #[test]
    fn test_trigger_spec_to_trigger_mapping() {
        let spec = TriggerSpec {
            event: "push".to_string(),
            filter: Some(FilterSpec::Expression(
                "ref == 'refs/heads/main'".to_string(),
            )),
            workflow: "deploy".to_string(),
            workflow_namespace: Some("prod".to_string()),
            inputs: HashMap::from([("commit".to_string(), "{{ .head_commit.id }}".to_string())]),
        };

        let mapping = spec.to_trigger_mapping("default");
        assert_eq!(mapping.event, "push");
        assert_eq!(mapping.filter, Some("ref == 'refs/heads/main'".to_string()));
        assert_eq!(mapping.workflow, "deploy");
        assert_eq!(mapping.workflow_namespace, Some("prod".to_string()));
        assert!(mapping.inputs.contains_key("commit"));
    }

    #[test]
    fn test_structured_filter() {
        let spec = TriggerSpec {
            event: "push".to_string(),
            filter: Some(FilterSpec::Structured(StructuredFilter {
                fields: HashMap::from([
                    ("ref".to_string(), serde_json::json!("refs/heads/main")),
                    ("action".to_string(), serde_json::json!("created")),
                ]),
            })),
            workflow: "deploy".to_string(),
            workflow_namespace: None,
            inputs: HashMap::new(),
        };

        let mapping = spec.to_trigger_mapping("default");
        assert!(mapping.filter.is_some());
        let filter = mapping.filter.unwrap();
        // The filter should contain both field comparisons
        assert!(filter.contains("ref") || filter.contains("action"));
    }

    #[test]
    fn test_default_path_generation() {
        let config = EndpointConfig {
            api_version: default_api_version(),
            kind: default_endpoint_kind(),
            metadata: EndpointMetadata {
                name: "my-endpoint".to_string(),
                namespace: "default".to_string(),
                labels: HashMap::new(),
                annotations: HashMap::new(),
            },
            spec: EndpointSpec {
                path: None, // No path specified
                source_type: "generic".to_string(),
                auth: None,
                triggers: vec![],
                max_payload_bytes: 1_048_576,
                rate_limit: None,
                ip_allowlist: vec![],
                enabled: true,
                validation: None,
            },
        };

        let endpoint = config.to_webhook_endpoint().unwrap();
        assert_eq!(endpoint.path, "/webhooks/my-endpoint");
    }

    #[test]
    fn test_rate_limit_spec() {
        let yaml = r#"
apiVersion: circuitbreaker.io/v1
kind: WebhookEndpoint
metadata:
  name: rate-limited
  namespace: default
spec:
  rateLimit:
    requests: 100
    period: 1m
    key: source.ip
  triggers:
    - event: any
      workflow: test
"#;

        let config = WebhookConfig::from_yaml(yaml).unwrap();
        let endpoint = &config.endpoints[0];

        assert!(endpoint.spec.rate_limit.is_some());
        let rate_limit = endpoint.spec.rate_limit.as_ref().unwrap();
        assert_eq!(rate_limit.requests, 100);
        assert_eq!(rate_limit.period, "1m");
    }
}
