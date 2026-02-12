// Authentication module for PDF.dk Desktop
// Handles login, token storage, and PRO subscription validation

use crate::config::{self, AuthConfig};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const API_BASE_URL: &str = "https://pdf.dk/api";

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Invalid credentials")]
    InvalidCredentials,
    #[error("Token expired")]
    TokenExpired,
    #[error("PRO subscription required")]
    ProRequired,
    #[error("Keyring error: {0}")]
    Keyring(String),
    #[error("Server error: {0}")]
    ServerError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthState {
    pub is_authenticated: bool,
    pub is_pro: bool,
    pub user: Option<User>,
    pub token: Option<String>,
    // Plan and usage limits
    pub plan: Option<String>,           // "guest", "free", "pro", "team"
    pub jobs_limit: Option<i32>,        // -1 = unlimited
    pub jobs_used: Option<i32>,
    pub jobs_remaining: Option<i32>,
    pub max_file_size_mb: Option<i32>,
    pub is_unlimited: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: i64,
    pub email: String,
    pub name: Option<String>,
    #[serde(default)]
    pub is_superadmin: bool,
    #[serde(default)]
    pub admin_granted_subscription: bool,
    pub role: Option<String>,
}

// API Response structures matching Laravel backend
#[derive(Debug, Deserialize)]
struct LoginResponse {
    success: bool,
    data: Option<LoginData>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginData {
    user: ApiUser,
    token: String,
}

#[derive(Debug, Deserialize)]
struct ApiUser {
    id: i64,
    email: String,
    name: Option<String>,
    #[serde(default)]
    is_superadmin: bool,
    role: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserResponse {
    success: bool,
    data: Option<UserData>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserData {
    user: ApiUserFull,
}

#[derive(Debug, Deserialize)]
struct ApiUserFull {
    id: i64,
    email: String,
    name: Option<String>,
    #[serde(default)]
    is_superadmin: bool,
    #[serde(default)]
    admin_granted_subscription: bool,
    role: Option<String>,
}

/// Login to PDF.dk and get authentication token
pub async fn login(email: &str, password: &str) -> Result<AuthState, AuthError> {
    let client = Client::new();

    let response = client
        .post(format!("{}/auth/login", API_BASE_URL))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "email": email,
            "password": password,
        }))
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    // Log for debugging
    tracing::debug!("Login response status: {}, body: {}", status, &body);

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AuthError::InvalidCredentials);
    }

    if !status.is_success() {
        return Err(AuthError::ServerError(format!(
            "Server returned {}: {}",
            status, body
        )));
    }

    let login_response: LoginResponse = serde_json::from_str(&body)
        .map_err(|e| AuthError::ServerError(format!("Failed to parse response: {}", e)))?;

    if !login_response.success {
        return Err(AuthError::InvalidCredentials);
    }

    let data = login_response.data.ok_or(AuthError::InvalidCredentials)?;

    // Check if user has PRO subscription (is_superadmin or admin_granted_subscription)
    // For now, allow superadmins as PRO
    let is_pro = data.user.is_superadmin;

    let user = User {
        id: data.user.id,
        email: data.user.email,
        name: data.user.name,
        is_superadmin: data.user.is_superadmin,
        admin_granted_subscription: false, // Will be checked on /user endpoint
        role: data.user.role,
    };

    Ok(AuthState {
        is_authenticated: true,
        is_pro,
        user: Some(user),
        token: Some(data.token),
        plan: None,
        jobs_limit: None,
        jobs_used: None,
        jobs_remaining: None,
        max_file_size_mb: None,
        is_unlimited: None,
    })
}

