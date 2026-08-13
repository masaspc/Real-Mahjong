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
- 依存の版は `tokio = { version = "1.53.1", ... }`、`rand = "0.9.5"`。
- 日本語のコメントとテスト名を使う。既存の `table.rs` に合わせる。
- `cargo clippy --all-targets -- -D warnings` と `cargo fmt --check` を通す。
- テストは仕様である。**期待値を実装に合わせて書き換えてはならない。**合わないときは実装を直す。合わないまま進むなら止めて報告する。

**この計画に載っているコードと期待値は、実際にコンパイルして実行し、全件通ることを確認済みである。**凍結済みの `Table` を外から使う試作クレートで、Actor をこの計画のとおりに組み、下の6点を実測した。

| 実測した前提 | 結果 |
|---|---|
| `attach` の ack が返った時点で配信済み | MatchStart・RoundStart・Deal・Draw の5件が `yield` 無しで inbox にある |
| 配牌の枚数 | **親も子も 13 枚。**親の14枚目は別の `Draw { tile: Some(_) }` |
| 無言の席が自動打牌されるまで | 仮想 **25,800ms**（式は 5,000 + 20,000 + 500 + lead_in 250 = 25,750、その次のポーリング） |
| 4人 CPU の半荘 | 1,304 イベント / 9局 / 全部別シード / `MatchEnd` あり / 実時間 2.7 秒 |
| 過去の時刻で刻印した `apply` | `Ok(())`。エンジンは時刻の巻き戻しを受けつける |
| 古い接続の `Detach` | 接続 ID で照合すれば新しい接続を切らない |

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

### コマンドは「入口に着いた時刻」で判定する

**これを外すと、締切前に押した操作が時間切れになる。**`RoundEngine::apply` は検証より先に `tick(now_ms)` を回し、`now_ms` が締切を過ぎていれば自動打牌してしまう。Actor が `select!` でどちらの枝を選ぶか、入口の待ち行列がどれだけ混んでいたかによって、同じ操作の結果が変わってはならない。

そこで `TableHandle::command` が**送る前に**時刻を刻み、Actor はその刻印を `Table::apply` へ渡す。刻印は Actor が処理する時刻より必ず古いが、エンジンは時刻の巻き戻しを受けつける（実測済み）。締切は絶対時刻なので、古い時刻を渡すことは「まだ切れていない」を意味し、これがまさに求める挙動である。

そのため `TableHandle` は Actor と**同じ `Clock` を共有する**（`Arc<Clock>`）。別々に `Clock::start()` すると原点がずれる。

### なぜ `since` だけで配るのか（`drain_for` を使わない）

Wave 3b の `Table` には `drain_for`（取り出したら消える）と `since`（何度呼んでも同じ）の2つがある。Actor は**`since` だけを使う**。席ごとに「どの連番まで送ったか」を Actor 側が持てば、生配信も再接続の再送も同じ呼び出しになる。2本の経路を持つと、片方だけ視界フィルタの扱いを間違える余地が生まれる。

水位の更新は**送った束の中の最大 `seq`**とする。視界フィルタで消えたイベントは束に現れないので水位が進まないが、次の可視イベントで一気に追い越すため取りこぼしも重複も起きない。

`drain_for` はこのウェーブで使われなくなるが、消さない。`Table` は凍結されている。

### `attach` は「配り終えてから」返事する

`attach` は接続 ID を返す。Actor は **sink を登録し、配り、それから ack を送る**。この順序により、`attach` が返った時点で送るべきものは既に inbox に入っている。`yield_now()` を何回呼べば届くかを当てにするテストは書かない。**スケジューラの都合に依存するテストは、いつか必ず落ちる。**

### `Detach` は接続 ID で照合する

同じ席への新しい `Attach` は古い sink を置き換える（**最後の接続が勝つ**）。このとき、置き換えられた古い接続が遅れて `Detach` を送ってくると、席だけで消す実装では**新しい接続が切れる**。Wave 3d の普通の再接続で起きる。`Detach { seat, connection }` とし、いま生きている ID と一致するときだけ外す。

