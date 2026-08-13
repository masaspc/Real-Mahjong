# Wave 3b: 卓の論理 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 1つの卓を回す論理を作る。`MatchEngine` を持ち、イベントに連番を振り、席ごとに視界フィルタを通して配り、CPU の席を自動で打たせる。**4人 CPU で半荘を最後まで回せるようにする。**

**この計画に含まないもの:** WebSocket・tokio・axum・永続化・マッチングは **Wave 3c** が担当する。ここは同期のまま、時刻も乱数も外から受け取る。

**Architecture:** 卓の論理と tokio の task を分ける。**論理側に非同期も実時間も入れない。**そうすれば、これまでと同じく決定的に検査できる。実時間と乱数は Wave 3c が外から与える。

**Tech Stack:** Rust 1.97.1 / edition 2021 / `protocol`・`mahjong-engine`・`mahjong-ai`（既に `server/Cargo.toml` に入っている）

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
**作業規約:** `AGENTS.md`
**前提:** Wave 3a がマージ済みであること（Rust 全体で524件のテストが通ること）

## Global Constraints

- **編集してよいのは次の2つだけである**
  - `crates/server/src/table.rs`
  - `crates/mahjong-engine/src/match_flow.rs`（**公開アクセサを1つ足すためだけ**。他は触らない）
- **`lib.rs` を編集しない。** どのクレートも Wave 0 で凍結済みである
- **`server/Cargo.toml` を編集しない。** 必要な依存はすでに入っている
- `crates/protocol` と `crates/mahjong-core` と `crates/mahjong-ai` は凍結済み。**編集しない**
- **時刻を直接読まない。** `Instant::now()` / `SystemTime::now()` / `rand` / `tokio` を使わない
- **既存の524件のテストを1つも壊さない**
- 完了条件は `cargo test --workspace` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` がすべて通ること

## `MatchEngine` に公開アクセサを1つ足す

卓が CPU の `View` を組み立てるには、動いている局の状態が要る。いまは
`#[cfg(test)]` の `round_state()` しか無く、`server` から読めない。

```rust
    /// 動いている局の状態。局と局のあいだは `None`。
    ///
    /// **卓が CPU へ渡す `View` を組み立てるために公開する。**
    /// ここから読めるのは卓（サーバ）だけであり、CPU には
    /// その席から見える分だけを詰め直して渡す。
    pub fn round_state(&self) -> Option<&RoundState> {
        self.engine.as_ref().map(|e| e.state())
    }
```

既存の `#[cfg(test)] fn round_state` は名前が衝突するので、テスト用のほうを
`test_round_state` へ改名し、呼び出し側も直す。**`round_state_mut` と
`force_scores` と `force_draw_turn` はテスト用のまま残す。**

## いかさまを構造で防ぐ

`View` を組み立てるのは卓である。卓は全席の手牌を持っているので、
**組み立て方を誤れば CPU に他家の手牌が見えてしまう。**

| 入れるもの | 出どころ |
|---|---|
| `hand` / `melds` | **その席の分だけ** |
| `rivers` | 4席ぶん。捨て牌は公開情報である |
| `riichi` | 4席ぶん。宣言は公開情報である |
| `dora_indicators` | `wall.dora_indicators()`。**裏ドラは入れない** |
| `wall_remaining` | `wall.live_remaining()` |
| `scores` / `seat_wind` / `round_wind` | 公開情報 |

**他家の手牌を読む式が1つも現れないこと**をテストで固定する。

## コーディネータが確定させたルール

