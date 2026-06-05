//! Application state and data model definitions.
//!
//! This module defines:
//! - **AppState** — shared application state accessible from all handlers via `State<T>`
//! - **Database** — in-memory database with CRUD operations for users, articles, and tokens
//! - **Data model types** — User, Article, Role, Status, Profile, Address, etc.
//!
//! ## Derive Macros
//!
//! Data model types use these derive macros:
//!
//! - `AFastDeserialize` — enables deserialization from the afast binary protocol
//! - `AFastSerialize` — enables serialization to the afast binary protocol
//! - `Tag` — generates type metadata for code generation (TypeScript, JavaScript, Kotlin, Rust)
//! - `serde::Serialize` / `serde::Deserialize` — enables JSON serialization for HTTP ordinary routes
//!
//! ## Tag Attributes
//!
//! - `#[tag("description")]` — on structs/enums: description shown in generated docs and clients
//! - `#[tag("description")]` — on fields/variants: field-level description
//!
//! ## Conditional Serialization
//!
//! - `#[afast(skip_with("marker"))]` — field is excluded when the active marker matches
//!   The marker is set via `AFast::marker("value")` at application startup.
//!   In this example, `password` and `metadata` are excluded when marker is "afast".

use afast::{AFastDeserialize, AFastSerialize, Tag};

use std::sync::Arc;
use tokio::sync::Mutex;

// ─── Shared Application State ────────────────────────────────────

/// Shared application state, accessible from all handlers via `State<AppState>`.
///
/// Uses `Arc<Mutex<Database>>` so the database can be shared safely across
/// async tasks. Each handler extracts this with `afast::State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
}

impl AppState {
    /// Creates a new AppState with a seeded database.
    pub fn new() -> Self {
        Self {
            db: Arc::new(Mutex::new(Database::new())),
        }
    }
}

// ─── In-Memory Database ──────────────────────────────────────────

/// Simple in-memory database for demonstration.
///
/// In a real application, you would replace this with a connection pool
/// (e.g., sqlx, diesel, sea-orm) or an external service client.
pub struct Database {
    users: Vec<User>,
    articles: Vec<Article>,
    tokens: Vec<TokenInfo>,
}

/// Stores the mapping between auth tokens and user IDs.
pub struct TokenInfo {
    pub user_id: i64,
    pub token: String,
}

impl Database {
    /// Creates a new database with seed data.
    pub fn new() -> Self {
        Self {
            articles: vec![
                Article {
                    id: 1,
                    title: "Getting Started with Rust".into(),
                    content: "Rust is a systems programming language that runs blazingly fast..."
                        .into(),
                    author_id: 1,
                    published: true,
                    tags: vec!["rust".into(), "programming".into()],
                    created_at: 1715702400,
                    updated_at: 1715702400,
                },
                Article {
                    id: 2,
                    title: "Why WebAssembly Matters".into(),
                    content: "WebAssembly is changing the way we think about web development..."
                        .into(),
                    author_id: 1,
                    published: true,
                    tags: vec!["wasm".into(), "web".into()],
                    created_at: 1715788800,
                    updated_at: 1715875200,
                },
            ],
            tokens: vec![],
            // Seed user with extensive field coverage to demonstrate all data types
            users: vec![User {
                id: 1,
                username: "ahriknow".to_string(),
                password: "123456".to_string(),
                name: "Ahriknow".to_string(),
                age: 28,
                score: 98.5,
                balance: 100.50,
                active: true,
                tags: vec!["rust".to_string(), "backend".to_string()],
                metadata: None,
                role: Role::Admin,
                status: Status::Active,
                profile: Profile {
                    avatar: "https://example.com/avatar.png".to_string(),
                    bio: Some("Rust developer".to_string()),
                    level: 42,
                },
                addresses: vec![Address {
                    kind: AddressKind::Home,
                    street: "123 Main St".to_string(),
                    city: "Beijing".to_string(),
                    zip: Some("100000".to_string()),
                }],
                scores: vec![85, 92, 78, 95],
                flags: vec![true, false, true],
                bytes: vec![0x00, 0x01, 0x02, 0xff],
                ratio: 0.5,
                big_id: 9223372036854775807i64,
                small_num: 42i8,
                short_num: 300i16,
                unsigned_num: 255u8,
                med_unsigned: 60000u16,
                large_unsigned: 18446744073709551615u64,
                count: 1000usize,
                temperature: -15.5,
                optional_age: Some(30),
                optional_name: None,
                dimensions: Some(vec![1.0, 2.0, 3.0]),
                event_log: vec![
                    UserEvent::LoggedIn,
                    UserEvent::PasswordChanged {
                        old_hash: "abc123".to_string(),
                        new_hash: "def456".to_string(),
                    },
                    UserEvent::LoggedOut,
                ],
            }],
        }
    }

