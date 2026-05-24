use afast::{AFastDeserialize, AFastSerialize, Tag, delete, get, handler, post, put};

use crate::handler::auth::AuthCustom;
#[cfg(feature = "ordinary-http")]
use crate::handler::auth::AuthHeader;
use crate::state::{AppState, User};

// ─── Request Types ────────────────────────────────────────────────

#[derive(AFastDeserialize, Tag)]
#[tag("Request to create a new user")]
pub struct CreateUserRequest {
    #[tag("Unique username")]
    pub username: String,
    #[tag("Login password")]
    pub password: String,
    #[tag("Display name")]
    pub name: String,
}

#[derive(AFastSerialize, Tag)]
#[tag("Response with new user ID")]
pub struct CreateUserResponse {
    #[tag("New user ID")]
    pub id: i64,
}

#[derive(AFastDeserialize, Tag)]
#[tag("Pagination parameters for listing users")]
pub struct ListUsersRequest {
    #[tag("Page number, 1-indexed")]
    pub page: i64,
    #[tag("Number of items per page")]
    pub size: i64,
}

#[derive(AFastDeserialize, Tag)]
#[tag("Request to look up or delete a user by ID")]
pub struct UserIdRequest {
    #[tag("Unique user identifier")]
    pub user_id: i64,
}

#[derive(AFastDeserialize, Tag)]
#[tag("Request to update an existing user")]
pub struct UpdateUserRequest {
    #[tag("Unique user identifier")]
    pub user_id: i64,
    #[tag("New display name")]
    pub name: String,
    #[tag("New age")]
    pub age: i32,
    #[tag("New account status")]
    pub active: bool,
}

// ─── Response Types ────────────────────────────────────────────────

#[derive(AFastSerialize, Tag)]
#[tag("Paginated list of users")]
pub struct ListUsersResponse {
    #[tag("Total number of users")]
    pub total: i64,
    #[tag("Users for the current page")]
    pub items: Vec<User>,
}

// ─── Handlers ─────────────────────────────────────────────────────

#[handler(desc("Create a new user (requires auth)"))]
pub async fn create_user(
    afast::State(state): afast::State<AppState>,
    afast::Custom(auth): afast::Custom<AuthCustom>,
    afast::Data(req): afast::Data<CreateUserRequest>,
) -> afast::Result<CreateUserResponse> {
    let mut db = state.db.lock().await;
    let _user_id = db
        .get_user_id_by_token(&auth.token)
        .await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    let user = User::new(req.username, req.password, req.name);
    let new_id = db.create_user(user).await;
    Ok(CreateUserResponse { id: new_id })
}

#[handler(desc("List users with pagination (requires auth)"))]
pub async fn list_users(
    afast::State(state): afast::State<AppState>,
    afast::Custom(auth): afast::Custom<AuthCustom>,
    afast::Data(req): afast::Data<ListUsersRequest>,
) -> afast::Result<ListUsersResponse> {
    let db = state.db.lock().await;
    let _user_id = db
        .get_user_id_by_token(&auth.token)
        .await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    let all = db.read(0, 1000).await;
    let total = all.len() as i64;
    let skip = ((req.page - 1) * req.size) as usize;
    let limit = req.size as usize;
    let items = db.read(skip, limit).await;
    Ok(ListUsersResponse { total, items })
}

#[handler(desc("Update an existing user (requires auth)"))]
pub async fn update_user(
    afast::State(state): afast::State<AppState>,
    afast::Custom(auth): afast::Custom<AuthCustom>,
    afast::Data(req): afast::Data<UpdateUserRequest>,
) -> afast::Result<Option<User>> {
    let mut db = state.db.lock().await;
    let _user_id = db
        .get_user_id_by_token(&auth.token)
        .await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    let ok = db
        .update(
            req.user_id,
            User {
                name: req.name,
                age: req.age,
                active: req.active,
                ..User::new("".into(), "".into(), "".into())
            },
        )
        .await;
    if ok {
        Ok(db.get(req.user_id).await)
    } else {
        Ok(None)
    }
}

#[handler(desc("Delete a user by ID (requires auth)"))]
pub async fn delete_user(
    afast::State(state): afast::State<AppState>,
    afast::Custom(auth): afast::Custom<AuthCustom>,
    afast::Data(req): afast::Data<UserIdRequest>,
) -> afast::Result<Option<User>> {
    let mut db = state.db.lock().await;
    let _user_id = db
        .get_user_id_by_token(&auth.token)
        .await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    Ok(db.delete(req.user_id).await)
}

// ─── Ordinary HTTP Types ──────────────────────────────────────────

