use afast::{AFast, DocConfig, GenerateTarget, JsTsCallType, KtCallType, Lang, register, service};

mod handler;
mod state;

use handler::admin::{
    delete_user, delete_user_http, get_user_http, list_users, list_users_http, update_user,
    update_user_http,
};
use handler::article::{
    create_article, delete_article, get_article, list_articles, update_article,
};
use handler::auth::{create_token, get_user_id, login, register};
use handler::chat::chat_echo;
use handler::{health, info};
use state::AppState;

// ─── Entry Point ──────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let check_svc = service!("check", "Check Service" => {
        h(health),
        h(info)
    });

    let admin_svc = service!("admin", "Admin Service" => {
        group("user" => {
            h(handler::admin::create_user),
            h(list_users),
            h(update_user),
            h(delete_user),
            get("", list_users_http),
            post("", handler::admin::create_user_http),
            group(":user_id" => {
                get("", get_user_http),
                put("", update_user_http),
                delete("", delete_user_http),
            })
        })
    });

    let auth_svc = service!("auth", "Auth Service" => {
        h(register),
        h(login),
        h(create_token),
        h(get_user_id),
    });

    let article_svc = service!("article", "Article Service" => {
        h(create_article),
        h(list_articles),
        h(get_article),
        h(update_article),
        h(delete_article),
    });

    let chat_svc = service!("chat", "Chat Service" => {
        h(chat_echo),
    });

    let app = AFast::new()
        .state(AppState::new())
        .document(DocConfig::with("Blog API Docs", "./client/doc"))
        .generate(vec![
            GenerateTarget {
                debug: true,
                lang: Lang::TS(vec![
                    JsTsCallType::Fetch,
                    JsTsCallType::Ws,
                    JsTsCallType::BunTcp,
                    JsTsCallType::NodeTcp,
                    JsTsCallType::UniRequest,
                    JsTsCallType::UniWs,
                    JsTsCallType::WxRequest,
                    JsTsCallType::WxWs,
                ]),
                path: "./client".into(),
            },
            GenerateTarget {
                debug: true,
                lang: Lang::JS(vec![
                    JsTsCallType::Fetch,
                    JsTsCallType::Ws,
                    JsTsCallType::BunTcp,
                    JsTsCallType::NodeTcp,
                    JsTsCallType::UniRequest,
                    JsTsCallType::UniWs,
                    JsTsCallType::WxRequest,
                    JsTsCallType::WxWs,
                ]),
                path: "./client".into(),
            },
            GenerateTarget {
                debug: true,
                lang: Lang::KT(vec![KtCallType::Http, KtCallType::Ws, KtCallType::Tcp]),
                path: "./client".into(),
            },
        ])
        .service(check_svc)
        .service(admin_svc)
        .service(auth_svc)
        .service(article_svc)
        .service(chat_svc)
        .ws("[::]:3000")
        .http("[::]:5000")
        .tcp("[::]:4000");

    #[cfg(feature = "tls")]
    let app = app.https("[::]:5443", "./cert.pem", "./key.pem");

    app.run().await.unwrap();
}
