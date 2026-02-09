//! Authentication and authorization module for MLRunX API.
//!
//! Provides API key authentication middleware and key management.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::{
    extract::{Request, State},
    http::{StatusCode, request::Parts},
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::storage::{ApiKeyRow, SqliteStore};

/// An API key entry stored in the system.
#[derive(Debug, Clone)]
pub struct ApiKey {
    /// Unique identifier for the key
    pub id: String,
    /// SHA-256 hash of the key
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
}

impl ApiKey {
    /// Check if the key is valid (not revoked).
    pub fn is_valid(&self) -> bool {
        self.revoked_at.is_none()
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

/// API key store with optional SQLite persistence.
#[derive(Default)]
pub struct ApiKeyStore {
    /// Map from key_hash to ApiKey
    keys: RwLock<HashMap<String, ApiKey>>,
    /// Optional durable backing store.
    sqlite_store: Option<Arc<SqliteStore>>,
    /// Whether auth is disabled (for dev/testing)
    pub auth_disabled: std::sync::atomic::AtomicBool,
}

impl ApiKeyStore {
    /// Create a new API key store.
    pub fn new() -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
            sqlite_store: None,
            auth_disabled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Create a key store backed by SQLite for durable key storage.
    pub fn new_with_sqlite(sqlite_store: Arc<SqliteStore>) -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
            sqlite_store: Some(sqlite_store),
            auth_disabled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Create a new API key store with auth disabled (for testing).
    pub fn new_dev_mode() -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
            sqlite_store: None,
            auth_disabled: std::sync::atomic::AtomicBool::new(true),
        }
    }

