//! ブラウザから卓に座るためのサーバ。
//!
//! ```text
//! cargo run -p server --bin serve
//! PORT=8081 cargo run -p server --bin serve
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
use tower_http::compression::CompressionLayer;
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

/// 配信する静的ファイルの場所。
///
/// **カレントディレクトリを基準にしてはいけない。**リポジトリのルート以外から
/// 起動すると、サーバは「待っています」と出したまま全部 404 を返す。
/// 起動したように見えて画面が出ないので、原因に辿り着きにくい。
fn dist_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/web/dist")
}

#[tokio::main]
async fn main() {
    let dist = dist_dir();
    // **黙って 404 を返さない。**ビルドを忘れているのか、置き場所が違うのかを
    // 起動時に言い切る。
    if !dist.join("index.html").is_file() {
        eprintln!("画面が見つかりません: {}", dist.display());
        eprintln!("先に `pnpm --dir apps/web build` を実行してください。");
        std::process::exit(1);
    }

    let tables = Tables::new();
    let app = Router::new()
        .route("/ws", any(ws_handler))
        .fallback_service(ServeDir::new(&dist))
        // **画面の束は 1MB 近い。**牌図34枚を文字列として抱えているのが
        // 効いている。中身は SVG とスクリプト、つまり圧縮のよく効く文字列
        // なので、そのまま送る理由が無い。**WebSocket には掛からない**
        // （アップグレード要求は本体を持たない）。
        .layer(CompressionLayer::new())
        .with_state(tables);

    // **8080 は取り合いになる。**別のプロジェクトのサーバが先に握っていると
    // ここで落ちる。番号を渡せるようにし、塞がっていたら誰が使っているかを
    // 調べる手立てまで言う。
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("{port} を開けません: {error}");
            eprintln!("誰が使っているかは `lsof -nP -iTCP:{port} -sTCP:LISTEN` で分かります。");
            eprintln!("別の番号で開くなら `PORT=8081 cargo run -p server --bin serve`。");
            std::process::exit(1);
        }
    };
    println!("http://127.0.0.1:{port} で待っています");
    axum::serve(listener, app).await.expect("配信できる");
}
