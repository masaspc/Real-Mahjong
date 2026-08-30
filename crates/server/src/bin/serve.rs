//! ブラウザから卓に座るためのサーバ。起動だけを担う。
//!
//! ```text
//! cargo run -p server --bin serve
//! PORT=8081 cargo run -p server --bin serve
//! ```
//!
//! 外から届かせるときは待ち受けるアドレスを渡す。牌譜の置き場所も渡せる。
//!
//! ```text
//! BIND=0.0.0.0 PORT=8080 cargo run -p server --bin serve
//! RECORDS=:memory: cargo run -p server --bin serve   # 残さない
//! ```
//!
//! 部屋と卓の口は `server::http`、台帳は `server::rooms` にある。

use axum::Router;
use server::http::{AppState, Records};
use server::persistence::{Scribe, Store};
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

/// 待ち受ける先を決める。
///
/// **既定は手元だけに閉じる。**`0.0.0.0` を既定にすると、開発中に立てた卓が
/// 同じ網の中の誰からでも触れてしまう。外へ出すのは明示のときだけにする。
///
/// **8080 は取り合いになる。**別のプロジェクトのサーバが先に握っていると
/// 起動で落ちるので、番号も渡せるようにしてある。
fn listen_at(bind: Option<String>, port: Option<String>) -> (String, u16) {
    let host = bind
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_owned());
    let port = port
        .and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or(8080);
    (host, port)
}

/// 牌譜を置く既定の場所。
///
/// **カレントディレクトリを基準にしない。**静的ファイルと同じ理由で、
/// どこから起動しても同じ場所を指す必要がある。倉が起動場所ごとに
/// 分かれると、昨日の牌譜が消えたように見える。
fn default_records_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/records.sqlite")
}

/// 牌譜の置き場所を決める。
fn records_path(given: Option<String>) -> String {
    given
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_records_path().to_string_lossy().into_owned())
}

fn open_store(path: &str) -> rusqlite::Result<Store> {
    if path == ":memory:" {
        return Store::open_in_memory();
    }
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    Store::open(path)
}

/// 牌譜の倉を開く。
///
/// **開けなくても起動する。**牌譜は対局の前提ではない。置き場所が
/// 書けない状況で対局そのものを止める理由が無いので、言うだけ言って進む。
fn open_records(path: &str) -> Option<Store> {
    match open_store(path) {
        Ok(store) => {
            println!("牌譜: {path}");
            Some(store)
        }
        Err(error) => {
            eprintln!("牌譜を開けません（対局は続けられます）: {path}: {error}");
            None
        }
    }
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

    // 牌譜の置き場所。**既定はリポジトリの下の data/**。
    let records_path = records_path(std::env::var("RECORDS").ok());
    let records = open_records(&records_path);
    let rooms = Rooms::with_scribe(
        records
            .as_ref()
            .and_then(|_| open_store(&records_path).ok())
            .map(Scribe::spawn),
    );

    let app = Router::new()
        .merge(server::http::api_with(AppState {
            rooms,
            records: records.map(Records::new),
        }))
        .fallback_service(ServeDir::new(&dist))
        // **画面の束は 1MB 近い。**牌図34枚を文字列として抱えているのが
        // 効いている。中身は SVG とスクリプト、つまり圧縮のよく効く文字列
        // なので、そのまま送る理由が無い。**WebSocket には掛からない**
        // （アップグレード要求は本体を持たない）。
        .layer(CompressionLayer::new());

    let (host, port) = listen_at(std::env::var("BIND").ok(), std::env::var("PORT").ok());
    let listener = match tokio::net::TcpListener::bind((host.as_str(), port)).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("{host}:{port} を開けません: {error}");
            eprintln!("誰が使っているかは `lsof -nP -iTCP:{port} -sTCP:LISTEN` で分かります。");
            eprintln!("別の番号で開くなら `PORT=8081 cargo run -p server --bin serve`。");
            std::process::exit(1);
        }
    };
    println!("http://{host}:{port} で待っています");
    axum::serve(listener, app).await.expect("配信できる");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_records_go_next_to_the_repository_by_default() {
        // **起動場所で分かれてはいけない。**分かれると、昨日の牌譜が
        // 消えたように見える。
        let path = records_path(None);
        assert!(path.ends_with("records.sqlite"), "{path}");
        assert!(path.contains("data"), "{path}");
        assert_eq!(
            records_path(Some("  ".to_owned())),
            path,
            "空白は既定に倒す"
        );
    }

    #[test]
    fn a_given_place_wins() {
        assert_eq!(
            records_path(Some("/tmp/x.sqlite".to_owned())),
            "/tmp/x.sqlite"
        );
        assert_eq!(records_path(Some(":memory:".to_owned())), ":memory:");
    }

    #[test]
    fn the_default_is_closed_to_the_outside() {
        // **既定が 0.0.0.0 になっていたら落とす。**開発中の卓が同じ網の
        // 誰からでも触れる状態は、気付かないまま続きうる。
        assert_eq!(listen_at(None, None), ("127.0.0.1".to_owned(), 8080));
    }

    #[test]
    fn both_can_be_given() {
        assert_eq!(
            listen_at(Some("0.0.0.0".to_owned()), Some("9000".to_owned())),
            ("0.0.0.0".to_owned(), 9000)
        );
    }

    #[test]
    fn nonsense_falls_back_instead_of_dying() {
        // 起動時の環境変数で落とさない。読めなければ既定でいく。
        assert_eq!(
            listen_at(Some("  ".to_owned()), Some("たくさん".to_owned())),
            ("127.0.0.1".to_owned(), 8080)
        );
        assert_eq!(listen_at(None, Some("70000".to_owned())).1, 8080);
    }
}
