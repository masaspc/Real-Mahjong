//! 部屋の HTTP の口。
//!
//! **game protocol とは別系統にする。**凍結された `crates/protocol` に
//! 部屋の概念を持ち込まないためであり、また部屋の形はこれから変わる
//! （観戦・段位別マッチング）ので、凍結の外に置くのが正しい。
//!
//! 席の証明は `X-Mahjong-Token` ヘッダで受ける。**クエリ文字列に置かない。**
//! アクセスログや `Referer` に席の証明が残る。

use crate::persistence::{hash_token, Store};
use crate::rooms::{Code, JoinError, Lobby, Rooms, StartError, Token};
use crate::session::TableHandle;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use protocol::command::Command;
use protocol::project::project_envelope;
use protocol::seat::Seat;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 席の証明を運ぶヘッダ。
pub const TOKEN_HEADER: &str = "x-mahjong-token";

/// その browser を指す鍵を運ぶヘッダ。
///
/// **席の証明とは別物である。**証明は1対局にしか効かないので、
/// 「自分の打った半荘」を並べるには対局をまたぐ名札が要る。
pub const PLAYER_HEADER: &str = "x-mahjong-player";

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

fn player_of(headers: &HeaderMap) -> Option<String> {
    let text = headers.get(PLAYER_HEADER)?.to_str().ok()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

async fn create(
    State(rooms): State<Rooms>,
    headers: HeaderMap,
    body: Option<Json<NameBody>>,
) -> Response {
    let name = body.map(|Json(b)| b.name).unwrap_or_default();
    let (Code(code), Token(token)) =
        rooms.create(&name, player_of(&headers).as_deref(), rooms.now_ms());
    Json(Created { code, token }).into_response()
}

async fn join(
    State(rooms): State<Rooms>,
    Path(code): Path<String>,
    headers: HeaderMap,
    body: Option<Json<NameBody>>,
) -> Response {
    let name = body.map(|Json(b)| b.name).unwrap_or_default();
    match rooms.join(
        &Code(code),
        &name,
        player_of(&headers).as_deref(),
        rooms.now_ms(),
    ) {
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

/// 席の証明が通った接続に持たせる切符。
#[derive(Clone)]
struct Ticket {
    handle: TableHandle,
    seat: Seat,
}

/// 卓へ繋ぐ前に席を検める。
///
/// **握手より先に検める。**`WebSocketUpgrade` の抽出器を先に置くと、
/// 席の検査は握手が成り立った後にしか走らない。認証は入口で済ませる
/// のが順序として正しく、試験からも叩けるようになる。
///
/// **ここだけはトークンをクエリで受ける。**ブラウザは WebSocket の
/// 要求にヘッダを付けられないため、他に運ぶ道が無い。
///
/// 席はトークンが決める。**クライアントは席を名乗れない。**卓の id を
/// 知っているだけで座れた頃は、席ごとの視界フィルタが意味を持たなかった。
async fn require_seat(State(rooms): State<Rooms>, mut request: Request, next: Next) -> Response {
    // 終わった卓を捨てる機会は、接続のたびで足りる。
    rooms.sweep(rooms.now_ms());

    let token = request
        .uri()
        .query()
        .map(serde_urlencoded::from_str::<HashMap<String, String>>)
        .and_then(Result::ok)
        .and_then(|params| params.get("token").cloned())
        .map(Token);

    // **卓が立つ前と、知らないトークンを区別しない。**合言葉の総当たりに
    // 手がかりを与えない。
    let Some((handle, seat)) = token.and_then(|token| rooms.seat_of(&token)) else {
        return fail(StatusCode::UNAUTHORIZED, "bad_token");
    };
    request.extensions_mut().insert(Ticket { handle, seat });
    next.run(request).await
}

async fn socket(
    Extension(ticket): Extension<Ticket>,
    Query(params): Query<HashMap<String, String>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let last_seq = params.get("last_seq").and_then(|s| s.parse::<u32>().ok());
    upgrade.on_upgrade(move |ws| play(ws, ticket.handle, ticket.seat, last_seq))
}

async fn play(mut socket: WebSocket, handle: TableHandle, seat: Seat, last_seq: Option<u32>) {
    let Ok((connection, mut inbox)) = handle.attach(seat, last_seq).await else {
        return;
    };

    loop {
        tokio::select! {
            // **順序を固定する。**受け口を先に見る。
            //
            // 同じ席に新しい接続が来ると、卓はこちらの送り口を捨てる。
            // そのとき `inbox.recv()` は `None` を返す。公平に選ぶと、
            // それを知る前に古い画面から遅れて届いた枠を拾ってしまう。
            // **`Discard` は `window_id` を持たないので、いまの要求に
            // 偶然かなってしまい、意図しない牌を捨てる。**
            //
            // 受け口を先に見れば、閉じたことに必ず先に気づいて抜ける。
            // 送るものが無く、かつ開いている間だけ枠を読む。
            biased;

            outgoing = inbox.recv() => {
                let Some(envelope) = outgoing else { break };
                let Ok(text) = serde_json::to_string(&envelope) else { break };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        // **枠を読んだ直後に測る。**後ろで測ると、他席の混雑が
                        // 自分の締切を削る（Wave 3c で決めた契約）。
                        let at_ms = handle.now_ms();
                        let Ok(command) = serde_json::from_str::<Command>(&text) else {
                            continue;
                        };
                        if handle.command(seat, command, at_ms).await.is_err() {
                            break;
                        }
                    }
                    // Close は明示的に抜ける。`detach` の時点がはっきりする。
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => {}
                    _ => break,
                }
            }
        }
    }

    let _ = handle.detach(seat, connection).await;
}

/// この口が持つもの。
#[derive(Clone)]
pub struct AppState {
    pub rooms: Rooms,
    /// 牌譜の倉。無ければ牌譜の口は空を返す。
    pub records: Option<Records>,
}

impl axum::extract::FromRef<AppState> for Rooms {
    fn from_ref(state: &AppState) -> Rooms {
        state.rooms.clone()
    }
}

/// 牌譜を読むときの倉。
///
/// **書き手とは別の口である。**書き手は自分の `Connection` を1本抱えて
/// 順に書く。読みは要求のたびに来るので、別の1本を鍵で守って使う。
/// SQLite は同じファイルを複数の接続から読める。
#[derive(Clone)]
pub struct Records {
    store: Arc<Mutex<Store>>,
}

impl Records {
    pub fn new(store: Store) -> Self {
        Records {
            store: Arc::new(Mutex::new(store)),
        }
    }
}

#[derive(Serialize)]
struct RecordCard {
    id: String,
    players: Vec<String>,
    started_ms: u64,
    ended_ms: Option<u64>,
    /// 終局の点数と順位。まだなら null。
    result: Option<serde_json::Value>,
}

fn card(head: &crate::persistence::MatchHead) -> RecordCard {
    RecordCard {
        id: head.id.clone(),
        players: head.players.clone(),
        started_ms: head.started_ms,
        ended_ms: head.ended_ms,
        result: head
            .result_json
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok()),
    }
}

