//! Authentication module for webhook signature validation.
//!
//! This module provides various authentication methods for validating
//! incoming webhook requests, including:
//!
//! - HMAC-SHA256 signature validation (used by GitHub, GitLab, Stripe, etc.)
//! - Bearer token authentication
//! - IP allowlist validation
//! - mTLS (placeholder for future implementation)
//!
//! ## Usage
//!
//! ```rust,ignore
//! use cb_webhook::auth::{AuthValidator, AuthConfig, AuthMethod};
//!
//! let config = AuthConfig {
//!     method: AuthMethod::HmacSha256,
//!     secret: Some("webhook-secret".to_string()),
//!     header_name: Some("X-Hub-Signature-256".to_string()),
//!     ..Default::default()
//! };
//!
//! let validator = AuthValidator::new(config);
//! let is_valid = validator.validate(&headers, &body)?;
//! ```

use crate::error::{Result, WebhookError};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use std::net::IpAddr;

type HmacSha256 = Hmac<Sha256>;

/// Authentication method for webhook endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMethod {
    /// No authentication required.
    None,
    /// HMAC-SHA256 signature validation.
    HmacSha256,
    /// HMAC-SHA1 signature validation (legacy, used by some services).
    HmacSha1,
    /// Bearer token authentication.
    BearerToken,
    /// Basic authentication.
    Basic,
    /// IP allowlist only.
    IpAllowlist,
    /// Mutual TLS (client certificate).
    MTls,
}

impl Default for AuthMethod {
    fn default() -> Self {
        Self::None
    }
}

/// Authentication configuration for a webhook endpoint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Authentication method to use.
    #[serde(default)]
    pub method: AuthMethod,

    /// Secret for HMAC signature validation.
    #[serde(default)]
    pub secret: Option<String>,

    /// Reference to a Kubernetes secret (alternative to inline secret).
    #[serde(default)]
    pub secret_ref: Option<SecretRef>,

    /// Header name containing the signature or token.
    /// Defaults based on auth method:
    /// - HMAC-SHA256: "X-Hub-Signature-256"
    /// - Bearer: "Authorization"
    #[serde(default)]
    pub header_name: Option<String>,

    /// Signature prefix to strip (e.g., "sha256=" for GitHub).
    #[serde(default)]
    pub signature_prefix: Option<String>,

    /// Expected token value for bearer authentication.
    #[serde(default)]
    pub token: Option<String>,

    /// IP addresses or CIDR ranges to allow.
    #[serde(default)]
    pub ip_allowlist: Vec<String>,

    /// Whether to require authentication (vs optional).
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    true
}

/// Reference to a Kubernetes secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretRef {
    /// Secret name.
    pub name: String,
    /// Key within the secret.
    pub key: String,
    /// Namespace (defaults to endpoint namespace).
    #[serde(default)]
    pub namespace: Option<String>,
}

/// Trait for authentication validation.
#[async_trait]
pub trait Authenticate: Send + Sync {
    /// Validate the request authentication.
    async fn validate(
        &self,
        headers: &HashMap<String, String>,
        body: &[u8],
        source_ip: Option<IpAddr>,
    ) -> Result<AuthResult>;
}

/// Result of authentication validation.
#[derive(Debug, Clone)]
pub struct AuthResult {
    /// Whether authentication succeeded.
    pub valid: bool,
    /// Authentication method used.
    pub method: AuthMethod,
    /// Optional identity extracted from auth (e.g., username, IP).
    pub identity: Option<String>,
    /// Reason for failure (if any).
    pub failure_reason: Option<String>,
}

impl AuthResult {
    /// Create a successful auth result.
    pub fn success(method: AuthMethod, identity: Option<String>) -> Self {
        Self {
            valid: true,
            method,
            identity,
            failure_reason: None,
        }
    }

    /// Create a failed auth result.
    pub fn failure(method: AuthMethod, reason: impl Into<String>) -> Self {
        Self {
            valid: false,
            method,
            identity: None,
            failure_reason: Some(reason.into()),
        }
    }

    /// Create a skipped auth result (no auth required).
    pub fn skipped() -> Self {
        Self {
            valid: true,
            method: AuthMethod::None,
            identity: None,
            failure_reason: None,
        }
    }
}