/// Validate an existing token and get user info
pub async fn validate_token(token: &str) -> Result<AuthState, AuthError> {
    let client = Client::new();

    let response = client
        .get(format!("{}/user", API_BASE_URL))
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/json")
        .send()
        .await?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AuthError::TokenExpired);
    }

    if !response.status().is_success() {
        return Err(AuthError::ServerError(format!(
            "Server returned {}",
            response.status()
        )));
    }

    let body = response.text().await?;
    tracing::debug!("User response: {}", &body);

    let user_response: UserResponse = serde_json::from_str(&body)
        .map_err(|e| AuthError::ServerError(format!("Failed to parse response: {}", e)))?;

    if !user_response.success {
        return Err(AuthError::TokenExpired);
    }

    let data = user_response.data.ok_or(AuthError::TokenExpired)?;
    let api_user = data.user;

    // Check if user has PRO subscription
    let is_pro = api_user.is_superadmin || api_user.admin_granted_subscription;

    let user = User {
        id: api_user.id,
        email: api_user.email,
        name: api_user.name,
        is_superadmin: api_user.is_superadmin,
        admin_granted_subscription: api_user.admin_granted_subscription,
        role: api_user.role,
    };

    Ok(AuthState {
        is_authenticated: true,
        is_pro,
        user: Some(user),
        token: Some(token.to_string()),
        plan: None,
        jobs_limit: None,
        jobs_used: None,
        jobs_remaining: None,
        max_file_size_mb: None,
        is_unlimited: None,
    })
}

/// Save token to config file
pub fn save_token(token: &str) -> Result<(), AuthError> {
    let mut cfg = config::load_config().map_err(|e| AuthError::Keyring(e.to_string()))?;

    if cfg.auth.is_none() {
        cfg.auth = Some(AuthConfig::default());
    }
    if let Some(ref mut auth) = cfg.auth {
        auth.token = Some(token.to_string());
    }

    config::save_config(&cfg).map_err(|e| AuthError::Keyring(e.to_string()))?;
    Ok(())
}

/// Load token from config file
pub fn load_token() -> Result<String, AuthError> {
    let cfg = config::load_config().map_err(|e| AuthError::Keyring(e.to_string()))?;

    cfg.auth
        .and_then(|a| a.token)
        .ok_or_else(|| AuthError::Keyring("No saved token".to_string()))
}

/// Clear token from config file
pub fn clear_token() -> Result<(), AuthError> {
    let mut cfg = config::load_config().map_err(|e| AuthError::Keyring(e.to_string()))?;

    if let Some(ref mut auth) = cfg.auth {
        auth.token = None;
    }

    config::save_config(&cfg).map_err(|e| AuthError::Keyring(e.to_string()))?;
    Ok(())
}

/// Save credentials to config file (for "Remember me" feature)
pub fn save_credentials(email: &str, password: &str) -> Result<(), AuthError> {
    let mut cfg = config::load_config().map_err(|e| AuthError::Keyring(e.to_string()))?;

    if cfg.auth.is_none() {
        cfg.auth = Some(AuthConfig::default());
    }
    if let Some(ref mut auth) = cfg.auth {
        auth.email = Some(email.to_string());
        auth.password = Some(password.to_string());
    }

    config::save_config(&cfg).map_err(|e| AuthError::Keyring(e.to_string()))?;
    Ok(())
}

/// Load saved credentials from config file
pub fn load_credentials() -> Result<(String, String), AuthError> {
    let cfg = config::load_config().map_err(|e| AuthError::Keyring(e.to_string()))?;

    let auth = cfg.auth.ok_or_else(|| AuthError::Keyring("No saved credentials".to_string()))?;
    let email = auth.email.ok_or_else(|| AuthError::Keyring("No saved email".to_string()))?;
    let password = auth.password.ok_or_else(|| AuthError::Keyring("No saved password".to_string()))?;

    Ok((email, password))
}

/// Clear saved credentials from config file
pub fn clear_credentials() -> Result<(), AuthError> {
    let mut cfg = config::load_config().map_err(|e| AuthError::Keyring(e.to_string()))?;

    if let Some(ref mut auth) = cfg.auth {
        auth.email = None;
        auth.password = None;
    }

    config::save_config(&cfg).map_err(|e| AuthError::Keyring(e.to_string()))?;
    Ok(())
}