    /// Check if auth is disabled.
    pub fn is_auth_disabled(&self) -> bool {
        self.auth_disabled
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Initialize the store with bootstrap keys from environment.
    pub async fn init_from_env(&self) {
        // Check for dev mode (no auth required)
        if std::env::var("MLRUNX_AUTH_DISABLED").map_or(false, |v| v == "true" || v == "1") {
            self.auth_disabled
                .store(true, std::sync::atomic::Ordering::Relaxed);
            info!("Authentication disabled (dev mode)");
        }

        // Check for bootstrap key
        if let Ok(bootstrap_key) = std::env::var("MLRUNX_API_KEY") {
            if !bootstrap_key.is_empty() {
                let key = self.create_key_from_raw(
                    &bootstrap_key,
                    None, // Global admin key
                    Some("bootstrap".to_string()),
                    vec!["admin".to_string()],
                );

                if let Some(sqlite_store) = &self.sqlite_store {
                    if let Err(err) = sqlite_store
                        .upsert_bootstrap_api_key(&key.id, &key.key_hash, &key.key_prefix)
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
    fn create_key_from_raw(
        &self,
        raw_key: &str,
        project_id: Option<String>,
        name: Option<String>,
        scopes: Vec<String>,
    ) -> ApiKey {
        let key_hash = hash_api_key(raw_key);
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
        }
    }

    /// Validate an API key and return the key info if valid.
    pub async fn validate_key(&self, raw_key: &str) -> Option<ApiKey> {
        let key_hash = hash_api_key(raw_key);

        // Primary path: durable sqlite store.
        if let Some(sqlite_store) = &self.sqlite_store {
            match sqlite_store.get_api_key_by_hash(&key_hash).await {
                Ok(Some(row)) => {
                    if row.revoked_at.is_some() {
                        return None;
                    }

                    let mut key = Self::api_key_from_row(row);
                    if let Err(err) = sqlite_store.touch_api_key_last_used(&key_hash).await {
                        warn!("Failed to update API key last_used_at: {err}");
                    } else {
                        key.last_used_at = Some(SystemTime::now());
                    }

                    let mut keys = self.keys.write().await;
                    keys.insert(key_hash, key.clone());
                    return Some(key);
                }
                Ok(None) => {}
                Err(err) => {
                    warn!("Failed to validate API key from sqlite, falling back to memory: {err}");
                }
            }
        }

        // Fallback path: in-memory store.
        let mut keys = self.keys.write().await;

        if let Some(key) = keys.get_mut(&key_hash) {
            if key.is_valid() {
                // Update last used time
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
    ) -> (String, ApiKey) {
        // Generate a random key
        let raw_key = generate_api_key();
        let key = self.create_key_from_raw(&raw_key, project_id, name, scopes);

        if let Some(sqlite_store) = &self.sqlite_store {
            let scopes_json =
                serde_json::to_string(&key.scopes).unwrap_or_else(|_| "[]".to_string());
            if let Err(err) = sqlite_store
                .insert_api_key(
                    &key.id,
                    &key.key_hash,
                    &key.key_prefix,
                    key.project_id.as_deref(),
                    key.name.as_deref(),
                    &scopes_json,
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

/// Hash an API key using SHA-256.
pub fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Generate a random API key.
pub fn generate_api_key() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
    format!("mlrunx_{}", hex::encode(bytes))
}

/// Authenticated user context extracted from request.
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// The API key used for authentication
    pub api_key: ApiKey,
    /// Whether authentication is bypassed (dev mode)
    pub is_dev_mode: bool,
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
            },
            is_dev_mode: true,
        }
    }

    /// Get the project_id this key is scoped to (None = global/admin).
    pub fn project_id(&self) -> Option<&str> {
        self.api_key.project_id.as_deref()
    }

    /// Returns true if this context has global access (dev mode or admin with no project scope).
    pub fn is_global(&self) -> bool {
        self.is_dev_mode || self.api_key.project_id.is_none()
    }

    /// Check if the caller can access a specific project's resources.
    /// Returns Ok(()) if allowed, Err with 403 if denied.
    pub fn require_project_access(&self, run_project_id: &str) -> Result<(), (StatusCode, String)> {
        if self.is_global() {
            return Ok(());
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
                "API key required. Use Authorization: Bearer <key> or X-API-Key header."
            }
            AuthError::InvalidKey => "Invalid API key.",
            AuthError::InsufficientScope => "Insufficient permissions.",
            AuthError::ProjectAccessDenied => "Access to project denied.",
        }
    }
}

/// Extract API key from request headers.
pub fn extract_api_key_from_headers(parts: &Parts) -> Option<String> {
    // Try Authorization: Bearer <key>
    if let Some(auth_header) = parts.headers.get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(key) = auth_str.strip_prefix("Bearer ") {
                return Some(key.trim().to_string());
            }
        }
    }

    // Try X-API-Key header
    if let Some(key_header) = parts.headers.get("x-api-key") {
        if let Ok(key_str) = key_header.to_str() {
            return Some(key_str.trim().to_string());
        }
    }

    None
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

    // Extract API key from headers
    let raw_key = {
        let (parts, body) = request.into_parts();
        let key = extract_api_key_from_headers(&parts);
        request = Request::from_parts(parts, body);
        key
    };

    let raw_key = raw_key.ok_or_else(|| {
        (
            AuthError::MissingKey.status_code(),
            AuthError::MissingKey.message().to_string(),
        )
    })?;

    // Validate the key
    let api_key = key_store.validate_key(&raw_key).await.ok_or_else(|| {
        warn!(key_prefix = %raw_key.chars().take(8).collect::<String>(), "Invalid API key");
        (
            AuthError::InvalidKey.status_code(),
            AuthError::InvalidKey.message().to_string(),
        )
    })?;

    debug!(
        key_prefix = %api_key.key_prefix,
        project_id = ?api_key.project_id,
        "Authenticated request"
    );

    // Insert auth context into request extensions
    request.extensions_mut().insert(AuthContext {
        api_key,
        is_dev_mode: false,
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
    use std::sync::Arc;

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

    #[tokio::test]
    async fn test_api_key_store() {
        let store = ApiKeyStore::new();

        // Create a key
        let (raw_key, key) = store
            .create_key(
                Some("project-123".to_string()),
                Some("test-key".to_string()),
                vec!["ingest".to_string()],
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
        };

        assert!(admin_key.can_access_project("project-123"));
        assert!(admin_key.can_access_project("project-456"));
    }
}
