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

**この計画に載っているコードと期待値は、実際にコンパイルして実行し、38件すべて通ることを確認済みである。**凍結済みの `Table` を外から使う試作クレートで、Actor をこの計画のとおりに組み、下を実測した。`cargo clippy --all-targets -- -D warnings` も通る。

| 実測した前提 | 結果 |
|---|---|
| `attach` の ack が返った時点で配信済み | MatchStart・RoundStart・Deal・Draw の5件が `yield` 無しで inbox にある |
| 配牌の枚数 | **親も子も 13 枚。**親の14枚目は別の `Draw { tile: Some(_) }` |
| 無言の席が自動打牌されるまで | 仮想 **25,800ms**（式は 5,000 + 20,000 + 500 + lead_in 250 = 25,750、その次のポーリング） |
| 4人 CPU の半荘 | 1,304 イベント / 9局 / 全部別シード / `MatchEnd` あり / 実時間 2.7 秒 |
| 締切を越えた `tick` のあとの、締切前の刻印 | **`Err(NotYourTurn)`。**刻印だけでは足りず、`tick` の前にキューを空にする必要がある |
| ある席に見えない連番 | 151 件中 41 個。1つ申告すると**未受信の可視イベント 60 件が飛ぶ** |
| 最初の鳴きの要求（シード `[1u8; 32]`・席1） | 仮想 **55,200ms**、`Chi` と `Pon`、残り 7,650ms |
| 同じミリ秒にポンで応じたときの成立 | **55,600ms（+400ms）。**最低待機の終わり 55,550ms の次のポーリング |
| 古い接続の `Detach` | 接続 ID で照合すれば新しい接続を切らない |
| `TableHandle` | `Send + Sync + Clone + 'static` |
| 打牌から次のツモまで | **30回すべてちょうど 400ms。**誰も鳴けなくても一律に待っている |
| 1席あたりの可視イベント（半荘1回） | 1,304 / 1,875 / 1,677 件（シード3種）。ばらつき4割 |
| 5秒切断したあとの要求の再送 | 25,750ms → **20,750ms。**残り時間が引き直される |
| 2席以上へ提示されるウィンドウ | 半荘あたり 8〜10 回ある。同じミリ秒の2席の応答は両方受理される |

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

そこで `TableHandle::command`（到着時刻を受け取る）, `TableHandle::now_ms` が**入口の席を取ってから**時刻を刻み、Actor はその刻印を `Table::apply` へ渡す。`reserve()` より前に刻んではならない。入口が満杯のあいだ古い刻印を抱えたまま待つことになり、**入口を埋めるだけで締切を伸ばせてしまう。**刻印は Actor が処理する時刻より必ず古いが、エンジンは時刻の巻き戻しを受けつける（実測済み）。締切は絶対時刻なので、古い時刻を渡すことは「まだ切れていない」を意味し、これがまさに求める挙動である。

**しかし刻印だけでは足りない。**`select!` が ticker の枝を先に選び、その `tick` が締切を越えて自動打牌を済ませてしまうと、あとから締切前の刻印で `apply` しても局面は巻き戻らない。実測すると `Err(NotYourTurn)` になる。

そこで **`tick` を呼ぶ前に、入口に既に着いている分をすべて処理する。**`ticker.tick()` の枝で `rx.try_recv()` を空になるまで回してから `table.tick` を呼ぶ。これで「tick が起きた時点でキューにあったコマンドは、必ずその tick より先に適用される」が保証される。

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

**「最大値以下かどうか」で検めてはならない。**`Table::since` は全体のログを連番で絞ってから席ごとに投影するので、その席に見えない連番が正常に飛び飛びで存在する。`RequestAction` は当該席以外へ投影されないのがその代表である。実測すると、ある席に見えるのは 151 件中 110 件で、見えない連番が 41 個あった。そのうちの1つを申告すると、**まだ一度も見ていない可視イベント 60 件が飛ぶ。**

だから検めるのは「その席の可視列にその連番が実在するか」でなければならない。

**これが保証するのは「構造的にありえない申告を弾くこと」までである。**その接続が実際にそこまで受け取ったことの証明にはならない。可視列に実在する未受信の連番を申告されれば受理してしまう。嘘の申告で損をするのは申告した本人だけなので v1 はこれで足りるが、**「不正の拒否」ではなく「ありえない値の安全な受け止め」だと理解すること。**署名付きの再開トークンにサーバ側の水位を結びつけるのは Wave 3d の認証設計の一部である。

### 到着時刻は呼び出し側が測る

`command` は `at_ms` を受け取る。**卓の中で刻んではならない。**入口の待ち行列に空きが出た時刻を使うと、他席や再接続の大量投入で枠が埋まっているあいだ、締切前にサーバへ届いた正当な操作まで遅刻扱いになる。他人の混雑が自分の締切を削ってはならない。

Wave 3d は WebSocket の枠を読んだ直後に `TableHandle::now_ms()` を呼び、それを `command` へ渡す。**クライアントが時刻を指定するわけではない。**測るのは常にサーバ側である。

`at_ms` は**サーバ内部だけが渡す。**未来の時刻や別の卓の時計の値を渡してはならない。古い時刻を渡すとバンクの消費まで巻き戻る（`timing.rs` の `charge_bank` は `saturating_sub` で引く）ので、信頼できる呼び出し側に限る。再送のときも刻印を取り直さない。**取り直すと、通信が詰まったぶんだけ本人の持ち時間が削られる。**

**それでも入口が満杯なら、外で待つコマンドは tick に追い越される。**そこは救えない。救えるように見せかけないこと。4人が1手ずつ出す卓で入口に同時に並ぶのは高々4〜5件であり、`INBOX = 32` はその6倍以上ある。ここが埋まるのは濫用のときだけなので、**接続ごとの流量制限で埋めさせない**のが正しい対処であり、それは Wave 3d の仕事である。

### 再送する要求は残り時間を引き直す

`ClientEvent::RequestAction` の `deadline_ms` は**発行時点からの残り時間**であって絶対時刻ではない。切断から数秒後に同じ包みをそのまま送り直すと、クライアントは満額の持ち時間で表示するのに、サーバは元の絶対締切で判定する。**表示と判定が食い違う。**

`protocol` は凍結されているので型は変えられない。そこで Actor が、要求を初めて見た時刻に絶対締切を控えておき、送るたびに「いまの残り」へ引き直す。控えるのは**配信先が無い席でも行う。**留守中に出た要求を、戻ってきたときに満額で見せてしまわないためである。

