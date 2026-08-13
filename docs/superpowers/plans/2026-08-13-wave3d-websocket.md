# Wave 3d WebSocket と axum 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wave 3c の卓 Actor をブラウザから触れるようにする。**このウェーブの終わりに、人が1人 CPU 3人と半荘を打てるサーバが立つ。**

**Architecture:** 1接続 = 1席。`ClientEventEnvelope` を JSON で下りへ、`Command` を JSON で上りへ流すだけの薄い層。どちらの型も `protocol` に既にあるので、**セッション用のメッセージ型を新たに作らない。**卓の指定と再開位置は接続時のクエリ文字列で渡す。

**Tech Stack:** Rust / axum 0.8.9 / tokio 1.53.1 / tower-http 0.7.0 / serde_json

---

## Global Constraints

- **`crates/protocol/`・`crates/mahjong-core/`・`crates/mahjong-engine/`・`crates/mahjong-ai/`・`crates/server/src/table.rs`・`crates/server/src/session.rs`・`crates/server/src/session_time.rs`・`crates/server/src/lib.rs` を変更しない。**Wave 3c までで凍結済み。
- 新しいモジュール宣言を `lib.rs` へ足さない。`matchmaking.rs` は既に宣言されている。WS の層は `session.rs` からではなく**独立したファイル**に置き、`main.rs` から使う（下の File Structure を見ること）。
- 日本語のコメントとテスト名を使う。
- `cargo clippy --all-targets -- -D warnings` と `cargo fmt --check` を通す。
- テストは仕様である。**期待値を実装に合わせて書き換えてはならない。**
- **このウェーブに認証は無い。**卓の id を知っていれば誰でもその席に座れる。ローカルで遊んで評価するための段階であり、公開するものではない。この制限は README とコードのコメントに書く。

**この計画の中核は、実際に動かして確かめてある。**`axum 0.8.9` の WebSocket で Wave 3c の `TableHandle` を包み、ブラウザから接続したところ、次の往復が成立した。

```
0 match_start → 1 round_start → 2 deal（13枚）→ 3 draw
→ 4 request_action（options=[discard...], deadline_ms=25749）
→ クライアントから {"type":"discard","tile":3,"riichi":false} を送信
→ 5 discard → 7 action_passed → 8 draw
```

---

## File Structure

| ファイル | 責務 |
|---|---|
| `crates/server/Cargo.toml` | axum・tower-http・serde_json の追加、`[[bin]]` の宣言 |
| `crates/server/src/matchmaking.rs` | 卓の台帳（id から `TableHandle` を引く）。既に `lib.rs` が宣言している |
| `crates/server/src/bin/serve.rs` | 実行可能なサーバ。ルータ・WS ハンドラ・静的配信 |

**`session.rs` を触らない。**Wave 3c で凍結した。WS の層は `bin/serve.rs` に置く。`src/bin/` は Cargo が自動で拾うので `lib.rs` にも `Cargo.toml` にも宣言が要らない。

---

## 設計上の決めごと（読まずに実装しないこと）

### セッション用のメッセージ型を作らない

下りは `ClientEventEnvelope`、上りは `Command`。どちらも `protocol` にあり、`Serialize` / `Deserialize` と TypeScript 型の生成が既に付いている。**ここで新しい封筒型を作ると、クライアント側の型が手書きになり、いずれ実体とずれる。**

「どの卓か」「どこから再送するか」は接続時のクエリ文字列で渡す。

```
ws://127.0.0.1:8080/ws?table=<id>&last_seq=<n>
```

こうすると、繋がったあとに流れるのはイベントとコマンドだけになり、**枠の種類が増えない。**

### 卓の id はクライアントが作る

サーバが id を発行すると、それを伝えるための往復が別に要る。クライアントが乱数で作って `localStorage` に置き、サーバは**知らない id を見たらその卓を作る。**同じ id で繋ぎ直せば同じ卓に戻る。

**これは認証ではない。**id を知っていれば誰でも座れる。ローカル評価用と割り切る。公開するときは Wave 3f 以降で署名付きの再開トークンに置き換える（仕様 8.1）。

### 人は席0に座る

このウェーブでは席0が人、1〜3が CPU で固定する。4人打ちの席替えとマッチングは後のウェーブの仕事である。

### 台帳は終わった卓を捨てる