/// Authentication validator that supports multiple auth methods.
pub struct AuthValidator {
    config: AuthConfig,
    /// Cached/resolved secret value.
    secret: Option<String>,
}

impl AuthValidator {
    /// Create a new authentication validator.
    pub fn new(config: AuthConfig) -> Self {
        let secret = config.secret.clone();
        Self { config, secret }
    }

    /// Create a validator with a resolved secret.
    pub fn with_secret(mut config: AuthConfig, secret: String) -> Self {
        config.secret = Some(secret.clone());
        Self {
            config,
            secret: Some(secret),
        }
    }

    /// Get the default header name for the auth method.
    fn default_header_name(&self) -> &'static str {
        match self.config.method {
            AuthMethod::HmacSha256 => "x-hub-signature-256",
            AuthMethod::HmacSha1 => "x-hub-signature",
            AuthMethod::BearerToken => "authorization",
            AuthMethod::Basic => "authorization",
            _ => "",
        }
    }

    /// Get the header name to use (config override or default).
    fn get_header_name(&self) -> String {
        self.config
            .header_name
            .clone()
            .unwrap_or_else(|| self.default_header_name().to_string())
            .to_lowercase()
    }

    /// Validate HMAC-SHA256 signature.
    fn validate_hmac_sha256(
        &self,
        headers: &HashMap<String, String>,
        body: &[u8],
    ) -> Result<AuthResult> {
        let secret = self
            .secret
            .as_ref()
            .ok_or_else(|| WebhookError::Config("HMAC secret not configured".to_string()))?;

        let header_name = self.get_header_name();
        let signature_header = headers
            .get(&header_name)
            .or_else(|| headers.get(&header_name.to_uppercase()))
            .ok_or_else(|| {
                WebhookError::MissingAuthHeader(format!("Missing {} header", header_name))
            })?;

        // Strip prefix if configured (e.g., "sha256=" for GitHub)
        let signature = if let Some(prefix) = &self.config.signature_prefix {
            signature_header
                .strip_prefix(prefix)
                .unwrap_or(signature_header)
        } else {
            // Try common prefixes
            signature_header
                .strip_prefix("sha256=")
                .or_else(|| signature_header.strip_prefix("SHA256="))
                .unwrap_or(signature_header)
        };

        // Decode the hex signature
        let expected_signature =
            hex::decode(signature).map_err(|e| WebhookError::InvalidSignature(e.to_string()))?;

        // Compute HMAC
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|e| WebhookError::Internal(format!("HMAC error: {}", e)))?;
        mac.update(body);

        // Constant-time comparison
        match mac.verify_slice(&expected_signature) {
            Ok(()) => Ok(AuthResult::success(AuthMethod::HmacSha256, None)),
            Err(_) => Ok(AuthResult::failure(
                AuthMethod::HmacSha256,
                "Signature mismatch",
            )),
        }
    }

    /// Validate bearer token.
    fn validate_bearer_token(&self, headers: &HashMap<String, String>) -> Result<AuthResult> {
        let expected_token = self
            .config
            .token
            .as_ref()
            .or(self.secret.as_ref())
            .ok_or_else(|| WebhookError::Config("Bearer token not configured".to_string()))?;

        let header_name = self.get_header_name();
        let auth_header = headers
            .get(&header_name)
            .or_else(|| headers.get(&header_name.to_uppercase()))
            .ok_or_else(|| {
                WebhookError::MissingAuthHeader(format!("Missing {} header", header_name))
            })?;

        // Extract token from "Bearer <token>"
        let token = auth_header
            .strip_prefix("Bearer ")
            .or_else(|| auth_header.strip_prefix("bearer "))
            .unwrap_or(auth_header);

        // Constant-time comparison
        if constant_time_eq(token.as_bytes(), expected_token.as_bytes()) {
            Ok(AuthResult::success(AuthMethod::BearerToken, None))
        } else {
            Ok(AuthResult::failure(
                AuthMethod::BearerToken,
                "Invalid token",
            ))
        }
    }

    /// Validate basic authentication.
    fn validate_basic_auth(&self, headers: &HashMap<String, String>) -> Result<AuthResult> {
        let expected_secret = self
            .secret
            .as_ref()
            .ok_or_else(|| WebhookError::Config("Basic auth credentials not configured".to_string()))?;

        let header_name = self.get_header_name();
        let auth_header = headers
            .get(&header_name)
            .or_else(|| headers.get(&header_name.to_uppercase()))
            .ok_or_else(|| {
                WebhookError::MissingAuthHeader(format!("Missing {} header", header_name))
            })?;

        // Extract credentials from "Basic <base64>"
        let encoded = auth_header
            .strip_prefix("Basic ")
            .or_else(|| auth_header.strip_prefix("basic "))
            .ok_or_else(|| WebhookError::InvalidSignature("Invalid Basic auth format".to_string()))?;

        let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
            .map_err(|e| WebhookError::InvalidSignature(format!("Base64 decode error: {}", e)))?;

        let credentials = String::from_utf8(decoded)
            .map_err(|e| WebhookError::InvalidSignature(format!("Invalid UTF-8: {}", e)))?;

        // Compare with expected (could be "username:password" format or just the whole thing)
        if constant_time_eq(credentials.as_bytes(), expected_secret.as_bytes()) {
            // Extract username if present
            let identity = credentials.split(':').next().map(String::from);
            Ok(AuthResult::success(AuthMethod::Basic, identity))
        } else {
            Ok(AuthResult::failure(
                AuthMethod::Basic,
                "Invalid credentials",
            ))
        }
    }

    /// Validate IP allowlist.
    fn validate_ip_allowlist(&self, source_ip: Option<IpAddr>) -> Result<AuthResult> {
        if self.config.ip_allowlist.is_empty() {
            // No allowlist configured, allow all
            return Ok(AuthResult::success(AuthMethod::IpAllowlist, None));
        }

        let ip = source_ip.ok_or_else(|| {
            WebhookError::IpNotAllowed("Source IP not available".to_string())
        })?;

        let ip_str = ip.to_string();

        for allowed in &self.config.ip_allowlist {
            // Simple string match for now
            // TODO: Add CIDR support
            if allowed == &ip_str || allowed == "*" {
                return Ok(AuthResult::success(
                    AuthMethod::IpAllowlist,
                    Some(ip_str),
                ));
            }

            // Check if it's a CIDR range (basic implementation)
            if allowed.contains('/') {
                if let Ok(allowed_network) = parse_cidr(allowed) {
                    if allowed_network.contains(ip) {
                        return Ok(AuthResult::success(
                            AuthMethod::IpAllowlist,
                            Some(ip_str),
                        ));
                    }
                }
            }
        }

        Ok(AuthResult::failure(
            AuthMethod::IpAllowlist,
            format!("IP {} not in allowlist", ip),
        ))
    }
}