| 項目 | 決定 |
|---|---|
| 連番 | 卓が発行する。**局をまたいでも半荘をまたいでもリセットしない。**0 から始める |
| 席ごとの配布 | `project_envelope` を通す。`None` が返る席へは何も送らない |
| CPU の代打ち | `RequestAction` がその席へ出たら、その場で決めて `apply` する。時間を待たない |
| CPU の決定 | 打牌は `mahjong_ai::discard::choose`、反応は `mahjong_ai::call::respond` |
| 人の席 | 卓は何もしない。`apply` が外から来るのを待つ |
| シード | 外から受け取る。卓は乱数を持たない |
| 再接続 | **`seq` からの再送だけにする。**スナップショットは作らない。`protocol` が凍結済みでスナップショット型を足せないうえ、半荘1回のイベントは数百件なので全件再送でも実用上困らない |
| 卓の終わり | `MatchEngine::is_over()` が真になったら、それ以上コマンドを受け付けない |

---

## タスクの依存関係

```
1 骨格と配布 → 2 CPU の代打ち → 3 再接続と通し対局
```

直列である。同じ構造体を段階的に育てる。

---

### Task 1: 骨格と配布

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`（公開アクセサとテスト用の改名のみ）
- Modify: `crates/server/src/table.rs`

**Interfaces:**
- Produces:
  - `pub enum Occupant { Human(PlayerId), Cpu(PlayerId) }`
  - `pub struct Table`
  - `Table::new(rules: Ruleset, occupants: [Occupant; 4], now_ms: u64) -> Self`
  - `Table::needs_seed(&self) -> bool`
  - `Table::begin_round(&mut self, seed: &Seed, now_ms: u64)`
  - `Table::apply(&mut self, seat: Seat, command: Command, now_ms: u64) -> Result<(), Reject>`
  - `Table::tick(&mut self, now_ms: u64)`
  - `Table::drain_for(&mut self, seat: Seat) -> Vec<ClientEventEnvelope>`
  - `Table::is_over(&self) -> bool`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::client_event::ClientEvent;

    pub(super) fn seed_of(index: u8) -> Seed {
        Seed::from_hex(&format!("{index:02x}").repeat(32)).expect("正しい hex")
    }

    pub(super) fn humans() -> [Occupant; 4] {
        std::array::from_fn(|i| Occupant::Human(PlayerId(format!("p{i}"))))
    }

    pub(super) fn table_of(occupants: [Occupant; 4]) -> Table {
        Table::new(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            occupants,
            0,
        )
    }

    /// 卓を作ると MatchStart が全席へ届く。
    #[test]
    fn a_new_table_announces_itself_to_everyone() {
        let mut table = table_of(humans());
        for seat in Seat::ALL {
            let events = table.drain_for(seat);
            assert_eq!(events.len(), 1);
            assert!(matches!(events[0].event, ClientEvent::MatchStart { .. }));
        }
    }

    /// 連番は0から始まり、席ごとに飛ばない。
    #[test]
    fn the_sequence_starts_at_zero() {
        let mut table = table_of(humans());
        let events = table.drain_for(Seat::new(0));
        assert_eq!(events[0].seq, 0);
    }

    /// MatchStart の you は受け取る席になる。
    #[test]
    fn each_seat_learns_which_one_it_is() {
        let mut table = table_of(humans());
        for seat in Seat::ALL {
            let events = table.drain_for(seat);
            let ClientEvent::MatchStart { you, .. } = events[0].event else {
                panic!("MatchStart でない");
            };
            assert_eq!(you, seat);
        }
    }

    /// 一度取り出したイベントは二度出ない。
    #[test]
    fn draining_twice_yields_nothing_the_second_time() {
        let mut table = table_of(humans());
        assert!(!table.drain_for(Seat::new(0)).is_empty());
        assert!(table.drain_for(Seat::new(0)).is_empty());
    }

    /// 席ごとに独立して溜まる。1席が取り出しても他席は残る。
    #[test]
    fn each_seat_has_its_own_queue() {
        let mut table = table_of(humans());
        table.drain_for(Seat::new(0));
        assert!(!table.drain_for(Seat::new(1)).is_empty());
    }

    /// シードを渡すと局が始まる。
    #[test]
    fn giving_a_seed_starts_the_round() {
        let mut table = table_of(humans());
        assert!(table.needs_seed());
        table.begin_round(&seed_of(1), 0);
        assert!(!table.needs_seed());

        let events = table.drain_for(Seat::new(0));
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::RoundStart { .. })));
    }

    /// 自分のツモ牌は見えるが、他家のツモ牌は見えない。
    #[test]
    fn only_the_drawer_sees_the_drawn_tile() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);

        let own = table.drain_for(Seat::new(0));
        let Some(ClientEvent::Draw { tile, .. }) = own
            .iter()
            .find_map(|e| match e.event {
                ClientEvent::Draw { .. } => Some(e.event.clone()),
                _ => None,
            })
        else {
            panic!("親のツモが見えていない");
        };
        assert!(tile.is_some(), "自分のツモ牌は見える");

        let other = table.drain_for(Seat::new(1));
        let Some(ClientEvent::Draw { tile, .. }) = other
            .iter()
            .find_map(|e| match e.event {
                ClientEvent::Draw { .. } => Some(e.event.clone()),
                _ => None,
            })
        else {
            panic!("他家にもツモの事実は見える");
        };
        assert_eq!(tile, None, "他家のツモ牌は見えない");
    }

    /// 配牌で見えるのは自分の手牌だけである。
    #[test]
    fn the_deal_shows_only_your_own_hand() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        let events = table.drain_for(Seat::new(2));
        let Some(ClientEvent::Deal {
            your_hand,
            hand_sizes,
            ..
        }) = events
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::Deal { .. } => Some(e.event.clone()),
                _ => None,
            })
        else {
            panic!("配牌が届いていない");
        };
        assert_eq!(your_hand.len(), 13, "自分の手牌だけが見える");
        assert_eq!(hand_sizes, [13; 4], "他家は枚数しか見えない");
    }

    /// 行動の要求は当事者にだけ届く。
    #[test]
    fn a_request_reaches_only_its_seat() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        for seat in Seat::ALL {
            let events = table.drain_for(seat);
            let requested = events
                .iter()
                .any(|e| matches!(e.event, ClientEvent::RequestAction { .. }));
            assert_eq!(requested, seat == Seat::new(0), "{seat:?}");
        }
    }

    /// コマンドは局へ委譲される。
    #[test]
    fn commands_reach_the_engine() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        table.drain_for(Seat::new(0));

        let tile = table.round_state().expect("局が動いている").seat(Seat::new(0)).hand[0];
        table
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        let events = table.drain_for(Seat::new(1));
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::Discard { .. })));
    }

    /// 連番は席をまたいでも同じイベントに同じ値が付く。
    #[test]
    fn the_same_event_carries_the_same_sequence_everywhere() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        let a = table.drain_for(Seat::new(1));
        let b = table.drain_for(Seat::new(2));
        // どちらも RoundStart から始まる。
        assert_eq!(a[0].seq, b[0].seq);
    }

    /// 同じシードと同じ操作からは同じイベント列が出る。
    #[test]
    fn the_same_input_gives_the_same_output() {
        let build = || {
            let mut table = table_of(humans());
            table.begin_round(&seed_of(1), 0);
            table.drain_for(Seat::new(0))
        };
        assert_eq!(build(), build());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package server`
