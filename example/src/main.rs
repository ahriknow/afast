use afast::{
    AFast, Algorithm, DocConfig, GenerateTarget, JsTsCallType, KtCallType, Lang, RateLimitConfig,
    RateLimitKey, RateLimitPolicy, RsCallType, register, service,
};

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
use handler::{health, info, ping};
use state::AppState;

// ─── Entry Point ──────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let check_svc = service!("check", "Check Service" => {
        h(health),
        group("inner" => {
            h(info),
        })
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

    // Duplicate service name: handlers will be merged into the first "admin" service
    let admin_extra_svc = service!("admin", "Admin Extra" => {
        h(health),
    });

    // Empty service name: handlers are registered and callable via binary protocol,
    // but excluded from client code generation and API documentation.
    let internal_svc = service!("", "Internal" => {
        h(info),
        get("ping", ping),
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
            GenerateTarget {
                debug: true,
                lang: Lang::RS(vec![RsCallType::TcpAsync]),
                path: "./example/src/bin/client".into(),
            },
        ])
        .service(check_svc)
        .service(admin_svc)
        .service(auth_svc)
        .service(article_svc)
        .service(chat_svc)
        .service(admin_extra_svc) // merges into "admin"
        .service(internal_svc) // empty name: excluded from codegen/docs
        .marker("afast") // marker for conditional field skipping (afastdata 0.0.7+)
        .rate_limit(
            RateLimitConfig::new()
                // Login: 每 IP 每分钟最多 5 次
                .policy(RateLimitPolicy {
                    id: "login".into(),
                    max_requests: 5,
                    window_secs: 60,
                    key: RateLimitKey::Ip,
                    algorithm: Algorithm::SlidingWindow,
                })
                // 默认策略：每 IP 每秒最多 100 次（未显式配置限流的接口自动使用）
                .default_policy("global")
                .policy(RateLimitPolicy {
                    id: "global".into(),
                    max_requests: 100,
                    window_secs: 1,
                    key: RateLimitKey::Ip,
                    algorithm: Algorithm::SlidingWindow,
                }),
        )
        .ws("[::]:3001")
        .http("[::]:5001")
        .tcp("[::]:4001");

    #[cfg(feature = "tls")]
    let app = app.https("[::]:5443", "./cert.pem", "./key.pem");

    app.run().await.unwrap();
}