    // ─── User CRUD ─────────────────────────────────────────────

    /// Creates a new user and returns their ID.
    pub async fn create_user(&mut self, user: User) -> i64 {
        let max_id = self.users.iter().map(|u| u.id).max().unwrap_or(1);
        let new_id = max_id + 1;
        self.users.push(User { id: new_id, ..user });
        new_id
    }

    /// Lists users with pagination (skip + limit).
    pub async fn read(&self, skip: usize, limit: usize) -> Vec<User> {
        self.users.iter().skip(skip).take(limit).cloned().collect()
    }

    /// Gets a user by ID.
    pub async fn get(&self, id: i64) -> Option<User> {
        self.users.iter().find(|u| u.id == id).cloned()
    }

    /// Updates a user's mutable fields. Returns true if found.
    pub async fn update(&mut self, id: i64, user: User) -> bool {
        if let Some(existing) = self.users.iter_mut().find(|u| u.id == id) {
            existing.name = user.name;
            existing.age = user.age;
            existing.active = user.active;
            existing.tags = user.tags;
            existing.role = user.role;
            existing.status = user.status;
            existing.profile = user.profile;
            existing.addresses = user.addresses;
            true
        } else {
            false
        }
    }

    /// Deletes a user by ID. Returns the deleted user if found.
    pub async fn delete(&mut self, id: i64) -> Option<User> {
        if let Some(pos) = self.users.iter().position(|u| u.id == id) {
            Some(self.users.remove(pos))
        } else {
            None
        }
    }

    // ─── Auth ────────────────────────────────────────────────

    /// Finds a user by username and password.
    pub async fn find_user_by_credentials(&self, username: &str, password: &str) -> Option<User> {
        self.users
            .iter()
            .find(|u| u.username == username && u.password == password)
            .cloned()
    }

    /// Creates a new auth token for a user.
    pub async fn create_token(&mut self, user_id: i64) -> String {
        let token = format!("tok_{}_{}", user_id, rand_id());
        self.tokens.push(TokenInfo {
            user_id,
            token: token.clone(),
        });
        token
    }

    /// Resolves a token to a user ID.
    pub async fn get_user_id_by_token(&self, token: &str) -> Option<i64> {
        self.tokens
            .iter()
            .find(|t| t.token == token)
            .map(|t| t.user_id)
    }

    // ─── Article CRUD ────────────────────────────────────────

    /// Creates a new article and returns its ID.
    pub async fn create_article(&mut self, article: Article) -> i64 {
        let max_id = self.articles.iter().map(|a| a.id).max().unwrap_or(0);
        let new_id = max_id + 1;
        self.articles.push(Article {
            id: new_id,
            ..article
        });
        new_id
    }

