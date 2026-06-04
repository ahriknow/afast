pub mod admin;
pub mod article;
pub mod auth;
pub mod chat;
pub mod sse_stream;
pub mod ws_chat;

use afast::{AFastDeserialize, AFastSerialize, Tag, get, handler};
use serde::Serialize;

#[derive(AFastSerialize, Tag)]
#[tag("Health check result")]
pub struct TokenResponse {
    #[tag("Version")]
    version: String,
}

#[handler(desc("Health check"))]
pub async fn health() -> afast::Result<TokenResponse> {
    Ok(TokenResponse {
        version: "0.0.0".to_string(),
    })
}

#[derive(AFastDeserialize, Tag)]
#[tag("First data")]
pub struct FirstData {
    id: i64,
}

#[derive(AFastDeserialize, Tag)]
#[tag("Second data")]
pub struct SecondData {
    name: String,
}

#[derive(AFastSerialize, Tag)]
#[tag("Health check result")]
pub struct InfoResponst {
    message: String,
}

#[handler(desc("System info"))]
pub async fn info(
    afast::Data(first): afast::Data<FirstData>,
    afast::Data(second): afast::Data<SecondData>,
) -> afast::Result<InfoResponst> {
    println!("first => {}, second => {}", first.id, second.name);
    Ok(InfoResponst {
        message: "message".to_string(),
    })
}

#[derive(Serialize, Tag)]
#[tag("Pong response")]
pub struct PongResponse {
    #[tag("Whether the server is alive")]
    pong: bool,
}

#[get(desc("Simple ping endpoint"))]
pub async fn ping() -> afast::HttpResult<afast::Json<PongResponse>> {
    Ok(afast::Json(PongResponse { pong: true }))
}
