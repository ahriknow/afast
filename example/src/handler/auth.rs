use afast::{AFastDeserialize, AFastSerialize, Tag, handler};

use crate::state::{AppState, User};

// ─── Auth Custom ──────────────────────────────────────────────────

#[derive(AFastDeserialize, Tag)]
#[tag("Authentication token used as Custom extractor")]
pub struct AuthCustom {
    #[tag("Bearer token")]
    pub token: String,
}

// ─── Auth Header (ordinary HTTP) ──────────────────────────────────

#[cfg(feature = "ordinary-http")]
#[derive(Debug, serde::Deserialize, Tag)]
#[tag("Authorization header for HTTP authentication")]
pub struct AuthHeader {
    #[tag("Authorization header value (e.g. Bearer <token>)")]
    pub authorization: String,
}

#[cfg(feature = "ordinary-http")]
impl AuthHeader {
    pub fn token(&self) -> &str {
        self.authorization
            .strip_prefix("Bearer ")
            .unwrap_or(&self.authorization)
    }
}

// ─── Request Types ────────────────────────────────────────────────

#[derive(AFastDeserialize, Tag)]
#[tag("Registration request")]
pub struct RegisterRequest {
    #[tag("Desired username")]
    pub username: String,
    #[tag("Login password")]
    pub password: String,
    #[tag("Display name")]
    pub name: String,
}

#[derive(AFastDeserialize, Tag)]
#[tag("Login request")]
pub struct LoginRequest {
    #[tag("Username")]
    pub username: String,
    #[tag("Password")]
    pub password: String,
}

#[derive(AFastDeserialize, Tag)]
#[tag("Create token for a user by ID")]
pub struct CreateTokenRequest {
    #[tag("Target user ID")]
    pub user_id: i64,
}

// ─── Response Types ────────────────────────────────────────────────

#[derive(AFastSerialize, Tag)]
#[tag("Registration result")]
pub struct RegisterResponse {
    #[tag("The newly created user")]
    pub user: User,
    #[tag("Authentication token")]
    pub token: String,
}

#[derive(AFastSerialize, Tag)]
#[tag("Login result")]
pub struct LoginResponse {
    #[tag("The authenticated user")]
    pub user: User,
    #[tag("Authentication token")]
    pub token: String,
}

#[derive(AFastSerialize, Tag)]
#[tag("Token creation result")]
pub struct TokenResponse {
    #[tag("Newly created token")]
    pub token: String,
}

#[derive(AFastSerialize, Tag)]
#[tag("User ID resolved from token")]
pub struct UserIdResponse {
    #[tag("Resolved user ID")]
    pub user_id: i64,
}

// ─── Handlers ─────────────────────────────────────────────────────

#[handler(desc("Register a new account and get a token"), name("signup"))]
pub async fn register(
    afast::State(state): afast::State<AppState>,
    afast::Data(req): afast::Data<RegisterRequest>,
) -> afast::Result<RegisterResponse> {
    let mut db = state.db.lock().await;
    let user = User::new(req.username, req.password, req.name);
    let new_id = db.create_user(user).await;
    let token = db.create_token(new_id).await;
    let user = db.get(new_id).await.unwrap();
    Ok(RegisterResponse { user, token })
}

#[handler(desc("Login with username and password"))]
pub async fn login(
    afast::State(state): afast::State<AppState>,
    afast::Data(req): afast::Data<LoginRequest>,
) -> afast::Result<LoginResponse> {
    let mut db = state.db.lock().await;
    if let Some(user) = db
        .find_user_by_credentials(&req.username, &req.password)
        .await
    {
        let token = db.create_token(user.id).await;
        Ok(LoginResponse { user, token })
    } else {
        Err(afast::Error::custom(401, "invalid credentials"))
    }
}

#[handler(desc("Create a new token for a user"))]
pub async fn create_token(
    afast::State(state): afast::State<AppState>,
    afast::Data(req): afast::Data<CreateTokenRequest>,
) -> afast::Result<TokenResponse> {
    let mut db = state.db.lock().await;
    let token = db.create_token(req.user_id).await;
    Ok(TokenResponse { token })
}

#[handler(desc("Resolve a token to a user ID"))]
pub async fn get_user_id(
    afast::State(state): afast::State<AppState>,
    afast::Custom(custom): afast::Custom<AuthCustom>,
) -> afast::Result<UserIdResponse> {
    let db = state.db.lock().await;
    if let Some(user_id) = db.get_user_id_by_token(&custom.token).await {
        Ok(UserIdResponse { user_id })
    } else {
        Err(afast::Error::custom(401, "invalid token"))
    }
}