Expected: コンパイルエラー（`Table` が未定義）

- [ ] **Step 3: 実装を書く**

`match_flow.rs` の変更は2点だけである。

```rust
    /// 動いている局の状態。局と局のあいだは `None`。
    ///
    /// 卓が CPU へ渡す `View` を組み立てるために公開する。
    pub fn round_state(&self) -> Option<&RoundState> {
        self.engine.as_ref().map(|e| e.state())
    }
```

既存の `#[cfg(test)] pub(crate) fn round_state` を `test_round_state` へ
改名し、テスト内の呼び出しをすべて直す。**返り値の型が違う**（テスト用は
`&RoundState` を直接返す）ので、名前を分けないと衝突する。

`table.rs` の本体。

```rust
//! 1つの卓。`MatchEngine` を持ち、イベントに連番を振って席ごとに配る。
//!
//! **非同期も実時間も持たない。**時刻もシードも外から受け取る。
//! tokio の task で包むのは Wave 3c の仕事である。

use mahjong_engine::match_flow::{MatchEngine, Reject};
use mahjong_engine::state::RoundState;
use mahjong_engine::wall::Seed;
use protocol::client_event::ClientEventEnvelope;
use protocol::command::Command;
use protocol::event::{EventEnvelope, PlayerId};
use protocol::project::project_envelope;
use protocol::ruleset::Ruleset;
use protocol::seat::Seat;

/// 席にいるのが人か CPU か。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Occupant {
    Human(PlayerId),
    Cpu(PlayerId),
}

impl Occupant {
    fn player_id(&self) -> PlayerId {
        match self {
            Occupant::Human(id) | Occupant::Cpu(id) => id.clone(),
        }
    }

    fn is_cpu(&self) -> bool {
        matches!(self, Occupant::Cpu(_))
    }
}

pub struct Table {
    engine: MatchEngine,
    occupants: [Occupant; 4],
    /// 卓が出した真実。再接続の再送に使う。
    log: Vec<EventEnvelope>,
    next_seq: u32,
    /// 席ごとの、まだ取り出されていない分。`log` への添字を持つ。
    pending: [Vec<usize>; 4],
}

impl Table {
    pub fn new(rules: Ruleset, occupants: [Occupant; 4], now_ms: u64) -> Self {
        let players = std::array::from_fn(|i| occupants[i].player_id());
        let mut table = Table {
            engine: MatchEngine::start(rules, players, now_ms),
            occupants,
            log: Vec::new(),
            next_seq: 0,
            pending: std::array::from_fn(|_| Vec::new()),
        };
        table.collect();
        table
    }

    pub fn is_over(&self) -> bool {
        self.engine.is_over()
    }

    pub fn needs_seed(&self) -> bool {
        self.engine.needs_seed()
    }

    /// 動いている局の状態。卓は全席の手牌を持つ。
    /// **ここから CPU へ渡すものは `View` に詰め直す。**
    pub fn round_state(&self) -> Option<&RoundState> {
        self.engine.round_state()
    }

    pub fn begin_round(&mut self, seed: &Seed, now_ms: u64) {
        self.engine.begin_round(seed, now_ms);
        self.collect();
    }

    pub fn apply(
        &mut self,
        seat: Seat,
        command: Command,
        now_ms: u64,
    ) -> Result<(), Reject> {
        let result = self.engine.apply(seat, command, now_ms);
        self.collect();
        result
    }

    pub fn tick(&mut self, now_ms: u64) {
        self.engine.tick(now_ms);
        self.collect();
    }

    /// その席へまだ届けていない分を取り出す。
    pub fn drain_for(&mut self, seat: Seat) -> Vec<ClientEventEnvelope> {
        std::mem::take(&mut self.pending[seat.index()])
            .into_iter()
            .filter_map(|index| project_envelope(&self.log[index], seat))
            .collect()
    }

    /// 局のイベントを取り込み、連番を振って席ごとの待ち行列へ入れる。
    ///
    /// **射影はここでは行わない。**`drain_for` まで遅らせることで、
    /// `log` には真実だけが残り、再接続の再送でも同じ経路を通る。
    fn collect(&mut self) {
        for event in self.engine.drain_events() {
            let index = self.log.len();
            self.log.push(EventEnvelope {
                seq: self.next_seq,
                event,
            });
            self.next_seq += 1;
            for queue in &mut self.pending {
                queue.push(index);
            }
        }
    }
}
```

