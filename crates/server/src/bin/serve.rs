//! ブラウザから卓に座るためのサーバ。
//!
//! ```text
//! cargo run -p server --bin serve
//! ```
//!
//! **認証は無い。**卓の id を知っていれば誰でもその席に座れる。
//! ローカルで遊んで評価するための段階であり、公開するものではない。

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::any;
use axum::Router;
use protocol::command::Command;
use protocol::seat::Seat;
use server::matchmaking::{TableId, Tables};
use std::collections::HashMap;
use tower_http::services::ServeDir;

/// 人が座る席。**このウェーブでは固定。**
const YOU: u8 = 0;

async fn ws_handler(
    upgrade: WebSocketUpgrade,
    State(tables): State<Tables>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    // 終わった卓を捨てる機会は、接続のたびで足りる。
    tables.sweep();

    let id = TableId(
        params
            .get("table")
            .cloned()
            .unwrap_or_else(|| "default".to_owned()),
    );
    let last_seq = params.get("last_seq").and_then(|s| s.parse::<u32>().ok());
    upgrade.on_upgrade(move |socket| play(socket, tables, id, last_seq))
}

async fn play(mut socket: WebSocket, tables: Tables, id: TableId, last_seq: Option<u32>) {
    let handle = tables.get_or_create(&id);
    let seat = Seat::new(YOU);
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

#[tokio::main]
async fn main() {
    let tables = Tables::new();
    let app = Router::new()
        .route("/ws", any(ws_handler))
        .fallback_service(ServeDir::new("apps/web/dist"))
        .with_state(tables);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("8080 を開ける");
    println!("http://127.0.0.1:8080 で待っています");
    println!("（先に `pnpm --dir apps/web build` を実行しておくこと）");
    axum::serve(listener, app).await.expect("配信できる");
}