`TableHandle::is_closed()` が真になった卓は台帳から外す。**外さないと、遊ぶたびに卓が積み上がる。**接続のたびに掃除する。

### 置き換えられた接続からコマンドを通さない

同じ席に新しい接続が来ると、卓はこちらの送り口を捨てる。ところが**古いハンドラのループはまだ生きている。**`select!` が公平に選ぶと、受け口が閉じたことを知る前に、古い画面から遅れて届いた枠を読んでしまう。

**`Discard` は `window_id` を持たない。**古い画面の打牌が、いまの打牌要求に偶然かなってしまい、意図しない牌が切られる。

そこで `biased;` を置き、**受け口を先に見る。**送り口が捨てられていれば `inbox.recv()` が `None` を返すので、必ず先に気づいて抜けられる。枠を読むのは「送るものが無く、かつ受け口が開いている」ときだけになる。

### 時刻は枠を読んだ直後に測る

`handle.now_ms()` を**`recv()` が返った直後**に呼び、それを `command` へ渡す。Wave 3c で決めた契約であり、ここが唯一の到着点である。後ろで測ると、他席の混雑が自分の締切を削る。

---

### Task 1: 卓の台帳

**Files:**
- Modify: `crates/server/Cargo.toml`
- Modify: `crates/server/src/matchmaking.rs`

**Interfaces:**
- Consumes: `crate::session::{spawn, SeedSource, TableHandle}`、`crate::table::Occupant`、`protocol::event::PlayerId`、`protocol::ruleset::{MatchLength, Ruleset}`
- Produces:
  - `pub struct TableId(pub String)`
  - `pub struct Tables`（`Clone`）、`Tables::new()`、`Tables::get_or_create(&self, id: &TableId) -> TableHandle`、`Tables::sweep(&self) -> usize`、`Tables::len(&self) -> usize`

- [ ] **Step 1: 依存を足す**

`crates/server/Cargo.toml` を次の内容にする。