    /// Lists articles with optional published-only filter and pagination.
    pub async fn list_articles(
        &self,
        skip: usize,
        limit: usize,
        published_only: bool,
    ) -> Vec<Article> {
        self.articles
            .iter()
            .filter(|a| !published_only || a.published)
            .skip(skip)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Counts articles with optional published-only filter.
    pub async fn count_articles(&self, published_only: bool) -> i64 {
        self.articles
            .iter()
            .filter(|a| !published_only || a.published)
            .count() as i64
    }

    /// Gets an article by ID.
    pub async fn get_article(&self, id: i64) -> Option<Article> {
        self.articles.iter().find(|a| a.id == id).cloned()
    }

    /// Updates an article's mutable fields. Returns true if found.
    pub async fn update_article(&mut self, id: i64, article: Article) -> bool {
        if let Some(existing) = self.articles.iter_mut().find(|a| a.id == id) {
            existing.title = article.title;
            existing.content = article.content;
            existing.published = article.published;
            existing.tags = article.tags;
            existing.updated_at = now_ts();
            true
        } else {
            false
        }
    }

    /// Deletes an article by ID. Returns the deleted article if found.
    pub async fn delete_article(&mut self, id: i64) -> Option<Article> {
        if let Some(pos) = self.articles.iter().position(|a| a.id == id) {
            Some(self.articles.remove(pos))
        } else {
            None
        }
    }
}

// ─── Helper Functions ────────────────────────────────────────────

/// Returns the current Unix timestamp in seconds.
fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Generates a random-looking ID from the current timestamp hash.
fn rand_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut s = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut s);
    format!("{:016x}", s.finish())
}

// ─── User ────────────────────────────────────────────────────────
//
// The User struct demonstrates all primitive types, nested types,
// Option, Vec, and enum variants. The #[afast(skip_with("afast"))]
// attribute on `password` and `metadata` means these fields are
// excluded from serialization when the marker is "afast".

/// User account information.
///
/// This struct demonstrates all data types supported by afast:
/// - Primitives: i8, i16, i32, i64, u8, u16, u64, f32, f64, bool, String
/// - Collections: `Vec<T>`, `Vec<u8>` (bytes)
/// - Optional: `Option<T>`
/// - Nested structs: Profile, Address
/// - Enums: Role, Status, UserEvent
#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("User account information")]
pub struct User {
    #[tag("User ID")]
    pub id: i64,
    #[tag("Login username")]
    pub username: String,
    /// Password is excluded from client code when marker is "afast"
    #[afast(skip_with("afast"))]
    pub password: String,
    #[tag("Display name")]
    pub name: String,
    #[tag("Age in years")]
    pub age: i32,
    #[tag("Score")]
    pub score: f64,
    #[tag("Account balance")]
    pub balance: f32,
    #[tag("Whether the account is active")]
    pub active: bool,
    #[tag("User tags")]
    pub tags: Vec<String>,
    /// Metadata is excluded from client code when marker is "afast"
    #[afast(skip_with("afast"))]
    pub metadata: Option<String>,
    #[tag("User role")]
    pub role: Role,
    #[tag("Account status")]
    pub status: Status,
    #[tag("User profile")]
    pub profile: Profile,
    #[tag("User addresses")]
    pub addresses: Vec<Address>,
    #[tag("Test scores")]
    pub scores: Vec<i32>,
    #[tag("Feature flags")]
    pub flags: Vec<bool>,
    #[tag("Raw bytes")]
    pub bytes: Vec<u8>,
    #[tag("Float ratio")]
    pub ratio: f32,
    #[tag("Large ID (i64)")]
    pub big_id: i64,
    #[tag("Small number (i8)")]
    pub small_num: i8,
    #[tag("Short number (i16)")]
    pub short_num: i16,
    #[tag("Unsigned byte (u8)")]
    pub unsigned_num: u8,
    #[tag("Medium unsigned (u16)")]
    pub med_unsigned: u16,
    #[tag("Large unsigned (u64)")]
    pub large_unsigned: u64,
    #[tag("Size count (usize)")]
    pub count: usize,
    #[tag("Temperature in Celsius")]
    pub temperature: f64,
    #[tag("Optional age")]
    pub optional_age: Option<i32>,
    #[tag("Optional name")]
    pub optional_name: Option<String>,
    #[tag("Optional dimensions")]
    pub dimensions: Option<Vec<f64>>,
    #[tag("Event log")]
    pub event_log: Vec<UserEvent>,
}

