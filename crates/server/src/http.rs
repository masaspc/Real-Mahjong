//! 部屋の HTTP の口。
//!
//! **game protocol とは別系統にする。**凍結された `crates/protocol` に
//! 部屋の概念を持ち込まないためであり、また部屋の形はこれから変わる
//! （観戦・段位別マッチング）ので、凍結の外に置くのが正しい。
//!
//! 席の証明は `X-Mahjong-Token` ヘッダで受ける。**クエリ文字列に置かない。**
//! アクセスログや `Referer` に席の証明が残る。

use crate::rooms::{Code, JoinError, Lobby, Rooms, StartError, Token};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

/// 席の証明を運ぶヘッダ。
pub const TOKEN_HEADER: &str = "x-mahjong-token";

#[derive(Deserialize, Default)]
pub struct NameBody {
    #[serde(default)]
    name: String,
}

#[derive(Serialize)]
struct Created {
    code: String,
    token: String,
}

#[derive(Serialize)]
struct Joined {
    token: String,
}

#[derive(Serialize)]
struct Started {
    state: &'static str,
}

/// 失敗の返し方。
///
/// **`error` は画面が分岐に使う名前で、人に見せる文ではない。**
/// 表示文をサーバが持つと、言い回しを直すたびにサーバを出し直すことになる。
fn fail(status: StatusCode, slug: &'static str) -> Response {
    (status, Json(serde_json::json!({ "error": slug }))).into_response()
}

fn token_of(headers: &HeaderMap) -> Option<Token> {
    headers
        .get(TOKEN_HEADER)?
        .to_str()
        .ok()
        .map(|text| Token(text.to_owned()))
}

async fn create(State(rooms): State<Rooms>, body: Option<Json<NameBody>>) -> Response {
    let name = body.map(|Json(b)| b.name).unwrap_or_default();
    let (Code(code), Token(token)) = rooms.create(&name, rooms.now_ms());
    Json(Created { code, token }).into_response()
}

async fn join(
    State(rooms): State<Rooms>,
    Path(code): Path<String>,
    body: Option<Json<NameBody>>,
) -> Response {
    let name = body.map(|Json(b)| b.name).unwrap_or_default();
    match rooms.join(&Code(code), &name, rooms.now_ms()) {
        Ok(Token(token)) => Json(Joined { token }).into_response(),
        Err(JoinError::NoSuchRoom) => fail(StatusCode::NOT_FOUND, JoinError::NoSuchRoom.slug()),
        Err(other) => fail(StatusCode::CONFLICT, other.slug()),
    }
}

async fn look(
    State(rooms): State<Rooms>,
    Path(code): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = token_of(&headers) else {
        return fail(StatusCode::UNAUTHORIZED, "bad_token");
    };
    // 掃く機会は覗くたびで足りる。待合は1秒ごとに引かれる。
    rooms.sweep(rooms.now_ms());
    match rooms.look(&token, rooms.now_ms()) {
        // **別の部屋のトークンでこの部屋を覗かせない。**古いトークンが
        // 残っていると、入ったつもりのない部屋の様子が返ってしまう。
        Some(lobby) if lobby.code == code => Json::<Lobby>(lobby).into_response(),
        _ => fail(StatusCode::UNAUTHORIZED, "bad_token"),
    }
}

async fn start(State(rooms): State<Rooms>, headers: HeaderMap) -> Response {
    let Some(token) = token_of(&headers) else {
        return fail(StatusCode::UNAUTHORIZED, "bad_token");
    };
    match rooms.start(&token, rooms.now_ms()) {
        Ok(()) => Json(Started { state: "playing" }).into_response(),
        Err(StartError::BadToken) => fail(StatusCode::UNAUTHORIZED, StartError::BadToken.slug()),
        Err(StartError::NotHost) => fail(StatusCode::FORBIDDEN, StartError::NotHost.slug()),
        Err(StartError::AlreadyStarted) => {
            fail(StatusCode::CONFLICT, StartError::AlreadyStarted.slug())
        }
    }
}