### 辻褄の合わない `last_seq` は信用しない

クライアントの申告をそのまま `since` へ渡すと、`u32::MAX` を渡された席は卓が終わるまで一件も受け取れない。かといって「見えている最大値」へ丸めると、そのクライアントは文脈を持たないまま以降のイベントだけを受け取り、復旧できない。**存在しない連番を申告してきたら、最初から送り直す。**

### シードは1本のマスターから繰り出す

局ごとに OS 乱数を引くのではなく、卓ごとに1本のマスターシードを持ち、そこから `StdRng` で局のシードを繰り出す。**卓ぜんぶが1本の 32 バイトから再現できる。**Wave 3d の牌譜再生がこれに乗る。

### ループの順序

```
出来ている分を配る → 終局なら抜ける → シードが要るなら局を立ててもう一度配る → select!
```

配るのを先にするのは、**局が終わったことを、次局を立てる前に伝える**ため。局を立てたあとにもう一度配るのは、次局の頭を最大 100ms 待たせないため。

---

### Task 1: 実時間の時計とシードの種源

**Files:**
- Modify: `crates/server/Cargo.toml`
- Create: `crates/server/src/session_time.rs`
- Modify: `crates/server/src/session.rs`

**Interfaces:**
- Consumes: `mahjong_engine::wall::Seed`（`Seed::new([u8; 32])`, `Seed::to_hex()`）
- Produces:
  - `pub struct Clock`, `Clock::start() -> Clock`, `Clock::now_ms(&self) -> u64`
  - `pub struct SeedSource`, `SeedSource::from_master([u8; 32]) -> SeedSource`, `SeedSource::from_os() -> SeedSource`, `SeedSource::next_seed(&mut self) -> Seed`

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
tokio = { version = "1.53.1", features = ["rt", "macros", "sync", "time"] }
rand = "0.9.5"

[dev-dependencies]
tokio = { version = "1.53.1", features = ["rt", "macros", "sync", "time", "test-util"] }

[lints]
workspace = true
```

`test-util` は `#[tokio::test(start_paused = true)]` に要る。

- [ ] **Step 2: `session.rs` から宣言する**

`crates/server/src/session.rs` を次の内容にする。**先に宣言しないと `session_time.rs` はコンパイル対象に入らず、次のステップで何も測れない。**

```rust
//! 1卓 = 1 tokio task の Actor。**唯一 I/O と時間を持つ層。**

#[path = "session_time.rs"]
mod time;

pub use time::{Clock, SeedSource};
```

- [ ] **Step 3: 失敗するテストを書く**

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
            assert_eq!(a.next_seed().to_hex(), b.next_seed().to_hex());
        }
    }

    #[test]
    fn a_different_master_gives_different_seeds() {
        let mut a = SeedSource::from_master([7u8; 32]);
        let mut b = SeedSource::from_master([8u8; 32]);
        assert_ne!(a.next_seed().to_hex(), b.next_seed().to_hex());
    }

    #[test]
    fn successive_seeds_differ() {
        let mut source = SeedSource::from_master([1u8; 32]);
        let first = source.next_seed().to_hex();
        let second = source.next_seed().to_hex();
        let third = source.next_seed().to_hex();
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(first, third);
    }

    #[test]
    fn a_seed_is_thirty_two_bytes() {
        let mut source = SeedSource::from_master([1u8; 32]);
        assert_eq!(source.next_seed().to_hex().len(), 64);
    }

    #[test]
    fn two_tables_from_the_operating_system_differ() {
        let mut a = SeedSource::from_os();
        let mut b = SeedSource::from_os();
        assert_ne!(a.next_seed().to_hex(), b.next_seed().to_hex());
    }
}
```

- [ ] **Step 4: 落ちることを確かめる**

Run: `cargo test --package server session_time`
Expected: コンパイルエラー。`Clock` と `SeedSource` が無い。

- [ ] **Step 5: 実装する**

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

    /// clippy が `Iterator::next` と紛らわしいと言うので `next_seed`。
    pub fn next_seed(&mut self) -> Seed {
        let mut bytes = [0u8; 32];
        self.rng.fill_bytes(&mut bytes);
        Seed::new(bytes)
    }
}
```

