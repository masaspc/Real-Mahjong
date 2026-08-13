# Wave 3c 卓 Actor（tokio）実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wave 3b の同期な `Table` を tokio task で包み、実時間・乱数・チャネルを与えて「動き続ける卓」にする。

**Architecture:** 1卓 = 1 tokio task の Actor。外からはメッセージだけで触る。Actor は 100ms ごとに目を覚まして `Table::tick` を呼び、局の切れ目では自分でシードを作る。席ごとの配信は `Table::since` と「どこまで送ったか」の水位だけで行い、生配信と再接続の再送を**同じ1本の経路**に統一する。

**Tech Stack:** Rust / tokio 1（rt・macros・sync・time）/ rand 0.9

---

## Global Constraints

- **`crates/server/src/table.rs` を変更しない。** Wave 3b で凍結済み。このウェーブが `Table` に新しいメソッドを要求したら、それは設計の誤りである。
- **`crates/protocol/`・`crates/mahjong-core/`・`crates/mahjong-engine/`・`crates/mahjong-ai/` を変更しない。**
- **`crates/server/src/lib.rs` を変更しない。** `pub mod session;` は既に宣言されている。`session_time.rs` は `session.rs` の中から `#[path]` で宣言する。
- **時計は `tokio::time::Instant` を使う。`std::time::Instant` と `std::time::SystemTime` を書いてはならない。** `#[tokio::test(start_paused = true)]` が仮想化するのは前者だけであり、後者を使うとテストが実時間で数十秒かかるうえ非決定的になる。
- 依存の版は `tokio = { version = "1.53.1", ... }`、`rand = "0.9.5"` と明記する。両方ともこの計画を書く前に実際にコンパイルして API を確認済み。
- 日本語のコメントとテスト名を使う。既存の `table.rs` に合わせる。
- `cargo clippy --all-targets -- -D warnings` と `cargo fmt --check` を通す。
- テストは仕様である。**期待値を実装に合わせて書き換えてはならない。**合わないときは実装を直す。合わないまま進むなら止めて報告する。

---

## File Structure

| ファイル | 責務 |
|---|---|
| `crates/server/Cargo.toml` | tokio と rand の追加（Task 1） |
| `crates/server/src/session_time.rs` | 実時間の時計と、マスターシードから局のシードを繰り出す種源（Task 1） |
| `crates/server/src/session.rs` | メッセージ型・ハンドル・Actor ループ（Task 2, 3） |

`session.rs` の先頭で次のように宣言する。`#[path]` は**そのファイルのあるディレクトリからの相対**であり、`session.rs` は `crates/server/src/` にあるので、これで `crates/server/src/session_time.rs` を指す。

```rust
#[path = "session_time.rs"]
mod time;
```

---

## 設計上の決めごと（読まずに実装しないこと）

### なぜ 100ms のポーリングなのか

`Table` は「次の締切はいつか」を公開していない。締切ちょうどまで眠る設計にすると `Table` と `MatchEngine` に新しいアクセサが要り、凍結を破ることになる。一方で基準思考時間は 5,000ms、反応ウィンドウの最低待機は 350ms なので、100ms の粒度は人には見えない。単一 VPS に数十卓という規模では、100ms ポーリングの費用は問題にならない。

**`MissedTickBehavior::Skip` を設定する。**既定の `Burst` は、何かで遅れたときに取りこぼした tick をまとめて撃つ。この Actor が知りたいのは「いまの時刻」だけなので、遅れを取り返す意味がない。

### なぜ `since` だけで配るのか（`drain_for` を使わない）

Wave 3b の `Table` には `drain_for`（取り出したら消える）と `since`（何度呼んでも同じ）の2つがある。Actor は**`since` だけを使う**。席ごとに「どの連番まで送ったか」を Actor 側が持てば、生配信も再接続の再送も同じ呼び出しになる。2本の経路を持つと、片方だけ視界フィルタの扱いを間違える余地が生まれる。

水位の更新は**送った束の中の最大 `seq`**とする。視界フィルタで消えたイベントは束に現れないので水位が進まないが、次の可視イベントで一気に追い越すため取りこぼしも重複も起きない。

`drain_for` はこのウェーブで使われなくなるが、消さない。`Table` は凍結されている。

### なぜ出口が unbounded で入口が bounded なのか