**`occupants` は Task 2 まで読まれない。**`-D warnings` の `dead_code` を
避けるため、Task 1 では `is_cpu` と `player_id` のうち `player_id` だけを
使う。`is_cpu` は Task 2 で足す。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package server`
Expected: 12テスト PASS

- [ ] **Step 5: 既存のテストを壊していないことを確認する**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: すべて PASS

- [ ] **Step 6: コミット**

```bash
git add crates/server crates/mahjong-engine
git commit -m "feat(server): 卓の骨格と席ごとのイベント配布を実装"
```

---

### Task 2: CPU の代打ち

**Files:**
- Modify: `crates/server/src/table.rs`

**Interfaces:**
- Produces: `Table` が CPU 席の `RequestAction` を自動で処理する

**`View` の組み立てがこのタスクの要である。**卓は全席の手牌を持っているので、
詰め直しを誤れば CPU に他家の手牌が渡る。**その席の分だけを読む**ことを
テストで固定する。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod cpu_tests {
    use super::tests::{seed_of, table_of};
    use super::*;
    use protocol::client_event::ClientEvent;

    /// Task 3 も使う。**兄弟モジュールから見えるように `pub(super)` にする。**
    /// Task 1 に置くと、そこでは使われず `dead_code` で落ちる。
    pub(super) fn all_cpu() -> [Occupant; 4] {
        std::array::from_fn(|i| Occupant::Cpu(PlayerId(format!("cpu{i}"))))
    }

    fn mixed() -> [Occupant; 4] {
        [
            Occupant::Human(PlayerId("human".to_owned())),
            Occupant::Cpu(PlayerId("cpu1".to_owned())),
            Occupant::Cpu(PlayerId("cpu2".to_owned())),
            Occupant::Cpu(PlayerId("cpu3".to_owned())),
        ]
    }

    /// View に入るのは自分の手牌だけである。
    #[test]
    fn the_view_carries_only_its_own_hand() {
        let mut table = table_of(all_cpu());
        table.begin_round(&seed_of(1), 0);
        let state = table.round_state().expect("局が動いている");
        let view = build_view(state, Seat::new(2), table.engine.round());

        assert_eq!(view.hand, state.seat(Seat::new(2)).hand);
        // 他家の手牌がどこにも混ざっていない。
        for other in [Seat::new(0), Seat::new(1), Seat::new(3)] {
            for tile in &state.seat(other).hand {
                assert!(
                    !view.hand.contains(tile) || state.seat(Seat::new(2)).hand.contains(tile),
                    "他家の手牌が混ざっている"
                );
            }
        }
    }

    /// View に裏ドラは入らない。
    #[test]
    fn the_view_never_carries_the_ura_indicators() {
        let mut table = table_of(all_cpu());
        table.begin_round(&seed_of(1), 0);
        let state = table.round_state().expect("局が動いている");
        let view = build_view(state, Seat::new(0), table.engine.round());
        assert_eq!(view.dora_indicators, state.wall.dora_indicators().to_vec());
        assert_eq!(view.dora_indicators.len(), 1, "局の頭は1枚だけ");
    }

    /// View の河は4席ぶん見える。
    #[test]
    fn the_view_carries_every_river() {
        let mut table = table_of(all_cpu());
        table.begin_round(&seed_of(1), 0);
        let state = table.round_state().expect("局が動いている");
        let view = build_view(state, Seat::new(0), table.engine.round());
        assert_eq!(view.rivers.len(), 4);
    }

    /// CPU の席は要求が出た時点で自分で打つ。
    #[test]
    fn a_cpu_seat_acts_on_its_own() {
        let mut table = table_of(all_cpu());
        table.begin_round(&seed_of(1), 0);

        // 親は CPU なので、局が始まった時点でもう打っている。
        let events = table.drain_for(Seat::new(1));
        assert!(
            events
                .iter()
                .any(|e| matches!(e.event, ClientEvent::Discard { .. })),
            "CPU が打っていない: {events:?}"
        );
    }

    /// 人の席は待つ。卓は勝手に打たない。
    #[test]
    fn a_human_seat_is_left_alone() {
        let mut table = table_of(mixed());
        table.begin_round(&seed_of(1), 0);

        let events = table.drain_for(Seat::new(0));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e.event, ClientEvent::Discard { .. })),
            "人の席で勝手に打っている"
        );
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::RequestAction { .. })));
    }

    /// 人が打つと、次の CPU たちが続けて動く。
    #[test]
    fn the_cpus_continue_after_a_human_move() {
        let mut table = table_of(mixed());
        table.begin_round(&seed_of(1), 0);
        for seat in Seat::ALL {
            table.drain_for(seat);
        }

        let tile = table
            .round_state()
            .expect("局が動いている")
            .seat(Seat::new(0))
            .hand[0];
        table
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");

        // 席1以降は CPU なので、人の次の手番が来るまで進む。
        let events = table.drain_for(Seat::new(0));
        let discards = events
            .iter()
            .filter(|e| matches!(e.event, ClientEvent::Discard { .. }))
            .count();
        assert!(discards >= 2, "CPU が続いていない: {discards}");
    }

    /// CPU の反応も自動で決まる。誰も待たされない。
    #[test]
    fn a_cpu_answers_reaction_windows() {
        let mut table = table_of(all_cpu());
        table.begin_round(&seed_of(1), 0);
        // 一度も tick しなくても、CPU だけの卓は進み続ける。
        let events = table.drain_for(Seat::new(0));
        let draws = events
            .iter()
            .filter(|e| matches!(e.event, ClientEvent::Draw { .. }))
            .count();
        assert!(draws >= 2, "反応が解決していない: {draws}");
    }

    /// CPU は同じ局面から同じ手を打つ。卓ごと再現できる。
    #[test]
    fn a_cpu_table_is_reproducible() {
        let build = || {
            let mut table = table_of(all_cpu());
            table.begin_round(&seed_of(1), 0);
            table.drain_for(Seat::new(0))
        };
        assert_eq!(build(), build());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package server cpu_tests`
