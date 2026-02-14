//! Authentication and authorization module for MLRunX API.
//!
//! Provides API key authentication middleware and key management.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use axum::{
    extract::{Request, State},
    http::{Method, StatusCode, request::Parts},
    middleware::Next,
    response::Response,
};
use hmac::{Hmac, Mac};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

use crate::storage::{ApiKeyRow, ProjectMembershipRow, SqliteStore};

/// An API key entry stored in the system.
#[derive(Debug, Clone)]
pub struct ApiKey {
    /// Unique identifier for the key
    pub id: String,
    /// Stored API key hash (argon2id for new keys, legacy SHA-256 for old keys)
    pub key_hash: String,
    /// First 8 chars of the key for identification
    pub key_prefix: String,
    /// Project this key is scoped to (None = global admin)
    pub project_id: Option<String>,
    /// Human-readable name
    pub name: Option<String>,
    /// Permitted scopes
    pub scopes: Vec<String>,
    /// When the key was created
    pub created_at: std::time::SystemTime,
    /// When the key was last used
    pub last_used_at: Option<std::time::SystemTime>,
    /// When the key was revoked (None = active)
    pub revoked_at: Option<std::time::SystemTime>,
    /// When the key expires (None = never expires)
    pub expires_at: Option<std::time::SystemTime>,
}

impl ApiKey {
    /// Check if the key is valid (not revoked and not expired).
    pub fn is_valid(&self) -> bool {
        if self.revoked_at.is_some() {
            return false;
        }
        if let Some(expires_at) = self.expires_at {
            if SystemTime::now() >= expires_at {
                return false;
            }
        }
        true
    }

    /// Check if the key has a specific scope.
    pub fn has_scope(&self, scope: &str) -> bool {
        // Admin scope grants all permissions
        if self.scopes.contains(&"admin".to_string()) {
            return true;
        }
        self.scopes.contains(&scope.to_string())
    }

    /// Check if the key can access a project.
    pub fn can_access_project(&self, project_id: &str) -> bool {
        // Global admin keys can access all projects
        if self.project_id.is_none() {
            return true;
        }
        // Otherwise, must match the project
        self.project_id.as_ref().map_or(false, |p| p == project_id)
    }
}

#[derive(Debug, Clone)]
struct UiJwtConfig {
    enabled: bool,
    secret: Option<String>,
    public_key_pem: Option<String>,
    algorithm: UiJwtAlgorithm,
    auto_provision_project: bool,
    personal_project_prefix: String,
    issuer: Option<String>,
    audience: Option<String>,
    provider: String,
    session_cookie_name: String,
    csrf_cookie_name: String,
    session_ttl_seconds: u64,
    cookie_secure: bool,
    cookie_same_site: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiJwtAlgorithm {
    Hs256,
    Rs256,
    Es256,
}

impl UiJwtAlgorithm {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "hs256" => Some(Self::Hs256),
            "rs256" => Some(Self::Rs256),
            "es256" => Some(Self::Es256),
            _ => None,
        }
    }

    fn env_value(self) -> &'static str {
        match self {
            Self::Hs256 => "HS256",
            Self::Rs256 => "RS256",
            Self::Es256 => "ES256",
        }
    }

    fn jsonwebtoken_algorithm(self) -> Algorithm {
        match self {
            Self::Hs256 => Algorithm::HS256,
            Self::Rs256 => Algorithm::RS256,
            Self::Es256 => Algorithm::ES256,
        }
    }
}

fn normalize_pem_env(raw: &str) -> String {
    raw.trim().replace("\\n", "\n")
}

impl UiJwtConfig {
    fn from_env(enabled: bool) -> Self {
        let session_ttl_seconds = std::env::var("MLRUNX_UI_SESSION_TTL_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(900);

        let cookie_same_site = std::env::var("MLRUNX_UI_COOKIE_SAMESITE")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(|v| {
                if v.eq_ignore_ascii_case("strict") {
                    "Strict".to_string()
                } else if v.eq_ignore_ascii_case("none") {
                    "None".to_string()
                } else {
                    "Lax".to_string()
                }
            })
            .unwrap_or_else(|| "Lax".to_string());

        let secret = std::env::var("MLRUNX_JWT_SECRET")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let public_key_pem = std::env::var("MLRUNX_JWT_PUBLIC_KEY_PEM")
            .ok()
            .map(|v| normalize_pem_env(&v))
            .filter(|v| !v.trim().is_empty());
        let algorithm = std::env::var("MLRUNX_JWT_ALGORITHM")
            .ok()
            .as_deref()
            .and_then(UiJwtAlgorithm::parse)
            .unwrap_or_else(|| {
                if public_key_pem.is_some() {
                    UiJwtAlgorithm::Rs256
                } else {
                    UiJwtAlgorithm::Hs256
                }
            });

        Self {
            enabled,
            secret,
            public_key_pem,
            algorithm,
            auto_provision_project: std::env::var("MLRUNX_UI_AUTO_PROVISION_PROJECT")
                .ok()
                .map_or(true, |value| env_flag_value(&value)),
            personal_project_prefix: std::env::var("MLRUNX_UI_PERSONAL_PROJECT_PREFIX")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "personal".to_string()),
            issuer: std::env::var("MLRUNX_JWT_ISSUER")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            audience: std::env::var("MLRUNX_JWT_AUDIENCE")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            provider: std::env::var("MLRUNX_JWT_PROVIDER")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| "jwt".to_string()),
            session_cookie_name: std::env::var("MLRUNX_UI_SESSION_COOKIE_NAME")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| "mlrunx_ui_session".to_string()),
            csrf_cookie_name: std::env::var("MLRUNX_UI_CSRF_COOKIE_NAME")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| "mlrunx_ui_csrf".to_string()),
            session_ttl_seconds,
            cookie_secure: env_flag("MLRUNX_UI_COOKIE_SECURE"),
            cookie_same_site,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UiSessionIssue {
    pub session_token: String,
    pub csrf_token: String,
    pub expires_at: String,
    pub user_id: String,
    pub allowed_project_ids: Vec<String>,
}

struct ResolvedUiIdentity {
    user_id: String,
    user_name: Option<String>,
    allowed_project_ids: HashSet<String>,
    scopes: Vec<String>,
    is_platform_admin: bool,
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).map_or(false, |v| env_flag_value(&v))
}

fn env_flag_value(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")
}

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
struct AuthRateLimitConfig {
    enabled: bool,
    max_failures_per_window: u32,
    window: Duration,
    base_backoff: Duration,
    max_backoff: Duration,
}