#[async_trait]
impl Authenticate for AuthValidator {
    async fn validate(
        &self,
        headers: &HashMap<String, String>,
        body: &[u8],
        source_ip: Option<IpAddr>,
    ) -> Result<AuthResult> {
        // First check IP allowlist if configured
        if !self.config.ip_allowlist.is_empty() {
            let ip_result = self.validate_ip_allowlist(source_ip)?;
            if !ip_result.valid {
                return Ok(ip_result);
            }
        }

        // Then validate based on auth method
        match self.config.method {
            AuthMethod::None => Ok(AuthResult::skipped()),
            AuthMethod::HmacSha256 => self.validate_hmac_sha256(headers, body),
            AuthMethod::HmacSha1 => {
                // TODO: Implement HMAC-SHA1 if needed
                Err(WebhookError::Config(
                    "HMAC-SHA1 not yet implemented".to_string(),
                ))
            }
            AuthMethod::BearerToken => self.validate_bearer_token(headers),
            AuthMethod::Basic => self.validate_basic_auth(headers),
            AuthMethod::IpAllowlist => self.validate_ip_allowlist(source_ip),
            AuthMethod::MTls => {
                // TODO: Implement mTLS validation
                Err(WebhookError::Config("mTLS not yet implemented".to_string()))
            }
        }
    }
}

