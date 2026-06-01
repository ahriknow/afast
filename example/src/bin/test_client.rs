//! Test binary for the generated Rust client — covers ALL interfaces.
//!
//! Usage:
//!   1. Start the server:  cargo run -p example
//!   2. In another terminal:  cargo run -p example --bin test_client

mod client;

use client::admin::{
    AdminClient, UserCreateUserRequest, UserDeleteUserIdRequest, UserListUsersRequest,
    UserUpdateUserRequest,
};
use client::article::{
    ArticleClient, CreateArticleRequest, DeleteArticleIdRequest, GetArticleIdRequest,
    ListArticlesRequest, UpdateArticleRequest,
};
use client::auth::{AuthClient, CreateTokenRequest, LoginRequest, RegisterRequest};
use client::chat::{ChatClient, ChatEchoChatJoin};
use client::check::{CheckClient, InnerInfoFirstData, InnerInfoSecondData};
use client::common::AuthCustom;

#[tokio::main]
async fn main() {
    let addr = "127.0.0.1:4001";
    let mut passed: u32 = 0;
    let mut failed: u32 = 0;

    macro_rules! ok {
        ($label:expr, $expr:expr) => {
            match $expr {
                Ok(v) => {
                    println!("  ✓ {} => {:?}", $label, v);
                    passed += 1;
                    Some(v)
                }
                Err(e) => {
                    eprintln!("  ✗ {} error: {}", $label, e);
                    failed += 1;
                    None
                }
            }
        };
    }

    println!("╔══════════════════════════════════════════════╗");
    println!("║   afast Generated Client — Full API Test     ║");
    println!("╚══════════════════════════════════════════════╝\n");

    // ====================================================================
    //  1. check service
    // ====================================================================
    println!("━━━ [check] service ━━━");
    let check = CheckClient::new(addr).await.expect("connect failed");

    ok!("health()", check.health().await);

    ok!(
        "inner().info()",
        check
            .inner()
            .info(
                &InnerInfoFirstData { id: 42 },
                &InnerInfoSecondData {
                    name: "test".into(),
                },
            )
            .await
    );

    // ====================================================================
    //  2. auth service
    // ====================================================================
    println!("\n━━━ [auth] service ━━━");
    let auth = AuthClient::new(addr).await.expect("connect failed");

    // 2a. signup()
    let signup_resp = auth
        .signup(&RegisterRequest {
            username: "testuser".into(),
            password: "pass123".into(),
            name: "Test User".into(),
        })
        .await;
    let signup_resp = ok!("signup()", signup_resp.as_ref());
    let user_id = signup_resp.map(|r| r.user.id).unwrap_or(0);
    let token = signup_resp.map(|r| r.token.clone()).unwrap_or_default();

    // 2b. login()
    ok!(
        "login()",
        auth.login(&LoginRequest {
            username: "testuser".into(),
            password: "pass123".into(),
        })
        .await
    );

    // 2c. create_token()
    if user_id > 0 {
        ok!(
            "create_token()",
            auth.create_token(&CreateTokenRequest { user_id }).await
        );
    }

    // 2d. get_user_id() — uses AuthCustom via the public customs field
    {
        let mut auth2 = AuthClient::new(addr).await.expect("connect failed");
        auth2.customs.push(Box::new({
            let t = token.clone();
            move || Ok(Box::new(AuthCustom { token: t.clone() }))
        }));
        ok!("get_user_id()", auth2.get_user_id().await);
    }

    // ====================================================================
    //  3. admin service
    // ====================================================================
    println!("\n━━━ [admin] service ━━━");
    let mut admin = AdminClient::new(addr).await.expect("connect failed");
    admin.customs.push(Box::new({
        let t = token.clone();
        move || Ok(Box::new(AuthCustom { token: t.clone() }))
    }));

    // 3a. health()
    ok!("health()", admin.health().await);

    // 3b. user().create_user()
    let new_user = ok!(
        "user().create_user()",
        admin
            .user()
            .create_user(&UserCreateUserRequest {
                username: "admin_created".into(),
                password: "pw".into(),
                name: "Admin Created".into(),
            })
            .await
    );
    let new_admin_user_id = new_user.map(|r| r.id).unwrap_or(0);

    // 3c. user().list_users()
    ok!(
        "user().list_users()",
        admin
            .user()
            .list_users(&UserListUsersRequest { page: 1, size: 10 })
            .await
    );

    // 3d. user().update_user()
    if new_admin_user_id > 0 {
        ok!(
            "user().update_user()",
            admin
                .user()
                .update_user(&UserUpdateUserRequest {
                    user_id: new_admin_user_id,
                    name: "Updated Name".into(),
                    age: 25,
                    active: true,
                })
                .await
        );
    }

    // 3e. user().delete_user()
    if new_admin_user_id > 0 {
        ok!(
            "user().delete_user()",
            admin
                .user()
                .delete_user(&UserDeleteUserIdRequest {
                    user_id: new_admin_user_id,
                })
                .await
        );
    }

    // ====================================================================
    //  4. article service
    // ====================================================================
    println!("\n━━━ [article] service ━━━");
    let mut article = ArticleClient::new(addr).await.expect("connect failed");
    article.customs.push(Box::new({
        let t = token.clone();
        move || Ok(Box::new(AuthCustom { token: t.clone() }))
    }));

    // 4a. create_article()
    let new_article = ok!(
        "create_article()",
        article
            .create_article(&CreateArticleRequest {
                title: "Test Article".into(),
                content: "Hello world".into(),
                published: true,
                tags: vec!["test".into(), "afast".into()],
            })
            .await
    );
    let article_id = new_article.map(|r| r.id).unwrap_or(0);

    // 4b. list_articles()
    ok!(
        "list_articles()",
        article
            .list_articles(&ListArticlesRequest {
                page: 1,
                size: 10,
                published_only: false,
            })
            .await
    );

    // 4c. get_article() — returns Option<Article>
    if article_id > 0 {
        ok!(
            "get_article()",
            article
                .get_article(&GetArticleIdRequest { article_id })
                .await
        );
    }

    // 4d. update_article()
    if article_id > 0 {
        ok!(
            "update_article()",
            article
                .update_article(&UpdateArticleRequest {
                    article_id,
                    title: "Updated Title".into(),
                    content: "Updated content".into(),
                    published: false,
                    tags: vec!["updated".into()],
                })
                .await
        );
    }

    // 4e. delete_article()
    if article_id > 0 {
        ok!(
            "delete_article()",
            article
                .delete_article(&DeleteArticleIdRequest { article_id })
                .await
        );
    }

    // ====================================================================
    //  5. chat service
    // ====================================================================
    println!("\n━━━ [chat] service ━━━");
    let chat = ChatClient::new(addr).await.expect("connect failed");

    ok!(
        "chat_echo()",
        chat.chat_echo(&ChatEchoChatJoin {
            name: "tester".into(),
        })
        .await
    );

    // ====================================================================
    //  Summary
    // ====================================================================
    println!("\n╔══════════════════════════════════════════════╗");
    println!(
        "║  Results: {} passed, {} failed              ║",
        passed, failed
    );
    println!("╚══════════════════════════════════════════════╝");
}