Expected: コンパイルエラー（`build_view` が未定義）

- [ ] **Step 3: 実装を書く**

```rust
use mahjong_ai::call;
use mahjong_ai::discard::{self, View};
use protocol::command::ActionOption;
use protocol::event::Event;
use protocol::seat::Round;

/// CPU へ渡す `View` を組み立てる。
///
/// **その席の分だけを読む。**手牌と副露は `seat` のものに限り、
/// 裏ドラは触れない。ここを誤ると CPU が他家の手を見られる。
fn build_view(state: &RoundState, seat: Seat, round: Round) -> View {
    View {
        seat,
        seat_wind: state.seat_wind(seat),
        round_wind: round.wind,
        hand: state.seat(seat).hand.clone(),
        melds: state.seat(seat).melds.clone(),
        rivers: std::array::from_fn(|i| {
            state
                .seat(Seat::new(i as u8))
                .river
                .iter()
                .map(|d| d.tile)
                .collect()
        }),
        riichi: std::array::from_fn(|i| {
            matches!(
                &state.seat(Seat::new(i as u8)).riichi,
                Some(r) if r.step == protocol::event::RiichiStep::Accepted
            )
        }),
        dora_indicators: state.wall.dora_indicators().to_vec(),
        wall_remaining: state.wall.live_remaining(),
        scores: state.scores,
    }
}
```

