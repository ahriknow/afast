use afast::{AFastDeserialize, AFastSerialize, Tag, handler};

use crate::handler::auth::AuthCustom;
use crate::state::{AppState, Article};

// ─── Request Types ────────────────────────────────────────────────

#[derive(AFastDeserialize, Tag)]
#[tag("Request to create a new article")]
pub struct CreateArticleRequest {
    #[tag("Article title")]
    pub title: String,
    #[tag("Article content in markdown")]
    pub content: String,
    #[tag("Whether to publish immediately")]
    pub published: bool,
    #[tag("Article tags")]
    pub tags: Vec<String>,
}

#[derive(AFastDeserialize, Tag)]
#[tag("Pagination parameters for listing articles")]
pub struct ListArticlesRequest {
    #[tag("Page number, 1-indexed")]
    pub page: i64,
    #[tag("Number of items per page")]
    pub size: i64,
    #[tag("If true, only return published articles")]
    pub published_only: bool,
}

#[derive(AFastDeserialize, Tag)]
#[tag("Request to get or delete an article by ID")]
pub struct ArticleIdRequest {
    #[tag("Article ID")]
    pub article_id: i64,
}

#[derive(AFastDeserialize, Tag)]
#[tag("Request to update an existing article")]
pub struct UpdateArticleRequest {
    #[tag("Article ID")]
    pub article_id: i64,
    #[tag("New title")]
    pub title: String,
    #[tag("New content")]
    pub content: String,
    #[tag("Whether the article is published")]
    pub published: bool,
    #[tag("New tags")]
    pub tags: Vec<String>,
}

// ─── Response Types ────────────────────────────────────────────────

#[derive(AFastSerialize, Tag)]
#[tag("Response with new article ID")]
pub struct CreateArticleResponse {
    #[tag("New article ID")]
    pub id: i64,
}

#[derive(AFastSerialize, Tag)]
#[tag("Paginated list of articles")]
pub struct ListArticlesResponse {
    #[tag("Total number of articles")]
    pub total: i64,
    #[tag("Articles for the current page")]
    pub items: Vec<Article>,
}

// ─── Handlers ─────────────────────────────────────────────────────

#[handler(desc("Create a new article"))]
pub async fn create_article(
    afast::State(state): afast::State<AppState>,
    afast::Custom(auth): afast::Custom<AuthCustom>,
    afast::Data(req): afast::Data<CreateArticleRequest>,
) -> afast::Result<CreateArticleResponse> {
    let mut db = state.db.lock().await;
    let user_id = db
        .get_user_id_by_token(&auth.token)
        .await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    let article = Article {
        author_id: user_id,
        ..Article::new(req.title, req.content, user_id)
    };
    let article = Article {
        published: req.published,
        tags: req.tags,
        ..article
    };
    let new_id = db.create_article(article).await;
    Ok(CreateArticleResponse { id: new_id })
}

#[handler(desc("List articles with pagination"))]
pub async fn list_articles(
    afast::State(state): afast::State<AppState>,
    afast::Data(req): afast::Data<ListArticlesRequest>,
) -> afast::Result<ListArticlesResponse> {
    let db = state.db.lock().await;
    let total = db.count_articles(req.published_only).await;
    let skip = ((req.page - 1) * req.size) as usize;
    let limit = req.size as usize;
    let items = db.list_articles(skip, limit, req.published_only).await;
    Ok(ListArticlesResponse { total, items })
}

#[handler(desc("Get an article by ID"))]
pub async fn get_article(
    afast::State(state): afast::State<AppState>,
    afast::Data(req): afast::Data<ArticleIdRequest>,
) -> afast::Result<Option<Article>> {
    let db = state.db.lock().await;
    Ok(db.get_article(req.article_id).await)
}

#[handler(desc("Update an existing article"))]
pub async fn update_article(
    afast::State(state): afast::State<AppState>,
    afast::Custom(auth): afast::Custom<AuthCustom>,
    afast::Data(req): afast::Data<UpdateArticleRequest>,
) -> afast::Result<Option<Article>> {
    let mut db = state.db.lock().await;
    let user_id = db
        .get_user_id_by_token(&auth.token)
        .await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    let article = Article {
        author_id: user_id,
        ..Article::new(req.title, req.content, user_id)
    };
    let article = Article {
        published: req.published,
        tags: req.tags,
        ..article
    };
    let ok = db.update_article(req.article_id, article).await;
    if ok {
        Ok(db.get_article(req.article_id).await)
    } else {
        Ok(None)
    }
}

#[handler(desc("Delete an article by ID"))]
pub async fn delete_article(
    afast::State(state): afast::State<AppState>,
    afast::Custom(auth): afast::Custom<AuthCustom>,
    afast::Data(req): afast::Data<ArticleIdRequest>,
) -> afast::Result<Option<Article>> {
    let mut db = state.db.lock().await;
    let _user_id = db
        .get_user_id_by_token(&auth.token)
        .await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    Ok(db.delete_article(req.article_id).await)
}