/// Constant-time byte comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Simple CIDR parsing (basic implementation).
struct CidrRange {
    network: u128,
    mask: u128,
    is_v6: bool,
}

impl CidrRange {
    fn contains(&self, ip: IpAddr) -> bool {
        let ip_bits = match ip {
            IpAddr::V4(v4) if !self.is_v6 => u128::from(u32::from(v4)),
            IpAddr::V6(v6) if self.is_v6 => u128::from(v6),
            _ => return false,
        };
        (ip_bits & self.mask) == self.network
    }
}

fn parse_cidr(cidr: &str) -> std::result::Result<CidrRange, ()> {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return Err(());
    }

    let prefix_len: u8 = parts[1].parse().map_err(|_| ())?;

    if let Ok(v4) = parts[0].parse::<std::net::Ipv4Addr>() {
        if prefix_len > 32 {
            return Err(());
        }
        let ip_bits = u128::from(u32::from(v4));
        let mask = if prefix_len == 0 {
            0
        } else {
            !((1u128 << (32 - prefix_len)) - 1) & 0xFFFFFFFF
        };
        Ok(CidrRange {
            network: ip_bits & mask,
            mask,
            is_v6: false,
        })
    } else if let Ok(v6) = parts[0].parse::<std::net::Ipv6Addr>() {
        if prefix_len > 128 {
            return Err(());
        }
        let ip_bits = u128::from(v6);
        let mask = if prefix_len == 0 {
            0
        } else {
            !((1u128 << (128 - prefix_len)) - 1)
        };
        Ok(CidrRange {
            network: ip_bits & mask,
            mask,
            is_v6: true,
        })
    } else {
        Err(())
    }
}

/// Well-known webhook providers and their authentication details.
pub mod providers {
    use super::*;

    /// Create auth config for GitHub webhooks.
    pub fn github(secret: String) -> AuthConfig {
        AuthConfig {
            method: AuthMethod::HmacSha256,
            secret: Some(secret),
            header_name: Some("X-Hub-Signature-256".to_string()),
            signature_prefix: Some("sha256=".to_string()),
            required: true,
            ..Default::default()
        }
    }

    /// Create auth config for GitLab webhooks.
    pub fn gitlab(token: String) -> AuthConfig {
        AuthConfig {
            method: AuthMethod::BearerToken,
            token: Some(token),
            header_name: Some("X-Gitlab-Token".to_string()),
            required: true,
            ..Default::default()
        }
    }

    /// Create auth config for Stripe webhooks.
    pub fn stripe(secret: String) -> AuthConfig {
        AuthConfig {
            method: AuthMethod::HmacSha256,
            secret: Some(secret),
            header_name: Some("Stripe-Signature".to_string()),
            required: true,
            ..Default::default()
        }
    }

    /// Create auth config for Docker Hub webhooks.
    pub fn docker_hub() -> AuthConfig {
        // Docker Hub doesn't support webhook authentication by default
        AuthConfig {
            method: AuthMethod::None,
            ..Default::default()
        }
    }