出口（Actor → 接続）が詰まると Actor 全体が止まり、他の3人まで巻き添えになる。半荘のイベントは数百件しかないので、unbounded の memory は問題にならない。入口（接続 → Actor）は bounded にして、壊れたクライアントが Actor を溺れさせないようにする。

### シードは1本のマスターから繰り出す

局ごとに OS 乱数を引くのではなく、卓ごとに1本のマスターシードを持ち、そこから `StdRng` で局のシードを繰り出す。**卓ぜんぶが1本の 32 バイトから再現できる。**Wave 3d の牌譜再生がこれに乗る。

---

### Task 1: 実時間の時計とシードの種源

**Files:**
- Modify: `crates/server/Cargo.toml`
- Create: `crates/server/src/session_time.rs`

**Interfaces:**
- Consumes: `mahjong_engine::wall::Seed`（`Seed::new([u8; 32])`, `Seed::to_hex()`）
- Produces:
  - `pub struct Clock`, `Clock::start() -> Clock`, `Clock::now_ms(&self) -> u64`
  - `pub struct SeedSource`, `SeedSource::from_master([u8; 32]) -> SeedSource`, `SeedSource::from_os() -> SeedSource`, `SeedSource::next(&mut self) -> Seed`

- [ ] **Step 1: 依存を足す**

`crates/server/Cargo.toml` の `[dependencies]` の末尾に足し、`[dev-dependencies]` を新設する。

```toml
[dependencies]
protocol = { path = "../protocol" }
mahjong-engine = { path = "../mahjong-engine" }
mahjong-ai = { path = "../mahjong-ai" }
tokio = { version = "1.53.1", features = ["rt", "macros", "sync", "time"] }
rand = "0.9.5"

[dev-dependencies]
tokio = { version = "1.53.1", features = ["rt", "macros", "sync", "time", "test-util"] }

[lints]
workspace = true
```

`test-util` は `#[tokio::test(start_paused = true)]` に要る。

- [ ] **Step 2: 失敗するテストを書く**

