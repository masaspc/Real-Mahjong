//! ブラウザから卓に座るためのサーバ。起動だけを担う。
//!
//! ```text
//! cargo run -p server --bin serve
//! PORT=8081 cargo run -p server --bin serve
//! ```
//!
//! 部屋と卓の口は `server::http`、台帳は `server::rooms` にある。

use axum::Router;
use server::rooms::Rooms;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;

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

    let app = Router::new()
        .merge(server::http::api(Rooms::new()))
        .fallback_service(ServeDir::new(&dist))
        // **画面の束は 1MB 近い。**牌図34枚を文字列として抱えているのが
        // 効いている。中身は SVG とスクリプト、つまり圧縮のよく効く文字列
        // なので、そのまま送る理由が無い。**WebSocket には掛からない**
        // （アップグレード要求は本体を持たない）。
        .layer(CompressionLayer::new());

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