`rand` 0.9 では `thread_rng()` が `rng()` に改名されている。`rand::rng()` が正しい。

- [ ] **Step 6: 通ることを確かめる**

Run: `cargo test --package server session_time`
Expected: 7 passed

- [ ] **Step 7: crate 全体を測る**

Run: `cargo test --package server`
期待: 35 件（table 28 + session_time 7）

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
  - `pub struct ConnectionId(pub u64);`
  - `pub enum TableMsg { Command { seat, command, at_ms, reply }, Attach { seat, last_seq, out, ack }, Detach { seat, connection } }`
  - `pub struct TableHandle`（`Clone + Send + Sync + 'static`）, `TableHandle::command`, `TableHandle::attach`, `TableHandle::detach`, `TableHandle::is_closed`
  - `pub fn spawn(rules: Ruleset, occupants: [Occupant; 4], seeds: SeedSource) -> TableHandle`

- [ ] **Step 1: 失敗するテストを書く**

`session.rs` の末尾に足す。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Table;
    use mahjong_engine::wall::Seed;
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
    async fn attaching_delivers_the_opening_without_waiting() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        // yield_now を呼ばない。ack が返った時点で届いていなければならない。
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let events = take_ready(&mut inbox);
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::MatchStart { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::RoundStart { .. })));
        // **配牌は親も子も13枚。**親の14枚目は別の Draw で来る。
        assert_eq!(dealt_hand(&events).len(), 13);
        assert!(
            events.iter().any(|e| matches!(
                &e.event,
                ClientEvent::Draw { seat, tile, .. } if *seat == Seat::new(0) && tile.is_some()
            )),
            "親の14枚目が Draw で来ていない"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_actor_deals_a_seed_without_being_asked() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut inbox) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        let events = take_ready(&mut inbox);
        assert!(
            events
                .iter()
                .any(|e| matches!(e.event, ClientEvent::RoundStart { .. })),
            "外からシードを渡していないのに局が始まっている"
        );
        assert_eq!(dealt_hand(&events).len(), 13);
    }

    #[tokio::test(start_paused = true)]
    async fn two_seats_are_dealt_different_hands() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut east) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let (_, mut south) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        let east_hand = dealt_hand(&take_ready(&mut east));
        let south_hand = dealt_hand(&take_ready(&mut south));
        assert_ne!(east_hand, south_hand, "視界フィルタが効いていない");
    }

    #[tokio::test(start_paused = true)]
    async fn a_seat_never_sees_another_draw() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut south) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

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
        let (_, mut east) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let (_, mut west) = handle
            .attach(Seat::new(2), None)
            .await
            .expect("卓は生きている");

        let tile = dealt_hand(&take_ready(&mut east))[0];
        let _ = take_ready(&mut west);

        handle
            .command(Seat::new(0), Command::Discard { tile, riichi: false })
            .await
            .expect("卓は生きている")
            .expect("親は打てる");

        let seen = tokio::time::timeout(Duration::from_millis(1_000), west.recv())
            .await
            .expect("1秒のうちに届く")
            .expect("卓は生きている");
        assert!(
            matches!(
                &seen.event,
                ClientEvent::Discard { seat, tile: t, .. } if *seat == Seat::new(0) && *t == tile
            ) || take_ready(&mut west).iter().any(|e| matches!(
                &e.event,
                ClientEvent::Discard { seat, tile: t, .. } if *seat == Seat::new(0) && *t == tile
            )),
            "打牌が西家へ届いていない"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_command_from_the_wrong_seat_is_rejected() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut south) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

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
        let (_, mut inbox) = handle
            .attach(Seat::new(2), None)
            .await
            .expect("卓は生きている");

        let events = take_ready(&mut inbox);
        assert!(events.len() >= 3);
        for pair in events.windows(2) {
            assert!(pair[0].seq < pair[1].seq, "連番が戻っている");
        }
    }

    /// Wave 3d で axum のハンドラへ持たせるには `Send + Sync` が要る。
    /// **ここで確かめておかないと、次のウェーブで型を作り直すことになる。**
    #[test]
    fn the_handle_can_cross_threads() {
        fn assert_send_sync<T: Send + Sync + Clone + 'static>() {}
        assert_send_sync::<TableHandle>();
    }

    /// **これが成り立たないと、入口に着いた時刻で判定する設計が崩れる。**
    /// 刻印は Actor が処理する時刻より必ず古い。エンジンがそれを拒むなら
    /// 別の手を考えねばならない。
    #[test]
    fn the_engine_accepts_a_command_stamped_in_the_past() {
        let mut table = Table::new(rules(), humans(), 0);
        table.begin_round(&Seed::from_hex(&"01".repeat(32)).expect("正しい hex"), 0);
        let tile = table
            .round_state()
            .expect("局が動いている")
            .seat(Seat::new(0))
            .hand[0];

        table.tick(5_000);
        let result = table.apply(
            Seat::new(0),
            Command::Discard { tile, riichi: false },
            1_000,
        );
        assert_eq!(result, Ok(()), "過去の刻印で弾かれた");
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_seat_is_made_to_discard_when_its_bank_runs_out() {
        let handle = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let clock = Clock::start();
        let (_, mut east) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let _ = take_ready(&mut east);

        let envelope = tokio::time::timeout(Duration::from_millis(60_000), east.recv())
            .await
            .expect("60秒のうちに何か届く")
            .expect("卓は生きている");

        assert!(
            matches!(&envelope.event, ClientEvent::Discard { seat, .. } if *seat == Seat::new(0)),
            "時間切れで自動打牌されていない"
        );
        // 基準 5,000 + バンク 20,000 + 通信猶予 500 + ツモの lead_in 250 = 25,750。
        // ポーリングは 100ms 刻みなので、実際に切られるのは 25,800。
        let elapsed = clock.now_ms();
        assert!(elapsed > 25_750, "締切より前に切られた: {elapsed}ms");
        assert!(elapsed <= 25_850, "ポーリング1周を超えて遅れた: {elapsed}ms");
    }
}
```

- [ ] **Step 2: 落ちることを確かめる**

Run: `cargo test --package server session::tests`
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
use std::sync::Arc;
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
/// なる。半荘のイベントは千数百件しかないので memory は問題にならない。
pub type Outbound = mpsc::UnboundedSender<ClientEventEnvelope>;
pub type Inbound = mpsc::UnboundedReceiver<ClientEventEnvelope>;

/// 卓が既に畳まれている。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gone;

/// 席への接続1本を指す。
///
/// **同じ席に2本目が来たら1本目は無効になる。**この ID がないと、
/// 置き換えられた古い接続の切断が、新しい接続を巻き添えにする。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConnectionId(pub u64);

pub enum TableMsg {
    Command {
        seat: Seat,
        command: Command,
        /// **入口へ着いた時刻。**Actor が取り出した時刻ではない。
        at_ms: u64,
        reply: oneshot::Sender<Result<(), Reject>>,
    },
    /// 接続または再接続。`last_seq` より後を送り直してから生配信へ移る。
    Attach {
        seat: Seat,
        last_seq: Option<u32>,
        out: Outbound,
        /// 配り終えてから返す。これが返れば inbox には既に入っている。
        ack: oneshot::Sender<ConnectionId>,
    },
    Detach {
        seat: Seat,
        connection: ConnectionId,
    },
}

#[derive(Clone)]
pub struct TableHandle {
    tx: mpsc::Sender<TableMsg>,
    /// **Actor と同じ時計。**別に start すると原点がずれ、刻印が狂う。
    clock: Arc<Clock>,
}

impl TableHandle {
    /// 席の操作を送る。**送る前に時刻を刻む。**
    pub async fn command(&self, seat: Seat, command: Command) -> Result<Result<(), Reject>, Gone> {
        let at_ms = self.clock.now_ms();
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(TableMsg::Command {
                seat,
                command,
                at_ms,
                reply,
            })
            .await
            .map_err(|_| Gone)?;
        answer.await.map_err(|_| Gone)
    }

    /// その席の配信を受け取る。
    ///
    /// **返ってきた時点で、送るべきものは既に受け口に入っている。**
    /// `last_seq` が卓の知らない連番なら、最初から送り直す。
    pub async fn attach(
        &self,
        seat: Seat,
        last_seq: Option<u32>,
    ) -> Result<(ConnectionId, Inbound), Gone> {
        let (out, inbox) = mpsc::unbounded_channel();
        let (ack, done) = oneshot::channel();
        self.tx
            .send(TableMsg::Attach {
                seat,
                last_seq,
                out,
                ack,
            })
            .await
            .map_err(|_| Gone)?;
        let connection = done.await.map_err(|_| Gone)?;
        Ok((connection, inbox))
    }

    /// その接続を外す。**既に別の接続へ置き換わっていれば何もしない。**
    pub async fn detach(&self, seat: Seat, connection: ConnectionId) -> Result<(), Gone> {
        self.tx
            .send(TableMsg::Detach { seat, connection })
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
    let clock = Arc::new(Clock::start());
    let table = Table::new(rules, occupants, clock.now_ms());
    tokio::spawn(run(table, seeds, Arc::clone(&clock), rx));
    TableHandle { tx, clock }
}

/// 席ごとの配信先と、どこまで送ったか。
struct Sinks {
    out: [Option<Outbound>; 4],
    sent_upto: [Option<u32>; 4],
    live: [Option<ConnectionId>; 4],
    next_id: u64,
}

impl Sinks {
    fn new() -> Self {
        Sinks {
            out: std::array::from_fn(|_| None),
            sent_upto: [None; 4],
            live: [None; 4],
            next_id: 1,
        }
    }

    /// クライアントの申告した `last_seq` を検める。
    ///
    /// **辻褄の合わない申告は信用せず、最初から送り直す。**存在しない連番を
    /// そのまま受け入れると、その席は卓が終わるまで一件も受け取れない。
    /// 見えている最大値へ丸める手もあるが、それだと文脈のないまま途中の
    /// イベントだけが届き、クライアントは組み立て直せない。
    fn checked(table: &Table, seat: Seat, last_seq: Option<u32>) -> Option<u32> {
        let high = table.since(seat, None).last().map(|e| e.seq);
        match (last_seq, high) {
            (Some(claimed), Some(highest)) if claimed <= highest => Some(claimed),
            (Some(_), _) => None,
            (None, _) => None,
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
                self.live[index] = None;
            }
        }
    }
}

async fn run(
    mut table: Table,
    mut seeds: SeedSource,
    clock: Arc<Clock>,
    mut rx: mpsc::Receiver<TableMsg>,
) {
    let mut sinks = Sinks::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(POLL_MS));
    // 遅れを取り返す意味はない。知りたいのは「いまの時刻」だけ。
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        // 出来ている分を先に配る。**局が終わったことを、次局を立てる前に伝える。**
        sinks.flush(&table);
        if table.is_over() {
            break;
        }
        if table.needs_seed() {
            table.begin_round(&seeds.next_seed(), clock.now_ms());
            // 次局の頭は待たせない。ここで配らないと最大 100ms 遅れる。
            sinks.flush(&table);
        }

        tokio::select! {
            message = rx.recv() => match message {
                None => break,
                Some(TableMsg::Command { seat, command, at_ms, reply }) => {
                    // **入口へ着いた時刻で判定する。**取り出した時刻を使うと、
                    // 締切前に押した操作が時間切れになる。
                    let result = table.apply(seat, command, at_ms);
                    let _ = reply.send(result);
                }
                Some(TableMsg::Attach { seat, last_seq, out, ack }) => {
                    let index = seat.index();
                    sinks.sent_upto[index] = Sinks::checked(&table, seat, last_seq);
                    sinks.out[index] = Some(out);
                    let connection = ConnectionId(sinks.next_id);
                    sinks.next_id += 1;
                    sinks.live[index] = Some(connection);
                    // 登録し、配り、それから返事する。この順序が
                    // 「attach が返れば届いている」を保証する。
                    sinks.flush(&table);
                    let _ = ack.send(connection);
                }
                Some(TableMsg::Detach { seat, connection }) => {
                    // 置き換えられた古い接続の切断が、新しい接続を切ってはならない。
                    if sinks.live[seat.index()] == Some(connection) {
                        sinks.out[seat.index()] = None;
                        sinks.live[seat.index()] = None;
                    }
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

- [ ] **Step 5: 通ることを確かめる**

Run: `cargo test --package server session::tests`
Expected: 10 passed

`a_silent_seat_is_made_to_discard_when_its_bank_runs_out` が落ちる場合、時計が仮想化されていない疑いが濃い。`session_time.rs` に `std::time` が紛れ込んでいないか確認する。

- [ ] **Step 6: crate 全体を測る**

Run: `cargo test --package server`
期待: 45 件（table 28 + session_time 7 + session 10）

- [ ] **Step 7: 検査してコミット**

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt
git add crates/server/src/session.rs
git commit -m "feat(server): 卓 Actor の骨格と席ごとの配信

1卓 = 1 tokio task。100ms ごとに tick し、局の切れ目では自分でシードを作る。

コマンドは入口へ着いた時刻で判定する。Actor が取り出した時刻を使うと、
締切前に押した操作が select! の選択順や待ち行列の混み具合で
時間切れになる。エンジンは時刻の巻き戻しを受けつける。

attach は登録し配ってから返事する。yield_now を何回呼べば届くかを
当てにするテストは書かない。**スケジューラの都合に依存するテストは
いつか必ず落ちる。**

配信は since と水位だけで行い、drain_for を使わない。"
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
    use protocol::client_event::ClientEvent;
    use protocol::event::PlayerId;

    fn all_cpu() -> [Occupant; 4] {
        std::array::from_fn(|i| Occupant::Cpu(PlayerId(format!("cpu{i}"))))
    }

    /// 仮想時間でこれを超えたら、卓が終わらない不具合とみなす。
    const A_VIRTUAL_HOUR_MS: u64 = 3_600_000;

    #[tokio::test(start_paused = true)]
    async fn reattaching_from_a_sequence_skips_what_was_already_seen() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut first) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let seen = take_ready(&mut first);
        let last = seen.last().expect("何か届いている").seq;

        let (_, mut second) = handle
            .attach(Seat::new(0), Some(last))
            .await
            .expect("卓は生きている");

        for envelope in take_ready(&mut second) {
            assert!(envelope.seq > last, "見たものがまた来ている");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn reattaching_from_nothing_replays_the_whole_match() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut first) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let original = take_ready(&mut first);

        let (_, mut second) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
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
    async fn an_impossible_sequence_replays_from_the_beginning() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut inbox) = handle
            .attach(Seat::new(0), Some(u32::MAX))
            .await
            .expect("卓は生きている");

        let events = take_ready(&mut inbox);
        assert!(!events.is_empty(), "未来の連番で配信が永久に止まった");
        assert_eq!(events.first().map(|e| e.seq), Some(0), "先頭が seq 0 でない");
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::MatchStart { .. })));
    }

    #[tokio::test(start_paused = true)]
    async fn every_connection_gets_its_own_id() {
        let handle = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (first, _a) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let (second, _b) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let (third, _c) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");
        assert_ne!(first, second, "同じ席で ID が使い回されている");
        assert_ne!(second, third, "別の席と ID が衝突している");
    }

    #[tokio::test(start_paused = true)]
    async fn a_stale_detach_does_not_kill_the_new_connection() {
        let handle = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (old_id, mut old) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let seen = take_ready(&mut old).last().map(|e| e.seq);
        let (new_id, mut fresh) = handle
            .attach(Seat::new(0), seen)
            .await
            .expect("卓は生きている");
        assert_ne!(old_id, new_id);

        // 置き換えられた古い接続が、遅れて切断を申し出る。
        handle
            .detach(Seat::new(0), old_id)
            .await
            .expect("卓は生きている");
        drop(old);

        tokio::time::sleep(Duration::from_millis(30_000)).await;
        assert!(
            !take_ready(&mut fresh).is_empty(),
            "古い接続の切断が新しい接続を巻き添えにした"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_detached_seat_catches_up_when_it_comes_back() {
        let handle = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (id, mut inbox) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");
        let last = take_ready(&mut inbox)
            .last()
            .expect("何か届いている")
            .seq;

        handle
            .detach(Seat::new(1), id)
            .await
            .expect("卓は生きている");
        drop(inbox);

        // 席が居ないあいだも卓は進む。**親は席0の人間**なので、
        // その持ち時間が尽きて自動打牌されるまで待つ必要がある。
        tokio::time::sleep(Duration::from_millis(30_000)).await;

        let (_, mut back) = handle
            .attach(Seat::new(1), Some(last))
            .await
            .expect("卓は生きている");
        let caught_up = take_ready(&mut back);

        assert!(!caught_up.is_empty(), "留守中の分が追いついていない");
        assert!(caught_up.iter().all(|e| e.seq > last));
        for pair in caught_up.windows(2) {
            assert!(pair[0].seq < pair[1].seq);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_a_receiver_does_not_stop_the_table() {
        let handle = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (_, inbox) = handle
            .attach(Seat::new(2), None)
            .await
            .expect("卓は生きている");
        drop(inbox);

        tokio::time::sleep(Duration::from_millis(30_000)).await;

        let (_, mut other) = handle
            .attach(Seat::new(3), None)
            .await
            .expect("卓は生きている");
        assert!(!take_ready(&mut other).is_empty(), "卓が止まっている");
    }

    #[tokio::test(start_paused = true)]
    async fn four_cpus_play_a_whole_match_and_the_actor_shuts_down() {
        let handle = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let mut saw_match_end = false;
        let mut previous = None;
        loop {
            // 卓が終わらない不具合を、ハングでなく assertion で捕まえる。
            let next = tokio::time::timeout(Duration::from_millis(A_VIRTUAL_HOUR_MS), watcher.recv())
                .await
                .expect("仮想1時間のうちに終わる");
            let Some(envelope) = next else { break };
            if let Some(prev) = previous {
                assert!(envelope.seq > prev, "連番が戻った");
            }
            previous = Some(envelope.seq);
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
    async fn a_finished_table_refuses_new_connections() {
        let handle = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        loop {
            let next = tokio::time::timeout(Duration::from_millis(A_VIRTUAL_HOUR_MS), watcher.recv())
                .await
                .expect("仮想1時間のうちに終わる");
            if next.is_none() {
                break;
            }
        }
        for _ in 0..100 {
            if handle.is_closed() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            handle.attach(Seat::new(0), None).await.err(),
            Some(Gone),
            "終わった卓が接続を受けつけている"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn every_round_uses_a_fresh_seed() {
        let handle = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let mut commits = Vec::new();
        loop {
            let next = tokio::time::timeout(Duration::from_millis(A_VIRTUAL_HOUR_MS), watcher.recv())
                .await
                .expect("仮想1時間のうちに終わる");
            let Some(envelope) = next else { break };
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

- [ ] **Step 2: 落ちることを確かめる**

Run: `cargo test --package server reconnect_tests`
Expected: いくつか落ちる。Task 2 の実装が正しければ全部通る可能性もある。その場合は Step 3 を飛ばしてよい。

- [ ] **Step 3: 落ちたものを直す**

新しいコードは要らないはずである。落ちるなら原因は次のどれかである。いずれも **Task 2 のコードを直す。テストの期待値を動かさない。**

1. `flush` を `is_over` の判定より後に置いている → 終局のイベントが届かない
2. `Attach` で `Sinks::checked` を通していない → `u32::MAX` でその席が黙る
3. `Detach` を接続 ID で照合していない → 古い切断が新しい接続を殺す
4. `flush` の水位を束の最大 `seq` でなく件数などで進めている → 取りこぼし
5. 送信失敗で `out` と `live` を `None` にしていない → 落ちた接続へ永久に送り続ける
6. `ack` を `flush` より先に送っている → `attach` 直後の `take_ready` が空になる

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test --package server reconnect_tests`
Expected: 10 passed