impl AuthRateLimitConfig {
    fn from_env() -> Self {
        let enabled = std::env::var("MLRUNX_AUTH_RATE_LIMIT_ENABLED")
            .ok()
            .map_or(true, |v| env_flag_value(&v));
        let max_failures_per_window = std::env::var("MLRUNX_AUTH_RATE_LIMIT_MAX_FAILURES")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(10);
        let window_seconds = std::env::var("MLRUNX_AUTH_RATE_LIMIT_WINDOW_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(60);
        let base_backoff_seconds = std::env::var("MLRUNX_AUTH_RATE_LIMIT_BASE_BACKOFF_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(5);
        let max_backoff_seconds = std::env::var("MLRUNX_AUTH_RATE_LIMIT_MAX_BACKOFF_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(300);

        Self {
            enabled,
            max_failures_per_window,
            window: Duration::from_secs(window_seconds),
            base_backoff: Duration::from_secs(base_backoff_seconds),
            max_backoff: Duration::from_secs(max_backoff_seconds),
        }
    }
}

#[derive(Debug, Clone)]
struct AuthRateLimitEntry {
    window_started_at: Instant,
    failures_in_window: u32,
    blocked_until: Option<Instant>,
}

impl AuthRateLimitEntry {
    fn new(now: Instant) -> Self {
        Self {
            window_started_at: now,
            failures_in_window: 0,
            blocked_until: None,
        }
    }
}

#[derive(Debug)]
struct AuthRateLimiter {
    config: AuthRateLimitConfig,
    entries: Mutex<HashMap<String, AuthRateLimitEntry>>,
}

impl AuthRateLimiter {
    fn new(config: AuthRateLimitConfig) -> Self {
        Self {
            config,
            entries: Mutex::new(HashMap::new()),
        }
    }

    async fn retry_after_seconds(&self, client_key: &str) -> Option<u64> {
        if !self.config.enabled || client_key.is_empty() {
            return None;
        }

        let now = Instant::now();
        let mut entries = self.entries.lock().await;
        if let Some(entry) = entries.get_mut(client_key) {
            if now.duration_since(entry.window_started_at) > self.config.window {
                *entry = AuthRateLimitEntry::new(now);
                return None;
            }

            if let Some(until) = entry.blocked_until {
                if now < until {
                    return Some((until - now).as_secs().max(1));
                }
                entry.blocked_until = None;
                entry.failures_in_window = 0;
                entry.window_started_at = now;
            }
        }

        None
    }

    async fn record_failure(&self, client_key: &str) {
        if !self.config.enabled || client_key.is_empty() {
            return;
        }

        let now = Instant::now();
        let mut entries = self.entries.lock().await;
        let entry = entries
            .entry(client_key.to_string())
            .or_insert_with(|| AuthRateLimitEntry::new(now));

        if now.duration_since(entry.window_started_at) > self.config.window {
            *entry = AuthRateLimitEntry::new(now);
        }

        entry.failures_in_window = entry.failures_in_window.saturating_add(1);
        if entry.failures_in_window >= self.config.max_failures_per_window {
            let over_limit = entry.failures_in_window - self.config.max_failures_per_window;
            let exponent = over_limit.min(16);
            let backoff_multiplier = 1u64 << exponent;
            let candidate_backoff = self
                .config
                .base_backoff
                .as_secs()
                .saturating_mul(backoff_multiplier);
            let backoff_secs = candidate_backoff.min(self.config.max_backoff.as_secs());
            entry.blocked_until = Some(now + Duration::from_secs(backoff_secs));
        }
    }

    async fn record_success(&self, client_key: &str) {
        if !self.config.enabled || client_key.is_empty() {
            return;
        }

        let mut entries = self.entries.lock().await;
        entries.remove(client_key);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HmacSecretSource {
    ExplicitEnv,
    JwtSecretFallback,
    InsecureDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeAuthMode {
    Disabled,
    ApiKey,
    Hybrid,
}

impl RuntimeAuthMode {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "disabled" | "none" | "off" => Some(Self::Disabled),
            "api_key" | "api-key" | "apikey" => Some(Self::ApiKey),
            "hybrid" | "mixed" | "api_key_ui_jwt" | "api-key-ui-jwt" | "ui_jwt" | "ui-jwt" => {
                Some(Self::Hybrid)
            }
            _ => None,
        }
    }

    fn from_flags(auth_disabled: bool, ui_jwt_enabled: bool) -> Self {
        if auth_disabled {
            Self::Disabled
        } else if ui_jwt_enabled {
            Self::Hybrid
        } else {
            Self::ApiKey
        }
    }

    fn from_env() -> Self {
        if let Ok(raw_mode) = std::env::var("MLRUNX_AUTH_MODE") {
            if let Some(mode) = Self::parse(&raw_mode) {
                return mode;
            }

            warn!(
                mode = %raw_mode,
                "Invalid MLRUNX_AUTH_MODE value; falling back to legacy auth flags."
            );
        }

        Self::from_flags(
            env_flag("MLRUNX_AUTH_DISABLED"),
            env_flag("MLRUNX_UI_JWT_AUTH_ENABLED"),
        )
    }

    fn auth_disabled(self) -> bool {
        matches!(self, Self::Disabled)
    }

    fn ui_jwt_enabled(self) -> bool {
        matches!(self, Self::Hybrid)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::ApiKey => "api_key",
            Self::Hybrid => "hybrid",
        }
    }
}

/// API key store with optional SQLite persistence.
pub struct ApiKeyStore {
    /// Map from key_hash to ApiKey
    keys: RwLock<HashMap<String, ApiKey>>,
    /// Optional durable backing store.
    sqlite_store: Option<Arc<SqliteStore>>,
    /// UI JWT auth configuration (feature-flagged).
    ui_jwt: UiJwtConfig,
    /// Resolved runtime auth mode from environment.
    runtime_auth_mode: RuntimeAuthMode,
    /// Shared HMAC secret for deterministic API key fingerprints + UI session token hashes.
    auth_hmac_secret: Vec<u8>,
    auth_hmac_secret_source: HmacSecretSource,
    auth_rate_limiter: AuthRateLimiter,
    /// Email of the designated platform admin (from MLRUNX_ADMIN_EMAIL env var).
    admin_email: Option<String>,
    /// Whether auth is disabled (for dev/testing)
    pub auth_disabled: std::sync::atomic::AtomicBool,
}

impl ApiKeyStore {
    /// Create a new API key store.
    pub fn new() -> Self {
        let runtime_auth_mode = RuntimeAuthMode::from_env();
        let (auth_hmac_secret, auth_hmac_secret_source) = load_auth_hmac_secret();
        let auth_rate_limiter = AuthRateLimiter::new(AuthRateLimitConfig::from_env());
        Self {
            keys: RwLock::new(HashMap::new()),
            sqlite_store: None,
            ui_jwt: UiJwtConfig::from_env(runtime_auth_mode.ui_jwt_enabled()),
            runtime_auth_mode,
            auth_hmac_secret,
            auth_hmac_secret_source,
            auth_rate_limiter,
            admin_email: load_admin_email(),
            auth_disabled: std::sync::atomic::AtomicBool::new(runtime_auth_mode.auth_disabled()),
        }
    }

    /// Create a key store backed by SQLite for durable key storage.
    pub fn new_with_sqlite(sqlite_store: Arc<SqliteStore>) -> Self {
        let runtime_auth_mode = RuntimeAuthMode::from_env();
        let (auth_hmac_secret, auth_hmac_secret_source) = load_auth_hmac_secret();
        let auth_rate_limiter = AuthRateLimiter::new(AuthRateLimitConfig::from_env());
        Self {
            keys: RwLock::new(HashMap::new()),
            sqlite_store: Some(sqlite_store),
            ui_jwt: UiJwtConfig::from_env(runtime_auth_mode.ui_jwt_enabled()),
            runtime_auth_mode,
            auth_hmac_secret,
            auth_hmac_secret_source,
            auth_rate_limiter,
            admin_email: load_admin_email(),
            auth_disabled: std::sync::atomic::AtomicBool::new(runtime_auth_mode.auth_disabled()),
        }
    }

    /// Create a new API key store with auth disabled (for testing).
    pub fn new_dev_mode() -> Self {
        let (auth_hmac_secret, auth_hmac_secret_source) = load_auth_hmac_secret();
        let auth_rate_limiter = AuthRateLimiter::new(AuthRateLimitConfig::from_env());
        Self {
            keys: RwLock::new(HashMap::new()),
            sqlite_store: None,
            ui_jwt: UiJwtConfig::from_env(false),
            runtime_auth_mode: RuntimeAuthMode::Disabled,
            auth_hmac_secret,
            auth_hmac_secret_source,
            auth_rate_limiter,
            admin_email: load_admin_email(),
            auth_disabled: std::sync::atomic::AtomicBool::new(true),
        }
    }

    #[cfg(test)]
    pub fn new_with_sqlite_and_ui_jwt(sqlite_store: Arc<SqliteStore>, jwt_secret: &str) -> Self {
        let (auth_hmac_secret, auth_hmac_secret_source) = load_auth_hmac_secret();
        let auth_rate_limiter = AuthRateLimiter::new(AuthRateLimitConfig::from_env());
        Self {
            keys: RwLock::new(HashMap::new()),
            sqlite_store: Some(sqlite_store),
            ui_jwt: UiJwtConfig {
                enabled: true,
                secret: Some(jwt_secret.to_string()),
                public_key_pem: None,
                algorithm: UiJwtAlgorithm::Hs256,
                auto_provision_project: true,
                personal_project_prefix: "personal".to_string(),
                issuer: Some("https://test-auth.local".to_string()),
                audience: Some("authenticated".to_string()),
                provider: "jwt".to_string(),
                session_cookie_name: "mlrunx_ui_session".to_string(),
                csrf_cookie_name: "mlrunx_ui_csrf".to_string(),
                session_ttl_seconds: 900,
                cookie_secure: false,
                cookie_same_site: "Lax".to_string(),
            },
            runtime_auth_mode: RuntimeAuthMode::Hybrid,
            auth_hmac_secret,
            auth_hmac_secret_source,
            auth_rate_limiter,
            admin_email: load_admin_email(),
            auth_disabled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    #[cfg(test)]
    fn new_with_sqlite_and_ui_jwt_public_key(
        sqlite_store: Arc<SqliteStore>,
        public_key_pem: &str,
        algorithm: UiJwtAlgorithm,
    ) -> Self {
        let (auth_hmac_secret, auth_hmac_secret_source) = load_auth_hmac_secret();
        let auth_rate_limiter = AuthRateLimiter::new(AuthRateLimitConfig::from_env());
        Self {
            keys: RwLock::new(HashMap::new()),
            sqlite_store: Some(sqlite_store),
            ui_jwt: UiJwtConfig {
                enabled: true,
                secret: None,
                public_key_pem: Some(public_key_pem.to_string()),
                algorithm,
                auto_provision_project: true,
                personal_project_prefix: "personal".to_string(),
                issuer: Some("https://test-auth.local".to_string()),
                audience: Some("authenticated".to_string()),
                provider: "jwt".to_string(),
                session_cookie_name: "mlrunx_ui_session".to_string(),
                csrf_cookie_name: "mlrunx_ui_csrf".to_string(),
                session_ttl_seconds: 900,
                cookie_secure: false,
                cookie_same_site: "Lax".to_string(),
            },
            runtime_auth_mode: RuntimeAuthMode::Hybrid,
            auth_hmac_secret,
            auth_hmac_secret_source,
            auth_rate_limiter,
            admin_email: load_admin_email(),
            auth_disabled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn is_ui_jwt_enabled(&self) -> bool {
        self.ui_jwt.enabled
    }

    pub fn admin_email(&self) -> Option<&str> {
        self.admin_email.as_deref()
    }

    pub fn ui_session_cookie_name(&self) -> &str {
        &self.ui_jwt.session_cookie_name
    }

    pub fn ui_csrf_cookie_name(&self) -> &str {
        &self.ui_jwt.csrf_cookie_name
    }

    pub fn ui_session_ttl_seconds(&self) -> u64 {
        self.ui_jwt.session_ttl_seconds
    }

    pub fn ui_cookie_secure(&self) -> bool {
        self.ui_jwt.cookie_secure
    }

    pub fn ui_cookie_same_site(&self) -> &str {
        &self.ui_jwt.cookie_same_site
    }

    pub fn validate_startup_configuration(&self) -> Result<(), String> {
        if self.ui_jwt.enabled {
            if self
                .ui_jwt
                .issuer
                .as_deref()
                .map(str::trim)
                .is_none_or(str::is_empty)
            {
                return Err(
                    "MLRUNX_JWT_ISSUER is required when UI JWT auth is enabled.".to_string()
                );
            }
            if self
                .ui_jwt
                .audience
                .as_deref()
                .map(str::trim)
                .is_none_or(str::is_empty)
            {
                return Err(
                    "MLRUNX_JWT_AUDIENCE is required when UI JWT auth is enabled.".to_string(),
                );
            }
        }
        Ok(())
    }

    pub fn using_insecure_default_hmac_secret(&self) -> bool {
        matches!(
            self.auth_hmac_secret_source,
            HmacSecretSource::InsecureDefault
        )
    }

    /// Check if auth is disabled.
    pub fn is_auth_disabled(&self) -> bool {
        self.auth_disabled
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn auth_client_key(client_ip: Option<&str>) -> String {
        client_ip
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("unknown")
            .to_string()
    }

    pub async fn auth_rate_limit_retry_after_seconds(
        &self,
        client_ip: Option<&str>,
    ) -> Option<u64> {
        let client_key = Self::auth_client_key(client_ip);
        self.auth_rate_limiter
            .retry_after_seconds(&client_key)
            .await
    }

    pub async fn record_auth_success(&self, client_ip: Option<&str>) {
        let client_key = Self::auth_client_key(client_ip);
        self.auth_rate_limiter.record_success(&client_key).await;
    }

    pub async fn record_auth_failure(
        &self,
        client_ip: Option<&str>,
        user_agent: Option<&str>,
        path: &str,
        method: &str,
        reason: &str,
        auth_channel: &str,
    ) {
        let client_key = Self::auth_client_key(client_ip);
        self.auth_rate_limiter.record_failure(&client_key).await;

        let Some(sqlite_store) = &self.sqlite_store else {
            return;
        };

        let metadata = serde_json::json!({
            "reason": reason,
            "auth_channel": auth_channel,
            "path": path,
            "method": method,
            "client_ip": client_ip,
            "user_agent": user_agent,
        });
        let metadata_json = serde_json::to_string(&metadata).ok();
        if let Err(err) = sqlite_store
            .insert_audit_event(
                None,
                None,
                None,
                None,
                "auth.failure",
                "auth",
                None,
                "denied",
                None,
                None,
                None,
                metadata_json.as_deref(),
            )
            .await
        {
            warn!(error = %err, "Failed to persist failed-auth audit event");
        }
    }

    /// Initialize the store with bootstrap keys from environment.
    pub async fn init_from_env(&self) {
        info!(
            mode = self.runtime_auth_mode.as_str(),
            "Resolved authentication mode"
        );

        match self.auth_hmac_secret_source {
            HmacSecretSource::ExplicitEnv => {
                info!("Using explicit MLRUNX_AUTH_HMAC_SECRET for auth token hashing");
            }
            HmacSecretSource::JwtSecretFallback => {
                info!(
                    "MLRUNX_AUTH_HMAC_SECRET not set; reusing MLRUNX_JWT_SECRET for auth token hashing"
                );
            }
            HmacSecretSource::InsecureDefault => {
                warn!(
                    "MLRUNX_AUTH_HMAC_SECRET is not configured; using insecure built-in fallback. \
Set MLRUNX_AUTH_HMAC_SECRET in all non-local environments."
                );
            }
        }

        if self.ui_jwt.enabled {
            let configured = match self.ui_jwt.algorithm {
                UiJwtAlgorithm::Hs256 => self.ui_jwt.secret.is_some(),
                UiJwtAlgorithm::Rs256 | UiJwtAlgorithm::Es256 => {
                    self.ui_jwt.public_key_pem.is_some()
                }
            };

            if configured {
                info!(
                    algorithm = self.ui_jwt.algorithm.env_value(),
                    provider = %self.ui_jwt.provider,
                    "UI JWT auth enabled (feature-flagged)"
                );
            } else {
                let missing = match self.ui_jwt.algorithm {
                    UiJwtAlgorithm::Hs256 => "MLRUNX_JWT_SECRET",
                    UiJwtAlgorithm::Rs256 | UiJwtAlgorithm::Es256 => "MLRUNX_JWT_PUBLIC_KEY_PEM",
                };
                warn!(
                    algorithm = self.ui_jwt.algorithm.env_value(),
                    missing,
                    "UI JWT auth is enabled but JWT verifier material is missing; JWT auth path will reject tokens"
                );
            }
        }

        // Check for bootstrap key
        if let Ok(bootstrap_key) = std::env::var("MLRUNX_API_KEY") {
            if !bootstrap_key.is_empty() {
                let key = self.create_key_from_raw(
                    &bootstrap_key,
                    None, // Global admin key
                    Some("bootstrap".to_string()),
                    vec!["admin".to_string()],
                    None, // Bootstrap keys don't expire
                );
                let mut key = key;
                key.id = "bootstrap".to_string();
                let key_fingerprint = self.hash_with_hmac(&bootstrap_key);

                if let Some(sqlite_store) = &self.sqlite_store {
                    if let Err(err) = sqlite_store
                        .upsert_bootstrap_api_key(
                            &key.id,
                            &key.key_hash,
                            Some(&key_fingerprint),
                            &key.key_prefix,
                        )
                        .await
                    {
                        warn!("Failed to persist bootstrap API key: {err}");
                    }
                }

                let mut keys = self.keys.write().await;
                keys.insert(key.key_hash.clone(), key);
                info!("Loaded bootstrap API key from environment");
            }
        }
    }

    /// Create an API key from a raw key string.
    fn hash_with_hmac(&self, value: &str) -> String {
        hmac_sha256_hex(&self.auth_hmac_secret, value.as_bytes())
    }

    /// Create an API key from a raw key string.
    fn create_key_from_raw(
        &self,
        raw_key: &str,
        project_id: Option<String>,
        name: Option<String>,
        scopes: Vec<String>,
        expires_at: Option<SystemTime>,
    ) -> ApiKey {
        let key_hash = hash_api_key_argon2(raw_key).unwrap_or_else(|err| {
            warn!("Failed to hash API key with argon2id, falling back to legacy hash: {err}");
            hash_api_key(raw_key)
        });
        let key_prefix = raw_key.chars().take(8).collect();

        ApiKey {
            id: uuid::Uuid::now_v7().to_string(),
            key_hash,
            key_prefix,
            project_id,
            name,
            scopes,
            created_at: std::time::SystemTime::now(),
            last_used_at: None,
            revoked_at: None,
            expires_at,
        }
    }

    /// Validate an API key and return the key info if valid.
    pub async fn validate_key(&self, raw_key: &str) -> Option<ApiKey> {
        let key_fingerprint = self.hash_with_hmac(raw_key);
        let legacy_hash = hash_api_key(raw_key);

        // Primary path: durable sqlite store.
        if let Some(sqlite_store) = &self.sqlite_store {
            match sqlite_store
                .get_api_key_by_fingerprint(&key_fingerprint)
                .await
            {
                Ok(Some(row)) => {
                    if row.revoked_at.is_some() {
                        return None;
                    }

                    if !verify_api_key_hash(raw_key, &row.key_hash) {
                        return None;
                    }

                    let mut key = Self::api_key_from_row(row);

                    // Reject expired keys.
                    if !key.is_valid() {
                        return None;
                    }

                    if let Err(err) = sqlite_store.touch_api_key_last_used(&key.key_hash).await {
                        warn!("Failed to update API key last_used_at: {err}");
                    } else {
                        key.last_used_at = Some(SystemTime::now());
                    }

                    let mut keys = self.keys.write().await;
                    keys.insert(key.key_hash.clone(), key.clone());
                    return Some(key);
                }
                Ok(None) => {}
                Err(err) => {
                    warn!(
                        "Failed to validate API key by fingerprint from sqlite, falling back to legacy lookup: {err}"
                    );
                }
            }

            // Backward compatibility for legacy rows (pre-fingerprint or pre-argon2 migration).
            match sqlite_store.get_api_key_by_hash(&legacy_hash).await {
                Ok(Some(row)) => {
                    if row.revoked_at.is_some() {
                        return None;
                    }

                    if !verify_api_key_hash(raw_key, &row.key_hash) {
                        return None;
                    }

                    let mut key = Self::api_key_from_row(row);

                    // Reject expired keys.
                    if !key.is_valid() {
                        return None;
                    }

                    if let Err(err) = sqlite_store.touch_api_key_last_used(&key.key_hash).await {
                        warn!("Failed to update API key last_used_at: {err}");
                    } else {
                        key.last_used_at = Some(SystemTime::now());
                    }

                    let mut keys = self.keys.write().await;
                    keys.insert(key.key_hash.clone(), key.clone());
                    return Some(key);
                }
                Ok(None) => {}
                Err(err) => {
                    warn!(
                        "Failed to validate API key by legacy hash from sqlite, falling back to memory: {err}"
                    );
                }
            }
        }

        // Fallback path: in-memory store.
        let mut keys = self.keys.write().await;

        for key in keys.values_mut() {
            if key.is_valid() && verify_api_key_hash(raw_key, &key.key_hash) {
                key.last_used_at = Some(SystemTime::now());
                return Some(key.clone());
            }
        }

        None
    }

    /// Create a new API key.
    pub async fn create_key(
        &self,
        project_id: Option<String>,
        name: Option<String>,
        scopes: Vec<String>,
        expires_in_seconds: Option<u64>,
    ) -> (String, ApiKey) {
        // Generate a random key
        let raw_key = generate_api_key();
        let expires_at = expires_in_seconds.map(|secs| SystemTime::now() + Duration::from_secs(secs));
        let key = self.create_key_from_raw(&raw_key, project_id, name, scopes, expires_at);

        if let Some(sqlite_store) = &self.sqlite_store {
            let scopes_json =
                serde_json::to_string(&key.scopes).unwrap_or_else(|_| "[]".to_string());
            let expires_at_str = key.expires_at.map(|t| {
                let dt = chrono::DateTime::<chrono::Utc>::from(t);
                dt.naive_utc().format("%Y-%m-%d %H:%M:%S").to_string()
            });
            if let Err(err) = sqlite_store
                .insert_api_key(
                    &key.id,
                    &key.key_hash,
                    Some(&self.hash_with_hmac(&raw_key)),
                    &key.key_prefix,
                    key.project_id.as_deref(),
                    key.name.as_deref(),
                    &scopes_json,
                    expires_at_str.as_deref(),
                )
                .await
            {
                warn!("Failed to persist API key, using in-memory fallback: {err}");
            }
        }

        let mut keys = self.keys.write().await;
        keys.insert(key.key_hash.clone(), key.clone());

        (raw_key, key)
    }

    /// Rotate the bootstrap API key: revoke the old one and create a new one.
    /// Returns the new raw key on success.
    pub async fn rotate_bootstrap_key(&self) -> Result<String, String> {
        // Revoke the old bootstrap key if it exists.
        let mut keys = self.keys.write().await;
        if let Some(old_key) = keys.get_mut("bootstrap") {
            old_key.revoked_at = Some(SystemTime::now());
        }
        // Also try to revoke by ID in the in-memory map.
        for key in keys.values_mut() {
            if key.id == "bootstrap" && key.revoked_at.is_none() {
                key.revoked_at = Some(SystemTime::now());
            }
        }
        drop(keys);

        // Revoke in sqlite too.
        if let Some(sqlite_store) = &self.sqlite_store {
            let _ = sqlite_store.revoke_api_key_by_id("bootstrap").await;
        }

        // Generate a new bootstrap key.
        let raw_key = generate_api_key();
        let mut new_key = self.create_key_from_raw(
            &raw_key,
            None,
            Some("bootstrap".to_string()),
            vec!["admin".to_string()],
            None,
        );
        new_key.id = "bootstrap".to_string();
        let key_fingerprint = self.hash_with_hmac(&raw_key);

        if let Some(sqlite_store) = &self.sqlite_store {
            let scopes_json = serde_json::to_string(&new_key.scopes)
                .unwrap_or_else(|_| r#"["admin"]"#.to_string());
            sqlite_store
                .upsert_bootstrap_api_key(
                    &new_key.id,
                    &new_key.key_hash,
                    Some(&key_fingerprint),
                    &new_key.key_prefix,
                )
                .await
                .map_err(|e| format!("Failed to persist rotated bootstrap key: {e}"))?;

            // Also update scopes in case upsert doesn't handle them.
            let _ = sqlite_store
                .insert_api_key(
                    &new_key.id,
                    &new_key.key_hash,
                    Some(&key_fingerprint),
                    &new_key.key_prefix,
                    None,
                    Some("bootstrap"),
                    &scopes_json,
                    None,
                )
                .await;
        }

        let mut keys = self.keys.write().await;
        keys.insert(new_key.key_hash.clone(), new_key);

        info!("Bootstrap API key rotated successfully");
        Ok(raw_key)
    }

    /// Revoke an API key.
    pub async fn revoke_key(&self, key_hash: &str) -> bool {
        if let Some(sqlite_store) = &self.sqlite_store {
            match sqlite_store.revoke_api_key_by_hash(key_hash).await {
                Ok(true) => {
                    let mut keys = self.keys.write().await;
                    if let Some(key) = keys.get_mut(key_hash) {
                        key.revoked_at = Some(SystemTime::now());
                    }
                    return true;
                }
                Ok(false) => {}
                Err(err) => {
                    warn!("Failed to revoke API key in sqlite, falling back to memory: {err}");
                }
            }
        }

        let mut keys = self.keys.write().await;

        if let Some(key) = keys.get_mut(key_hash) {
            key.revoked_at = Some(SystemTime::now());
            return true;
        }

        false
    }

    /// List all keys for a project.
    pub async fn list_keys(&self, project_id: Option<&str>) -> Vec<ApiKey> {
        if let Some(sqlite_store) = &self.sqlite_store {
            match sqlite_store.list_api_keys(project_id).await {
                Ok(rows) => return rows.into_iter().map(Self::api_key_from_row).collect(),
                Err(err) => {
                    warn!("Failed to list API keys from sqlite, falling back to memory: {err}");
                }
            }
        }

        let keys = self.keys.read().await;

        keys.values()
            .filter(|k| {
                if let Some(pid) = project_id {
                    k.project_id.as_ref().map_or(false, |p| p == pid)
                } else {
                    true
                }
            })
            .cloned()
            .collect()
    }

    pub async fn create_ui_session_from_jwt(
        &self,
        raw_jwt: &str,
        user_agent: Option<&str>,
        client_ip: Option<&str>,
    ) -> Result<UiSessionIssue, String> {
        if !self.ui_jwt.enabled {
            return Err("UI JWT auth is disabled.".to_string());
        }

        let sqlite_store = self
            .sqlite_store
            .as_ref()
            .ok_or_else(|| "SQLite-backed auth store is required for UI JWT auth.".to_string())?;

        let identity = self.resolve_ui_identity_from_jwt(raw_jwt).await?;

        let session_token = generate_session_token();
        let csrf_token = generate_session_token();
        let token_hash = self.hash_with_hmac(&session_token);
        let csrf_hash = self.hash_with_hmac(&csrf_token);
        let expires_at = (chrono::Utc::now()
            + chrono::Duration::seconds(self.ui_jwt.session_ttl_seconds as i64))
        .naive_utc()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
        let session_id = uuid::Uuid::now_v7().to_string();

        sqlite_store
            .insert_auth_session(
                &session_id,
                &identity.user_id,
                &token_hash,
                &csrf_hash,
                &expires_at,
                user_agent,
                client_ip,
            )
            .await
            .map_err(|e| format!("Failed to persist UI session: {e}"))?;

        let mut allowed_project_ids: Vec<String> =
            identity.allowed_project_ids.into_iter().collect();
        allowed_project_ids.sort();

        Ok(UiSessionIssue {
            session_token,
            csrf_token,
            expires_at,
            user_id: identity.user_id,
            allowed_project_ids,
        })
    }

    async fn authenticate_ui_jwt(
        &self,
        raw_token: &str,
    ) -> Result<(ApiKey, HashSet<String>, bool), String> {
        let identity = self.resolve_ui_identity_from_jwt(raw_token).await?;
        let api_key = Self::build_ui_auth_api_key(&identity, "jwt");
        Ok((api_key, identity.allowed_project_ids, identity.is_platform_admin))
    }

    async fn authenticate_ui_session(
        &self,
        raw_session_token: &str,
        csrf_token: Option<&str>,
        require_csrf: bool,
    ) -> Result<(ApiKey, HashSet<String>, bool), String> {
        if !self.ui_jwt.enabled {
            return Err("UI JWT auth is disabled.".to_string());
        }

        let sqlite_store = self.sqlite_store.as_ref().ok_or_else(|| {
            "SQLite-backed auth store is required for UI session auth.".to_string()
        })?;

        let token_hash = self.hash_with_hmac(raw_session_token);
        let session = if let Some(session) = sqlite_store
            .get_active_auth_session_by_token_hash(&token_hash)
            .await
            .map_err(|e| format!("Failed to resolve UI session: {e}"))?
        {
            session
        } else {
            let legacy_token_hash = hash_api_key(raw_session_token);
            sqlite_store
                .get_active_auth_session_by_token_hash(&legacy_token_hash)
                .await
                .map_err(|e| format!("Failed to resolve legacy UI session: {e}"))?
                .ok_or_else(|| "Invalid or expired UI session.".to_string())?
        };

        if require_csrf {
            let provided = csrf_token
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| "Missing CSRF token for mutating request.".to_string())?;
            let provided_hash = self.hash_with_hmac(provided);
            let provided_legacy_hash = hash_api_key(provided);
            if provided_hash != session.csrf_hash && provided_legacy_hash != session.csrf_hash {
                return Err("Invalid CSRF token.".to_string());
            }
        }

        let (scopes, allowed_project_ids) = self
            .resolve_memberships_for_user(&session.user_id, None, None)
            .await
            .map_err(|e| format!("Failed to resolve UI session memberships: {e}"))?;

        if let Err(err) = sqlite_store.touch_auth_session(&session.id).await {
            warn!("Failed to update UI session last_seen_at: {err}");
        }

        let is_platform_admin = sqlite_store
            .is_user_platform_admin(&session.user_id)
            .await
            .unwrap_or(false);

        let identity = ResolvedUiIdentity {
            user_id: session.user_id,
            user_name: None,
            allowed_project_ids,
            scopes,
            is_platform_admin,
        };
        let api_key = Self::build_ui_auth_api_key(&identity, "session");
        Ok((api_key, identity.allowed_project_ids, identity.is_platform_admin))
    }

    pub async fn revoke_ui_session(
        &self,
        raw_session_token: &str,
        csrf_token: Option<&str>,
    ) -> Result<(), String> {
        if !self.ui_jwt.enabled {
            return Ok(());
        }

        let sqlite_store = self.sqlite_store.as_ref().ok_or_else(|| {
            "SQLite-backed auth store is required for UI session auth.".to_string()
        })?;

        let token_hash = self.hash_with_hmac(raw_session_token);
        let session = if let Some(session) = sqlite_store
            .get_active_auth_session_by_token_hash(&token_hash)
            .await
            .map_err(|e| format!("Failed to resolve UI session: {e}"))?
        {
            Some((session, token_hash.clone()))
        } else {
            let legacy_token_hash = hash_api_key(raw_session_token);
            sqlite_store
                .get_active_auth_session_by_token_hash(&legacy_token_hash)
                .await
                .map_err(|e| format!("Failed to resolve legacy UI session: {e}"))?
                .map(|session| (session, legacy_token_hash))
        };

        let token_hash_to_revoke = if let Some((session, resolved_hash)) = session {
            let provided = csrf_token
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| "Missing CSRF token.".to_string())?;
            let provided_hash = self.hash_with_hmac(provided);
            let provided_legacy_hash = hash_api_key(provided);
            if provided_hash != session.csrf_hash && provided_legacy_hash != session.csrf_hash {
                return Err("Invalid CSRF token.".to_string());
            }
            resolved_hash
        } else {
            token_hash
        };

        sqlite_store
            .revoke_auth_session_by_token_hash(&token_hash_to_revoke)
            .await
            .map_err(|e| format!("Failed to revoke UI session: {e}"))?;

        Ok(())
    }

    async fn resolve_ui_identity_from_jwt(
        &self,
        raw_token: &str,
    ) -> Result<ResolvedUiIdentity, String> {
        if !self.ui_jwt.enabled {
            return Err("UI JWT auth is disabled.".to_string());
        }

        let claims = self.decode_ui_jwt_claims(raw_token)?;
        let sqlite_store = self
            .sqlite_store
            .as_ref()
            .ok_or_else(|| "SQLite-backed auth store is required for UI JWT auth.".to_string())?;

        let user_id = sqlite_store
            .get_or_create_user_identity(
                &self.ui_jwt.provider,
                claims.subject(),
                claims.email.as_deref(),
                claims.name.as_deref(),
            )
            .await
            .map_err(|e| format!("Failed to resolve JWT user identity: {e}"))?;

        let (scopes, allowed_project_ids) = self
            .resolve_memberships_for_user(&user_id, claims.email.as_deref(), claims.name.as_deref())
            .await?;

        let is_platform_admin = sqlite_store
            .sync_platform_admin_flag(&user_id, self.admin_email.as_deref())
            .await
            .unwrap_or(false);

        Ok(ResolvedUiIdentity {
            user_id,
            user_name: claims.email.or(claims.name),
            allowed_project_ids,
            scopes,
            is_platform_admin,
        })
    }

    async fn resolve_memberships_for_user(
        &self,
        user_id: &str,
        email: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<(Vec<String>, HashSet<String>), String> {
        let sqlite_store = self
            .sqlite_store
            .as_ref()
            .ok_or_else(|| "SQLite-backed auth store is required for UI auth.".to_string())?;

        let mut memberships = sqlite_store
            .list_active_project_memberships(user_id)
            .await
            .map_err(|e| format!("Failed to load project memberships: {e}"))?;

        if memberships.is_empty() && self.ui_jwt.auto_provision_project {
            self.auto_provision_personal_project(user_id, email, display_name)
                .await?;

            memberships = sqlite_store
                .list_active_project_memberships(user_id)
                .await
                .map_err(|e| format!("Failed to load memberships after auto-provision: {e}"))?;
        }

        if memberships.is_empty() {
            return Err("No active project memberships found for this user.".to_string());
        }

        let (mut scopes, allowed_projects) = Self::derive_scopes_and_projects(&memberships);
        scopes.sort();
        Ok((scopes, allowed_projects))
    }

    async fn auto_provision_personal_project(
        &self,
        user_id: &str,
        email: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<(), String> {
        let sqlite_store = self
            .sqlite_store
            .as_ref()
            .ok_or_else(|| "SQLite-backed auth store is required for UI auth.".to_string())?;

        let project_name = self.personal_project_name(user_id, email, display_name);
        let project_id = sqlite_store
            .get_or_create_project(&project_name)
            .await
            .map_err(|e| format!("Failed to create personal project: {e}"))?;

        sqlite_store
            .grant_project_membership(&project_id, user_id, "owner", Some(user_id))
            .await
            .map_err(|e| format!("Failed to grant personal project membership: {e}"))?;

        info!(
            user_id = %user_id,
            project_id = %project_id,
            project_name = %project_name,
            "Auto-provisioned personal project membership for UI user",
        );

        Ok(())
    }

    fn personal_project_name(
        &self,
        user_id: &str,
        email: Option<&str>,
        display_name: Option<&str>,
    ) -> String {
        let prefix = Self::slugify_personal_project_component(&self.ui_jwt.personal_project_prefix);
        let prefix = if prefix.is_empty() {
            "personal".to_string()
        } else {
            prefix
        };

        let hint_source = email
            .and_then(|value| value.split('@').next())
            .or(display_name)
            .unwrap_or("user");
        let hint = Self::slugify_personal_project_component(hint_source);
        let hint = if hint.is_empty() {
            "user".to_string()
        } else {
            hint
        };

        let user_suffix: String = user_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(8)
            .collect();
        let suffix = if user_suffix.is_empty() {
            "account".to_string()
        } else {
            user_suffix.to_ascii_lowercase()
        };

        format!("{prefix}-{hint}-{suffix}")
    }

    fn slugify_personal_project_component(value: &str) -> String {
        let mut normalized = String::with_capacity(value.len());
        let mut previous_dash = false;

        for ch in value.trim().chars() {
            if ch.is_ascii_alphanumeric() {
                normalized.push(ch.to_ascii_lowercase());
                previous_dash = false;
            } else if !previous_dash {
                normalized.push('-');
                previous_dash = true;
            }
        }

        normalized.trim_matches('-').to_string()
    }

    fn build_ui_auth_api_key(identity: &ResolvedUiIdentity, mode_prefix: &str) -> ApiKey {
        let now = SystemTime::now();
        let key_prefix = format!(
            "{mode_prefix}_{}",
            identity.user_id.chars().take(6).collect::<String>()
        );
        ApiKey {
            id: format!("{mode_prefix}:{}", identity.user_id),
            key_hash: format!("{mode_prefix}:{}", identity.user_id),
            key_prefix,
            project_id: None,
            name: identity.user_name.clone(),
            scopes: identity.scopes.clone(),
            created_at: now,
            last_used_at: Some(now),
            revoked_at: None,
            expires_at: None,
        }
    }

    fn decode_ui_jwt_claims(&self, raw_token: &str) -> Result<UiJwtClaims, String> {
        let mut validation = Validation::new(self.ui_jwt.algorithm.jsonwebtoken_algorithm());
        // Require exp claim — reject tokens that never expire.
        validation.validate_exp = true;
        let issuer = self.ui_jwt.issuer.as_ref().ok_or_else(|| {
            "MLRUNX_JWT_ISSUER is required when UI JWT auth is enabled.".to_string()
        })?;
        let audience = self.ui_jwt.audience.as_ref().ok_or_else(|| {
            "MLRUNX_JWT_AUDIENCE is required when UI JWT auth is enabled.".to_string()
        })?;
        validation.set_issuer(&[issuer.clone()]);
        validation.set_audience(&[audience.clone()]);

        let decoding_key =
            match self.ui_jwt.algorithm {
                UiJwtAlgorithm::Hs256 => {
                    let secret = self
                        .ui_jwt
                        .secret
                        .as_ref()
                        .ok_or_else(|| "MLRUNX_JWT_SECRET is not configured.".to_string())?;
                    DecodingKey::from_secret(secret.as_bytes())
                }
                UiJwtAlgorithm::Rs256 => {
                    let pem = self.ui_jwt.public_key_pem.as_ref().ok_or_else(|| {
                        "MLRUNX_JWT_PUBLIC_KEY_PEM is not configured.".to_string()
                    })?;
                    DecodingKey::from_rsa_pem(pem.as_bytes())
                        .map_err(|e| format!("Invalid RSA public key PEM: {e}"))?
                }
                UiJwtAlgorithm::Es256 => {
                    let pem = self.ui_jwt.public_key_pem.as_ref().ok_or_else(|| {
                        "MLRUNX_JWT_PUBLIC_KEY_PEM is not configured.".to_string()
                    })?;
                    DecodingKey::from_ec_pem(pem.as_bytes())
                        .map_err(|e| format!("Invalid EC public key PEM: {e}"))?
                }
            };

        let token_data = decode::<UiJwtClaims>(raw_token, &decoding_key, &validation)
            .map_err(|e| format!("Invalid JWT token: {e}"))?;

        if token_data.claims.subject().is_empty() {
            return Err("JWT token subject is empty.".to_string());
        }

        Ok(token_data.claims)
    }

    fn derive_scopes_and_projects(
        memberships: &[ProjectMembershipRow],
    ) -> (Vec<String>, HashSet<String>) {
        let mut scopes: HashSet<String> = HashSet::new();
        let mut allowed_projects: HashSet<String> = HashSet::new();

        for membership in memberships {
            allowed_projects.insert(membership.project_id.clone());

            match membership.role.as_str() {
                "owner" => {
                    scopes.insert("read".to_string());
                    scopes.insert("write".to_string());
                    scopes.insert("admin".to_string());
                }
                "editor" => {
                    scopes.insert("read".to_string());
                    scopes.insert("write".to_string());
                }
                "viewer" => {
                    scopes.insert("read".to_string());
                }
                _ => {}
            }
        }

        (scopes.into_iter().collect(), allowed_projects)
    }

    fn api_key_from_row(row: ApiKeyRow) -> ApiKey {
        let scopes = serde_json::from_str::<Vec<String>>(&row.scopes_json).unwrap_or_default();

        ApiKey {
            id: row.id,
            key_hash: row.key_hash,
            key_prefix: row.key_prefix,
            project_id: row.project_id,
            name: row.name,
            scopes,
            created_at: parse_sqlite_datetime(&row.created_at).unwrap_or_else(SystemTime::now),
            last_used_at: row.last_used_at.as_deref().and_then(parse_sqlite_datetime),
            revoked_at: row.revoked_at.as_deref().and_then(parse_sqlite_datetime),
            expires_at: row.expires_at.as_deref().and_then(parse_sqlite_datetime),
        }
    }
}

fn parse_sqlite_datetime(value: &str) -> Option<SystemTime> {
    let naive = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").ok()?;
    let timestamp =
        chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc).timestamp();

    if timestamp >= 0 {
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(timestamp as u64))
    } else {
        SystemTime::UNIX_EPOCH.checked_sub(Duration::from_secs((-timestamp) as u64))
    }
}

#[derive(Debug, Deserialize)]
struct UiJwtClaims {
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default, alias = "display_name", alias = "preferred_username")]
    name: Option<String>,
}

impl UiJwtClaims {
    fn subject(&self) -> &str {
        self.sub
            .as_deref()
            .or(self.user_id.as_deref())
            .map(str::trim)
            .unwrap_or_default()
    }
}

/// Legacy API key hashing (SHA-256) retained for backward-compatible verification.
pub fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

fn hash_api_key_argon2(key: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(key.as_bytes(), &salt)
        .map(|value| value.to_string())
        .map_err(|e| format!("argon2id hashing failed: {e}"))
}

fn verify_api_key_hash(raw_key: &str, stored_hash: &str) -> bool {
    if stored_hash.starts_with("$argon2") {
        let parsed = match PasswordHash::new(stored_hash) {
            Ok(parsed) => parsed,
            Err(_) => return false,
        };
        return Argon2::default()
            .verify_password(raw_key.as_bytes(), &parsed)
            .is_ok();
    }
    hash_api_key(raw_key) == stored_hash
}

fn hmac_sha256_hex(secret: &[u8], message: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret).expect("HMAC-SHA256 accepts keys of any length");
    mac.update(message);
    hex::encode(mac.finalize().into_bytes())
}

fn load_auth_hmac_secret() -> (Vec<u8>, HmacSecretSource) {
    if let Ok(secret) = std::env::var("MLRUNX_AUTH_HMAC_SECRET") {
        let trimmed = secret.trim();
        if !trimmed.is_empty() {
            return (trimmed.as_bytes().to_vec(), HmacSecretSource::ExplicitEnv);
        }
    }

    if let Ok(secret) = std::env::var("MLRUNX_JWT_SECRET") {
        let trimmed = secret.trim();
        if !trimmed.is_empty() {
            return (
                trimmed.as_bytes().to_vec(),
                HmacSecretSource::JwtSecretFallback,
            );
        }
    }

    (
        b"mlrunx-insecure-default-hmac-secret-change-me".to_vec(),
        HmacSecretSource::InsecureDefault,
    )
}

fn load_admin_email() -> Option<String> {
    std::env::var("MLRUNX_ADMIN_EMAIL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Generate a random API key.
pub fn generate_api_key() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
    format!("mlrunx_{}", hex::encode(bytes))
}

/// Generate a random token for UI session/csrf cookies.
pub fn generate_session_token() -> String {
    use base64::Engine;
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Authenticated user context extracted from request.
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// The API key used for authentication
    pub api_key: ApiKey,
    /// Whether authentication is bypassed (dev mode)
    pub is_dev_mode: bool,
    /// Whether this user is a designated platform admin.
    pub is_platform_admin: bool,
    /// Optional set of project memberships (used by JWT user auth path).
    allowed_project_ids: Option<HashSet<String>>,
    /// Source auth mode.
    pub auth_mode: AuthMode,
    /// Unique request identifier for audit trail correlation.
    pub request_id: String,
    /// Client IP address extracted from the request.
    pub client_ip: Option<String>,
    /// User-Agent header value from the request.
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    ApiKey,
    UiJwt,
}

impl AuthContext {
    /// Create a dev mode context (no authentication).
    pub fn dev_mode() -> Self {
        Self {
            api_key: ApiKey {
                id: "dev".to_string(),
                key_hash: "dev".to_string(),
                key_prefix: "dev".to_string(),
                project_id: None,
                name: Some("Dev Mode".to_string()),
                scopes: vec!["admin".to_string()],
                created_at: std::time::SystemTime::now(),
                last_used_at: None,
                revoked_at: None,
                expires_at: None,
            },
            is_dev_mode: true,
            is_platform_admin: true,
            allowed_project_ids: None,
            auth_mode: AuthMode::ApiKey,
            request_id: uuid::Uuid::now_v7().to_string(),
            client_ip: None,
            user_agent: None,
        }
    }

    /// Get the project_id this key is scoped to (None = global/admin).
    pub fn project_id(&self) -> Option<&str> {
        if let Some(allowed_projects) = &self.allowed_project_ids {
            if allowed_projects.len() == 1 {
                return allowed_projects.iter().next().map(|p| p.as_str());
            }
            return None;
        }
        self.api_key.project_id.as_deref()
    }

    /// Returns true if this context has global access (dev mode or admin with no project scope).
    pub fn is_global(&self) -> bool {
        self.is_dev_mode
            || self.is_platform_admin
            || (self.allowed_project_ids.is_none() && self.api_key.project_id.is_none())
    }

    /// Returns the set of allowed projects for JWT user auth mode.
    pub fn allowed_project_ids(&self) -> Option<&HashSet<String>> {
        self.allowed_project_ids.as_ref()
    }

    /// Returns true when request authenticated via UI JWT path.
    pub fn is_ui_jwt(&self) -> bool {
        self.auth_mode == AuthMode::UiJwt
    }

    /// Check if the caller can access a specific project's resources.
    /// Returns Ok(()) if allowed, Err with 403 if denied.
    pub fn require_project_access(&self, run_project_id: &str) -> Result<(), (StatusCode, String)> {
        if self.is_global() {
            return Ok(());
        }

        if let Some(allowed_projects) = &self.allowed_project_ids {
            if allowed_projects.contains(run_project_id) {
                return Ok(());
            }
            return Err((
                StatusCode::FORBIDDEN,
                "Access denied: this user cannot access runs in this project.".to_string(),
            ));
        }

        if self.api_key.can_access_project(run_project_id) {
            return Ok(());
        }
        Err((
            StatusCode::FORBIDDEN,
            "Access denied: this API key cannot access runs in this project.".to_string(),
        ))
    }

    /// Check if the caller has a required scope.
    /// Returns Ok(()) if allowed, Err with 403 if insufficient.
    pub fn require_scope(&self, scope: &str) -> Result<(), (StatusCode, String)> {
        if self.is_dev_mode {
            return Ok(()); // dev mode has all scopes
        }
        if self.api_key.has_scope(scope) {
            return Ok(());
        }
        Err((
            StatusCode::FORBIDDEN,
            format!(
                "Insufficient permissions: this API key requires the '{}' scope.",
                scope
            ),
        ))
    }

    /// Check both project access AND required scope in one call.
    pub fn require_access(
        &self,
        run_project_id: &str,
        scope: &str,
    ) -> Result<(), (StatusCode, String)> {
        self.require_scope(scope)?;
        self.require_project_access(run_project_id)?;
        Ok(())
    }
}

/// Authentication error types.
#[derive(Debug, Clone)]
pub enum AuthError {
    /// No API key provided
    MissingKey,
    /// Invalid API key
    InvalidKey,
    /// Key doesn't have required scope
    InsufficientScope,
    /// Key cannot access requested project
    ProjectAccessDenied,
}

impl AuthError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            AuthError::MissingKey => StatusCode::UNAUTHORIZED,
            AuthError::InvalidKey => StatusCode::UNAUTHORIZED,
            AuthError::InsufficientScope => StatusCode::FORBIDDEN,
            AuthError::ProjectAccessDenied => StatusCode::FORBIDDEN,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            AuthError::MissingKey => {
                "Authentication required. Use X-API-Key, Authorization: Bearer <token>, or a valid UI session cookie."
            }
            AuthError::InvalidKey => "Invalid API key, JWT token, or UI session.",
            AuthError::InsufficientScope => "Insufficient permissions.",
            AuthError::ProjectAccessDenied => "Access to project denied.",
        }
    }
}

enum RequestCredential {
    ApiKey(String),
    Bearer(String),
    UiSession {
        session_token: String,
        csrf_token: Option<String>,
        require_csrf: bool,
    },
    Missing,
}

/// Extract auth credential from request headers.
fn extract_request_credential(
    parts: &Parts,
    key_store: &ApiKeyStore,
    method: &Method,
) -> RequestCredential {
    // Prefer explicit X-API-Key for SDK/service callers.
    if let Some(key_header) = parts.headers.get("x-api-key") {
        if let Ok(key_str) = key_header.to_str() {
            let value = key_str.trim();
            if !value.is_empty() {
                return RequestCredential::ApiKey(value.to_string());
            }
        }
    }

    // Fallback to Authorization: Bearer <token>.
    if let Some(auth_header) = parts.headers.get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                let value = token.trim();
                if !value.is_empty() {
                    return RequestCredential::Bearer(value.to_string());
                }
            }
        }
    }

    if key_store.is_ui_jwt_enabled() {
        if let Some(session_token) = extract_cookie(parts, key_store.ui_session_cookie_name()) {
            return RequestCredential::UiSession {
                session_token,
                csrf_token: extract_header_token(parts, "x-csrf-token"),
                require_csrf: requires_csrf(method),
            };
        }
    }

    RequestCredential::Missing
}