`collect` のあとに CPU を動かす。**要求が出た席が CPU なら、その場で決める。**

```rust
    fn collect(&mut self) {
        // ... 既存の取り込み ...
        self.let_cpus_act();
    }

    /// CPU の席へ出た要求を、その場で処理する。
    ///
    /// **時間を待たない。**CPU は考える必要がないので、締切を待つ意味がない。
    /// 人の席の要求はここで触らない。
    ///
    /// 1回の応答が次の要求を生むので、要求が尽きるまで繰り返す。
    /// 上限を置くのは、万一の取りこぼしで無限に回らないようにするためである。
    fn let_cpus_act(&mut self) {
        for _ in 0..1_000 {
            let Some((seat, options, now_ms)) = self.pending_cpu_request() else {
                return;
            };
            let Some(state) = self.engine.round_state() else {
                return;
            };
            let view = build_view(state, seat, self.engine.round());
            let command = if options
                .iter()
                .any(|o| matches!(o, ActionOption::Discard { .. }))
            {
                discard::choose(&view, &options)
            } else {
                Command::CallResponse {
                    window_id: self.last_window_id_for(seat),
                    response: call::respond(&view, &options),
                }
            };
            // 応答は engine を進める。取り込みは自分で行う。
            let _ = self.engine.apply(seat, command, now_ms);
            self.take_events();
        }
        panic!("CPU の応答が終わらない");
    }
```