/// その browser が打った対局。
async fn list_records(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(key) = player_of(&headers) else {
        return fail(StatusCode::UNAUTHORIZED, "bad_player");
    };
    let Some(records) = state.records.as_ref() else {
        return Json(serde_json::json!({ "records": [] })).into_response();
    };
    let store = records.store.lock().expect("毒されていない");
    match store.list(&key, 100) {
        Ok(heads) => {
            let cards: Vec<RecordCard> = heads.iter().map(card).collect();
            Json(serde_json::json!({ "records": cards })).into_response()
        }
        Err(_) => fail(StatusCode::INTERNAL_SERVER_ERROR, "unavailable"),
    }
}

/// その牌譜で自分がどの席だったか。
///
/// **卓が生きているうちは部屋から、畳まれた後は倉から引く。**部屋は
/// 終わった卓を掃くので、倉に残した証明のハッシュが後々の頼りになる。
fn seat_in_record(state: &AppState, id: &str, token: &Token) -> Option<Seat> {
    if let Some((record_id, seat)) = state.rooms.record_of(token) {
        if record_id == id {
            return Some(seat);
        }
    }
    let records = state.records.as_ref()?;
    let store = records.store.lock().expect("毒されていない");
    let seat = store.seat_of(id, &hash_token(&token.0)).ok()??;
    Some(Seat::new(seat))
}