fn extract_header_token(parts: &Parts, header_name: &str) -> Option<String> {
    parts
        .headers
        .get(header_name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn extract_cookie(parts: &Parts, cookie_name: &str) -> Option<String> {
    let cookie_header = parts.headers.get("cookie")?.to_str().ok()?;
    cookie_header.split(';').find_map(|part| {
        let mut kv = part.trim().splitn(2, '=');
        let name = kv.next()?.trim();
        let value = kv.next()?.trim();
        if name == cookie_name && !value.is_empty() {
            Some(value.to_string())
        } else {
            None
        }
    })
}

fn extract_client_ip(parts: &Parts) -> Option<String> {
    let forwarded = parts
        .headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if let Some(forwarded) = forwarded {
        let first = forwarded
            .split(',')
            .next()
            .map(str::trim)
            .unwrap_or_default();
        if !first.is_empty() {
            return Some(first.to_string());
        }
    }
    parts
        .headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

fn extract_user_agent(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

fn requires_csrf(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

/// Middleware for API key authentication.
pub async fn auth_middleware(
    State(key_store): State<Arc<ApiKeyStore>>,
    mut request: Request,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    // Check if auth is disabled (dev mode)
    if key_store.is_auth_disabled() {
        // Insert dev mode context
        request.extensions_mut().insert(AuthContext::dev_mode());
        return Ok(next.run(request).await);
    }

    let request_method = request.method().as_str().to_string();
    let request_path = request.uri().path().to_string();

    // Extract API key from headers.
    let raw_key = {
        let (parts, body) = request.into_parts();
        let client_ip = extract_client_ip(&parts);
        let user_agent = extract_user_agent(&parts);
        let key = extract_request_credential(&parts, key_store.as_ref(), &parts.method);
        request = Request::from_parts(parts, body);
        (key, client_ip, user_agent)
    };
    let (raw_key, client_ip, user_agent) = raw_key;

    if let Some(retry_after) = key_store
        .auth_rate_limit_retry_after_seconds(client_ip.as_deref())
        .await
    {
        key_store
            .record_auth_failure(
                client_ip.as_deref(),
                user_agent.as_deref(),
                &request_path,
                &request_method,
                "rate_limited",
                "middleware",
            )
            .await;
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "Too many failed authentication attempts. Retry in {}s.",
                retry_after
            ),
        ));
    }

    let (api_key, auth_mode, allowed_project_ids, is_platform_admin) = match raw_key {
        RequestCredential::Missing => {
            return Err((
                AuthError::MissingKey.status_code(),
                AuthError::MissingKey.message().to_string(),
            ));
        }
        RequestCredential::ApiKey(raw_key) => {
            let api_key = match key_store.validate_key(&raw_key).await {
                Some(api_key) => api_key,
                None => {
                    warn!(key_prefix = %raw_key.chars().take(8).collect::<String>(), "Invalid API key");
                    key_store
                        .record_auth_failure(
                            client_ip.as_deref(),
                            user_agent.as_deref(),
                            &request_path,
                            &request_method,
                            "invalid_api_key",
                            "x-api-key",
                        )
                        .await;
                    return Err((
                        AuthError::InvalidKey.status_code(),
                        AuthError::InvalidKey.message().to_string(),
                    ));
                }
            };
            (api_key, AuthMode::ApiKey, None, false)
        }
        RequestCredential::Bearer(raw_token) => {
            // Preserve existing SDK compatibility: first treat Bearer as API key.
            if let Some(api_key) = key_store.validate_key(&raw_token).await {
                (api_key, AuthMode::ApiKey, None, false)
            } else if key_store.is_ui_jwt_enabled() {
                match key_store.authenticate_ui_jwt(&raw_token).await {
                    Ok((api_key, allowed_projects, platform_admin)) => {
                        (api_key, AuthMode::UiJwt, Some(allowed_projects), platform_admin)
                    }
                    Err(err) => {
                        warn!("Invalid UI JWT token: {err}");
                        key_store
                            .record_auth_failure(
                                client_ip.as_deref(),
                                user_agent.as_deref(),
                                &request_path,
                                &request_method,
                                "invalid_ui_jwt",
                                "bearer_jwt",
                            )
                            .await;
                        return Err((
                            AuthError::InvalidKey.status_code(),
                            AuthError::InvalidKey.message().to_string(),
                        ));
                    }
                }
            } else {
                warn!(key_prefix = %raw_token.chars().take(8).collect::<String>(), "Invalid API key");
                key_store
                    .record_auth_failure(
                        client_ip.as_deref(),
                        user_agent.as_deref(),
                        &request_path,
                        &request_method,
                        "invalid_bearer_token",
                        "bearer",
                    )
                    .await;
                return Err((
                    AuthError::InvalidKey.status_code(),
                    AuthError::InvalidKey.message().to_string(),
                ));
            }
        }
        RequestCredential::UiSession {
            session_token,
            csrf_token,
            require_csrf,
        } => match key_store
            .authenticate_ui_session(&session_token, csrf_token.as_deref(), require_csrf)
            .await
        {
            Ok((api_key, allowed_projects, platform_admin)) => (api_key, AuthMode::UiJwt, Some(allowed_projects), platform_admin),
            Err(err) => {
                warn!("Invalid UI session: {err}");
                key_store
                    .record_auth_failure(
                        client_ip.as_deref(),
                        user_agent.as_deref(),
                        &request_path,
                        &request_method,
                        if err.contains("CSRF") {
                            "invalid_csrf_token"
                        } else {
                            "invalid_ui_session"
                        },
                        "ui_session",
                    )
                    .await;
                if err.contains("CSRF") {
                    return Err((StatusCode::FORBIDDEN, "Invalid CSRF token.".to_string()));
                }
                return Err((
                    AuthError::InvalidKey.status_code(),
                    AuthError::InvalidKey.message().to_string(),
                ));
            }
        },
    };

    key_store.record_auth_success(client_ip.as_deref()).await;

    debug!(
        key_prefix = %api_key.key_prefix,
        project_id = ?api_key.project_id,
        auth_mode = ?auth_mode,
        "Authenticated request",
    );

    // Insert auth context into request extensions
    request.extensions_mut().insert(AuthContext {
        api_key,
        is_dev_mode: false,
        is_platform_admin,
        allowed_project_ids,
        auth_mode,
        request_id: uuid::Uuid::now_v7().to_string(),
        client_ip,
        user_agent,
    });

    Ok(next.run(request).await)
}

/// Extractor for getting AuthContext from request extensions.
/// Use axum::Extension<AuthContext> instead, or access via request.extensions().
pub fn get_auth_context(extensions: &axum::http::Extensions) -> Option<&AuthContext> {
    extensions.get::<AuthContext>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde::Serialize;
    use std::sync::Arc;

    #[derive(Debug, Serialize)]
    struct TestJwtClaims {
        sub: String,
        email: Option<String>,
        name: Option<String>,
        iss: String,
        aud: String,
        exp: usize,
    }

    const TEST_ES256_PRIVATE_KEY_PEM: &str = r#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgby+rCcrDgW6NTYBt
JmWYreNddqDKuwX+CVD6c8FSvFqhRANCAASd7UWEwAENK2EGEWqXOLiuabfDARYK
+tDqX9RwFanOZesb0CLkbR1xBz/hovi0yJK+96axFoQAYX61QNN7rMfc
-----END PRIVATE KEY-----"#;

    const TEST_ES256_PUBLIC_KEY_PEM: &str = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEne1FhMABDSthBhFqlzi4rmm3wwEW
CvrQ6l/UcBWpzmXrG9Ai5G0dcQc/4aL4tMiSvvemsRaEAGF+tUDTe6zH3A==
-----END PUBLIC KEY-----"#;

    #[test]
    fn test_hash_api_key() {
        let key = "mlrunx_test123";
        let hash = hash_api_key(key);

        // Same key should produce same hash
        assert_eq!(hash, hash_api_key(key));

        // Different key should produce different hash
        assert_ne!(hash, hash_api_key("mlrunx_test456"));
    }

    #[test]
    fn test_runtime_auth_mode_parsing() {
        assert_eq!(
            RuntimeAuthMode::parse("disabled"),
            Some(RuntimeAuthMode::Disabled)
        );
        assert_eq!(
            RuntimeAuthMode::parse("api_key"),
            Some(RuntimeAuthMode::ApiKey)
        );
        assert_eq!(
            RuntimeAuthMode::parse("hybrid"),
            Some(RuntimeAuthMode::Hybrid)
        );
        assert_eq!(
            RuntimeAuthMode::parse("api-key-ui-jwt"),
            Some(RuntimeAuthMode::Hybrid)
        );
        assert_eq!(RuntimeAuthMode::parse("unknown"), None);
    }

    #[test]
    fn test_ui_jwt_algorithm_parsing() {
        assert_eq!(UiJwtAlgorithm::parse("HS256"), Some(UiJwtAlgorithm::Hs256));
        assert_eq!(UiJwtAlgorithm::parse("rs256"), Some(UiJwtAlgorithm::Rs256));
        assert_eq!(UiJwtAlgorithm::parse("Es256"), Some(UiJwtAlgorithm::Es256));
        assert_eq!(UiJwtAlgorithm::parse("hs512"), None);
    }

    #[test]
    fn test_ui_jwt_claim_subject_fallback() {
        let claims = UiJwtClaims {
            sub: None,
            user_id: Some("provider-user-123".to_string()),
            email: None,
            name: None,
        };
        assert_eq!(claims.subject(), "provider-user-123");
    }

    #[test]
    fn test_runtime_auth_mode_from_legacy_flags() {
        assert_eq!(
            RuntimeAuthMode::from_flags(true, false),
            RuntimeAuthMode::Disabled
        );
        assert_eq!(
            RuntimeAuthMode::from_flags(false, false),
            RuntimeAuthMode::ApiKey
        );
        assert_eq!(
            RuntimeAuthMode::from_flags(false, true),
            RuntimeAuthMode::Hybrid
        );
    }

    #[test]
    fn test_generate_api_key() {
        let key1 = generate_api_key();
        let key2 = generate_api_key();

        // Keys should be unique
        assert_ne!(key1, key2);

        // Keys should start with prefix
        assert!(key1.starts_with("mlrunx_"));
        assert!(key2.starts_with("mlrunx_"));

        // Keys should be reasonable length
        assert!(key1.len() > 40);
    }

    #[test]
    fn test_generate_session_token() {
        let token1 = generate_session_token();
        let token2 = generate_session_token();
        assert_ne!(token1, token2);
        assert!(token1.len() > 32);
    }

    #[tokio::test]
    async fn test_api_key_store() {
        let store = ApiKeyStore::new();

        // Create a key
        let (raw_key, key) = store
            .create_key(
                Some("project-123".to_string()),
                Some("test-key".to_string()),
                vec!["ingest".to_string()],
                None,
            )
            .await;

        assert!(raw_key.starts_with("mlrunx_"));
        assert_eq!(key.project_id, Some("project-123".to_string()));

        // Validate the key
        let validated = store.validate_key(&raw_key).await;
        assert!(validated.is_some());

        // Invalid key should fail
        let invalid = store.validate_key("invalid_key").await;
        assert!(invalid.is_none());
    }

    #[tokio::test]
    async fn test_key_revocation() {
        let store = ApiKeyStore::new();

        // Create and revoke a key
        let (raw_key, key) = store
            .create_key(
                None,
                Some("to-revoke".to_string()),
                vec!["admin".to_string()],
                None,
            )
            .await;

        // Should be valid before revocation
        assert!(store.validate_key(&raw_key).await.is_some());

        // Revoke
        store.revoke_key(&key.key_hash).await;

        // Should be invalid after revocation
        assert!(store.validate_key(&raw_key).await.is_none());
    }

    #[tokio::test]
    async fn test_sqlite_backed_key_persistence() {
        let db_path = std::env::temp_dir().join(format!("mlrunx-auth-{}.db", uuid::Uuid::now_v7()));

        let sqlite_store = Arc::new(SqliteStore::new(&db_path).await.unwrap());
        let project_id = sqlite_store
            .get_or_create_project("persist-project")
            .await
            .unwrap();

        let store = ApiKeyStore::new_with_sqlite(sqlite_store.clone());
        let (raw_key, key) = store
            .create_key(
                Some(project_id.clone()),
                Some("persistent-key".to_string()),
                vec!["read".to_string()],
                None,
            )
            .await;

        assert!(store.validate_key(&raw_key).await.is_some());
        drop(store);
        drop(sqlite_store);

        // Re-open the same sqlite file to simulate restart and ensure the key still validates.
        let sqlite_store_reopened = Arc::new(SqliteStore::new(&db_path).await.unwrap());
        let reopened_store = ApiKeyStore::new_with_sqlite(sqlite_store_reopened);

        assert!(reopened_store.validate_key(&raw_key).await.is_some());
        assert!(reopened_store.revoke_key(&key.key_hash).await);
        assert!(reopened_store.validate_key(&raw_key).await.is_none());

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    }

    #[tokio::test]
    async fn test_ui_jwt_auth_maps_memberships_to_scopes() {
        let sqlite_store = Arc::new(SqliteStore::new(":memory:").await.unwrap());
        let project_a = sqlite_store.get_or_create_project("proj-a").await.unwrap();
        let project_b = sqlite_store.get_or_create_project("proj-b").await.unwrap();

        let store = ApiKeyStore::new_with_sqlite_and_ui_jwt(sqlite_store.clone(), "test-secret");

        let user_id = sqlite_store
            .get_or_create_user_identity(
                "jwt",
                "user-subject",
                Some("jwt@example.com"),
                Some("JWT User"),
            )
            .await
            .unwrap();
        sqlite_store
            .grant_project_membership(&project_a, &user_id, "viewer", None)
            .await
            .unwrap();
        sqlite_store
            .grant_project_membership(&project_b, &user_id, "editor", None)
            .await
            .unwrap();

        let claims = TestJwtClaims {
            sub: "user-subject".to_string(),
            email: Some("jwt@example.com".to_string()),
            name: Some("JWT User".to_string()),
            iss: "https://test-auth.local".to_string(),
            aud: "authenticated".to_string(),
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret("test-secret".as_bytes()),
        )
        .unwrap();

        let (api_key, allowed_projects, _is_admin) = store.authenticate_ui_jwt(&token).await.unwrap();

        assert!(allowed_projects.contains(&project_a));
        assert!(allowed_projects.contains(&project_b));
        assert!(api_key.scopes.contains(&"read".to_string()));
        assert!(api_key.scopes.contains(&"write".to_string()));
        assert!(!api_key.scopes.contains(&"admin".to_string()));
    }

    #[tokio::test]
    async fn test_ui_jwt_auth_auto_provisions_personal_project_membership() {
        let sqlite_store = Arc::new(SqliteStore::new(":memory:").await.unwrap());
        let store = ApiKeyStore::new_with_sqlite_and_ui_jwt(sqlite_store.clone(), "test-secret");

        let claims = TestJwtClaims {
            sub: "new-user-subject".to_string(),
            email: Some("new.user@example.com".to_string()),
            name: Some("New User".to_string()),
            iss: "https://test-auth.local".to_string(),
            aud: "authenticated".to_string(),
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret("test-secret".as_bytes()),
        )
        .unwrap();

        let (api_key, allowed_projects, _is_admin) = store.authenticate_ui_jwt(&token).await.unwrap();
        assert_eq!(allowed_projects.len(), 1);
        assert!(api_key.scopes.contains(&"read".to_string()));
        assert!(api_key.scopes.contains(&"write".to_string()));
        assert!(api_key.scopes.contains(&"admin".to_string()));

        let (_, user_id) = api_key.id.split_once(':').unwrap();
        let memberships = sqlite_store
            .list_active_project_memberships(user_id)
            .await
            .unwrap();
        assert_eq!(memberships.len(), 1);
        assert_eq!(memberships[0].role, "owner");
        assert!(allowed_projects.contains(&memberships[0].project_id));
    }

    #[tokio::test]
    async fn test_ui_jwt_auth_es256_public_key_verification() {
        let sqlite_store = Arc::new(SqliteStore::new(":memory:").await.unwrap());
        let store = ApiKeyStore::new_with_sqlite_and_ui_jwt_public_key(
            sqlite_store.clone(),
            TEST_ES256_PUBLIC_KEY_PEM,
            UiJwtAlgorithm::Es256,
        );

        let claims = TestJwtClaims {
            sub: "es256-user-subject".to_string(),
            email: Some("es256.user@example.com".to_string()),
            name: Some("ES256 User".to_string()),
            iss: "https://test-auth.local".to_string(),
            aud: "authenticated".to_string(),
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        };
        let token = encode(
            &Header::new(Algorithm::ES256),
            &claims,
            &EncodingKey::from_ec_pem(TEST_ES256_PRIVATE_KEY_PEM.as_bytes()).unwrap(),
        )
        .unwrap();

        let (api_key, allowed_projects, _is_admin) = store.authenticate_ui_jwt(&token).await.unwrap();
        assert_eq!(allowed_projects.len(), 1);
        assert!(api_key.scopes.contains(&"read".to_string()));
        assert!(api_key.scopes.contains(&"write".to_string()));
        assert!(api_key.scopes.contains(&"admin".to_string()));
    }

    #[tokio::test]
    async fn test_ui_session_auth_requires_csrf_for_mutating_requests() {
        let sqlite_store = Arc::new(SqliteStore::new(":memory:").await.unwrap());
        let project_a = sqlite_store.get_or_create_project("proj-a").await.unwrap();
        let store = ApiKeyStore::new_with_sqlite_and_ui_jwt(sqlite_store.clone(), "test-secret");

        let user_id = sqlite_store
            .get_or_create_user_identity(
                "jwt",
                "user-subject",
                Some("jwt@example.com"),
                Some("JWT User"),
            )
            .await
            .unwrap();
        sqlite_store
            .grant_project_membership(&project_a, &user_id, "editor", None)
            .await
            .unwrap();

        let claims = TestJwtClaims {
            sub: "user-subject".to_string(),
            email: Some("jwt@example.com".to_string()),
            name: Some("JWT User".to_string()),
            iss: "https://test-auth.local".to_string(),
            aud: "authenticated".to_string(),
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret("test-secret".as_bytes()),
        )
        .unwrap();

        let issue = store
            .create_ui_session_from_jwt(&token, Some("test-agent"), Some("127.0.0.1"))
            .await
            .unwrap();

        let no_csrf = store
            .authenticate_ui_session(&issue.session_token, None, true)
            .await;
        assert!(no_csrf.is_err());

        let ok = store
            .authenticate_ui_session(&issue.session_token, Some(&issue.csrf_token), true)
            .await
            .unwrap();
        assert!(ok.1.contains(&project_a));
    }

    #[test]
    fn test_auth_context_project_access_for_ui_jwt() {
        let mut allowed = HashSet::new();
        allowed.insert("project-a".to_string());
        allowed.insert("project-b".to_string());

        let ctx = AuthContext {
            api_key: ApiKey {
                id: "jwt:user".to_string(),
                key_hash: "jwt:user".to_string(),
                key_prefix: "jwt_user".to_string(),
                project_id: None,
                name: Some("JWT User".to_string()),
                scopes: vec!["read".to_string()],
                created_at: SystemTime::now(),
                last_used_at: None,
                revoked_at: None,
                expires_at: None,
            },
            is_dev_mode: false,
            is_platform_admin: false,
            allowed_project_ids: Some(allowed),
            auth_mode: AuthMode::UiJwt,
            request_id: uuid::Uuid::now_v7().to_string(),
            client_ip: None,
            user_agent: None,
        };

        assert!(ctx.require_project_access("project-a").is_ok());
        assert!(ctx.require_project_access("project-b").is_ok());
        assert!(ctx.require_project_access("project-c").is_err());
    }

    #[test]
    fn test_api_key_scopes() {
        let key = ApiKey {
            id: "test".to_string(),
            key_hash: "hash".to_string(),
            key_prefix: "mlrunx_te".to_string(),
            project_id: Some("project-123".to_string()),
            name: Some("test".to_string()),
            scopes: vec!["ingest".to_string(), "query".to_string()],
            created_at: std::time::SystemTime::now(),
            last_used_at: None,
            revoked_at: None,
            expires_at: None,
        };

        assert!(key.has_scope("ingest"));
        assert!(key.has_scope("query"));
        assert!(!key.has_scope("admin"));

        // Admin key should have all scopes
        let admin_key = ApiKey {
            id: "admin".to_string(),
            key_hash: "hash".to_string(),
            key_prefix: "mlrunx_ad".to_string(),
            project_id: None,
            name: Some("admin".to_string()),
            scopes: vec!["admin".to_string()],
            created_at: std::time::SystemTime::now(),
            last_used_at: None,
            revoked_at: None,
            expires_at: None,
        };

        assert!(admin_key.has_scope("anything"));
        assert!(admin_key.has_scope("admin"));
    }

    #[test]
    fn test_project_access() {
        // Project-scoped key
        let project_key = ApiKey {
            id: "test".to_string(),
            key_hash: "hash".to_string(),
            key_prefix: "mlrunx_te".to_string(),
            project_id: Some("project-123".to_string()),
            name: None,
            scopes: vec!["ingest".to_string()],
            created_at: std::time::SystemTime::now(),
            last_used_at: None,
            revoked_at: None,
            expires_at: None,
        };

        assert!(project_key.can_access_project("project-123"));
        assert!(!project_key.can_access_project("project-456"));

        // Global admin key
        let admin_key = ApiKey {
            id: "admin".to_string(),
            key_hash: "hash".to_string(),
            key_prefix: "mlrunx_ad".to_string(),
            project_id: None,
            name: None,
            scopes: vec!["admin".to_string()],
            created_at: std::time::SystemTime::now(),
            last_used_at: None,
            revoked_at: None,
            expires_at: None,
        };

        assert!(admin_key.can_access_project("project-123"));
        assert!(admin_key.can_access_project("project-456"));
    }
}