- [ ] **Step 5: crate と workspace 全体を測る**

```bash
cargo test --package server     # 55 件（table 28 + session_time 7 + session 10 + reconnect 10）
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
Expected: 失敗ゼロ

- [ ] **Step 6: 禁じ手が入っていないか自分で確かめる**

```bash
# std の時計を使っていないか
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

attach は last_seq より後だけを送り直す。辻褄の合わない連番を
申告されたら信用せず最初から送り直す。丸めるとクライアントは
文脈のないまま途中から受け取ることになり、組み立て直せない。

detach は接続 ID で照合する。置き換えられた古い接続の切断が、
新しい接続を巻き添えにしてはならない。

半荘が終わると Actor が終了し、ハンドルの送り口が閉じる。
**4人 CPU の卓が Actor の上で最後まで回る。**"
```

---

## Self-Review

**仕様の網羅:** 仕様 3.2 節「server — 唯一 I/O と時間を持つ層。1卓 = 1 tokio task の Actor とし、卓同士を完全に独立させる」を Task 2 が満たす。8.1 節「再接続 = `seq` 以降のイベント再送」を Task 3 が満たす。6.2 節の時間モデルは、コマンドを入口の到着時刻で判定することと、自動打牌が 25,750ms を厳密に超えることの2点で検証している。シード開示（`SeedReveal`）を出すのはエンジン側の責務なので、このウェーブでは Actor が素通しするだけでよい。

**このウェーブがやらないこと:** WebSocket・axum・HTTP・認証は Wave 3d。永続化とマッチングも Wave 3d 以降。ここは「卓が実時間で動く」ところまでで止める。**非同期とネットワークを一度に入れると、失敗したときどちらが原因か切り分けられない。**

**Wave 3d への引き継ぎで未決なこと:** `Gone` は理由を持たない。WebSocket 層が「認証失敗」「卓が無い」「不正な resume」を区別してクライアントへ返したくなったら、`Gone` を理由付きの enum へ広げる必要がある。いま広げないのは、理由の一覧を決めるのが認証設計の一部であり、それが Wave 3d の仕事だからである。`ConnectionId` と ack はその拡張に耐える形にしてある。

**型の整合:** `Outbound` / `Inbound` / `Gone` / `ConnectionId` / `TableMsg` / `TableHandle` / `spawn` / `Clock` / `SeedSource` はすべて Task 1・2 で定義し、Task 3 は新しい型を足さない。`Reject` は `mahjong_engine::match_flow::Reject`（`PartialEq` と `Debug` を導出済み、`assert_eq!` で使える）。

**API の実在確認:** `rand::rng()`・`RngCore::fill_bytes`・`StdRng::from_seed`・`tokio::time::interval`・`MissedTickBehavior::Skip`・`#[tokio::test(start_paused = true)]`・`Seed::new([u8; 32])`・`Seed::from_hex`・`Seed::to_hex()`・`Table::since(seat, Option<u32>)`・`Table::needs_seed()`・`Table::is_over()`・`Table::begin_round(&Seed, u64)`・`Table::round_state()`・`Ruleset::kin_no_ma(MatchLength::Hanchan)`・`Seat::new(u8)`・`Seat::index()` はすべて実際にコンパイルして確認した。