/// 1対局の見出し。
async fn read_record(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = token_of(&headers) else {
        return fail(StatusCode::UNAUTHORIZED, "bad_token");
    };
    // **無い id と、見る資格の無い id を区別しない。**id の総当たりに
    // 手がかりを与えない。
    let Some(seat) = seat_in_record(&state, &id, &token) else {
        return fail(StatusCode::UNAUTHORIZED, "bad_token");
    };
    let Some(records) = state.records.as_ref() else {
        return fail(StatusCode::UNAUTHORIZED, "bad_token");
    };
    let store = records.store.lock().expect("毒されていない");
    let Ok(Some(head)) = store.head(&id) else {
        return fail(StatusCode::UNAUTHORIZED, "bad_token");
    };
    let mut body = serde_json::to_value(card(&head)).unwrap_or_default();
    if let Some(object) = body.as_object_mut() {
        object.insert("you".to_owned(), serde_json::json!(seat.index()));
    }
    Json(body).into_response()
}

/// 保存した真実を、その席の視界に射影して返す。
///
/// **生の配信と同じ `project_envelope` を通る。**牌譜のために別の絞り方を
/// 書くと、そこだけ抜けが生まれる。
async fn record_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = token_of(&headers) else {
        return fail(StatusCode::UNAUTHORIZED, "bad_token");
    };
    let Some(seat) = seat_in_record(&state, &id, &token) else {
        return fail(StatusCode::UNAUTHORIZED, "bad_token");
    };
    let Some(records) = state.records.as_ref() else {
        return fail(StatusCode::UNAUTHORIZED, "bad_token");
    };
    let store = records.store.lock().expect("毒されていない");
    let Ok(truth) = store.events(&id) else {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, "unavailable");
    };
    let lines: Vec<String> = truth
        .iter()
        .filter_map(|envelope| project_envelope(envelope, seat))
        .filter_map(|projected| serde_json::to_string(&projected).ok())
        .collect();
    (
        [(axum::http::header::CONTENT_TYPE, "application/x-ndjson")],
        lines.join("\n"),
    )
        .into_response()
}

/// 部屋の口と卓への接続を束ねる。静的配信は呼び手が足す。
pub fn api(rooms: Rooms) -> Router {
    api_with(AppState {
        rooms,
        records: None,
    })
}