`crates/server/src/session_time.rs` を作り、これだけを書く。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn the_clock_starts_at_zero() {
        let clock = Clock::start();
        assert_eq!(clock.now_ms(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn the_clock_follows_virtual_time() {
        let clock = Clock::start();
        tokio::time::sleep(Duration::from_millis(2_500)).await;
        assert_eq!(clock.now_ms(), 2_500);
    }

    #[test]
    fn the_same_master_gives_the_same_seeds() {
        let mut a = SeedSource::from_master([7u8; 32]);
        let mut b = SeedSource::from_master([7u8; 32]);
        for _ in 0..4 {
            assert_eq!(a.next().to_hex(), b.next().to_hex());
        }
    }

    #[test]
    fn a_different_master_gives_different_seeds() {
        let mut a = SeedSource::from_master([7u8; 32]);
        let mut b = SeedSource::from_master([8u8; 32]);
        assert_ne!(a.next().to_hex(), b.next().to_hex());
    }

    #[test]
    fn successive_seeds_differ() {
        let mut source = SeedSource::from_master([1u8; 32]);
        let first = source.next().to_hex();
        let second = source.next().to_hex();
        let third = source.next().to_hex();
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(first, third);
    }

    #[test]
    fn a_seed_is_thirty_two_bytes() {
        let mut source = SeedSource::from_master([1u8; 32]);
        assert_eq!(source.next().to_hex().len(), 64);
    }

    #[test]
    fn two_tables_from_the_operating_system_differ() {
        let mut a = SeedSource::from_os();
        let mut b = SeedSource::from_os();
        assert_ne!(a.next().to_hex(), b.next().to_hex());
    }
}
```

- [ ] **Step 3: 落ちることを確かめる**

Run: `cargo test --package server session_time`
Expected: コンパイルエラー。`Clock` と `SeedSource` が無い。

- [ ] **Step 4: 実装する**

`session_time.rs` のテストの**上**に置く。

```rust
//! 卓 Actor に実時間と乱数を与える。
//!
//! **`std::time` を使ってはならない。**`tokio::time::Instant` だけが
//! `#[tokio::test(start_paused = true)]` で仮想化される。ここを取り違えると
//! テストが実時間で数十秒かかり、しかも結果が揺れる。

use mahjong_engine::wall::Seed;
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};

/// 卓が生まれた瞬間からのミリ秒。
///
/// エンジンは `now_ms: u64` しか受け取らない。壁時計ではなく単調増加の
/// 経過時間を渡すことで、システム時刻が飛んでも局が壊れない。
pub struct Clock {
    origin: tokio::time::Instant,
}

impl Clock {
    pub fn start() -> Self {
        Clock {
            origin: tokio::time::Instant::now(),
        }
    }

    pub fn now_ms(&self) -> u64 {
        (tokio::time::Instant::now() - self.origin).as_millis() as u64
    }
}

/// 局のシードを繰り出す。
///
/// **卓ぜんぶが1本の 32 バイトから再現できる。**局ごとに OS 乱数を引くと
/// 牌譜を再生できなくなる。
pub struct SeedSource {
    rng: StdRng,
}

impl SeedSource {
    pub fn from_master(master: [u8; 32]) -> Self {
        SeedSource {
            rng: StdRng::from_seed(master),
        }
    }

    pub fn from_os() -> Self {
        let mut master = [0u8; 32];
        rand::rng().fill_bytes(&mut master);
        SeedSource::from_master(master)
    }

    pub fn next(&mut self) -> Seed {
        let mut bytes = [0u8; 32];
        self.rng.fill_bytes(&mut bytes);
        Seed::new(bytes)
    }
}
```

`rand` 0.9 では `thread_rng()` が `rng()` に改名されている。`rand::rng()` が正しい。

- [ ] **Step 5: 通ることを確かめる**

Run: `cargo test --package server session_time`
Expected: 7 passed

`session_time.rs` はまだどこからも宣言されていないので、この時点では**コンパイル対象に入らない**。Step 6 で `session.rs` に宣言を置いてから測る。

- [ ] **Step 6: `session.rs` から宣言する**

`crates/server/src/session.rs` の先頭に置く（既存の中身があれば残す）。

```rust
//! 1卓 = 1 tokio task の Actor。**唯一 I/O と時間を持つ層。**

#[path = "session_time.rs"]
mod time;

pub use time::{Clock, SeedSource};
```

- [ ] **Step 7: もう一度測る**

Run: `cargo test --package server`
Expected: table.rs の 28 件 + session_time の 7 件 = 35 passed

- [ ] **Step 8: 検査してコミット**

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt
git add crates/server/Cargo.toml crates/server/src/session_time.rs crates/server/src/session.rs Cargo.lock
git commit -m "feat(server): 卓 Actor の時計とシードの種源

tokio::time::Instant による経過ミリ秒と、1本のマスターシードから
局のシードを繰り出す種源。**卓ぜんぶが 32 バイトから再現できる。**

std::time を使わないのは、仮想時間で試験できなくなるため。"
```

---

### Task 2: Actor の骨格と席ごとの配信

**Files:**
- Modify: `crates/server/src/session.rs`

**Interfaces:**
- Consumes: Task 1 の `Clock`, `SeedSource`。`crate::table::{Table, Occupant}`。`mahjong_engine::match_flow::Reject`。`protocol::client_event::ClientEventEnvelope`、`protocol::command::Command`、`protocol::ruleset::Ruleset`、`protocol::seat::Seat`。
- Produces:
  - `pub type Outbound = tokio::sync::mpsc::UnboundedSender<ClientEventEnvelope>`
  - `pub type Inbound = tokio::sync::mpsc::UnboundedReceiver<ClientEventEnvelope>`
  - `pub struct Gone;`
  - `pub enum TableMsg { Command { seat, command, reply }, Attach { seat, last_seq, out }, Detach { seat } }`
  - `pub struct TableHandle`, `TableHandle::command`, `TableHandle::attach`, `TableHandle::detach`, `TableHandle::is_closed`
  - `pub fn spawn(rules: Ruleset, occupants: [Occupant; 4], seeds: SeedSource) -> TableHandle`

- [ ] **Step 1: 失敗するテストを書く**

`session.rs` の末尾に足す。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Occupant;
    use protocol::client_event::ClientEvent;
    use protocol::event::PlayerId;
    use protocol::ruleset::MatchLength;
    use protocol::tile::Tile;

    pub(super) fn rules() -> Ruleset {
        Ruleset::kin_no_ma(MatchLength::Hanchan)
    }

    pub(super) fn humans() -> [Occupant; 4] {
        std::array::from_fn(|i| Occupant::Human(PlayerId(format!("p{i}"))))
    }

    pub(super) fn one_human_three_cpus() -> [Occupant; 4] {
        [
            Occupant::Human(PlayerId("human".to_owned())),
            Occupant::Cpu(PlayerId("cpu1".to_owned())),
            Occupant::Cpu(PlayerId("cpu2".to_owned())),
            Occupant::Cpu(PlayerId("cpu3".to_owned())),
        ]
    }

    /// その席へ届いた分をいま取れるだけ取る。
    pub(super) fn take_ready(inbox: &mut Inbound) -> Vec<ClientEventEnvelope> {
        let mut out = Vec::new();
        while let Ok(envelope) = inbox.try_recv() {
            out.push(envelope);
        }
        out
    }

    /// 配牌で配られた自分の手牌。
    pub(super) fn dealt_hand(events: &[ClientEventEnvelope]) -> Vec<Tile> {
        events
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::Deal { your_hand, .. } => Some(your_hand.clone()),
                _ => None,
            })
            .expect("配牌が届いている")
    }

    #[tokio::test(start_paused = true)]
    async fn attaching_delivers_the_opening_of_the_match() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let mut inbox = handle.attach(Seat::new(0), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;

        let events = take_ready(&mut inbox);
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::MatchStart { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::RoundStart { .. })));
        assert_eq!(dealt_hand(&events).len(), 14, "親は14枚");
    }

    #[tokio::test(start_paused = true)]
    async fn the_actor_deals_a_seed_without_being_asked() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let mut inbox = handle.attach(Seat::new(1), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;

        let events = take_ready(&mut inbox);
        assert!(
            events
                .iter()
                .any(|e| matches!(e.event, ClientEvent::RoundStart { .. })),
            "外からシードを渡していないのに局が始まっている"
        );
        assert_eq!(dealt_hand(&events).len(), 13, "子は13枚");
    }

    #[tokio::test(start_paused = true)]
    async fn two_seats_are_dealt_different_hands() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let mut east = handle.attach(Seat::new(0), None).await.expect("卓は生きている");
        let mut south = handle.attach(Seat::new(1), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;

        let east_hand = dealt_hand(&take_ready(&mut east));
        let south_hand = dealt_hand(&take_ready(&mut south));
        assert_ne!(east_hand, south_hand, "視界フィルタが効いていない");
    }

    #[tokio::test(start_paused = true)]
    async fn a_seat_never_sees_another_hand() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let mut south = handle.attach(Seat::new(1), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;

        for envelope in take_ready(&mut south) {
            if let ClientEvent::Draw { seat, tile, .. } = &envelope.event {
                assert!(
                    *seat == Seat::new(1) || tile.is_none(),
                    "他家のツモ牌が見えている"
                );
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_discard_reaches_the_other_seats() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let mut east = handle.attach(Seat::new(0), None).await.expect("卓は生きている");
        let mut west = handle.attach(Seat::new(2), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;

        let tile = dealt_hand(&take_ready(&mut east))[0];
        let _ = take_ready(&mut west);

        handle
            .command(Seat::new(0), Command::Discard { tile, riichi: false })
            .await
            .expect("卓は生きている")
            .expect("親は打てる");
        tokio::task::yield_now().await;

        assert!(
            take_ready(&mut west).iter().any(|e| matches!(
                &e.event,
                ClientEvent::Discard { seat, tile: t, .. } if *seat == Seat::new(0) && *t == tile
            )),
            "打牌が西家へ届いていない"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_command_from_the_wrong_seat_is_rejected() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let mut south = handle.attach(Seat::new(1), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;

        let tile = dealt_hand(&take_ready(&mut south))[0];
        let rejected = handle
            .command(Seat::new(1), Command::Discard { tile, riichi: false })
            .await
            .expect("卓は生きている");
        assert_eq!(rejected, Err(Reject::NotYourTurn), "親でない席が打てている");
    }

    #[tokio::test(start_paused = true)]
    async fn sequence_numbers_only_go_up() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let mut inbox = handle.attach(Seat::new(2), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;

        let events = take_ready(&mut inbox);
        assert!(events.len() >= 3);
        for pair in events.windows(2) {
            assert!(pair[0].seq < pair[1].seq, "連番が戻っている");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_seat_is_made_to_discard_once_its_bank_runs_out() {
        let handle = spawn(rules(), one_human_three_cpus(), SeedSource::from_master([1u8; 32]));
        let mut east = handle.attach(Seat::new(0), None).await.expect("卓は生きている");
        let clock = Clock::start();
        tokio::task::yield_now().await;
        let _ = take_ready(&mut east);

        // 親が黙っている。基準思考時間 5,000ms とバンク 20,000ms を
        // 使い切るまで切らされない。
        let envelope = tokio::time::timeout(
            std::time::Duration::from_millis(60_000),
            east.recv(),
        )
        .await
        .expect("60秒のうちに何か届く")
        .expect("卓は生きている");

        assert!(
            matches!(&envelope.event, ClientEvent::Discard { seat, .. } if *seat == Seat::new(0)),
            "時間切れで自動打牌されていない"
        );
        assert!(
            clock.now_ms() >= 25_000,
            "基準思考時間だけで切られている。バンクが使われていない: {}ms",
            clock.now_ms()
        );
    }
}
```

- [ ] **Step 2: 落ちることを確かめる**

Run: `cargo test --package server session`
Expected: コンパイルエラー。`spawn` / `TableHandle` / `Inbound` が無い。

- [ ] **Step 3: メッセージ型とハンドルを書く**

`session.rs` の `pub use time::{Clock, SeedSource};` の下に置く。

```rust
use crate::table::{Occupant, Table};
use mahjong_engine::match_flow::Reject;
use protocol::client_event::ClientEventEnvelope;
use protocol::command::Command;
use protocol::ruleset::Ruleset;
use protocol::seat::Seat;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::MissedTickBehavior;

/// Actor が目を覚ます間隔。
///
/// `Table` は「次の締切はいつか」を教えてくれないので、締切ちょうどで
/// 起きることはできない。基準思考時間は 5,000ms、反応ウィンドウの
/// 最低待機は 350ms なので、100ms の粒度は人には見えない。
const POLL_MS: u64 = 100;

/// 接続へイベントを押し出す口。
///
/// **unbounded。**ここが詰まると Actor ごと止まり、他の3人まで巻き添えに
/// なる。半荘のイベントは数百件しかないので memory は問題にならない。
pub type Outbound = mpsc::UnboundedSender<ClientEventEnvelope>;
pub type Inbound = mpsc::UnboundedReceiver<ClientEventEnvelope>;

/// 卓が既に畳まれている。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gone;

pub enum TableMsg {
    Command {
        seat: Seat,
        command: Command,
        reply: oneshot::Sender<Result<(), Reject>>,
    },
    /// 接続または再接続。`last_seq` より後を送り直してから生配信へ移る。
    Attach {
        seat: Seat,
        last_seq: Option<u32>,
        out: Outbound,
    },
    Detach {
        seat: Seat,
    },
}

#[derive(Clone)]
pub struct TableHandle {
    tx: mpsc::Sender<TableMsg>,
}

impl TableHandle {
    pub async fn command(&self, seat: Seat, command: Command) -> Result<Result<(), Reject>, Gone> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(TableMsg::Command {
                seat,
                command,
                reply,
            })
            .await
            .map_err(|_| Gone)?;
        answer.await.map_err(|_| Gone)
    }

    /// その席の配信を受け取る。`last_seq` より後が最初に流れてくる。
    pub async fn attach(&self, seat: Seat, last_seq: Option<u32>) -> Result<Inbound, Gone> {
        let (out, inbox) = mpsc::unbounded_channel();
        self.tx
            .send(TableMsg::Attach {
                seat,
                last_seq,
                out,
            })
            .await
            .map_err(|_| Gone)?;
        Ok(inbox)
    }

    pub async fn detach(&self, seat: Seat) -> Result<(), Gone> {
        self.tx
            .send(TableMsg::Detach { seat })
            .await
            .map_err(|_| Gone)
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}
```

- [ ] **Step 4: Actor ループを書く**

同じファイルの続きに置く。

```rust
/// 卓を立ち上げ、ハンドルを返す。
///
/// **時計は卓が生まれた瞬間から始まる。**`Table::new` に渡す最初の時刻は 0。
pub fn spawn(rules: Ruleset, occupants: [Occupant; 4], seeds: SeedSource) -> TableHandle {
    let (tx, rx) = mpsc::channel(32);
    let clock = Clock::start();
    let table = Table::new(rules, occupants, clock.now_ms());
    tokio::spawn(run(table, seeds, clock, rx));
    TableHandle { tx }
}

/// 席ごとの配信先と、どこまで送ったか。
struct Sinks {
    out: [Option<Outbound>; 4],
    sent_upto: [Option<u32>; 4],
}

impl Sinks {
    fn new() -> Self {
        Sinks {
            out: std::array::from_fn(|_| None),
            sent_upto: [None; 4],
        }
    }

    /// まだ送っていない分を席ごとに押し出す。
    ///
    /// **`since` だけを使う。**生配信と再接続の再送を1本の経路にまとめる
    /// ことで、片方だけ視界フィルタを取り違える余地を消す。
    fn flush(&mut self, table: &Table) {
        for index in 0..4 {
            let Some(out) = &self.out[index] else {
                continue;
            };
            let seat = Seat::new(index as u8);
            let batch = table.since(seat, self.sent_upto[index]);
            let mut highest = self.sent_upto[index];
            let mut alive = true;
            for envelope in batch {
                let seq = envelope.seq;
                if out.send(envelope).is_err() {
                    alive = false;
                    break;
                }
                highest = Some(seq);
            }
            self.sent_upto[index] = highest;
            if !alive {
                self.out[index] = None;
            }
        }
    }
}

async fn run(
    mut table: Table,
    mut seeds: SeedSource,
    clock: Clock,
    mut rx: mpsc::Receiver<TableMsg>,
) {
    let mut sinks = Sinks::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(POLL_MS));
    // 遅れを取り返す意味はない。知りたいのは「いまの時刻」だけ。
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        if table.needs_seed() {
            table.begin_round(&seeds.next(), clock.now_ms());
        }
        sinks.flush(&table);
        if table.is_over() {
            break;
        }

        tokio::select! {
            message = rx.recv() => match message {
                None => break,
                Some(TableMsg::Command { seat, command, reply }) => {
                    let result = table.apply(seat, command, clock.now_ms());
                    let _ = reply.send(result);
                }
                Some(TableMsg::Attach { seat, last_seq, out }) => {
                    sinks.sent_upto[seat.index()] = last_seq;
                    sinks.out[seat.index()] = Some(out);
                }
                Some(TableMsg::Detach { seat }) => {
                    sinks.out[seat.index()] = None;
                }
            },
            _ = ticker.tick() => {
                table.tick(clock.now_ms());
            }
        }
    }

    // 終局のイベントを配ってから閉じる。
    sinks.flush(&table);
}
```

`is_over` の判定を `flush` の**後**に置くのが要点。終局のイベントは最後の `tick` か `apply` で出るので、先に判定すると誰にも届かないまま卓が消える。

- [ ] **Step 5: 通ることを確かめる**

Run: `cargo test --package server session::tests`
Expected: 8 passed

`a_silent_seat_is_made_to_discard_once_its_bank_runs_out` が落ちる場合、時計が仮想化されていない疑いが濃い。`session_time.rs` に `std::time` が紛れ込んでいないか確認する。

- [ ] **Step 6: 全体を測る**

Run: `cargo test --package server`
Expected: 43 passed（table 28 + session_time 7 + session 8）

- [ ] **Step 7: 検査してコミット**

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt
git add crates/server/src/session.rs
git commit -m "feat(server): 卓 Actor の骨格と席ごとの配信

1卓 = 1 tokio task。100ms ごとに tick し、局の切れ目では自分でシードを作る。

配信は since と水位だけで行い、drain_for を使わない。生配信と再接続の
再送を1本の経路にまとめることで、片方だけ視界フィルタを取り違える
余地を消す。

is_over の判定は flush の後。先に判定すると終局のイベントが
誰にも届かないまま卓が消える。"
```

---

### Task 3: 再接続・切断・卓の終わり

**Files:**
- Modify: `crates/server/src/session.rs`

**Interfaces:**
- Consumes: Task 2 のすべて。
- Produces: 新しい公開 API は無い。Task 2 の `Attach` / `Detach` の意味づけを固める。

- [ ] **Step 1: 失敗するテストを書く**

`session.rs` の末尾に新しいモジュールとして足す。

```rust
#[cfg(test)]
mod reconnect_tests {
    use super::tests::{humans, one_human_three_cpus, rules, take_ready};
    use super::*;
    use crate::table::Occupant;
    use protocol::client_event::ClientEvent;
    use protocol::event::PlayerId;

    fn all_cpu() -> [Occupant; 4] {
        std::array::from_fn(|i| Occupant::Cpu(PlayerId(format!("cpu{i}"))))
    }

    #[tokio::test(start_paused = true)]
    async fn reattaching_from_a_sequence_skips_what_was_already_seen() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let mut first = handle.attach(Seat::new(0), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;

        let seen = take_ready(&mut first);
        let last = seen.last().expect("何か届いている").seq;

        let mut second = handle
            .attach(Seat::new(0), Some(last))
            .await
            .expect("卓は生きている");
        tokio::task::yield_now().await;

        for envelope in take_ready(&mut second) {
            assert!(envelope.seq > last, "見たものがまた来ている");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn reattaching_from_nothing_replays_the_whole_match() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let mut first = handle.attach(Seat::new(0), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;
        let original = take_ready(&mut first);

        let mut second = handle.attach(Seat::new(0), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;
        let replayed = take_ready(&mut second);

        assert_eq!(
            original.iter().map(|e| e.seq).collect::<Vec<_>>(),
            replayed.iter().map(|e| e.seq).collect::<Vec<_>>(),
            "再送が元と食い違っている"
        );
        assert!(
            replayed
                .iter()
                .any(|e| matches!(e.event, ClientEvent::MatchStart { .. })),
            "最初から再送されていない"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_detached_seat_catches_up_when_it_comes_back() {
        let handle = spawn(rules(), one_human_three_cpus(), SeedSource::from_master([1u8; 32]));
        let mut inbox = handle.attach(Seat::new(1), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;
        let seen = take_ready(&mut inbox);
        let last = seen.last().expect("何か届いている").seq;

        handle.detach(Seat::new(1)).await.expect("卓は生きている");
        drop(inbox);

        // 席が居ないあいだも卓は進む。親は CPU なので勝手に打つ。
        tokio::time::sleep(std::time::Duration::from_millis(3_000)).await;

        let mut back = handle
            .attach(Seat::new(1), Some(last))
            .await
            .expect("卓は生きている");
        tokio::task::yield_now().await;
        let caught_up = take_ready(&mut back);

        assert!(!caught_up.is_empty(), "留守中の分が追いついていない");
        assert!(caught_up.iter().all(|e| e.seq > last));
        for pair in caught_up.windows(2) {
            assert!(pair[0].seq < pair[1].seq);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_a_receiver_does_not_stop_the_table() {
        let handle = spawn(rules(), one_human_three_cpus(), SeedSource::from_master([1u8; 32]));
        let inbox = handle.attach(Seat::new(2), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;
        drop(inbox);

        tokio::time::sleep(std::time::Duration::from_millis(3_000)).await;

        let mut other = handle.attach(Seat::new(3), None).await.expect("卓は生きている");
        tokio::task::yield_now().await;
        assert!(!take_ready(&mut other).is_empty(), "卓が止まっている");
    }

    #[tokio::test(start_paused = true)]
    async fn four_cpus_play_a_whole_match_and_the_actor_shuts_down() {
        let handle = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let mut watcher = handle.attach(Seat::new(0), None).await.expect("卓は生きている");

        let mut saw_match_end = false;
        while let Some(envelope) = watcher.recv().await {
            if matches!(envelope.event, ClientEvent::MatchEnd { .. }) {
                saw_match_end = true;
            }
        }
        assert!(saw_match_end, "半荘が終わっていない");

        // Actor が落ちるとハンドルの送り口も閉じる。
        for _ in 0..100 {
            if handle.is_closed() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(handle.is_closed(), "卓が終わったのに Actor が生きている");
    }

    #[tokio::test(start_paused = true)]
    async fn a_finished_table_refuses_new_commands() {
        let handle = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let mut watcher = handle.attach(Seat::new(0), None).await.expect("卓は生きている");
        while watcher.recv().await.is_some() {}

        for _ in 0..100 {
            if handle.is_closed() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(handle.attach(Seat::new(0), None).await.err(), Some(Gone));
    }

    #[tokio::test(start_paused = true)]
    async fn the_seed_commitment_changes_from_round_to_round() {
        let handle = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let mut watcher = handle.attach(Seat::new(0), None).await.expect("卓は生きている");

        let mut commits = Vec::new();
        while let Some(envelope) = watcher.recv().await {
            if let ClientEvent::RoundStart { seed_commit, .. } = &envelope.event {
                commits.push(seed_commit.clone());
            }
        }
        assert!(commits.len() >= 2, "局が1つしか立っていない");
        let unique: std::collections::HashSet<_> = commits.iter().collect();
        assert_eq!(unique.len(), commits.len(), "同じシードが2度使われている");
    }
}
```

`ClientEvent::MatchEnd` の綴りが違う場合は `crates/protocol/src/client_event.rs` を読んで**実際の変種名に合わせる**。テストの意図（半荘の終わりを見た）は変えない。

- [ ] **Step 2: 落ちることを確かめる**

Run: `cargo test --package server reconnect_tests`
Expected: いくつか落ちる。特に `four_cpus_play_a_whole_match_and_the_actor_shuts_down` は、Task 2 の実装のままだと**永久に終わらない可能性がある**。`CPU しか居ない卓では誰も `apply` を呼ばないので、Actor は `ticker.tick()` だけで進む。`Table::tick` の中で CPU が代打ちするので進むはずだが、進まないなら Task 2 のループの順序が誤っている。

- [ ] **Step 3: 必要なら直す**

Task 2 の実装が正しければ、このタスクで新しいコードは要らない。落ちるテストがあれば、原因は次のどれかである。

1. `is_over` を `flush` より先に判定している → 終局のイベントが届かない
2. `Attach` で `sent_upto` を上書きしていない → 再接続で最初から全部来てしまう
3. `flush` の水位を束の最大 `seq` でなく `len` などで進めている → 取りこぼし
4. 送信失敗で `out` を `None` にしていない → 落ちた接続へ永久に送り続ける

いずれも Task 2 のコードを直す。**テストの期待値を動かさない。**

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test --package server reconnect_tests`
Expected: 7 passed

そのうえで crate 全体を測る。

Run: `cargo test --package server`
期待: 50 件（table 28 + session_time 7 + session 8 + reconnect 7）

- [ ] **Step 5: workspace 全体を測る**

```bash
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
Expected: 失敗ゼロ

- [ ] **Step 6: 禁じ手が入っていないか自分で確かめる**

```bash
# std の時計を使っていないか（コメントを除く）
grep -n "std::time::Instant\|SystemTime" crates/server/src/session*.rs
# table.rs を触っていないか
git diff --stat main -- crates/server/src/table.rs
# 凍結クレートを触っていないか
git diff --stat main -- crates/protocol crates/mahjong-core crates/mahjong-engine crates/mahjong-ai
```
Expected: すべて空

- [ ] **Step 7: コミット**

```bash
git add crates/server/src/session.rs
git commit -m "feat(server): 再接続・切断・卓の終わり

attach は last_seq より後だけを送り直す。席が居ないあいだも卓は進み、
戻ってきたら留守中の分が追いつく。受け口が落ちても卓は止まらない。

半荘が終わると Actor が終了し、ハンドルの送り口が閉じる。
**4人 CPU の卓が Actor の上で最後まで回る。**"
```

---

## Self-Review

**仕様の網羅:** 仕様 §3.2「server — 唯一 I/O と時間を持つ層。1卓 = 1 tokio task の Actor とし、卓同士を完全に独立させる」を Task 2 が満たす。§8.1「再接続 = `seq` 以降のイベント再送」を Task 3 が満たす。シード開示（`SeedReveal`）は半荘終了後の話で、`Event` を出すのはエンジン側の責務なので、このウェーブでは Actor が素通しするだけでよい。

**このウェーブが**やらない**こと:** WebSocket・axum・HTTP は Wave 3d。永続化とマッチングも Wave 3d 以降。ここは「卓が実時間で動く」ところまでで止める。**非同期とネットワークを一度に入れると、失敗したときどちらが原因か切り分けられない。**

**型の整合:** `Outbound` / `Inbound` / `Gone` / `TableMsg` / `TableHandle` / `spawn` / `Clock` / `SeedSource` はすべて Task 1・2 で定義し、Task 3 は新しい型を足さない。`Reject` は `mahjong_engine::match_flow::Reject`（`PartialEq` と `Debug` を導出済み、`assert_eq!` で使える）。

**API の実在確認:** `rand::rng()`・`RngCore::fill_bytes`・`StdRng::from_seed`・`tokio::time::interval`・`MissedTickBehavior::Skip`・`#[tokio::test(start_paused = true)]` の自動時間送りは、この計画を書く前に実際にコンパイルして動作を確認した。`Seed::new([u8; 32])`・`Seed::to_hex()`・`Table::since(seat, Option<u32>)`・`Table::needs_seed()`・`Table::is_over()`・`Table::begin_round(&Seed, u64)`・`Ruleset::kin_no_ma(MatchLength::Hanchan)`・`Seat::new(u8)`・`Seat::index()` はすべて実コードから読み取った。
