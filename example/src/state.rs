use afast::{AFastDeserialize, AFastSerialize, Tag};

use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            db: Arc::new(Mutex::new(Database::new())),
        }
    }
}

pub struct Database {
    users: Vec<User>,
    articles: Vec<Article>,
    tokens: Vec<TokenInfo>,
}

pub struct TokenInfo {
    pub user_id: i64,
    pub token: String,
}

impl Database {
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

    pub async fn create_user(&mut self, user: User) -> i64 {
        let max_id = self.users.iter().map(|u| u.id).max().unwrap_or(1);
        let new_id = max_id + 1;
        self.users.push(User { id: new_id, ..user });
        new_id
    }

    pub async fn read(&self, skip: usize, limit: usize) -> Vec<User> {
        self.users.iter().skip(skip).take(limit).cloned().collect()
    }

    pub async fn get(&self, id: i64) -> Option<User> {
        self.users.iter().find(|u| u.id == id).cloned()
    }

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

    pub async fn delete(&mut self, id: i64) -> Option<User> {
        if let Some(pos) = self.users.iter().position(|u| u.id == id) {
            Some(self.users.remove(pos))
        } else {
            None
        }
    }

    // ─── Auth ────────────────────────────────────────────────

    pub async fn find_user_by_credentials(&self, username: &str, password: &str) -> Option<User> {
        self.users
            .iter()
            .find(|u| u.username == username && u.password == password)
            .cloned()
    }

    pub async fn create_token(&mut self, user_id: i64) -> String {
        let token = format!("tok_{}_{}", user_id, rand_id());
        self.tokens.push(TokenInfo {
            user_id,
            token: token.clone(),
        });
        token
    }

    pub async fn get_user_id_by_token(&self, token: &str) -> Option<i64> {
        self.tokens
            .iter()
            .find(|t| t.token == token)
            .map(|t| t.user_id)
    }

    // ─── Articles ────────────────────────────────────────────

    pub async fn create_article(&mut self, article: Article) -> i64 {
        let max_id = self.articles.iter().map(|a| a.id).max().unwrap_or(0);
        let new_id = max_id + 1;
        self.articles.push(Article {
            id: new_id,
            ..article
        });
        new_id
    }

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

    pub async fn count_articles(&self, published_only: bool) -> i64 {
        self.articles
            .iter()
            .filter(|a| !published_only || a.published)
            .count() as i64
    }

    pub async fn get_article(&self, id: i64) -> Option<Article> {
        self.articles.iter().find(|a| a.id == id).cloned()
    }

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

    pub async fn delete_article(&mut self, id: i64) -> Option<Article> {
        if let Some(pos) = self.articles.iter().position(|a| a.id == id) {
            Some(self.articles.remove(pos))
        } else {
            None
        }
    }
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn rand_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut s = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut s);
    format!("{:016x}", s.finish())
}

// ─── User ────────────────────────────────────────────────────────

#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("User account information")]
pub struct User {
    pub id: i64,
    pub username: String,
    #[afast(skip_with("afast"))]
    pub password: String,
    pub name: String,
    pub age: i32,
    pub score: f64,
    pub balance: f32,
    pub active: bool,
    pub tags: Vec<String>,
    #[afast(skip_with("afast"))]
    pub metadata: Option<String>,
    pub role: Role,
    pub status: Status,
    pub profile: Profile,
    pub addresses: Vec<Address>,
    pub scores: Vec<i32>,
    pub flags: Vec<bool>,
    pub bytes: Vec<u8>,
    pub ratio: f32,
    pub big_id: i64,
    pub small_num: i8,
    pub short_num: i16,
    pub unsigned_num: u8,
    pub med_unsigned: u16,
    pub large_unsigned: u64,
    pub count: usize,
    pub temperature: f64,
    pub optional_age: Option<i32>,
    pub optional_name: Option<String>,
    pub dimensions: Option<Vec<f64>>,
    pub event_log: Vec<UserEvent>,
}

impl User {
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

#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("Account status")]
pub enum Status {
    Active,
    Inactive,
    Banned,
}

#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("User profile")]
pub struct Profile {
    pub avatar: String,
    pub bio: Option<String>,
    pub level: u32,
}

#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("User address")]
pub struct Address {
    pub kind: AddressKind,
    pub street: String,
    pub city: String,
    pub zip: Option<String>,
}

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

#[derive(
    Debug, AFastDeserialize, AFastSerialize, Tag, Clone, serde::Serialize, serde::Deserialize,
)]
#[tag("User event log entry")]
pub enum UserEvent {
    #[tag("User logged in")]
    LoggedIn,
    #[tag("User logged out")]
    LoggedOut,
    #[tag("Error occurred")]
    Error(String),
    #[tag("Password was changed")]
    PasswordChanged {
        #[tag("Previous password hash")]
        old_hash: String,
        #[tag("New password hash")]
        new_hash: String,
    },
}

// ─── Article ──────────────────────────────────────────────────────

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