impl User {
    /// Creates a new user with default values.
    pub fn new(username: String, password: String, name: String) -> Self {
        Self {
            id: 0,
            username,
            password,
            name,
            age: 0,
            score: 0.0,
            balance: 0.0,
            active: true,
            tags: vec![],
            metadata: None,
            role: Role::User,
            status: Status::Inactive,
            profile: Profile {
                avatar: String::new(),
                bio: None,
                level: 0,
            },
            addresses: vec![],
            scores: vec![],
            flags: vec![],
            bytes: vec![],
            ratio: 0.0,
            big_id: 0,
            small_num: 0,
            short_num: 0,
            unsigned_num: 0,
            med_unsigned: 0,
            large_unsigned: 0,
            count: 0,
            temperature: 0.0,
            optional_age: None,
            optional_name: None,
            dimensions: None,
            event_log: vec![],
        }
    }
}

// ─── Nested Types ─────────────────────────────────────────────────

/// User role — demonstrates simple enum variants (unit variants).
///
/// In the generated TypeScript client, this becomes:
/// ```typescript
/// type Role = { tag: 'Admin', data: null } | { tag: 'User', data: null } | { tag: 'Guest', data: null };
/// ```
#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("User role")]
pub enum Role {
    #[tag("System administrator")]
    Admin,
    #[tag("Regular user")]
    User,
    #[tag("Read-only guest")]
    Guest,
}

/// Account status — another simple enum.
#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("Account status")]
pub enum Status {
    Active,
    Inactive,
    Banned,
}

/// User profile — demonstrates nested struct.
#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("User profile")]
pub struct Profile {
    #[tag("Avatar URL")]
    pub avatar: String,
    #[tag("Short biography")]
    pub bio: Option<String>,
    #[tag("Experience level")]
    pub level: u32,
}

/// User address — demonstrates nested struct with enum field.
#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("User address")]
pub struct Address {
    #[tag("Address category")]
    pub kind: AddressKind,
    #[tag("Street address")]
    pub street: String,
    #[tag("City")]
    pub city: String,
    #[tag("ZIP/postal code")]
    pub zip: Option<String>,
}

/// Address category — demonstrates enum inside a struct.
#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("Address category")]
pub enum AddressKind {
    #[tag("Home address")]
    Home,
    #[tag("Work address")]
    Work,
    #[tag("Other address")]
    Other,
}

/// User event log entry — demonstrates enum with data-carrying variants.
///
/// In the generated TypeScript client, this becomes:
/// ```typescript
/// type UserEvent =
///     { tag: 'LoggedIn', data: null } |
///     { tag: 'LoggedOut', data: null } |
///     { tag: 'Error', data: string } |
///     { tag: 'PasswordChanged', data: { old_hash: string, new_hash: string } };
/// ```
#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("User event log entry")]
pub enum UserEvent {
    #[tag("User logged in")]
    LoggedIn,
    #[tag("User logged out")]
    LoggedOut,
    /// Tuple variant — carries a String
    #[tag("Error occurred")]
    Error(String),
    /// Struct variant — carries named fields
    #[tag("Password was changed")]
    PasswordChanged {
        #[tag("Previous password hash")]
        old_hash: String,
        #[tag("New password hash")]
        new_hash: String,
    },
}

// ─── Article ──────────────────────────────────────────────────────

/// Blog article — demonstrates a simpler data model with tagged fields.
#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("Blog article")]
pub struct Article {
    #[tag("Article ID")]
    pub id: i64,
    #[tag("Article title")]
    pub title: String,
    #[tag("Article content in markdown")]
    pub content: String,
    #[tag("Author's user ID")]
    pub author_id: i64,
    #[tag("Whether the article is published")]
    pub published: bool,
    #[tag("Article tags")]
    pub tags: Vec<String>,
    #[tag("Unix timestamp of creation")]
    pub created_at: i64,
    #[tag("Unix timestamp of last update")]
    pub updated_at: i64,
}

impl Article {
    /// Creates a new article with default timestamps.
    pub fn new(title: String, content: String, author_id: i64) -> Self {
        let now = now_ts();
        Self {
            id: 0,
            title,
            content,
            author_id,
            published: false,
            tags: vec![],
            created_at: now,
            updated_at: now,
        }
    }
}