**`pending_cpu_request` / `last_window_id_for` / `take_events` の3つは
実装者が決めてよい。**満たすべき性質だけを書く。

| 関数 | 満たすこと |
|---|---|
| `take_events` | `engine.drain_events()` を `log` へ移し、連番を振り、全席の待ち行列へ入れる。Task 1 の `collect` の前半をそのまま切り出す |
| `pending_cpu_request` | まだ応答していない CPU 席の `RequestAction` を1つ返す。無ければ `None`。**`log` の末尾から探す** |
| `last_window_id_for` | その席へ最後に出た `RequestAction` の `window_id` |

`collect` は `take_events` と `let_cpus_act` を順に呼ぶだけにする。

**`now_ms` は要求が出た時刻をそのまま使う。**CPU は時間を消費しない。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package server`
Expected: 8テスト PASS（クレート全体では20件）

- [ ] **Step 5: 既存のテストを壊していないことを確認する**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: すべて PASS

- [ ] **Step 6: コミット**

```bash
git add crates/server
git commit -m "feat(server): CPU の代打ちを実装"
```

---

### Task 3: 再接続と通し対局

**Files:**
- Modify: `crates/server/src/table.rs`

**Interfaces:**
- Produces: `Table::since(&self, seat: Seat, last_seq: Option<u32>) -> Vec<ClientEventEnvelope>`

**再送だけにする。**スナップショットは作らない。`protocol` が凍結済みで
スナップショット型を足せないうえ、半荘1回のイベントは数百件なので
全件再送でも実用上困らない。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod resume_tests {
    use super::cpu_tests::all_cpu;
    use super::tests::{humans, seed_of, table_of};
    use super::*;
    use protocol::client_event::ClientEvent;

    /// 初回は最初から全部送る。
    #[test]
    fn a_first_connection_gets_everything() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        table.drain_for(Seat::new(0));

        let all = table.since(Seat::new(0), None);
        assert!(matches!(all[0].event, ClientEvent::MatchStart { .. }));
        assert_eq!(all[0].seq, 0);
    }

    /// 続きからは、その連番より後だけを送る。
    #[test]
    fn a_resume_sends_only_what_came_after() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        let seen = table.drain_for(Seat::new(0));
        let last = seen.last().expect("何か届いている").seq;

        assert!(table.since(Seat::new(0), Some(last)).is_empty());
    }

    /// 再送は視界フィルタを通る。他家のツモ牌は復帰しても見えない。
    #[test]
    fn a_resume_is_filtered_the_same_way() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);

        let live = table.drain_for(Seat::new(1));
        let resent = table.since(Seat::new(1), None);
        assert_eq!(live, resent, "生と再送で内容が変わってはならない");
    }

    /// 再送しても卓の状態は動かない。
    #[test]
    fn a_resume_does_not_advance_anything() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        let before = table.since(Seat::new(0), None);
        let after = table.since(Seat::new(0), None);
        assert_eq!(before, after);
    }

    /// 4人 CPU で半荘が最後まで回る。
    #[test]
    fn four_cpus_finish_a_whole_match() {
        let mut table = table_of(all_cpu());
        let mut now = 1_000u64;
        let mut seed_index = 1u8;

        for _ in 0..200 {
            if table.is_over() {
                break;
            }
            if table.needs_seed() {
                table.begin_round(&seed_of(seed_index), now);
                seed_index = seed_index.wrapping_add(1);
                continue;
            }
            // CPU だけの卓は自分で進むが、念のため時間も進める。
            now += 1_000_000;
            table.tick(now);
        }

        assert!(table.is_over(), "半荘が終わらなかった");
        let events = table.since(Seat::new(0), None);
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::MatchEnd { .. })));
    }

    /// 通し対局のあとも、連番は飛ばずに並んでいる。
    #[test]
    fn the_sequence_has_no_holes() {
        let mut table = table_of(all_cpu());
        let mut now = 1_000u64;
        let mut seed_index = 1u8;
        for _ in 0..200 {
            if table.is_over() {
                break;
            }
            if table.needs_seed() {
                table.begin_round(&seed_of(seed_index), now);
                seed_index = seed_index.wrapping_add(1);
                continue;
            }
            now += 1_000_000;
            table.tick(now);
        }

        // 射影で落ちる席があるので、連番は飛びうる。**単調増加だけを見る。**
        let events = table.since(Seat::new(0), None);
        for pair in events.windows(2) {
            assert!(pair[0].seq < pair[1].seq, "連番が戻っている");
        }
    }

    /// 終局したあとはコマンドを受け付けない。
    #[test]
    fn a_finished_table_takes_no_more_commands() {
        let mut table = table_of(all_cpu());
        let mut now = 1_000u64;
        let mut seed_index = 1u8;
        for _ in 0..200 {
            if table.is_over() {
                break;
            }
            if table.needs_seed() {
                table.begin_round(&seed_of(seed_index), now);
                seed_index = seed_index.wrapping_add(1);
                continue;
            }
            now += 1_000_000;
            table.tick(now);
        }
        assert!(table.apply(Seat::new(0), Command::Tsumo, now).is_err());
    }
}
```