実測では、初回 25,750ms の要求が、5秒切断したあとの再送で 20,750ms になった。

### 出口は有界にし、溢れたら切る

出口（Actor → 接続）で待つと、1人の遅い接続が Actor ごと止め、他の3人まで巻き添えになる。かといって無制限にすると、遅い接続の数だけ memory が伸びる。

そこで**有界にしたうえで、Actor は決して待たない。**`try_send` で押し込み、溢れたらその接続を切る。切られた側は `attach(seat, last_seq)` で追いつける。**遅い接続への対処は、既に持っている再接続の経路そのものである。**新しい仕組みを足す必要がない。

容量は**追いつきぶんの件数を見てから決める。**受け口を作るのは卓であり、`attach` の時点で `since` の件数が分かっているので、`backlog + OUTBOX` を容量にする。これで**正当な再接続が容量不足で溢れることが構造的に起きない。**「実測値に余裕を掛けた定数」では、長い半荘や将来のルール値で破れる。

`OUTBOX` は生配信ぶんの余裕である。実測では1席あたりの可視イベントが 1,304 / 1,875 / 1,677 件（シード3種）だったので 8,192 とする。tokio の mpsc は容量ぶんを先に確保しないので、健全な接続ではこの数を大きくしても費用はかからない。

入口（接続 → Actor）も bounded にして、壊れたクライアントが Actor を溺れさせないようにする。

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
  - `pub type Outbound = tokio::sync::mpsc::Sender<ClientEventEnvelope>`（有界）
  - `pub type Inbound = tokio::sync::mpsc::Receiver<ClientEventEnvelope>`
  - `pub trait Seeds { fn next_seed(&mut self) -> impl Future<Output = Seed> + Send; }`（`SeedSource` が実装する）
  - `pub struct Gone;`
  - `pub struct ConnectionId(u64);`（中身は非公開）
  - `pub enum TableMsg { Command { seat, command, at_ms, reply }, Attach { seat, last_seq, ack }, Detach { seat, connection } }`
  - `pub struct TableHandle`（`Clone + Send + Sync + 'static`）, `TableHandle::command`, `TableHandle::attach`, `TableHandle::detach`, `TableHandle::is_closed`
  - `pub fn spawn<S: Seeds>(rules: Ruleset, occupants: [Occupant; 4], seeds: S) -> (TableHandle, tokio::task::JoinHandle<()>)`

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
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
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
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
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
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
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
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
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
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
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
            .command(
                Seat::new(0),
                Command::Discard { tile, riichi: false },
                handle.now_ms(),
            )
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
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut south) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        let tile = dealt_hand(&take_ready(&mut south))[0];
        let rejected = handle
            .command(
                Seat::new(1),
                Command::Discard { tile, riichi: false },
                handle.now_ms(),
            )
            .await
            .expect("卓は生きている");
        assert_eq!(rejected, Err(Reject::NotYourTurn), "親でない席が打てている");
    }

    #[tokio::test(start_paused = true)]
    async fn sequence_numbers_only_go_up() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
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

    /// **刻印だけでは足りないことの証明。**
    ///
    /// 締切前に処理すれば通る。締切を越えた `tick` のあとでは、同じ刻印でも
    /// 通らない。だから Actor は `tick` の前にキューを空にしなければならない。
    #[test]
    fn an_expired_tick_cannot_be_undone() {
        let seed = Seed::from_hex(&"01".repeat(32)).expect("正しい hex");

        let mut early = Table::new(rules(), one_human_three_cpus(), 0);
        early.begin_round(&seed, 0);
        let tile = early
            .round_state()
            .expect("局が動いている")
            .seat(Seat::new(0))
            .hand[0];
        early.tick(25_700);
        assert_eq!(
            early.apply(
                Seat::new(0),
                Command::Discard { tile, riichi: false },
                25_700
            ),
            Ok(()),
            "締切前に処理すれば通る"
        );

        let mut late = Table::new(rules(), one_human_three_cpus(), 0);
        late.begin_round(&seed, 0);
        let tile = late
            .round_state()
            .expect("局が動いている")
            .seat(Seat::new(0))
            .hand[0];
        late.tick(25_800);
        assert_eq!(
            late.apply(
                Seat::new(0),
                Command::Discard { tile, riichi: false },
                25_700
            ),
            Err(Reject::NotYourTurn),
            "締切を越えた tick は巻き戻らない"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_seat_is_made_to_discard_when_its_bank_runs_out() {
        let (handle, _actor) = spawn(
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
use mahjong_engine::wall::Seed;
use protocol::client_event::{ClientEvent, ClientEventEnvelope};
use protocol::command::Command;
use protocol::ruleset::Ruleset;
use protocol::seat::Seat;
use std::collections::HashMap;
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

/// 入口の容量。**`tick` の前に片づける件数の上限でもある。**
///
/// 空になるまで回すと、受信で空いたスロットへ待機中の送信者が補充する
/// ので、コマンドが途切れないかぎり `tick` が永久に来ない。
const INBOX: usize = 32;

/// 1接続ぶんの出口の余裕。
///
/// 追いつきぶんはこれとは別に確保するので、ここは生配信のための余裕
/// である。実測では1席あたりの可視イベントが 1,304 / 1,875 / 1,677 件
/// （シード3種）だった。tokio の mpsc は容量ぶんを先に確保しないので、
/// 健全な接続ではこの数を大きくしても費用はかからない。
const OUTBOX: usize = 8_192;

/// 局のシードを配る。
///
/// **Wave 3d はここで永続化してから返す。**局頭で配った `seed_commit` と、
/// あとで開示するシードが食い違うと、プレイヤーが検算したときに
/// **サーバが山を操作したように見える。**不正の疑いに答えるための仕組みが、
/// 逆に不正の証拠を作ってしまう（仕様 8.3）。
///
/// だから「シードを作る」と「局を始める」のあいだに待てる形にしておく。
/// Wave 3c の `SeedSource` は待たずに返すが、契約は同じである。
pub trait Seeds: Send + 'static {
    fn next_seed(&mut self) -> impl std::future::Future<Output = Seed> + Send;
}

impl Seeds for SeedSource {
    async fn next_seed(&mut self) -> Seed {
        SeedSource::next_seed(self)
    }
}

/// 接続へイベントを押し出す口。
///
/// **有界だが、Actor は決して待たない。**`try_send` で押し込み、溢れたら
/// その接続を切る。切られた側は `attach(seat, last_seq)` で追いつける。
/// **遅い接続への対処は、既に持っている再接続の経路そのものである。**
pub type Outbound = mpsc::Sender<ClientEventEnvelope>;
pub type Inbound = mpsc::Receiver<ClientEventEnvelope>;

/// 卓が既に畳まれている。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gone;

/// 席への接続1本を指す。
///
/// **同じ席に2本目が来たら1本目は無効になる。**この ID がないと、
/// 置き換えられた古い接続の切断が、新しい接続を巻き添えにする。
///
/// **中身は非公開。**外から作れないので、Wave 3d で他人の接続 ID を
/// 騙ることができない。卓が配ったものを返すことしかできない。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConnectionId(u64);

pub enum TableMsg {
    Command {
        seat: Seat,
        command: Command,
        at_ms: u64,
        reply: oneshot::Sender<Result<(), Reject>>,
    },
    Attach {
        seat: Seat,
        last_seq: Option<u32>,
        ack: oneshot::Sender<(ConnectionId, Inbound)>,
    },
    Detach {
        seat: Seat,
        connection: ConnectionId,
    },
}

#[derive(Clone)]
pub struct TableHandle {
    tx: mpsc::Sender<TableMsg>,
    clock: Arc<Clock>,
}

impl TableHandle {
    /// 卓が生まれてからの経過ミリ秒。
    ///
    /// **Wave 3d は WebSocket の枠を読んだ直後にこれを呼び、`command` へ渡す。**
    /// 入口の待ち行列に並ぶ前の時刻でなければ、他席の混雑が締切判定に混ざる。
    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    /// 席の操作を送る。`at_ms` は**サーバへ届いた時刻**であり、呼び出し側が
    /// 測る。クライアントが指定するものではない。
    pub async fn command(
        &self,
        seat: Seat,
        command: Command,
        at_ms: u64,
    ) -> Result<Result<(), Reject>, Gone> {
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
    /// **受け口は卓が作る。**追いつきぶんの件数を見てから容量を決めるので、
    /// 正当な再接続が容量不足で溢れることが構造的に起きない。
    pub async fn attach(
        &self,
        seat: Seat,
        last_seq: Option<u32>,
    ) -> Result<(ConnectionId, Inbound), Gone> {
        let (ack, done) = oneshot::channel();
        self.tx
            .send(TableMsg::Attach {
                seat,
                last_seq,
                ack,
            })
            .await
            .map_err(|_| Gone)?;
        done.await.map_err(|_| Gone)
    }

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
/// 卓を立ち上げる。
///
/// **`JoinHandle` を返す。**捨てると panic と正常終局が区別できず、どちらも
/// `Gone` に潰れる。Wave 3d の卓の台帳が障害を記録し、局頭から再開するかを
/// 判断するには終了理由が要る（仕様 8.3）。
pub fn spawn<S: Seeds>(
    rules: Ruleset,
    occupants: [Occupant; 4],
    seeds: S,
) -> (TableHandle, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(INBOX);
    let clock = Arc::new(Clock::start());
    let table = Table::new(rules, occupants, clock.now_ms());
    let actor = tokio::spawn(run(table, seeds, Arc::clone(&clock), rx));
    (TableHandle { tx, clock }, actor)
}

struct Sinks {
    out: [Option<Outbound>; 4],
    sent_upto: [Option<u32>; 4],
    live: [Option<ConnectionId>; 4],
    next_id: u64,
    /// 締切を控えたところまで。配信先の有無に関わらず進む。
    noted_upto: [Option<u32>; 4],
    /// `RequestAction` の**絶対**締切。再送のとき残り時間を引き直す。
    ///
    /// **消せない。**`attach(seat, None)` は対局の頭から送り直すので、
    /// 遠い過去の要求も「残り0」に引き直す必要がある。伸び方は卓のログと
    /// 同じで、延長戦を含めても半荘1回ぶんに収まる。卓が畳まれれば消える。
    deadlines: HashMap<u32, u64>,
}

impl Sinks {
    fn new() -> Self {
        Sinks {
            out: std::array::from_fn(|_| None),
            sent_upto: [None; 4],
            live: [None; 4],
            next_id: 1,
            noted_upto: [None; 4],
            deadlines: HashMap::new(),
        }
    }

    fn checked(table: &Table, seat: Seat, last_seq: Option<u32>) -> Option<u32> {
        let claimed = last_seq?;
        table
            .since(seat, None)
            .iter()
            .any(|envelope| envelope.seq == claimed)
            .then_some(claimed)
    }

    /// 新しく出た `RequestAction` の絶対締切を控える。
    ///
    /// **`engine_now_ms` は、そのイベントを生んだ呼び出しへ渡した時刻**で
    /// なければならない。`deadline_ms` はその時刻からの残りなので、Actor が
    /// 別に測り直した時刻を足すと、待ち行列の滞留ぶんだけ絶対締切が
    /// 後ろへずれる。
    ///
    /// **配信先が無い席でも控える。**留守中に出た要求を、戻ってきたときに
    /// 満額の残り時間で見せてしまわないため。
    fn note_deadlines(&mut self, table: &Table, engine_now_ms: u64) {
        for index in 0..4 {
            let seat = Seat::new(index as u8);
            let fresh = table.since(seat, self.noted_upto[index]);
            for envelope in &fresh {
                if let ClientEvent::RequestAction { deadline_ms, .. } = &envelope.event {
                    self.deadlines
                        .entry(envelope.seq)
                        .or_insert(engine_now_ms + u64::from(*deadline_ms));
                }
            }
            if let Some(last) = fresh.last() {
                self.noted_upto[index] = Some(last.seq);
            }
        }
    }

    /// 再送のときに残り時間を引き直す。
    ///
    /// `deadline_ms` は**発行時点からの残り**であって絶対時刻ではない。
    /// 切断から数秒後に同じ包みをそのまま送り直すと、とっくに過ぎた要求が
    /// 満額の持ち時間で表示され、サーバの判定と食い違う。
    fn retimed(&self, mut envelope: ClientEventEnvelope, now_ms: u64) -> ClientEventEnvelope {
        if let ClientEvent::RequestAction { deadline_ms, .. } = &mut envelope.event {
            if let Some(absolute) = self.deadlines.get(&envelope.seq) {
                *deadline_ms = absolute.saturating_sub(now_ms).min(u64::from(u32::MAX)) as u32;
            }
        }
        envelope
    }

    fn flush(&mut self, table: &Table, now_ms: u64) {
        for index in 0..4 {
            if self.out[index].is_none() {
                continue;
            }
            let seat = Seat::new(index as u8);
            let batch = table.since(seat, self.sent_upto[index]);
            let mut highest = self.sent_upto[index];
            let mut alive = true;
            for envelope in batch {
                let seq = envelope.seq;
                let envelope = self.retimed(envelope, now_ms);
                let out = self.out[index].as_ref().expect("直前に確かめた");
                if out.try_send(envelope).is_err() {
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

fn handle(table: &mut Table, sinks: &mut Sinks, message: TableMsg, flush_now_ms: u64) {
    match message {
        TableMsg::Command {
            seat,
            command,
            at_ms,
            reply,
        } => {
            let result = table.apply(seat, command, at_ms);
            // **渡した時刻で控える。**この apply が生んだ要求の残り時間は
            // at_ms からの相対値である。
            sinks.note_deadlines(table, at_ms);
            let _ = reply.send(result);
        }
        TableMsg::Attach {
            seat,
            last_seq,
            ack,
        } => {
            let index = seat.index();
            let start = Sinks::checked(table, seat, last_seq);
            // **追いつきぶんが必ず入る容量にする。**足りないと正当な再接続が
            // 溢れて切られる。OUTBOX は生配信ぶんの余裕として上乗せする。
            let backlog = table.since(seat, start).len();
            let (out, inbox) = mpsc::channel(backlog + OUTBOX);
            sinks.sent_upto[index] = start;
            sinks.out[index] = Some(out);
            let connection = ConnectionId(sinks.next_id);
            sinks.next_id += 1;
            sinks.live[index] = Some(connection);
            sinks.flush(table, flush_now_ms);
            let _ = ack.send((connection, inbox));
        }
        TableMsg::Detach { seat, connection } => {
            if sinks.live[seat.index()] == Some(connection) {
                sinks.out[seat.index()] = None;
                sinks.live[seat.index()] = None;
            }
        }
    }
}

async fn run<S: Seeds>(
    mut table: Table,
    mut seeds: S,
    clock: Arc<Clock>,
    mut rx: mpsc::Receiver<TableMsg>,
) {
    let mut sinks = Sinks::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(POLL_MS));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        let now = clock.now_ms();
        sinks.flush(&table, now);
        if table.is_over() {
            break;
        }
        if table.needs_seed() {
            // **シードを受け取ってから局を始める。**Wave 3d はこの `await` の
            // 中で永続化する。ここで待てないと、`seed_commit` を配ったあとに
            // 落ちて別のシードで再開する経路ができてしまう。
            let seed = seeds.next_seed().await;
            let now = clock.now_ms();
            table.begin_round(&seed, now);
            sinks.note_deadlines(&table, now);
            sinks.flush(&table, now);
        }

        tokio::select! {
            biased;

            _ = ticker.tick() => {
                // **時計を進める前に、既に着いている分を片づける。**
                // 刻印だけでは足りない。tick が締切を越えて自動打牌したあとでは、
                // 締切前の刻印で apply しても局面は巻き戻らない。
                //
                // **上限は入口の容量と同じにする。**空になるまで回すと、受信で
                // 空いたスロットへ待機中の送信者が補充するので、コマンドが
                // 途切れないかぎり tick が永久に来ない。保証するのは
                // 「**この tick を選んだ時点で入口にあった分**は、時計を進める
                // 前に片づける」ところまでである。
                let now = clock.now_ms();
                for _ in 0..INBOX {
                    match rx.try_recv() {
                        Ok(message) => handle(&mut table, &mut sinks, message, now),
                        Err(_) => break,
                    }
                }
                let ticked_at = clock.now_ms();
                table.tick(ticked_at);
                sinks.note_deadlines(&table, ticked_at);
            }
            message = rx.recv() => match message {
                None => break,
                Some(message) => {
                    let now = clock.now_ms();
                    handle(&mut table, &mut sinks, message, now);
                }
            },
        }
    }

    let now = clock.now_ms();
    sinks.flush(&table, now);
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

    /// **正直な申告は必ず受理される。**
    ///
    /// 卓を進めてから繋ぎ直すので、受け取る束が空にならない。空のまま
    /// 「戻ってきたものは無かった」で通ると、`checked` が常に最初から
    /// 送り直していても気づけない。
    #[tokio::test(start_paused = true)]
    async fn reattaching_from_a_sequence_skips_what_was_already_seen() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (_, mut first) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        let seen = take_ready(&mut first);
        let last = seen.last().expect("何か届いている").seq;

        // 親は席0の人間。その持ち時間が尽きて卓が進むまで待つ。
        tokio::time::sleep(Duration::from_millis(30_000)).await;

        let (_, mut second) = handle
            .attach(Seat::new(1), Some(last))
            .await
            .expect("卓は生きている");

        let caught_up = take_ready(&mut second);
        assert!(!caught_up.is_empty(), "正直な申告が受理されていない");
        assert_ne!(
            caught_up.first().map(|e| e.seq),
            Some(0),
            "正直な申告なのに最初から送り直している"
        );
        for envelope in &caught_up {
            assert!(envelope.seq > last, "見たものがまた来ている");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn reattaching_from_nothing_replays_the_whole_match() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
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
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
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

    /// **その席に投影されなかった連番を申告されたら、最初から送り直す。**
    ///
    /// 全体のログを連番で絞ってから席ごとに投影するので、その席に見えない
    /// 連番が正常に飛び飛びで存在する。`RequestAction` は当該席以外へ
    /// 投影されないのが代表例。それを申告されて受理すると、まだ一度も
    /// 見ていない可視イベントが飛ぶ。
    #[tokio::test(start_paused = true)]
    async fn a_sequence_hidden_from_this_seat_is_refused() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let (_, mut inbox) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        let mut visible: Vec<u32> = Vec::new();
        while visible.len() < 40 {
            let next = tokio::time::timeout(Duration::from_millis(60_000), inbox.recv())
                .await
                .expect("仮想60秒のうちに届く");
            let Some(envelope) = next else { break };
            visible.push(envelope.seq);
        }
        let highest = *visible.last().expect("何か届いている");
        let hidden: Vec<u32> = (0..highest).filter(|s| !visible.contains(s)).collect();
        assert!(!hidden.is_empty(), "見えない連番が無いと検証にならない");

        let claim = hidden[hidden.len() / 2];
        let (_, mut liar) = handle
            .attach(Seat::new(1), Some(claim))
            .await
            .expect("卓は生きている");
        assert_eq!(
            take_ready(&mut liar).first().map(|e| e.seq),
            Some(0),
            "自席に見えない連番を受理して途中から送っている"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn every_connection_gets_its_own_id() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
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
        let (handle, _actor) = spawn(
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
        let (handle, _actor) = spawn(
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
        let (handle, _actor) = spawn(
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
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
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

    /// **何度繋ぎ直しても、残り時間は同じ一本の絶対締切を指す。**
    ///
    /// 絶対締切を Actor が測り直した時刻から作ると、繋ぎ直すたびに
    /// 指す先がずれる。控えるのは「エンジンへ渡した時刻」でなければ
    /// ならない。実測では4回の再送がすべて 25,750ms を指した。
    #[tokio::test(start_paused = true)]
    async fn the_remaining_time_shrinks_exactly_with_the_clock() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let seq = take_ready(&mut inbox)
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::RequestAction { .. } => Some(e.seq),
                _ => None,
            })
            .expect("要求が届いている");

        let mut readings: Vec<(u64, u32)> = Vec::new();
        for _ in 0..4 {
            let (_, mut fresh) = handle
                .attach(Seat::new(0), None)
                .await
                .expect("卓は生きている");
            let now = handle.now_ms();
            let value = take_ready(&mut fresh)
                .iter()
                .find_map(|e| match &e.event {
                    ClientEvent::RequestAction { deadline_ms, .. } if e.seq == seq => {
                        Some(*deadline_ms)
                    }
                    _ => None,
                })
                .expect("同じ要求が再送される");
            readings.push((now, value));
            drop(fresh);
            tokio::time::sleep(Duration::from_millis(3_000)).await;
        }

        let absolute = readings[0].0 + u64::from(readings[0].1);
        for (at, value) in &readings {
            assert_eq!(
                at + u64::from(*value),
                absolute,
                "時刻 {at} の再送が別の絶対締切を指している"
            );
        }
    }

    /// **締切を過ぎた要求は、残り0で再送される。**
    ///
    /// `attach(seat, None)` は対局の頭から送り直すので、とっくに過ぎた
    /// 要求も混ざる。満額の残り時間で送ると、クライアントは終わった
    /// ウィンドウのタイマーを回し始める。
    #[tokio::test(start_paused = true)]
    async fn an_expired_request_is_resent_with_no_time_left() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let seq = take_ready(&mut inbox)
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::RequestAction { .. } => Some(e.seq),
                _ => None,
            })
            .expect("要求が届いている");

        // 締切（5,000 + 20,000 + 500 + 250 = 25,750ms）を大きく越える。
        tokio::time::sleep(Duration::from_millis(40_000)).await;

        let (_, mut back) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        assert_eq!(
            take_ready(&mut back).iter().find_map(|e| match &e.event {
                ClientEvent::RequestAction { deadline_ms, .. } if e.seq == seq => Some(*deadline_ms),
                _ => None,
            }),
            Some(0),
            "過ぎた要求が残り時間を持ったまま再送されている"
        );
    }

    /// **最新の連番を申告したら、何も返らない。**
    ///
    /// `>` と `>=` を取り違えると、直前に見たものがもう一度届く。
    #[tokio::test(start_paused = true)]
    async fn reattaching_from_the_latest_sequence_sends_nothing() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let latest = take_ready(&mut inbox)
            .last()
            .expect("何か届いている")
            .seq;

        let (_, mut again) = handle
            .attach(Seat::new(0), Some(latest))
            .await
            .expect("卓は生きている");
        assert!(
            take_ready(&mut again).is_empty(),
            "最新の連番を申告したのに送り直された"
        );
    }

    /// **一度も読まない接続が切られない。**
    ///
    /// 受け口の容量は追いつきぶんを見てから決めるので、半荘1回ぶんが
    /// たまっても溢れない。溢れて切られると、生きている接続が黙る。
    #[tokio::test(start_paused = true)]
    async fn a_seat_that_never_reads_still_gets_the_whole_match() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([2u8; 32]));
        let (_, mut idle) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");
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
        let piled = take_ready(&mut idle);
        assert!(piled.len() > 500, "溢れて切られている: {} 件", piled.len());
    }

    /// **再送する要求は、いま本当に残っている時間を載せる。**
    ///
    /// `deadline_ms` は発行時点からの残りなので、そのまま送り直すと
    /// クライアントは満額の持ち時間で表示し、サーバは元の絶対締切で
    /// 判定する。実測では初回 25,750ms が5秒後の再送で 20,750ms になる。
    #[tokio::test(start_paused = true)]
    async fn a_resent_request_shows_the_time_that_is_actually_left() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (id, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let (seq, original) = take_ready(&mut inbox)
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::RequestAction { deadline_ms, .. } => Some((e.seq, *deadline_ms)),
                _ => None,
            })
            .expect("要求が届いている");

        handle
            .detach(Seat::new(0), id)
            .await
            .expect("卓は生きている");
        drop(inbox);
        tokio::time::sleep(Duration::from_millis(5_000)).await;

        let (_, mut back) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let resent = take_ready(&mut back)
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::RequestAction { deadline_ms, .. } if e.seq == seq => Some(*deadline_ms),
                _ => None,
            })
            .expect("同じ要求が再送されている");

        assert!(
            resent < original,
            "再送された要求が満額の残り時間のまま: {resent} / {original}"
        );
        let drained = original - resent;
        assert!(
            (4_900..=5_100).contains(&drained),
            "引き方がずれている: {drained}ms 減った"
        );
    }

    /// **出口の容量が、半荘1回ぶんの再送に足りている。**
    ///
    /// 足りないと `attach(seat, None)` が溢れ、正当な再接続が切られる。
    /// 実測は 1,304 / 1,875 / 1,677 件（シード3種）だが、ばらつきが4割
    /// あるので、将来 CPU の打ち方が変わったときに気づけるようにしておく。
    #[tokio::test(start_paused = true)]
    async fn a_whole_match_fits_in_one_outbox() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([2u8; 32]));
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let mut count = 0usize;
        loop {
            let next = tokio::time::timeout(Duration::from_millis(A_VIRTUAL_HOUR_MS), watcher.recv())
                .await
                .expect("仮想1時間のうちに終わる");
            if next.is_none() {
                break;
            }
            count += 1;
        }
        assert!(count > 500, "半荘にしてはイベントが少なすぎる: {count}");
        assert!(count < OUTBOX, "出口の容量が足りない: {count} 件 / {OUTBOX}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_finished_table_refuses_new_connections() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
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
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
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
Expected: 17 passed

- [ ] **Step 5: crate 全体を測る**

Run: `cargo test --package server`
期待: 62 件（table 28 + session_time 7 + session 10 + reconnect 17）

- [ ] **Step 6: コミット**

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

### Task 4: 反応ウィンドウを Actor 越しに検証する

**Files:**
- Modify: `crates/server/src/session.rs`

**Interfaces:**
- Consumes: Task 2・3 のすべて。
- Produces: 新しい公開 API は無い。

**なぜ別のタスクにするか。** ポン・チー・カン・ロンは Actor の存在理由そのものである。Wave 3b で「**卓は時間を作らない**」と決めたため、反応ウィンドウの最低待機 350ms を越えさせるのは呼び出し側の `tick` であり、いまその呼び出し側は Actor である。ここが壊れていると、鳴きが永久に成立しないか、逆に待たずに確定する。

**実測した値。**シード `[1u8; 32]`・席1 が人・他が CPU のとき、最初の鳴きの要求は **仮想 55,200ms** に届き、選択肢は `Chi` と `Pon`、残り時間は 7,650ms。同じミリ秒にポンで応じると、鳴きの成立は **55,600ms**（+400ms）である。最低待機の終わりは 55,550ms なので、その次のポーリングで越えている。

- [ ] **Step 1: 失敗するテストを書く**

`session.rs` の末尾に新しいモジュールとして足す。

```rust
#[cfg(test)]
mod reaction_tests {
    use super::tests::rules;
    use super::*;
    use protocol::client_event::ClientEvent;
    use protocol::command::{ActionOption, CallResponse};
    use protocol::event::PlayerId;

    fn human_at(seat: usize) -> [Occupant; 4] {
        std::array::from_fn(|index| {
            if index == seat {
                Occupant::Human(PlayerId("human".to_owned()))
            } else {
                Occupant::Cpu(PlayerId(format!("cpu{index}")))
            }
        })
    }

    fn all_cpu() -> [Occupant; 4] {
        std::array::from_fn(|index| Occupant::Cpu(PlayerId(format!("cpu{index}"))))
    }

    fn humans_at(first: usize, second: usize) -> [Occupant; 4] {
        std::array::from_fn(|index| {
            if index == first || index == second {
                Occupant::Human(PlayerId(format!("human{index}")))
            } else {
                Occupant::Cpu(PlayerId(format!("cpu{index}")))
            }
        })
    }

    /// その席へ最初に届く「鳴きの要求」まで進める。
    /// 打牌の要求（自分の番）は読み飛ばす。
    async fn advance_to_a_call_window(inbox: &mut Inbound) -> (u32, Vec<ActionOption>) {
        for _ in 0..3_000 {
            let next = tokio::time::timeout(Duration::from_millis(120_000), inbox.recv())
                .await
                .expect("仮想2分のうちに何か届く");
            let Some(envelope) = next else {
                break;
            };
            if let ClientEvent::RequestAction {
                window_id, options, ..
            } = &envelope.event
            {
                if options
                    .iter()
                    .any(|option| !matches!(option, ActionOption::Discard { .. }))
                {
                    return (*window_id, options.clone());
                }
            }
        }
        panic!("鳴きの要求に到達しなかった");
    }

    /// **卓は時間を作らない。**最低待機 350ms を越えさせるのは Actor の tick。
    #[tokio::test(start_paused = true)]
    async fn the_actor_carries_a_call_across_the_minimum_wait() {
        let (handle, _actor) = spawn(rules(), human_at(1), SeedSource::from_master([1u8; 32]));
        let clock = Clock::start();
        let (_, mut inbox) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        let (window_id, options) = advance_to_a_call_window(&mut inbox).await;
        // 実測では 55,200ms だが、そこは CPU の打牌方針しだいで動く。
        // **仕様は「応答から最低待機ぶん後に成立する」であって、
        // 要求が何ミリ秒に届くかではない。**時刻そのものは固定しない。
        let opened_at = clock.now_ms();

        let tiles = options
            .iter()
            .find_map(|option| match option {
                ActionOption::Pon { candidates } if !candidates.is_empty() => Some(candidates[0]),
                _ => None,
            })
            .expect("ポンの候補がある");

        // **同じミリ秒に応じる。**卓が自分で時間を進めるなら、ここで成立して
        // しまう。成立が 350ms 後になることが、Actor が越えさせている証拠。
        let accepted = handle
            .command(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pon { tiles },
                },
                handle.now_ms(),
            )
            .await
            .expect("卓は生きている");
        assert_eq!(accepted, Ok(()));
        assert_eq!(clock.now_ms(), opened_at, "応答で時間が進んでいる");

        let mut called_at = None;
        for _ in 0..40 {
            let next = tokio::time::timeout(Duration::from_millis(60_000), inbox.recv())
                .await
                .expect("仮想60秒のうちに届く");
            let Some(envelope) = next else {
                break;
            };
            if matches!(envelope.event, ClientEvent::Call { .. }) {
                called_at = Some(clock.now_ms());
                break;
            }
        }
        let called_at = called_at.expect("鳴きが成立しなかった");
        assert!(
            called_at >= opened_at + 350,
            "最低待機を越えずに成立した: +{}ms",
            called_at - opened_at
        );
        assert!(
            called_at <= opened_at + 450,
            "ポーリング1周を超えて遅れた: +{}ms",
            called_at - opened_at
        );
    }

    /// **誰も鳴けない打牌でも一律に待つ。**（仕様 6.4）
    ///
    /// 鳴ける者がいないときだけ次のツモが速いと、待ち時間の長短から
    /// 「誰も鳴けなかった」が読めてしまう。一律に待つのは情報を漏らさない
    /// ためであり、その待機を越えさせるのも Actor の `tick` である。
    ///
    /// 実測では、4人 CPU の対局で打牌から次のツモまでが**30回すべて
    /// ちょうど 400ms**（最低待機 350ms の直後のポーリング）だった。
    #[tokio::test(start_paused = true)]
    async fn even_an_uncalled_discard_waits_out_the_minimum() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let clock = Clock::start();
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let mut discarded_at: Option<u64> = None;
        let mut gaps: Vec<u64> = Vec::new();
        while gaps.len() < 20 {
            let next = tokio::time::timeout(Duration::from_millis(600_000), inbox.recv())
                .await
                .expect("仮想10分のうちに届く");
            let Some(envelope) = next else { break };
            match &envelope.event {
                ClientEvent::Discard { .. } => discarded_at = Some(clock.now_ms()),
                // **鳴きが挟まった区間は数えない。**鳴けた場合の待機と
                // 混ぜると「誰も鳴けなくても待つ」の検証にならない。
                ClientEvent::Call { .. } | ClientEvent::RequestAction { .. } => {
                    discarded_at = None;
                }
                ClientEvent::Draw { .. } => {
                    if let Some(at) = discarded_at.take() {
                        gaps.push(clock.now_ms() - at);
                    }
                }
                _ => {}
            }
        }

        assert_eq!(gaps.len(), 20, "測れた間隔が足りない");
        for gap in &gaps {
            assert!(
                *gap >= 350,
                "誰も鳴かなかった打牌の直後に次のツモが来た: {gap}ms"
            );
            assert!(*gap <= 450, "ポーリング1周を超えて遅れた: {gap}ms");
        }
    }

    /// **同じミリ秒に2席が同じウィンドウへ応じても、両方が受理される。**
    ///
    /// エンジンの判定規則は 298 件のテストが見ているが、「締切前に入口へ
    /// 着いた応答が、解決の tick より先に全部適用される」のは Actor が
    /// 新たに担う輸送の保証であり、エンジン単体では確かめられない。
    #[tokio::test(start_paused = true)]
    async fn two_seats_answer_the_same_window_before_the_tick() {
        let (handle, _actor) = spawn(rules(), humans_at(0, 2), SeedSource::from_master([1u8; 32]));
        let (_, mut east) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let (_, mut west) = handle
            .attach(Seat::new(2), None)
            .await
            .expect("卓は生きている");

        let mut seen_east: Vec<u32> = Vec::new();
        let mut seen_west: Vec<u32> = Vec::new();
        let mut shared = None;
        for _ in 0..6_000 {
            for (inbox, seen) in [(&mut east, &mut seen_east), (&mut west, &mut seen_west)] {
                while let Ok(envelope) = inbox.try_recv() {
                    if let ClientEvent::RequestAction {
                        window_id, options, ..
                    } = &envelope.event
                    {
                        if options
                            .iter()
                            .any(|option| !matches!(option, ActionOption::Discard { .. }))
                        {
                            seen.push(*window_id);
                        }
                    }
                }
            }
            if let Some(window) = seen_east.iter().find(|w| seen_west.contains(w)) {
                shared = Some(*window);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let window_id = shared.expect("2席共通のウィンドウに到達しなかった");

        // **同時に投入する。**逐次に送ると1件ずつ処理されるだけで、
        // 「入口に並んだ複数の応答が tick より先に片づく」を試せない。
        let at = handle.now_ms();
        let (first, second) = tokio::join!(
            handle.command(
                Seat::new(0),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pass,
                },
                at,
            ),
            handle.command(
                Seat::new(2),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pass,
                },
                at,
            )
        );
        assert_eq!(first.expect("卓は生きている"), Ok(()), "先の応答が拒まれた");
        assert_eq!(
            second.expect("卓は生きている"),
            Ok(()),
            "同じミリ秒の2席目の応答が拒まれた"
        );
    }

    /// `window_id` は再送や遅れた応答を別のウィンドウへ当てないための鍵。
    #[tokio::test(start_paused = true)]
    async fn a_response_to_an_unknown_window_is_refused() {
        let (handle, _actor) = spawn(rules(), human_at(1), SeedSource::from_master([1u8; 32]));
        let (_, mut inbox) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");
        let (window_id, _) = advance_to_a_call_window(&mut inbox).await;

        assert_eq!(
            handle
                .command(
                    Seat::new(1),
                    Command::CallResponse {
                        window_id: window_id + 999,
                        response: CallResponse::Pass,
                    },
                    handle.now_ms(),
                )
                .await
                .expect("卓は生きている"),
            Err(Reject::StaleWindow),
            "知らないウィンドウへの応答が通った"
        );

        assert_eq!(
            handle
                .command(
                    Seat::new(1),
                    Command::CallResponse {
                        window_id,
                        response: CallResponse::Pass,
                    },
                    handle.now_ms(),
                )
                .await
                .expect("卓は生きている"),
            Ok(()),
            "正しいウィンドウへの応答が通らない"
        );

        assert_eq!(
            handle
                .command(
                    Seat::new(1),
                    Command::CallResponse {
                        window_id,
                        response: CallResponse::Pass,
                    },
                    handle.now_ms(),
                )
                .await
                .expect("卓は生きている"),
            Err(Reject::StaleWindow),
            "同じウィンドウへ二度応えられている"
        );
    }
}
```

- [ ] **Step 2: 落ちることを確かめる**

Run: `cargo test --package server reaction_tests`
Expected: いくつか落ちる。Task 2 の実装が正しければ全部通る可能性もある。

- [ ] **Step 3: 通ることを確かめる**

Run: `cargo test --package server reaction_tests`
Expected: 4 passed

`the_actor_carries_a_call_across_the_minimum_wait` が「鳴きの要求に到達しなかった」で落ちるなら、`SeedSource` の繰り出し方が計画と違っている。`+350ms` の下限で落ちるなら、Actor が `tick` を呼んでいないか `POLL_MS` が違う。**期待値を書き換える前にそちらを疑う。**

**要求が届く時刻そのものは固定していない。**実測は 55,200ms だが、そこは `mahjong-ai` の打牌方針が変われば動く。仕様が要求しているのは「応答から最低待機ぶん後に成立すること」であって、何ミリ秒目に鳴けるかではない。付随的な値を期待値に据えると、無関係な変更でテストが落ちる。

- [ ] **Step 4: workspace 全体を測る**

```bash
cargo test --package server     # 66 件（table 28 + session_time 7 + session 10 + reconnect 17 + reaction 4）
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
Expected: 失敗ゼロ

- [ ] **Step 5: 禁じ手が入っていないか自分で確かめる**

```bash
# std の時計を使っていないか
grep -n "std::time::Instant\|SystemTime" crates/server/src/session*.rs
# table.rs を触っていないか
git diff --stat main -- crates/server/src/table.rs
# 凍結クレートを触っていないか
git diff --stat main -- crates/protocol crates/mahjong-core crates/mahjong-engine crates/mahjong-ai
```
Expected: すべて空

- [ ] **Step 6: コミット**

```bash
git add crates/server/src/session.rs
git commit -m "feat(server): 反応ウィンドウを Actor 越しに検証する

卓は時間を作らない。最低待機 350ms を越えさせるのは Actor の tick で
ある。同じミリ秒にポンで応じても成立は 350ms 後になることを測って
確かめる。ここが壊れると鳴きが永久に成立しないか、待たずに確定する。

window_id は再送や遅れた応答を別のウィンドウへ当てないための鍵なので、
知らないウィンドウと二度目の応答が拒まれることも確かめる。"
```

---

## Self-Review

**仕様の網羅:** 仕様 3.2 節「server — 唯一 I/O と時間を持つ層。1卓 = 1 tokio task の Actor とし、卓同士を完全に独立させる」を Task 2 が満たす。8.1 節「再接続 = `seq` 以降のイベント再送」を Task 3 が満たす。6.2 節の時間モデルは、コマンドを入口の到着時刻で判定することと、自動打牌が 25,750ms を厳密に超えることの2点で検証している。6.4 節の反応ウィンドウは Task 4 が受け持つ。シード開示（`SeedReveal`）を出すのはエンジン側の責務なので、このウェーブでは Actor が素通しするだけでよい。

**反応まわりで Actor 級の試験を書かなかったもの:** ダブロンと複数席の同時応答は、`mahjong-engine` の 298 テスト（Wave 2d・2e）が優先順位・供託分配・頭ハネまで網羅している。Actor が新たに担うのは「最低待機 350ms を越えさせること」と「`window_id` で遅れた応答を弾くこと」の2つであり、Task 4 はそこに絞ってある。**同じ判定を二重に試験しても、壊れたときに二重に落ちるだけで、原因の切り分けは楽にならない。**

**このウェーブがやらないこと:** WebSocket・axum・HTTP・認証は Wave 3d。永続化とマッチングも Wave 3d 以降。ここは「卓が実時間で動く」ところまでで止める。**非同期とネットワークを一度に入れると、失敗したときどちらが原因か切り分けられない。**

**Wave 3d へ渡す宿題（ここで決めておく）:**

**仕様 8.2 の CPU 代打ち切り替えは、「実行」と「判定」を分けて置く。**

| | 所有者 | 理由 |
|---|---|---|
| CPU の打牌を実行する | `Table` | 牌姿を持つのは `Table` だけ。既に `Occupant::Cpu` の席を代打ちしている |
| 代打ちへ切り替えると決める | Actor | 接続 ID と `Attach` / `Detach` と時計を持つのは Actor だけ |

**判定まで `Table` へ押し込んではならない。**`Table` は接続を知らないし、知るべきでもない。同期で決定的な卓に接続状態を持ち込むと、Wave 3b で分けた境界が崩れる。逆に WebSocket 層へ状態機械を置けば、接続層と卓の進行が密結合になる。

Wave 3d で決めること。

- **引き金**: その席が `Detach` されており、かつ連続して `n` 回自動打牌された（`n` の値は Wave 3d で決める）
- **解除**: その席が `Attach` し直した時点で人へ戻す
- **要る API**: `table.rs` に `Table::hand_over_to_cpu(seat)` と `Table::take_back_from_cpu(seat)` を足す。`occupants[seat]` を差し替えるだけの2メソッドで、卓の他の論理には触れない

**Wave 3c では足さない。**引き金に切断の概念が要り、それは WebSocket が入って初めて意味を持つ。**凍結は並行作業の衝突を防ぐための取り決めであり、マージ済みのファイルへ後から必要なメソッドを足すことを禁じるものではない。**

**シードは Actor の外から配る。**`Seeds` トレイトの `next_seed` は非同期であり、Wave 3d はこの中で永続化してから返す。局頭で配った `seed_commit` と、あとで開示するシードが食い違うと、プレイヤーが検算したときに**サーバが山を操作したように見える**（仕様 8.3）。だから「シードを作る」と「局を始める」のあいだに待てる形にしてある。Wave 3c の `SeedSource` は待たずに返すが、契約は同じである。

`spawn` が返す `JoinHandle` は Wave 3d の卓の台帳が持つ。panic・入口の消滅・正常な終局がすべて `Gone` に潰れると、障害を記録することも局頭から再開すべきかを判断することもできない（仕様 8.3）。

**再接続の濫用は Wave 3d が止める。**`attach(seat, None)` は追いつきぶんを丸ごと詰めた受け口を作る。古い受け口は新しい `attach` で回収されないので、繰り返せばその数だけ履歴が積まれる。同時接続数の上限、`attach` の頻度制限、古い WebSocket の強制切断は Wave 3d の責務である。

**シードだけでは局頭の永続化に足りない。**仕様 8.3 は、シードと一緒に「局番号・親・本場・供託・開始時点の点数」も局頭で書けと定めている。`Seeds::next_seed` は引数を取らないので、Wave 3d はこのフックからそれらを知れない。`Table` は `MatchEngine` を非公開で包んでおり、局番号や本場を読む手段も無い。

**したがって Wave 3d は、`next_seed` に局頭の状態を渡す引数を足すか、永続化のフックを別に設ける。**どちらにせよ `table.rs` に局頭の状態を読むアクセサが要る。CPU 代打ちのメソッドと同じく、そこで `table.rs` を開ける。

**Wave 3d への引き継ぎで未決なこと:** `Gone` は理由を持たない。WebSocket 層が「認証失敗」「卓が無い」「不正な resume」を区別してクライアントへ返したくなったら、`Gone` を理由付きの enum へ広げる必要がある。いま広げないのは、理由の一覧を決めるのが認証設計の一部であり、それが Wave 3d の仕事だからである。`ConnectionId` と ack はその拡張に耐える形にしてある。

**型の整合:** `Outbound` / `Inbound` / `Gone` / `ConnectionId` / `TableMsg` / `TableHandle` / `spawn` / `Clock` / `SeedSource` はすべて Task 1・2 で定義し、Task 3 は新しい型を足さない。`Reject` は `mahjong_engine::match_flow::Reject`（`PartialEq` と `Debug` を導出済み、`assert_eq!` で使える）。

**API の実在確認:** `rand::rng()`・`RngCore::fill_bytes`・`StdRng::from_seed`・`tokio::time::interval`・`MissedTickBehavior::Skip`・`#[tokio::test(start_paused = true)]`・`Seed::new([u8; 32])`・`Seed::from_hex`・`Seed::to_hex()`・`Table::since(seat, Option<u32>)`・`Table::needs_seed()`・`Table::is_over()`・`Table::begin_round(&Seed, u64)`・`Table::round_state()`・`Ruleset::kin_no_ma(MatchLength::Hanchan)`・`Seat::new(u8)`・`Seat::index()` はすべて実際にコンパイルして確認した。