```toml
[package]
name = "server"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
protocol = { path = "../protocol" }
mahjong-engine = { path = "../mahjong-engine" }
mahjong-ai = { path = "../mahjong-ai" }
tokio = { version = "1.53.1", features = ["rt", "rt-multi-thread", "macros", "net", "sync", "time"] }
rand = "0.9.5"
axum = { version = "0.8.9", features = ["ws"] }
tower-http = { version = "0.7.0", features = ["fs"] }
serde_json = "1.0.151"

[dev-dependencies]
tokio = { version = "1.53.1", features = ["rt", "rt-multi-thread", "macros", "net", "sync", "time", "test-util"] }

[lints]
workspace = true
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/server/src/matchmaking.rs` を次の内容にする（テストだけ）。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn the_same_id_gives_the_same_table() {
        let tables = Tables::new();
        let id = TableId("abc".to_owned());
        let first = tables.get_or_create(&id);
        let second = tables.get_or_create(&id);
        assert!(!first.is_closed());
        assert!(!second.is_closed());
        assert_eq!(tables.len(), 1, "同じ id で2卓できている");
    }

    #[tokio::test(start_paused = true)]
    async fn different_ids_give_different_tables() {
        let tables = Tables::new();
        let _ = tables.get_or_create(&TableId("a".to_owned()));
        let _ = tables.get_or_create(&TableId("b".to_owned()));
        assert_eq!(tables.len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_table_seats_one_human_and_three_cpus() {
        use protocol::client_event::ClientEvent;
        use protocol::seat::Seat;

        let tables = Tables::new();
        let handle = tables.get_or_create(&TableId("x".to_owned()));
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let mut hand = None;
        while let Ok(envelope) = inbox.try_recv() {
            if let ClientEvent::Deal { your_hand, .. } = &envelope.event {
                hand = Some(your_hand.len());
            }
        }
        assert_eq!(hand, Some(13), "席0へ配牌が届いていない");
    }

    /// **終わった卓を捨てないと、遊ぶたびに積み上がる。**
    #[tokio::test(start_paused = true)]
    async fn a_finished_table_is_swept_away() {
        use protocol::seat::Seat;

        let tables = Tables::new();
        let handle = tables.get_or_create(&TableId("done".to_owned()));
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        // 席0も CPU ではないが、誰も打たなくても持ち時間が尽きて進む。
        while watcher.recv().await.is_some() {}
        for _ in 0..100 {
            if handle.is_closed() {
                break;
            }
            tokio::task::yield_now().await;
        }

        assert_eq!(tables.sweep(), 1, "終わった卓が捨てられていない");
        assert_eq!(tables.len(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn sweeping_keeps_the_living() {
        let tables = Tables::new();
        let _ = tables.get_or_create(&TableId("alive".to_owned()));
        assert_eq!(tables.sweep(), 0);
        assert_eq!(tables.len(), 1);
    }
}
```

- [ ] **Step 3: 落ちることを確かめる**

Run: `cargo test --package server matchmaking`
Expected: コンパイルエラー。`Tables` と `TableId` が無い。

- [ ] **Step 4: 実装する**

テストの**上**に置く。

```rust
//! 卓の台帳。id から卓を引き、無ければ作る。
//!
//! **このウェーブに認証は無い。**id を知っていれば誰でもその席に座れる。
//! ローカルで遊んで評価するための段階であり、公開するものではない。
//! 署名付きの再開トークンは後のウェーブで入れる（仕様 8.1）。

use crate::session::{spawn, SeedSource, TableHandle};
use crate::table::Occupant;
use protocol::event::PlayerId;
use protocol::ruleset::{MatchLength, Ruleset};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 卓を指す文字列。クライアントが作って `localStorage` に置く。
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct TableId(pub String);

/// 動いている卓の台帳。
#[derive(Clone)]
pub struct Tables {
    inner: Arc<Mutex<HashMap<TableId, TableHandle>>>,
}

impl Default for Tables {
    fn default() -> Self {
        Tables::new()
    }
}

impl Tables {
    pub fn new() -> Self {
        Tables {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// その id の卓を返す。無ければ作る。
    ///
    /// **席0が人、1〜3が CPU。**席替えとマッチングは後のウェーブ。
    pub fn get_or_create(&self, id: &TableId) -> TableHandle {
        let mut map = self.inner.lock().expect("毒されていない");
        if let Some(handle) = map.get(id) {
            if !handle.is_closed() {
                return handle.clone();
            }
        }
        let (handle, _actor) = spawn(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            [
                Occupant::Human(PlayerId("you".to_owned())),
                Occupant::Cpu(PlayerId("cpu1".to_owned())),
                Occupant::Cpu(PlayerId("cpu2".to_owned())),
                Occupant::Cpu(PlayerId("cpu3".to_owned())),
            ],
            SeedSource::from_os(),
        );
        map.insert(id.clone(), handle.clone());
        handle
    }

    /// 終わった卓を捨てる。捨てた数を返す。
    pub fn sweep(&self) -> usize {
        let mut map = self.inner.lock().expect("毒されていない");
        let before = map.len();
        map.retain(|_, handle| !handle.is_closed());
        before - map.len()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("毒されていない").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
```

**`_actor`（`JoinHandle`）をここでは捨てている。**Wave 3c の Self-Review で「卓の台帳が持つ」と決めたが、持たせるには終了理由を扱う設計が要る。**このウェーブでは遊べるところまでを優先し、持たない。**そのぶん、卓が panic しても記録は残らない。この妥協はここに書いておく。

- [ ] **Step 5: 通ることを確かめる**

Run: `cargo test --package server matchmaking`
Expected: 5 passed

- [ ] **Step 6: crate 全体を測る**

Run: `cargo test --package server`
Expected: 71 passed

- [ ] **Step 7: 検査してコミット**

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt
git add crates/server/Cargo.toml crates/server/src/matchmaking.rs Cargo.lock
git commit -m "feat(server): 卓の台帳

id から卓を引き、無ければ作る。終わった卓は捨てる。**捨てないと
遊ぶたびに積み上がる。**

このウェーブに認証は無い。id を知っていれば誰でも座れる。ローカルで
遊んで評価するための段階であり、公開するものではない。"
```

---

### Task 2: WebSocket のハンドラ

**Files:**
- Create: `crates/server/src/bin/serve.rs`

**Interfaces:**
- Consumes: Task 1 の `Tables` / `TableId`。`axum::extract::ws::{Message, WebSocket, WebSocketUpgrade}`。`protocol::command::Command`、`protocol::seat::Seat`。
- Produces: 実行可能なサーバ。`cargo run -p server --bin serve` で立ち上がる。

- [ ] **Step 1: ハンドラを書く**

`crates/server/src/bin/serve.rs` を作る。

```rust
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
```

- [ ] **Step 2: 立ち上がることを確かめる**

```bash
cargo build -p server --bin serve
cargo run -p server --bin serve &
sleep 3
curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8080/ws
kill %1
```
Expected: ビルドが通り、`/ws` が 400 か 426 を返す（WebSocket でない普通の GET なので、これが正しい）。

- [ ] **Step 3: 往復を確かめる**

`/tmp/ws_smoke.mjs` を作って実行する。**これは手元での確認であり、リポジトリへは入れない。**

```javascript
const ws = new WebSocket("ws://127.0.0.1:8080/ws?table=smoke");
let request = null;
let seen = 0;
ws.onmessage = (e) => {
  const envelope = JSON.parse(e.data);
  seen += 1;
  if (envelope.event.type === "request_action") request = envelope.event;
};
setTimeout(() => {
  console.log("受信", seen, "件");
  const discard = request?.options.find((o) => o.type === "discard");
  console.log("打てる牌", discard?.allowed.length);
  ws.send(JSON.stringify({ type: "discard", tile: discard.allowed[0], riichi: false }));
  setTimeout(() => {
    console.log("送信後", seen, "件");
    process.exit(seen > 5 ? 0 : 1);
  }, 2000);
}, 2500);
```

```bash
cargo run -p server --bin serve &
sleep 3
node /tmp/ws_smoke.mjs
kill %1
```
Expected: 受信5件・打てる牌13枚・送信後10件前後。**打牌が通って局が進むこと。**

- [ ] **Step 4: 検査してコミット**

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt
git add crates/server/src/bin/serve.rs
git commit -m "feat(server): ブラウザから卓に座るサーバ

1接続 = 1席。下りは ClientEventEnvelope、上りは Command を JSON で流す
だけの薄い層。**セッション用のメッセージ型を作らない。**どちらも
protocol に既にあり、TypeScript 型の生成も付いているので、新しい封筒を
作るとクライアント側が手書きになっていずれ実体とずれる。

どの卓か・どこから再送するかは接続時のクエリ文字列で渡す。繋がった
あとに流れるのはイベントとコマンドだけになり、枠の種類が増えない。

到着時刻は枠を読んだ直後に測る。後ろで測ると他席の混雑が自分の締切を
削る（Wave 3c で決めた契約）。"
```

---

### Task 3: 再接続が効くことを確かめる

**Files:**
- Modify: `crates/server/src/matchmaking.rs`（テストの追加のみ）

**Interfaces:**
- Consumes: Task 1・2 のすべて。
- Produces: 新しい公開 API は無い。

- [ ] **Step 1: 失敗するテストを書く**

`matchmaking.rs` のテストモジュールへ足す。

```rust
    /// **置き換えられた接続の受け口は閉じる。**
    ///
    /// WS のループで「受け口を先に見る」修正は、この性質に乗っている。
    /// ここが崩れると、古い画面から遅れて届いた打牌が通ってしまう。
    #[tokio::test(start_paused = true)]
    async fn a_superseded_connection_sees_its_inbox_close() {
        use protocol::seat::Seat;
        use tokio::sync::mpsc::error::TryRecvError;

        let tables = Tables::new();
        let handle = tables.get_or_create(&TableId("supersede".to_owned()));
        let (_, mut old) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        while old.try_recv().is_ok() {}

        // 同じ席へ2本目を繋ぐ。卓は1本目の送り口を捨てる。
        let (_, _new) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let closed = loop {
            match old.try_recv() {
                Ok(_) => {}
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };
        assert!(closed, "置き換えられた受け口が閉じていない");
    }

    /// **繋ぎ直しても同じ卓に戻る。**ブラウザを再読み込みしても
    /// 対局が最初からにならないことが、評価のしやすさに直結する。
    #[tokio::test(start_paused = true)]
    async fn the_same_id_resumes_the_same_match() {
        use protocol::client_event::ClientEvent;
        use protocol::seat::Seat;

        let tables = Tables::new();
        let id = TableId("resume".to_owned());

        let first = tables.get_or_create(&id);
        let (connection, mut inbox) = first
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let mut seen = Vec::new();
        while let Ok(envelope) = inbox.try_recv() {
            seen.push(envelope.seq);
        }
        let commitment = seen.len();
        assert!(commitment > 0, "何も届いていない");
        let last = *seen.last().expect("何か届いている");
        first
            .detach(Seat::new(0), connection)
            .await
            .expect("卓は生きている");
        drop(inbox);

        // 卓は動き続ける。
        tokio::time::sleep(std::time::Duration::from_millis(30_000)).await;

        let again = tables.get_or_create(&id);
        let (_, mut back) = again
            .attach(Seat::new(0), Some(last))
            .await
            .expect("卓は生きている");
        let mut caught_up = Vec::new();
        while let Ok(envelope) = back.try_recv() {
            caught_up.push(envelope.seq);
        }

        assert_eq!(tables.len(), 1, "繋ぎ直しで別の卓ができている");
        assert!(!caught_up.is_empty(), "留守中の分が追いついていない");
        assert!(
            caught_up.iter().all(|seq| *seq > last),
            "見たものがまた来ている"
        );
        // 対局の頭からやり直していないこと。
        assert!(
            !caught_up.contains(&0),
            "繋ぎ直しで対局が最初からになっている"
        );

        let mut restarted = false;
        let (_, mut fresh) = again
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        while let Ok(envelope) = fresh.try_recv() {
            if matches!(envelope.event, ClientEvent::MatchStart { .. }) {
                restarted = true;
            }
        }
        assert!(restarted, "last_seq なしなら最初から送り直すはず");
    }
```

- [ ] **Step 2: 通ることを確かめる**

Run: `cargo test --package server matchmaking::tests::a_superseded_connection_sees_its_inbox_close matchmaking::tests::the_same_id_resumes_the_same_match`
Expected: 2 passed

そのうえでモジュール全体を測る。

Run: `cargo test --package server matchmaking`
Expected: 7 passed

- [ ] **Step 3: workspace 全体を測る**

```bash
cargo test --package server     # 73 件（既存 66 + matchmaking 7）
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
Expected: 失敗ゼロ

- [ ] **Step 4: 凍結を破っていないか自分で確かめる**

```bash
git diff --stat main -- crates/protocol crates/mahjong-core crates/mahjong-engine crates/mahjong-ai
git diff --stat main -- crates/server/src/table.rs crates/server/src/session.rs crates/server/src/session_time.rs crates/server/src/lib.rs
```
Expected: どちらも空

- [ ] **Step 5: コミット**

```bash
git add crates/server/src/matchmaking.rs
git commit -m "test(server): 繋ぎ直しても同じ卓に戻る

ブラウザを再読み込みしても対局が最初からにならないことは、
評価のしやすさに直結する。last_seq を渡せば留守中の分だけが届き、
渡さなければ対局の頭から送り直される。"
```

---

## Self-Review

**仕様の網羅:** 仕様 3.2 節の「server — axum + tokio。WS・卓Actor」のうち WS を満たす。8.1 節の再接続は、`last_seq` をクエリ文字列で受け取ることで Wave 3c の機構へ繋いだ。

**このウェーブがやらないこと:** 認証、永続化（牌譜・段位・レート）、マッチング、友達対戦の部屋、席替え。**遊べるところまでを最短で通すため。**評価して方向が定まってから足すほうが、作り直しが少ない。

**認められた妥協:**

- **認証が無い。**卓の id を知っていれば誰でもその席に座れる。ローカル評価用と割り切る。
- **`JoinHandle` を台帳が持たない。**Wave 3c では「持つ」と決めたが、持たせるには終了理由を扱う設計が要る。卓が panic しても記録は残らない。
- **人は席0に固定。**
- **台帳の掃除は接続のたびだけ。**最後の接続より後に終わった卓は、サーバを止めるまで台帳に残る。ローカルで遊ぶあいだは問題にならないが、長く立ち上げ続ける段階になったら定期的な掃除が要る。
- **仕様 8.2 の CPU 代打ち切り替えは未実装。**切断中の席は自動打牌で進む。Wave 3c の引き継ぎに書いたとおり、`table.rs` にメソッドを足す必要があり、それは次のウェーブで行う。

**型の整合:** `TableId` / `Tables` は Task 1 で定義し、Task 2・3 は新しい型を足さない。`Command` と `ClientEventEnvelope` は `protocol` のものをそのまま使う。

**API の実在確認:** `axum::extract::ws::{Message, WebSocket, WebSocketUpgrade}`・`Message::Text(String::into())`・`axum::serve`・`Router::route`・`any`・`State`・`Query`・`ServeDir`・`TableHandle::{attach, command, detach, now_ms, is_closed}` は、実際に組んでブラウザから往復させて確認した。