/// 部屋の口と卓への接続と牌譜を束ねる。静的配信は呼び手が足す。
pub fn api_with(state: AppState) -> Router {
    let rooms = state.rooms.clone();
    Router::new()
        .route("/api/rooms", post(create))
        .route("/api/rooms/{code}", get(look))
        .route("/api/rooms/{code}/join", post(join))
        .route("/api/rooms/{code}/start", post(start))
        .route("/api/records", get(list_records))
        .route("/api/records/{id}", get(read_record))
        .route("/api/records/{id}/events", get(record_events))
        .route(
            "/ws",
            any(socket).layer(from_fn_with_state(rooms, require_seat)),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    pub(super) async fn call(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
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

    pub(super) fn post_json(path: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .expect("組み立てられる")
    }

    pub(super) fn with_token(mut request: Request<Body>, token: &str) -> Request<Body> {
        request
            .headers_mut()
            .insert(TOKEN_HEADER, token.parse().expect("ヘッダに入る"));
        request
    }

    pub(super) fn get(path: &str) -> Request<Body> {
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

    /// **トークンの無い接続は張らせない。**卓の id を知っていれば座れた
    /// 頃は、席ごとの視界フィルタが意味を持たなかった。
    #[tokio::test]
    async fn a_socket_without_a_token_is_refused() {
        let app = api(Rooms::new());
        for uri in ["/ws", "/ws?token=dead", "/ws?table=default"] {
            let (status, body) = call(&app, get(uri)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri} が通った");
            assert_eq!(body["error"], "bad_token", "{uri}");
        }
    }

    /// **待合のうちは繋がせない。**知らないトークンと同じ答えにするのは、
    /// 合言葉の総当たりに手がかりを与えないため。
    #[tokio::test]
    async fn a_socket_before_the_start_is_refused_the_same_way() {
        let app = api(Rooms::new());
        let (_, token) = make_room(&app, "まさ").await;
        let (status, body) = call(&app, get(&format!("/ws?token={token}"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "bad_token");
    }

    /// **拒む試験だけでは、全部拒んでいても通ってしまう。**正しい
    /// トークンが席の検査を抜けることを確かめる。
    ///
    /// 抜けた先は `WebSocketUpgrade` で、この要求は握手の形をしていない
    /// ので 400 で断られる。**401 でないこと**が、席の検査を通った証拠。
    #[tokio::test]
    async fn a_real_token_gets_past_the_gate() {
        let app = api(Rooms::new());
        let (code, token) = make_room(&app, "まさ").await;
        call(
            &app,
            with_token(post_json(&format!("/api/rooms/{code}/start"), ""), &token),
        )
        .await;

        let (status, _) = call(&app, get(&format!("/ws?token={token}"))).await;
        assert_ne!(status, StatusCode::UNAUTHORIZED, "正しい席が拒まれている");
        assert_eq!(status, StatusCode::BAD_REQUEST, "握手の手前まで来ていない");
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

#[cfg(test)]
mod record_tests {
    use super::tests::{call, get, post_json, with_token};
    use super::*;
    use crate::persistence::{Scribe, Store};
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use protocol::client_event::ClientEvent;
    use serde_json::Value;
    use tower::ServiceExt;

    fn with_player(mut request: Request<Body>, key: &str) -> Request<Body> {
        request
            .headers_mut()
            .insert(PLAYER_HEADER, key.parse().expect("ヘッダに入る"));
        request
    }

    /// 倉を抱えた口。読みと書きで別の接続を開く。
    fn app_with_store(tag: &str) -> (Router, crate::persistence::TempDb, Rooms) {
        let db = crate::persistence::TempDb::new(&format!("http-{tag}"));
        let writer = Store::open(&db.path).expect("開ける");
        let reader = Store::open(&db.path).expect("開ける");
        let rooms = Rooms::with_scribe(Some(Scribe::spawn(writer)));
        let state = AppState {
            rooms: rooms.clone(),
            records: Some(Records::new(reader)),
        };
        (api_with(state), db, rooms)
    }

    /// 部屋を作って開始し、牌譜の見出しが書かれるまで待つ。
    async fn played(app: &Router, name: &str, key: &str) -> (String, String) {
        let (status, body) = call(
            app,
            with_player(
                post_json("/api/rooms", &format!(r#"{{"name":"{name}"}}"#)),
                key,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let code = body["code"].as_str().expect("ある").to_owned();
        let token = body["token"].as_str().expect("ある").to_owned();
        call(
            app,
            with_token(post_json(&format!("/api/rooms/{code}/start"), ""), &token),
        )
        .await;
        // **書き手は別の task である。**見出しが届くまで順番を回す。
        for _ in 0..2_000 {
            if !my_records(app, key).await.is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
        (code, token)
    }

    /// 牌譜の本文を引く。
    async fn events_of(app: &Router, id: &str, token: &str) -> Vec<ClientEvent> {
        let response = app
            .clone()
            .oneshot(with_token(get(&format!("/api/records/{id}/events")), token))
            .await
            .expect("応答がある");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("読める")
            .to_bytes();
        String::from_utf8_lossy(&bytes)
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|value| serde_json::from_value(value["event"].clone()).ok())
            .collect()
    }

    /// 他席のツモを何件見られるか。**射影が効いているかを測る物差し。**
    fn foreign_draws(events: &[ClientEvent]) -> usize {
        let Some(you) = events.iter().find_map(|event| match event {
            ClientEvent::MatchStart { you, .. } => Some(*you),
            _ => None,
        }) else {
            return 0;
        };
        events
            .iter()
            .filter(|event| matches!(event, ClientEvent::Draw { seat, .. } if *seat != you))
            .count()
    }

    /// 1局が終わって牌譜に落ちるまで待つ。
    ///
    /// **時計だけを頼りにしない。**止めた時計を `sleep` で進める書き方は、
    /// 卓と書き手のどちらが先に動くかで結果が変わり、同じ試験が通ったり
    /// 落ちたりする（実際にそうなった）。卓を直に見張って、局が終わった
    /// ことを確かめてから読む。
    async fn wait_for_a_finished_round(rooms: &Rooms, token: &Token) -> Seat {
        let (handle, seat) = rooms.seat_of(token).expect("席がある");
        let (_, mut watcher) = handle.attach(seat, None).await.expect("卓は生きている");
        while let Some(envelope) = watcher.recv().await {
            if matches!(envelope.event, ClientEvent::RoundEnd { .. }) {
                break;
            }
        }
        // 書き手は別の task なので、吐き出す順番を回す。
        for _ in 0..500 {
            tokio::task::yield_now().await;
        }
        seat
    }

    async fn my_records(app: &Router, key: &str) -> Vec<Value> {
        let (_, body) = call(app, with_player(get("/api/records"), key)).await;
        body["records"].as_array().cloned().unwrap_or_default()
    }

    #[tokio::test(start_paused = true)]
    async fn a_finished_room_shows_up_in_my_list() {
        let (app, _db, _) = app_with_store("list");
        played(&app, "まさ", "key-a").await;

        let mine = my_records(&app, "key-a").await;
        assert_eq!(mine.len(), 1, "一覧に出ていない");
        assert_eq!(mine[0]["players"].as_array().expect("ある").len(), 4);

        // **他人の鍵では出ない。**
        assert!(
            my_records(&app, "key-b").await.is_empty(),
            "他人の牌譜が見えている"
        );
    }

    #[tokio::test]
    async fn listing_needs_a_player_key() {
        let (app, _db, _) = app_with_store("nokey");
        let (status, body) = call(&app, get("/api/records")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "bad_player");
    }

    #[tokio::test(start_paused = true)]
    async fn reading_a_record_needs_the_seat_token() {
        let (app, _db, _) = app_with_store("auth");
        let (_, token) = played(&app, "まさ", "key-a").await;
        let id = my_records(&app, "key-a").await[0]["id"]
            .as_str()
            .expect("ある")
            .to_owned();

        let (status, _) = call(&app, with_token(get(&format!("/api/records/{id}")), &token)).await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = call(&app, get(&format!("/api/records/{id}"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "bad_token");
    }

    /// **無い id と、見る資格の無い id を同じ答えにする。**
    /// 違う答えを返すと、id の総当たりに手がかりを与える。
    #[tokio::test(start_paused = true)]
    async fn a_missing_id_looks_like_a_forbidden_one() {
        let (app, _db, _) = app_with_store("same");
        let (_, mine) = played(&app, "まさ", "key-a").await;
        let (_, theirs) = played(&app, "たろう", "key-b").await;
        let id = my_records(&app, "key-a").await[0]["id"]
            .as_str()
            .expect("ある")
            .to_owned();

        // 他人の牌譜を、自分の証明で覗く。
        let (forbidden, body_a) = call(
            &app,
            with_token(get(&format!("/api/records/{id}")), &theirs),
        )
        .await;
        // まったく無い id。
        let (missing, body_b) = call(
            &app,
            with_token(
                get("/api/records/0000000000000000000000000000dead"),
                &theirs,
            ),
        )
        .await;

        assert_eq!(forbidden, missing, "無い id と資格の無い id で答えが違う");
        assert_eq!(body_a["error"], body_b["error"]);
        assert_eq!(forbidden, StatusCode::UNAUTHORIZED);
        drop(mine);
    }

    /// **牌譜は自分の席の視界で返る。**他家の手牌は入っていない。
    ///
    /// 読み出しは生の配信と同じ `project_envelope` を通る。牌譜のために
    /// 別の絞り方を書くと、そこだけ抜けが生まれる。
    #[tokio::test(start_paused = true)]
    async fn the_record_comes_back_through_the_same_filter() {
        let (app, _db, rooms) = app_with_store("view");

        // **2人で入り、席0でない方を見る。**1人だと4分の1の確率で席0に
        // 当たり、「いつも席0の視界で返す」誤りを見逃す回が出る。
        let (status, body) = call(
            &app,
            with_player(post_json("/api/rooms", r#"{"name":"まさ"}"#), "key-a"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let code = body["code"].as_str().expect("ある").to_owned();
        let host = body["token"].as_str().expect("ある").to_owned();
        let (_, body) = call(
            &app,
            with_player(
                post_json(&format!("/api/rooms/{code}/join"), r#"{"name":"たろう"}"#),
                "key-b",
            ),
        )
        .await;
        let guest = body["token"].as_str().expect("ある").to_owned();
        call(
            &app,
            with_token(post_json(&format!("/api/rooms/{code}/start"), ""), &host),
        )
        .await;

        // 席0でない方を選ぶ。
        let (token, key) = [(host.as_str(), "key-a"), (guest.as_str(), "key-b")]
            .into_iter()
            .find(|(token, _)| {
                rooms
                    .seat_of(&Token((*token).to_owned()))
                    .is_some_and(|(_, seat)| seat.index() != 0)
            })
            .expect("2人のどちらかは席0でない");

        let mut live_seat = Seat::new(0);
        let mut events = Vec::new();
        let mut id = String::new();
        // **他席のツモが十分に溜まるまで局を重ねる。**1局で終わる和了だと
        // 数件しか出ず、「漏れなし」の確認が空回りに近づく。実際に 30 回に
        // 1 回、3 件で落ちた。
        for _ in 0..6 {
            live_seat = wait_for_a_finished_round(&rooms, &Token(token.to_owned())).await;
            id = my_records(&app, key).await[0]["id"]
                .as_str()
                .expect("ある")
                .to_owned();
            events = events_of(&app, &id, token).await;
            if foreign_draws(&events) >= 8 {
                break;
            }
        }
        assert_ne!(live_seat.index(), 0, "席0を選んでしまっている");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ClientEvent::RoundEnd { .. })),
            "1局も残っていない（{} 件）",
            events.len()
        );

        // **対局中に着いていた席と突き合わせる。**射影後の `you` だけを
        // 基準にすると、どの席で射影しても辻褄が合ってしまい、席を取り
        // 違えたことに気付けない。
        let you = events
            .iter()
            .find_map(|event| match event {
                ClientEvent::MatchStart { you, .. } => Some(*you),
                _ => None,
            })
            .expect("MatchStart が無い");
        assert_eq!(you, live_seat, "牌譜が別の席の視界で返っている");

        // 見出しの `you` も同じ席を指す。
        let (_, head) = call(&app, with_token(get(&format!("/api/records/{id}")), token)).await;
        assert_eq!(
            head["you"].as_u64(),
            Some(live_seat.index() as u64),
            "見出しの席が違う"
        );

        // 配牌が自分のものであること。
        let dealt = events.iter().find_map(|event| match event {
            ClientEvent::Deal { your_hand, .. } => Some(your_hand.len()),
            _ => None,
        });
        assert_eq!(dealt, Some(13), "自分の配牌が入っていない");

        // **他席のツモに牌が乗っていない。**数えないと空回りに気付けない。
        for event in &events {
            if let ClientEvent::Draw { seat, tile, .. } = event {
                if *seat != you {
                    assert!(tile.is_none(), "牌譜に他席のツモ牌が入っている");
                }
            }
        }
        let foreign = foreign_draws(&events);
        assert!(foreign >= 8, "他席のツモを {foreign} 件しか見ていない");
    }

    /// 倉を持たない口でも、一覧は空を返すだけで落ちない。
    #[tokio::test]
    async fn a_storeless_api_answers_with_nothing() {
        let app = api(Rooms::new());
        let (status, body) = call(&app, with_player(get("/api/records"), "key-a")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["records"].as_array().expect("ある").len(), 0);
    }
}