    /// Create auth config for Slack webhooks.
    pub fn slack(signing_secret: String) -> AuthConfig {
        AuthConfig {
            method: AuthMethod::HmacSha256,
            secret: Some(signing_secret),
            header_name: Some("X-Slack-Signature".to_string()),
            signature_prefix: Some("v0=".to_string()),
            required: true,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_method_default() {
        let method = AuthMethod::default();
        assert_eq!(method, AuthMethod::None);
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
    }

    #[test]
    fn test_hmac_sha256_validation() {
        use hmac::Mac;

        let secret = "test-secret";
        let body = b"test body content";

        // Compute expected signature
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let signature = hex::encode(mac.finalize().into_bytes());

        let config = AuthConfig {
            method: AuthMethod::HmacSha256,
            secret: Some(secret.to_string()),
            header_name: Some("x-signature".to_string()),
            signature_prefix: Some("sha256=".to_string()),
            ..Default::default()
        };

        let validator = AuthValidator::new(config);

        let mut headers = HashMap::new();
        headers.insert("x-signature".to_string(), format!("sha256={}", signature));

        let result = tokio_test::block_on(validator.validate(&headers, body, None)).unwrap();
        assert!(result.valid);
        assert_eq!(result.method, AuthMethod::HmacSha256);
    }

    #[test]
    fn test_hmac_sha256_invalid_signature() {
        let secret = "test-secret";
        let body = b"test body content";

        let config = AuthConfig {
            method: AuthMethod::HmacSha256,
            secret: Some(secret.to_string()),
            header_name: Some("x-signature".to_string()),
            ..Default::default()
        };

        let validator = AuthValidator::new(config);

        let mut headers = HashMap::new();
        headers.insert(
            "x-signature".to_string(),
            "sha256=0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        );

        let result = tokio_test::block_on(validator.validate(&headers, body, None)).unwrap();
        assert!(!result.valid);
        assert_eq!(result.failure_reason, Some("Signature mismatch".to_string()));
    }

    #[test]
    fn test_bearer_token_validation() {
        let token = "my-secret-token";

        let config = AuthConfig {
            method: AuthMethod::BearerToken,
            token: Some(token.to_string()),
            ..Default::default()
        };

        let validator = AuthValidator::new(config);

        let mut headers = HashMap::new();
        headers.insert(
            "authorization".to_string(),
            format!("Bearer {}", token),
        );

        let result = tokio_test::block_on(validator.validate(&headers, b"", None)).unwrap();
        assert!(result.valid);
        assert_eq!(result.method, AuthMethod::BearerToken);
    }

    #[test]
    fn test_ip_allowlist_validation() {
        let config = AuthConfig {
            method: AuthMethod::IpAllowlist,
            ip_allowlist: vec!["192.168.1.100".to_string(), "10.0.0.0/8".to_string()],
            ..Default::default()
        };

        let validator = AuthValidator::new(config);

        // Allowed IP
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let result =
            tokio_test::block_on(validator.validate(&HashMap::new(), b"", Some(ip))).unwrap();
        assert!(result.valid);

        // IP in allowed CIDR
        let ip: IpAddr = "10.1.2.3".parse().unwrap();
        let result =
            tokio_test::block_on(validator.validate(&HashMap::new(), b"", Some(ip))).unwrap();
        assert!(result.valid);

        // Denied IP
        let ip: IpAddr = "192.168.1.200".parse().unwrap();
        let result =
            tokio_test::block_on(validator.validate(&HashMap::new(), b"", Some(ip))).unwrap();
        assert!(!result.valid);
    }

    #[test]
    fn test_auth_result_helpers() {
        let success = AuthResult::success(AuthMethod::HmacSha256, Some("user".to_string()));
        assert!(success.valid);
        assert_eq!(success.identity, Some("user".to_string()));

        let failure = AuthResult::failure(AuthMethod::BearerToken, "bad token");
        assert!(!failure.valid);
        assert_eq!(failure.failure_reason, Some("bad token".to_string()));

        let skipped = AuthResult::skipped();
        assert!(skipped.valid);
        assert_eq!(skipped.method, AuthMethod::None);
    }

    #[test]
    fn test_github_provider_config() {
        let config = providers::github("my-secret".to_string());
        assert_eq!(config.method, AuthMethod::HmacSha256);
        assert_eq!(config.header_name, Some("X-Hub-Signature-256".to_string()));
        assert_eq!(config.signature_prefix, Some("sha256=".to_string()));
    }

    #[test]
    fn test_cidr_parsing() {
        // IPv4 CIDR
        let cidr = parse_cidr("192.168.0.0/24").unwrap();
        assert!(!cidr.is_v6);
        assert!(cidr.contains("192.168.0.1".parse().unwrap()));
        assert!(cidr.contains("192.168.0.255".parse().unwrap()));
        assert!(!cidr.contains("192.168.1.1".parse().unwrap()));

        // IPv6 CIDR
        let cidr = parse_cidr("2001:db8::/32").unwrap();
        assert!(cidr.is_v6);
        assert!(cidr.contains("2001:db8::1".parse().unwrap()));
        assert!(!cidr.contains("2001:db9::1".parse().unwrap()));
    }
}