/// 部屋の口だけを束ねる。静的配信と WebSocket は呼び手が足す。
pub fn api(rooms: Rooms) -> Router {
    Router::new()
        .route("/api/rooms", post(create))
        .route("/api/rooms/{code}", get(look))
        .route("/api/rooms/{code}/join", post(join))
        .route("/api/rooms/{code}/start", post(start))
        .with_state(rooms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    async fn call(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.clone().oneshot(request).await.expect("応答がある");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("本文が読める")
            .to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    fn post_json(path: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .expect("組み立てられる")
    }

    fn with_token(mut request: Request<Body>, token: &str) -> Request<Body> {
        request
            .headers_mut()
            .insert(TOKEN_HEADER, token.parse().expect("ヘッダに入る"));
        request
    }

    fn get(path: &str) -> Request<Body> {
        Request::builder()
            .uri(path)
            .body(Body::empty())
            .expect("組み立てられる")
    }

    async fn make_room(app: &Router, name: &str) -> (String, String) {
        let (status, body) = call(
            app,
            post_json("/api/rooms", &format!(r#"{{"name":"{name}"}}"#)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        (
            body["code"].as_str().expect("コードがある").to_owned(),
            body["token"].as_str().expect("トークンがある").to_owned(),
        )
    }

    #[tokio::test]
    async fn a_room_comes_back_with_a_code_and_a_token() {
        let app = api(Rooms::new());
        let (code, token) = make_room(&app, "まさ").await;
        assert_eq!(code.chars().count(), 6, "{code}");
        assert_eq!(token.len(), 32);
    }

    /// 名前を送らなくても部屋は作れる。**入口で弾かない。**
    #[tokio::test]
    async fn a_nameless_request_still_makes_a_room() {
        let app = api(Rooms::new());
        let (status, body) = call(&app, post_json("/api/rooms", "{}")).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["code"].is_string());
    }

    #[tokio::test]
    async fn looking_needs_the_token() {
        let app = api(Rooms::new());
        let (code, token) = make_room(&app, "まさ").await;

        let (status, body) = call(&app, get(&format!("/api/rooms/{code}"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "bad_token");

        let (status, body) =
            call(&app, with_token(get(&format!("/api/rooms/{code}")), &token)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["you"]["name"], "まさ");
        assert_eq!(body["you"]["host"], true);
        assert_eq!(body["state"], "waiting");
        assert_eq!(body["can_start"], true);
    }

    /// **別の部屋のトークンでこの部屋を覗けない。**
    #[tokio::test]
    async fn a_token_from_another_room_is_refused() {
        let app = api(Rooms::new());
        let (mine, _) = make_room(&app, "まさ").await;
        let (_, other) = make_room(&app, "よそ").await;

        let (status, body) =
            call(&app, with_token(get(&format!("/api/rooms/{mine}")), &other)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "bad_token");
    }

    #[tokio::test]
    async fn an_unknown_code_is_four_oh_four() {
        let app = api(Rooms::new());
        let (status, body) = call(&app, post_json("/api/rooms/ZZZZZZ/join", "{}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "no_such_room");
    }

    #[tokio::test]
    async fn a_full_room_answers_with_a_name_the_screen_can_branch_on() {
        let app = api(Rooms::new());
        let (code, _) = make_room(&app, "1").await;
        for name in ["2", "3", "4"] {
            let (status, _) = call(
                &app,
                post_json(
                    &format!("/api/rooms/{code}/join"),
                    &format!(r#"{{"name":"{name}"}}"#),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        }
        let (status, body) = call(&app, post_json(&format!("/api/rooms/{code}/join"), "{}")).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "room_full");
    }

    #[tokio::test]
    async fn only_the_host_may_start() {
        let app = api(Rooms::new());
        let (code, host) = make_room(&app, "まさ").await;
        let (_, joined) = call(&app, post_json(&format!("/api/rooms/{code}/join"), "{}")).await;
        let guest = joined["token"].as_str().expect("トークンがある").to_owned();

        let (status, body) = call(
            &app,
            with_token(post_json(&format!("/api/rooms/{code}/start"), ""), &guest),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "not_host");

        let (status, body) = call(
            &app,
            with_token(post_json(&format!("/api/rooms/{code}/start"), ""), &host),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["state"], "playing");
    }

    #[tokio::test]
    async fn a_started_room_shows_as_playing() {
        let app = api(Rooms::new());
        let (code, host) = make_room(&app, "ひとり").await;
        call(
            &app,
            with_token(post_json(&format!("/api/rooms/{code}/start"), ""), &host),
        )
        .await;
        let (_, body) = call(&app, with_token(get(&format!("/api/rooms/{code}")), &host)).await;
        assert_eq!(body["state"], "playing");
        assert_eq!(body["can_start"], false, "始まった卓をまた立てられる");
    }

    /// 名前は入口で整える。**画面の側の作法に頼らない。**
    #[tokio::test]
    async fn a_name_is_tidied_at_the_door() {
        let app = api(Rooms::new());
        let (code, token) = make_room(&app, "  あいうえおかきくけこさしすせそ  ").await;
        let (_, body) = call(&app, with_token(get(&format!("/api/rooms/{code}")), &token)).await;
        assert_eq!(body["you"]["name"], "あいうえおかきくけこさし");
    }
}
