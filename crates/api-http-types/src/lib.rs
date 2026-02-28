//! Shared HTTP payload types for API handlers.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct UiAuthLoginRequest {
    #[serde(default)]
    pub jwt: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UiAuthLoginResponse {
    pub status: String,
    pub user_id: String,
    pub expires_at: String,
    pub project_count: usize,
}

#[derive(Debug, Serialize)]
pub struct UiAuthSessionResponse {
    pub authenticated: bool,
    pub auth_mode: String,
    pub scopes: Vec<String>,
    pub project_ids: Vec<String>,
    pub key_prefix: String,
    pub is_dev_mode: bool,
    pub is_platform_admin: bool,
    pub ui_session_ttl_seconds: u64,
    pub ui_key_max_ttl_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct UiAuthLogoutResponse {
    pub status: String,
}
