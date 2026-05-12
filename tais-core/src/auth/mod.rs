// Auth module — JWT authentication, QR login, WeChat binding
//
// Provides:
//   - Password hashing (argon2id)
//   - JWT creation/verification (HS256)
//   - QR code login flow (temp token → poll → confirm)
//   - WeChat binding via 6-digit code

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;

// ── JWT Claims ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,       // user_id
    pub exp: usize,        // expiry (unix timestamp)
    pub iat: usize,        // issued at
}

// ── QR Login Token ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct QrLoginToken {
    pub token: String,
    pub user_id: Option<String>,
    pub status: QrLoginStatus,
    pub created_at: chrono::NaiveDateTime,
    pub expires_at: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum QrLoginStatus {
    Pending,
    Confirmed,
    Expired,
}

// ── WeChat Bind Request ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BindRequest {
    pub user_id: String,
    pub bind_code: String,
    pub status: BindStatus,
    pub wechat_openid: Option<String>,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BindStatus {
    Pending,
    Confirmed,
    Expired,
}

// ── Auth State ──────────────────────────────────────────────────────────

pub struct AuthState {
    jwt_secret: String,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    qr_tokens: RwLock<HashMap<String, QrLoginToken>>,
    wechat_bindings: RwLock<HashMap<String, BindRequest>>,
    /// bind_code -> BindRequest lookup
    bind_codes: RwLock<HashMap<String, String>>,
    token_expiry_hours: u32,
}

impl AuthState {
    pub fn new(jwt_secret: &str, token_expiry_hours: u32) -> Self {
        Self {
            jwt_secret: jwt_secret.to_string(),
            encoding_key: EncodingKey::from_secret(jwt_secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(jwt_secret.as_bytes()),
            qr_tokens: RwLock::new(HashMap::new()),
            wechat_bindings: RwLock::new(HashMap::new()),
            bind_codes: RwLock::new(HashMap::new()),
            token_expiry_hours,
        }
    }

    // ── Password Hashing ─────────────────────────────────────────────

    pub fn hash_password(&self, password: &str) -> Result<String, String> {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| format!("Password hash error: {e}"))
    }

    pub fn verify_password(&self, password: &str, hash: &str) -> Result<bool, String> {
        let parsed = PasswordHash::new(hash)
            .map_err(|e| format!("Invalid hash format: {e}"))?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    }

    // ── JWT ─────────────────────────────────────────────────────────