mod http_types {
    use afast::Tag;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize, Tag)]
    #[tag("Query parameters for listing users")]
    pub struct ListUsersQuery {
        #[tag("Page number")]
        pub page: Option<i64>,
        #[tag("Items per page")]
        pub size: Option<i64>,
    }

    #[derive(Debug, Deserialize, Tag)]
    #[tag("Request body for creating a user")]
    pub struct CreateUserBody {
        #[tag("Username")]
        pub username: String,
        #[tag("Password")]
        pub password: String,
        #[tag("Display name")]
        pub name: String,
    }

    #[derive(Debug, Deserialize, Tag)]
    #[tag("User ID path parameter")]
    pub struct UserIdParam {
        #[tag("User ID")]
        pub user_id: i64,
    }

    #[derive(Debug, Deserialize, Tag)]
    #[tag("Request body for updating a user")]
    pub struct UpdateUserBody {
        #[tag("Display name")]
        pub name: String,
        #[tag("Age")]
        pub age: i32,
        #[tag("Active status")]
        pub active: bool,
    }

    #[derive(Debug, Serialize, Tag)]
    #[tag("User response for HTTP endpoints")]
    pub struct UserHttp {
        #[tag("User ID")]
        pub id: i64,
        #[tag("Username")]
        pub username: String,
        #[tag("Display name")]
        pub name: String,
        #[tag("Age")]
        pub age: i32,
        #[tag("Score")]
        pub score: f64,
        #[tag("Balance")]
        pub balance: f32,
        #[tag("Active status")]
        pub active: bool,
        #[tag("Tags")]
        pub tags: Vec<String>,
        #[tag("Optional metadata")]
        pub metadata: Option<String>,
    }

    impl UserHttp {
        pub fn from_user(u: &super::User) -> Self {
            Self {
                id: u.id,
                username: u.username.clone(),
                name: u.name.clone(),
                age: u.age,
                score: u.score,
                balance: u.balance,
                active: u.active,
                tags: u.tags.clone(),
                metadata: u.metadata.clone(),
            }
        }
    }

    #[derive(Debug, Serialize, Tag)]
    #[tag("Paginated list of users via HTTP")]
    pub struct ListUsersHttpResponse {
        #[tag("Total user count")]
        pub total: i64,
        #[tag("Users on current page")]
        pub items: Vec<UserHttp>,
    }

    #[derive(Debug, Serialize, Tag)]
    #[tag("HTTP create user response")]
    pub struct CreateUserHttpResponse {
        #[tag("New user ID")]
        pub id: i64,
    }
}

use http_types::*;

// ─── Ordinary HTTP Handlers ───────────────────────────────────────

#[get(desc("List users with pagination via HTTP (requires auth)"))]
pub async fn list_users_http(
    afast::State(state): afast::State<AppState>,
    afast::Header(auth): afast::Header<AuthHeader>,
    afast::Query(query): afast::Query<ListUsersQuery>,
) -> afast::HttpResult<afast::Json<ListUsersHttpResponse>> {
    let db = state.db.lock().await;
    let _user_id = db
        .get_user_id_by_token(auth.token())
        .await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    let all = db.read(0, 1000).await;
    let total = all.len() as i64;
    let page = query.page.unwrap_or(1).max(1);
    let size = query.size.unwrap_or(10).min(100);
    let skip = ((page - 1) * size) as usize;
    let limit = size as usize;
    let items: Vec<UserHttp> = db
        .read(skip, limit)
        .await
        .iter()
        .map(UserHttp::from_user)
        .collect();
    Ok(afast::Json(ListUsersHttpResponse { total, items }))
}

#[post(desc("Create a user via HTTP (requires auth)"))]
pub async fn create_user_http(
    afast::State(state): afast::State<AppState>,
    afast::Header(auth): afast::Header<AuthHeader>,
    afast::Body(body): afast::Body<CreateUserBody>,
) -> afast::HttpResult<afast::Json<CreateUserHttpResponse>> {
    let mut db = state.db.lock().await;
    let _user_id = db
        .get_user_id_by_token(auth.token())
        .await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    let user = User::new(body.username, body.password, body.name);
    let new_id = db.create_user(user).await;
    Ok(afast::Json(CreateUserHttpResponse { id: new_id }))
}

#[get(desc("Get a user by ID via HTTP (requires auth)"))]
pub async fn get_user_http(
    afast::State(state): afast::State<AppState>,
    afast::Header(auth): afast::Header<AuthHeader>,
    afast::Param(param): afast::Param<UserIdParam>,
) -> afast::HttpResult<afast::Json<Option<UserHttp>>> {
    let db = state.db.lock().await;
    let _user_id = db
        .get_user_id_by_token(auth.token())
        .await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    let user = db.get(param.user_id).await;
    Ok(afast::Json(user.as_ref().map(UserHttp::from_user)))
}

#[put(desc("Update a user via HTTP (requires auth)"))]
pub async fn update_user_http(
    afast::State(state): afast::State<AppState>,
    afast::Header(auth): afast::Header<AuthHeader>,
    afast::Param(param): afast::Param<UserIdParam>,
    afast::Body(body): afast::Body<UpdateUserBody>,
) -> afast::HttpResult<afast::Json<Option<UserHttp>>> {
    let mut db = state.db.lock().await;
    let _user_id = db
        .get_user_id_by_token(auth.token())
        .await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    let ok = db
        .update(
            param.user_id,
            User {
                name: body.name,
                age: body.age,
                active: body.active,
                ..User::new("".into(), "".into(), "".into())
            },
        )
        .await;
    if ok {
        let user = db.get(param.user_id).await;
        Ok(afast::Json(user.as_ref().map(UserHttp::from_user)))
    } else {
        Ok(afast::Json(None))
    }
}

#[delete(desc("Delete a user by ID via HTTP (requires auth)"))]
pub async fn delete_user_http(
    afast::State(state): afast::State<AppState>,
    afast::Header(auth): afast::Header<AuthHeader>,
    afast::Param(param): afast::Param<UserIdParam>,
) -> afast::HttpResult<afast::Json<Option<UserHttp>>> {
    let mut db = state.db.lock().await;
    let _user_id = db
        .get_user_id_by_token(auth.token())
        .await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    let deleted = db.delete(param.user_id).await;
    Ok(afast::Json(deleted.as_ref().map(UserHttp::from_user)))
}