**`all_cpu` は Task 2 の `cpu_tests` に `pub(super)` で置いてある。**
兄弟モジュールの項目は `use super::*` では入らないので、
`use super::cpu_tests::all_cpu;` と明示する。

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package server resume_tests`
Expected: コンパイルエラー（`since` が未定義）

- [ ] **Step 3: 実装を書く**

```rust
    /// その連番より後を、視界フィルタを通して返す。
    ///
    /// **卓の状態を変えない。**何度呼んでも同じものが返る。
    /// `None` なら最初から全部返す。
    pub fn since(&self, seat: Seat, last_seq: Option<u32>) -> Vec<ClientEventEnvelope> {
        self.log
            .iter()
            .filter(|envelope| last_seq.is_none_or(|last| envelope.seq > last))
            .filter_map(|envelope| project_envelope(envelope, seat))
            .collect()
    }
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package server`
Expected: 7テスト PASS（クレート全体では27件）

- [ ] **Step 5: 既存のテストを壊していないことを確認する**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: すべて PASS

- [ ] **Step 6: コミット**

```bash
git add crates/server
git commit -m "feat(server): 再接続の再送と通し対局を実装"
```

---

## Wave 3b 完了の判定

- [ ] `cargo test --workspace` が通る（server 27テスト）
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] 既存の524件を1つも壊していない
- [ ] `tokio` も `rand` も `Instant::now` も使っていない
- [ ] `server/Cargo.toml` を編集していない
- [ ] `match_flow.rs` の変更が公開アクセサ1つとテスト用の改名だけである
- [ ] **4人 CPU で半荘が最後まで回る**
- [ ] 再送が生の配信と同じ内容になる
- [ ] `View` に他家の手牌も裏ドラも入っていない

## Wave 3c へ渡すもの

| 部品 | 使われ方 |
|---|---|
| `Table` | tokio task で包む。1卓 = 1 task |
| `needs_seed` / `begin_round` | 乱数は Wave 3c。`rand` でシードを作って渡す |
| `drain_for` | WebSocket で各席へ送る |
| `since` | `Resume { last_seq }` に応える |
| `tick` | 実時間のタイマーから呼ぶ |