    pub fn create_jwt(&self, user_id: &str) -> Result<String, String> {
        let now = chrono::Utc::now();
        let exp = now + chrono::Duration::hours(self.token_expiry_hours as i64);

        let claims = Claims {
            sub: user_id.to_string(),
            iat: now.timestamp() as usize,
            exp: exp.timestamp() as usize,
        };

        encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|e| format!("JWT encode error: {e}"))
    }

    pub fn verify_jwt(&self, token: &str) -> Result<String, String> {
        let data = decode::<Claims>(token, &self.decoding_key, &Validation::default())
            .map_err(|e| format!("JWT decode error: {e}"))?;
        Ok(data.claims.sub)
    }

    /// Extract user_id from Authorization: Bearer <token> header
    pub fn extract_user(&self, auth_header: Option<&str>) -> Option<String> {
        let header = auth_header?;
        let token = header.strip_prefix("Bearer ")?;
        self.verify_jwt(token).ok()
    }

    // ── QR Login ─────────────────────────────────────────────────────

    /// Create a QR login token, return the token string for QR code encoding
    pub async fn create_qr_token(&self) -> QrLoginToken {
        self.cleanup_expired_qr().await;

        let token: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();

        let now = chrono::Utc::now().naive_utc();
        let qr = QrLoginToken {
            token: token.clone(),
            user_id: None,
            status: QrLoginStatus::Pending,
            created_at: now,
            expires_at: now + chrono::Duration::minutes(5),
        };

        self.qr_tokens.write().await.insert(token.clone(), qr.clone());
        qr
    }

    /// Get QR login status (for polling)
    pub async fn get_qr_status(&self, token: &str) -> Option<QrLoginStatus> {
        let tokens = self.qr_tokens.read().await;
        let qr = tokens.get(token)?;

        if chrono::Utc::now().naive_utc() > qr.expires_at {
            return Some(QrLoginStatus::Expired);
        }

        Some(qr.status.clone())
    }

    /// Confirm QR login from the phone side
    pub async fn confirm_qr(&self, token: &str, user_id: &str) -> Result<(), String> {
        let mut tokens = self.qr_tokens.write().await;
        let qr = tokens.get_mut(token)
            .ok_or_else(|| "QR token not found".to_string())?;

        if chrono::Utc::now().naive_utc() > qr.expires_at {
            qr.status = QrLoginStatus::Expired;
            return Err("QR token expired".to_string());
        }

        qr.status = QrLoginStatus::Confirmed;
        qr.user_id = Some(user_id.to_string());
        Ok(())
    }

    /// Complete QR login: get JWT token if confirmed
    pub async fn complete_qr_login(&self, token: &str) -> Option<String> {
        let tokens = self.qr_tokens.read().await;
        let qr = tokens.get(token)?;

        if qr.status != QrLoginStatus::Confirmed {
            return None;
        }

        let user_id = qr.user_id.as_ref()?;
        self.create_jwt(user_id).ok()
    }

    async fn cleanup_expired_qr(&self) {
        let now = chrono::Utc::now().naive_utc();
        let mut tokens = self.qr_tokens.write().await;
        tokens.retain(|_, qr| qr.expires_at > now);
    }

    // ── WeChat Binding ───────────────────────────────────────────────

    /// Create a 6-digit binding code for the user
    pub async fn create_bind_code(&self, user_id: &str) -> String {
        self.cleanup_expired_bindings().await;

        // Generate code in a block to drop ThreadRng before .await points
        let code: String = {
            let mut rng = rand::thread_rng();
            (0..6)
                .map(|_| rng.gen_range(0..10).to_string())
                .collect()
        };

        let now = chrono::Utc::now().naive_utc();
        let bind = BindRequest {
            user_id: user_id.to_string(),
            bind_code: code.clone(),
            status: BindStatus::Pending,
            wechat_openid: None,
            created_at: now,
        };

        // Remove any previous pending binding for this user
        self.wechat_bindings.write().await.retain(|_, b| b.user_id != user_id);
        self.bind_codes.write().await.retain(|_, u| u != user_id);

        self.bind_codes.write().await.insert(code.clone(), user_id.to_string());
        self.wechat_bindings.write().await.insert(user_id.to_string(), bind);

        code
    }

    /// Check if a bind code is valid, return user_id
    pub async fn check_bind_code(&self, code: &str) -> Option<String> {
        let codes = self.bind_codes.read().await;
        let user_id = codes.get(code)?;

        let bindings = self.wechat_bindings.read().await;
        let bind = bindings.get(user_id)?;

        // 10-minute expiry
        if chrono::Utc::now().naive_utc() > bind.created_at + chrono::Duration::minutes(10) {
            return None;
        }

        Some(user_id.clone())
    }

    /// Confirm WeChat binding with OpenID
    pub async fn confirm_bind(&self, code: &str, openid: &str) -> Result<String, String> {
        let user_id = {
            let codes = self.bind_codes.read().await;
            codes.get(code).cloned()
                .ok_or_else(|| "Invalid bind code".to_string())?
        };

        let mut bindings = self.wechat_bindings.write().await;
        if let Some(bind) = bindings.get_mut(&user_id) {
            bind.status = BindStatus::Confirmed;
            bind.wechat_openid = Some(openid.to_string());
        }

        // Clean up the code
        self.bind_codes.write().await.remove(code);

        Ok(user_id)
    }

    /// Get binding status for a user
    pub async fn get_bind_status(&self, user_id: &str) -> Option<BindStatus> {
        let bindings = self.wechat_bindings.read().await;
        let bind = bindings.get(user_id)?;

        if chrono::Utc::now().naive_utc() > bind.created_at + chrono::Duration::minutes(10) {
            return Some(BindStatus::Expired);
        }

        Some(bind.status.clone())
    }

    /// Get bound OpenID for user
    pub async fn get_bound_openid(&self, user_id: &str) -> Option<String> {
        let bindings = self.wechat_bindings.read().await;
        bindings.get(user_id)?.wechat_openid.clone()
    }

    async fn cleanup_expired_bindings(&self) {
        let now = chrono::Utc::now().naive_utc();
        let expiry = now - chrono::Duration::minutes(10);

        let mut bindings = self.wechat_bindings.write().await;
        let expired: Vec<String> = bindings.iter()
            .filter(|(_, b)| b.created_at < expiry && b.status == BindStatus::Pending)
            .map(|(_uid, b)| {
                // Also clean the bind code
                b.bind_code.clone()
            })
            .collect();

        for code in &expired {
            self.bind_codes.write().await.remove(code);
        }

        bindings.retain(|_, b| b.created_at >= expiry || b.status != BindStatus::Pending);
    }
}
