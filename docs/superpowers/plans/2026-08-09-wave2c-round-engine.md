# Wave 2c: 局の進行 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 配牌から終局までを進行させる。ツモ・打牌・鳴き・和了・荒牌平局までを扱い、ツモ切りの打ち手で1局を最後まで回せる状態にする。

**この計画に含まないもの:** 槓とリーチ、途中流局（九種九牌・四風連打・四家立直・四開槓・三家和）は **Wave 2d**、半荘の進行は **Wave 2e** が担当する。まず「鳴きあり・和了あり」の局が最後まで回ることを確かめてから、状態機械に枝を足す。

**Architecture:** 乱数も時間も外から注入する。`RoundEngine` は `Instant::now()` を呼ばず、`now_ms: u64` を引数で受け取る。同じシード・同じコマンド列・同じ時刻列からは必ず同じイベント列が出る。

**Tech Stack:** Rust 1.97.1 / edition 2021 / `protocol`・`mahjong-core`・Wave 2a の部品・Wave 2b の精算と合法手生成

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
**作業規約:** `AGENTS.md`
**前提:** `docs/superpowers/plans/2026-08-09-wave2b-engine-flow.md` が完了していること

## Global Constraints

- **編集してよいのは `crates/mahjong-engine/src/match_flow.rs` だけである**
- **`round.rs` を編集しない。** Wave 2b の成果物である。合法手生成に不足があれば実装を止めて報告する
- **`wall.rs` / `reaction.rs` / `state.rs` / `timing.rs` / `invariant.rs` を編集しない。** Wave 2a の成果物である
- **`lib.rs` を編集しない。** Wave 0 で凍結済みである
- `crates/protocol` と `crates/mahjong-core` は凍結済み。**編集も追加もしない**
- **時刻を直接読まない。** `Instant::now()` / `SystemTime::now()` / `rand` を呼ばない
- **時間の式をここに書き直さない。** `timing.rs` の4つの関数を呼ぶだけにする
- `Ruleset` に存在する値をハードコードしない
- 完了条件は `cargo test --workspace` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` がすべて通ること

## 使う既存 API（すべて実在を確認済み）

```rust
// crates/mahjong-engine/src/timing.rs
timing::lead_in_of(events: &[Event]) -> u32
timing::deadline_for(rules: &Ruleset, now_ms: u64, bank_remaining_ms: u32, lead_in_ms: u32) -> u64
timing::charge_bank(rules: &Ruleset, bank_remaining_ms: u32, elapsed_ms: u64, lead_in_ms: u32) -> u32
timing::remaining_for_event(absolute_deadline: u64, now_ms: u64) -> u32

// crates/mahjong-engine/src/invariant.rs
invariant::assert_tiles_conserved(state: &RoundState)
invariant::assert_scores_conserved(before: &[i32; 4], after: &[i32; 4], sticks_delta: i32)
invariant::assert_no_simultaneous_non_ron(window: &ReactionWindow)

// crates/mahjong-engine/src/reaction.rs
ReactionWindow::open(id, kind, from, tile, candidates: [Vec<ActionOption>; 4], opened_at_ms: u64, deadline_ms: u64)
ReactionWindow::respond(&mut self, seat, response: CallResponse) -> Result<(), Rejection>
ReactionWindow::resolve(&self, now_ms: u64, min_wait_ms: u32) -> Outcome
Outcome::{Pending, Ron(Vec<Seat>), Call { seat, response }, PassAll}

// crates/mahjong-engine/src/round.rs（Wave 2b）
round::discard_options(&RoundState, Seat, TurnStart) -> Vec<ActionOption>
round::reaction_options(&RoundState, Seat, Tile, Seat) -> Vec<ActionOption>
round::{settle_agari, settle_exhaustive, settle_nagashi, score_change, AgariInput}
```

**`deadline_for` は絶対時刻を返し、`Event::RequestAction.deadline_ms` は相対値である。**
`remaining_for_event` で必ず変換する。

## `lead_in` は席ごとのイベント列として持つ

`timing::lead_in_of` は席を引数に取らない。区間の切り出しは進行側の責務である、と
`timing.rs` の doc コメントが明記している。したがって `RoundEngine` は
**席ごとに「前回その席へ `RequestAction` を出して以降のイベント」を溜める。**

```rust
since_request: [Vec<Event>; 4],
```

`emit` は4席すべての buffer へ同じイベントを積み、`request` は
`lead_in_of(&since_request[seat])` を取ってからその席の buffer だけを空にする。

演出時間は席によらない。`Draw` は自分なら牌が見え他家なら裏だが、どちらも 250ms の
`Draw` 演出である。したがって射影の違いは `lead_in` に影響しない。

## `window_id` は外から渡す

仕様 6.4 のとおり `window_id` は半荘を通して単調増加し、`MatchEngine` が所有する。
Wave 2e がまだ無いので、`RoundEngine` は開始時に最初の値を受け取り、局の終わりに
次の値を返す。**手番の要求も反応ウィンドウも同じ採番を使う。**遅れて届いた応答を
取り違えないためである。

## コーディネータが確定させたルール

| 項目 | 決定 |
|---|---|
| 親の第一ツモ | 配牌13枚のあと、親が1枚引いて14枚になる。`Deal` の直後に `Draw` を出す |
| ドラ表示 | `Deal.dora_indicator` が最初の1枚を運ぶ。局の頭で `DoraReveal` は出さない |
| 反応ウィンドウ | 打牌のたびに必ず開く。候補が誰にも無くても開く |
| 反応が無い場合の待ち | `Ruleset.min_reaction_window_ms`（350ms）。鳴ける者の有無で間が変わると、それ自体が情報になる |
| `ActionPassed` を出す席 | 候補があった席のうち、`Pass` を選んだ席と、答えないまま確定した席。候補が無かった席には出さない |
| 同巡内フリテン | `ActionPassed` の `declined` に `Ron` が含まれる場合のみ、その席の待ち牌すべてを `passed_this_turn` へ積む |
| 手番の無応答 | 期限を過ぎたらツモ切りする。バンクは0になる |
| 鳴きの後 | 鳴いた席が手番になる。ツモは無い（`TurnStart::AfterCall`） |
| 流し満貫の失効 | 幺九牌以外を切った時点、または自分の捨て牌が鳴かれた時点で `nagashi_alive = false` |
| 荒牌平局 | 山の残りが0で、その打牌への反応が解決した時点。**その打牌にロンがあれば和了が優先される** |
| テンパイの判定 | `waiting_tiles` が空でないこと。`Ruleset.formal_tenpai` が真なので形式テンパイを認める |
| 流し満貫とテンパイ料 | 流し満貫が1人でも成立したら、テンパイ料は発生しない |
| ダブロン | `Ruleset.double_ron` が真なら成立する。`AgariResult` は席順で並べる |
| 三家和 | `Outcome::Ron` が3席を返したら流局にする。**Wave 2d の担当**。Wave 2c では `unreachable` にせず、まだ実装していないことがわかる形で落とす |

---

## タスクの依存関係

```
1 骨格 ─→ 2 打牌と反応 ─→ 3 鳴きの成立 ─→ 4 和了と荒牌平局
```

すべて直列である。同じ状態機械を段階的に育てるためで、並行させられない。

---

### Task 1: 骨格とイベント列

配牌から親の第一ツモ、最初の `RequestAction` までを出す。**まだコマンドを受け付けない。**

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**Interfaces:**
- Produces:
  - `pub struct RoundEngine`
  - `pub enum Phase { Turn { seat: Seat, start: TurnStart }, Reaction, Done }`
  - `RoundEngine::start(rules, round, dealer, honba, riichi_sticks, scores, seed, first_window_id, now_ms) -> Self`
  - `RoundEngine::drain_events(&mut self) -> Vec<Event>`
  - `RoundEngine::phase(&self) -> &Phase`
  - `RoundEngine::next_window_id(&self) -> u32`
  - `RoundEngine::state(&self) -> &RoundState`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod start_tests {
    use super::*;
    use crate::wall::Seed;
    use protocol::event::DrawSource;
    use protocol::ruleset::MatchLength;
    use protocol::seat::Wind;

    pub(super) fn seed() -> Seed {
        Seed::from_hex(&"31".repeat(32)).expect("正しい hex")
    }

    pub(super) fn rules() -> Ruleset {
        Ruleset::kin_no_ma(MatchLength::Hanchan)
    }

    pub(super) fn start_at(now_ms: u64) -> RoundEngine {
        RoundEngine::start(
            rules(),
            Round {
                wind: Wind::East,
                number: 1,
            },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &seed(),
            1,
            now_ms,
        )
    }

    /// 局の始まりは RoundStart → Deal → Draw → RequestAction の順に出る。
    #[test]
    fn a_round_opens_with_the_dealers_first_draw() {
        let mut engine = start_at(0);
        let events = engine.drain_events();
        assert!(matches!(events[0], Event::RoundStart { .. }));
        assert!(matches!(events[1], Event::Deal { .. }));
        assert!(matches!(
            events[2],
            Event::Draw {
                source: DrawSource::Wall,
                ..
            }
        ));
        assert!(matches!(events[3], Event::RequestAction { .. }));
        assert_eq!(events.len(), 4);
    }

    /// 山のシードのハッシュを最初に公開する。局の途中で山を差し替えられない。
    #[test]
    fn the_seed_is_committed_before_anything_is_dealt() {
        let mut engine = start_at(0);
        let events = engine.drain_events();
        let Event::RoundStart { seed_commit, .. } = &events[0] else {
            panic!("RoundStart が先頭にない");
        };
        assert_eq!(*seed_commit, seed().commitment());
    }

    /// 配牌は4人へ13枚ずつ。ドラ表示牌は Deal が運ぶ。
    #[test]
    fn the_deal_gives_thirteen_tiles_to_everyone() {
        let mut engine = start_at(0);
        let events = engine.drain_events();
        let Event::Deal {
            hands,
            dora_indicator,
        } = &events[1]
        else {
            panic!("Deal が2番目にない");
        };
        for hand in hands {
            assert_eq!(hand.len(), 13);
        }
        assert_eq!(*dora_indicator, engine.state().wall.dora_indicators()[0]);
    }

    /// 最初のツモは親が引き、親の手は14枚になる。
    #[test]
    fn the_dealer_draws_first_and_holds_fourteen() {
        let mut engine = start_at(0);
        let events = engine.drain_events();
        let Event::Draw { seat, .. } = &events[2] else {
            panic!("Draw が3番目にない");
        };
        assert_eq!(*seat, Seat::new(0));
        assert_eq!(engine.state().seat(Seat::new(0)).hand.len(), 14);
    }

    /// 手番は親で、ツモから始まっている。
    #[test]
    fn the_dealer_has_the_turn() {
        let mut engine = start_at(0);
        engine.drain_events();
        let Phase::Turn { seat, start } = engine.phase() else {
            panic!("手番になっていない");
        };
        assert_eq!(*seat, Seat::new(0));
        assert!(matches!(start, TurnStart::Draw { .. }));
    }

    /// 最初の要求は親へ出る。window_id は渡した値から始まる。
    #[test]
    fn the_first_request_goes_to_the_dealer() {
        let mut engine = start_at(0);
        let events = engine.drain_events();
        let Event::RequestAction {
            seat,
            window_id,
            options,
            ..
        } = &events[3]
        else {
            panic!("RequestAction が4番目にない");
        };
        assert_eq!(*seat, Seat::new(0));
        assert_eq!(*window_id, 1);
        assert!(options
            .iter()
            .any(|o| matches!(o, ActionOption::Discard { .. })));
        assert_eq!(engine.next_window_id(), 2);
    }

    /// 期限は基準思考時間 + バンク + 通信猶予 + lead_in。
    /// 局の頭の lead_in は Deal と Draw の演出時間の合計である。
    #[test]
    fn the_first_deadline_includes_the_opening_animation() {
        let mut engine = start_at(0);
        let events = engine.drain_events();
        let Event::RequestAction { deadline_ms, .. } = &events[3] else {
            panic!("RequestAction が無い");
        };
        let lead_in = timing::lead_in_of(&events[..3]);
        let absolute = timing::deadline_for(&rules(), 0, rules().think_bank_ms, lead_in);
        assert_eq!(*deadline_ms, timing::remaining_for_event(absolute, 0));
    }

    /// 期限は相対値なので、開始時刻が変わっても同じ値になる。
    #[test]
    fn the_deadline_is_relative_to_the_moment_it_is_issued() {
        let mut early = start_at(0);
        let mut late = start_at(1_000_000);
        let of = |events: &[Event]| {
            let Event::RequestAction { deadline_ms, .. } = &events[3] else {
                panic!("RequestAction が無い");
            };
            *deadline_ms
        };
        assert_eq!(of(&early.drain_events()), of(&late.drain_events()));
    }

    /// 同じシードと同じ時刻からは、必ず同じイベント列が出る。
    #[test]
    fn the_same_seed_produces_the_same_events() {
        let mut first = start_at(0);
        let mut second = start_at(0);
        assert_eq!(first.drain_events(), second.drain_events());
    }

    /// 違うシードなら違う配牌になる。
    #[test]
    fn a_different_seed_produces_a_different_deal() {
        let other = Seed::from_hex(&"92".repeat(32)).expect("正しい hex");
        let mut a = start_at(0);
        let mut b = RoundEngine::start(
            rules(),
            Round {
                wind: Wind::East,
                number: 1,
            },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &other,
            1,
            0,
        );
        assert_ne!(a.drain_events()[1], b.drain_events()[1]);
    }

    /// 一度取り出したイベントは二度出ない。
    #[test]
    fn draining_twice_yields_nothing_the_second_time() {
        let mut engine = start_at(0);
        assert_eq!(engine.drain_events().len(), 4);
        assert!(engine.drain_events().is_empty());
    }

    /// 牌は136枚から増えも減りもしない。
    #[test]
    fn the_opening_conserves_every_tile() {
        let mut engine = start_at(0);
        engine.drain_events();
        crate::invariant::assert_tiles_conserved(engine.state());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine start_tests`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
//! 局の進行。時刻も乱数も外から受け取る。
//!
//! 同じシード・同じコマンド列・同じ時刻列からは、必ず同じイベント列が出る。

use crate::invariant;
use crate::reaction::ReactionWindow;
use crate::round::{discard_options, TurnStart};
use crate::state::RoundState;
use crate::timing;
use crate::wall::Seed;
use protocol::command::ActionOption;
use protocol::event::{DrawSource, Event};
use protocol::ruleset::Ruleset;
use protocol::seat::{Round, Seat};

/// 局がいまどこにいるか。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 手番の席が打牌などを選ぶ。
    Turn { seat: Seat, start: TurnStart },
    /// 打牌への反応を待つ。
    Reaction,
    /// 局が終わった。
    Done,
}

pub struct RoundEngine {
    state: RoundState,
    phase: Phase,
    window: Option<ReactionWindow>,
    /// まだ取り出されていないイベント。
    pending: Vec<Event>,
    /// 席ごとの、前回その席へ RequestAction を出して以降に配信したイベント。
    /// `timing::lead_in_of` は席を取らないので、区間の切り出しはここで行う。
    since_request: [Vec<Event>; 4],
    /// 席ごとの、いま開いている要求。応答が来たらバンクを課金する。
    outstanding: [Option<Outstanding>; 4],
    next_window_id: u32,
}

/// 応答を待っている要求。課金に必要な値だけを持つ。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Outstanding {
    window_id: u32,
    issued_at_ms: u64,
    lead_in_ms: u32,
}

impl RoundEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        rules: Ruleset,
        round: Round,
        dealer: Seat,
        honba: u8,
        riichi_sticks: u8,
        scores: [i32; 4],
        seed: &Seed,
        first_window_id: u32,
        now_ms: u64,
    ) -> Self {
        let seed_commit = seed.commitment();
        let state = RoundState::new(rules, round, dealer, honba, riichi_sticks, scores, seed);

        let mut engine = RoundEngine {
            state,
            phase: Phase::Done,
            window: None,
            pending: Vec::new(),
            since_request: std::array::from_fn(|_| Vec::new()),
            outstanding: [None; 4],
            next_window_id: first_window_id,
        };

        let hands = std::array::from_fn(|i| engine.state.seats[i].hand.clone());
        let dora_indicator = engine.state.wall.dora_indicators()[0];

        engine.emit(Event::RoundStart {
            round,
            dealer,
            honba,
            riichi_sticks,
            scores,
            seed_commit,
        });
        engine.emit(Event::Deal {
            hands,
            dora_indicator,
        });
        engine.draw_for(dealer, DrawSource::Wall);
        engine.request_turn(now_ms);
        engine
    }

    pub fn state(&self) -> &RoundState {
        &self.state
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn next_window_id(&self) -> u32 {
        self.next_window_id
    }

    /// 生成済みのイベントを取り出す。一度出したものは二度出さない。
    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    /// イベントを1つ配信する。全席の lead_in 用 buffer へ積む。
    fn emit(&mut self, event: Event) {
        for buffer in &mut self.since_request {
            buffer.push(event.clone());
        }
        self.pending.push(event);
    }

    fn draw_for(&mut self, seat: Seat, source: DrawSource) {
        let tile = match source {
            DrawSource::Wall => self.state.wall.draw(),
            DrawSource::DeadWall => self.state.wall.draw_replacement(),
        }
        .expect("引ける牌がある局面でのみ呼ぶ");

        // 同巡内フリテンは自分のツモで解ける。
        self.state.begin_turn(seat);
        self.state.seat_mut(seat).hand.push(tile);
        self.state.last_draw = Some((seat, source));
        self.state.draw_count[seat.index()] += 1;

        let wall_remaining = self.state.wall.live_remaining();
        self.emit(Event::Draw {
            seat,
            tile,
            source,
            wall_remaining,
        });
        self.phase = Phase::Turn {
            seat,
            start: TurnStart::Draw { tile, source },
        };
        invariant::assert_tiles_conserved(&self.state);
    }

    /// 手番の席へ選択肢を送る。
    ///
    /// **`RequestAction` は `emit` を通さない。**要求そのものに演出は無く、
    /// これを lead_in へ積むと次の期限が二重に伸びる。
    fn request_turn(&mut self, now_ms: u64) {
        let Phase::Turn { seat, start } = self.phase.clone() else {
            panic!("手番でないのに要求を出そうとした");
        };
        let options = discard_options(&self.state, seat, start);
        self.request(seat, options, now_ms);
    }

    fn request(&mut self, seat: Seat, options: Vec<ActionOption>, now_ms: u64) {
        let lead_in_ms = timing::lead_in_of(&self.since_request[seat.index()]);
        self.since_request[seat.index()].clear();

        let absolute = timing::deadline_for(
            &self.state.rules,
            now_ms,
            self.state.seat(seat).think_bank_ms,
            lead_in_ms,
        );
        let window_id = self.take_window_id();
        self.outstanding[seat.index()] = Some(Outstanding {
            window_id,
            issued_at_ms: now_ms,
            lead_in_ms,
        });
        self.pending.push(Event::RequestAction {
            seat,
            window_id,
            options,
            deadline_ms: timing::remaining_for_event(absolute, now_ms),
        });
    }

    fn take_window_id(&mut self) -> u32 {
        let id = self.next_window_id;
        self.next_window_id += 1;
        id
    }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine start_tests`
Expected: 12テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 局の開始から第一要求までを実装"
```

---

### Task 2: 打牌と反応ウィンドウ

打牌を受け付け、反応ウィンドウを開き、誰も鳴かなければ次の席がツモる。

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**Interfaces:**
- Produces:
  - `pub enum Reject { NotYourTurn, NotOffered, NoWindow, Window(reaction::Rejection) }`
  - `RoundEngine::apply(&mut self, seat: Seat, command: Command, now_ms: u64) -> Result<(), Reject>`
  - `RoundEngine::tick(&mut self, now_ms: u64)`

**バンクの課金は応答を受け取った時に行う。** `timing::charge_bank` へ
`elapsed_ms = now_ms - issued_at_ms` と、要求時に記録した `lead_in_ms` を渡す。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod discard_tests {
    use super::start_tests::{rules, start_at};
    use super::*;
    use protocol::command::{CallResponse, Command};
    use protocol::event::DiscardManner;

    /// 手番の席の手牌から、いま引いた牌を返す。
    fn drawn_of(engine: &RoundEngine) -> Tile {
        let Phase::Turn {
            start: TurnStart::Draw { tile, .. },
            ..
        } = engine.phase()
        else {
            panic!("ツモ番ではない");
        };
        *tile
    }

    fn turn_seat(engine: &RoundEngine) -> Seat {
        let Phase::Turn { seat, .. } = engine.phase() else {
            panic!("手番ではない");
        };
        *seat
    }

    /// ツモ切りする。
    fn tsumogiri(engine: &mut RoundEngine, now_ms: u64) {
        let seat = turn_seat(engine);
        let tile = drawn_of(engine);
        engine
            .apply(
                seat,
                Command::Discard {
                    tile,
                    riichi: false,
                },
                now_ms,
            )
            .expect("ツモ切りは常に打てる");
    }

    /// 打牌すると Discard が出て、反応待ちになる。
    #[test]
    fn a_discard_opens_a_reaction_window() {
        let mut engine = start_at(0);
        engine.drain_events();
        tsumogiri(&mut engine, 1_000);

        let events = engine.drain_events();
        assert!(matches!(events[0], Event::Discard { .. }));
        assert_eq!(*engine.phase(), Phase::Reaction);
    }

    /// ツモ切りは Tsumogiri として記録される。河にも残る。
    #[test]
    fn a_drawn_tile_discarded_is_recorded_as_tsumogiri() {
        let mut engine = start_at(0);
        engine.drain_events();
        let tile = drawn_of(&engine);
        tsumogiri(&mut engine, 1_000);

        let events = engine.drain_events();
        let Event::Discard {
            seat,
            tile: discarded,
            manner,
        } = &events[0]
        else {
            panic!("Discard が出ていない");
        };
        assert_eq!(*seat, Seat::new(0));
        assert_eq!(*discarded, tile);
        assert_eq!(*manner, DiscardManner::Tsumogiri);
        assert_eq!(engine.state().seat(Seat::new(0)).river.len(), 1);
        assert_eq!(engine.state().seat(Seat::new(0)).hand.len(), 13);
    }

    /// 手から選んで切れば Tedashi になる。
    #[test]
    fn discarding_another_tile_is_recorded_as_tedashi() {
        let mut engine = start_at(0);
        engine.drain_events();
        let drawn = drawn_of(&engine);
        let other = *engine
            .state()
            .seat(Seat::new(0))
            .hand
            .iter()
            .find(|t| **t != drawn)
            .expect("14枚あるので別の牌がある");
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: other,
                    riichi: false,
                },
                1_000,
            )
            .expect("手出しできる");
        let events = engine.drain_events();
        let Event::Discard { manner, .. } = &events[0] else {
            panic!("Discard が出ていない");
        };
        assert_eq!(*manner, DiscardManner::Tedashi);
    }

    /// 手番でない席の打牌は拒否する。
    #[test]
    fn a_seat_out_of_turn_cannot_discard() {
        let mut engine = start_at(0);
        engine.drain_events();
        let tile = drawn_of(&engine);
        assert_eq!(
            engine.apply(
                Seat::new(1),
                Command::Discard {
                    tile,
                    riichi: false
                },
                1_000
            ),
            Err(Reject::NotYourTurn)
        );
    }

    /// 持っていない牌は切れない。
    #[test]
    fn a_tile_not_in_hand_cannot_be_discarded() {
        let mut engine = start_at(0);
        engine.drain_events();
        let missing = Seat::ALL // 手に無い牌を探す
            .iter()
            .flat_map(|_| (0..34u8))
            .filter_map(TileKind::from_index)
            .map(Tile::from_kind)
            .find(|t| !engine.state().seat(Seat::new(0)).hand.contains(t))
            .expect("14枚では34種を埋められない");
        assert_eq!(
            engine.apply(
                Seat::new(0),
                Command::Discard {
                    tile: missing,
                    riichi: false
                },
                1_000
            ),
            Err(Reject::NotOffered)
        );
    }

    /// 最低待機の前は誰も答えていなくても確定しない。
    #[test]
    fn nothing_resolves_before_the_minimum_wait() {
        let mut engine = start_at(0);
        engine.drain_events();
        tsumogiri(&mut engine, 1_000);
        engine.drain_events();

        engine.tick(1_000 + rules().min_reaction_window_ms as u64 - 1);
        assert_eq!(*engine.phase(), Phase::Reaction);
        assert!(engine.drain_events().is_empty());
    }

    /// 最低待機を過ぎ、鳴ける者がいなければ下家がツモる。
    #[test]
    fn the_next_seat_draws_once_the_window_closes() {
        let mut engine = start_at(0);
        engine.drain_events();
        tsumogiri(&mut engine, 1_000);
        engine.drain_events();

        engine.tick(1_000 + rules().min_reaction_window_ms as u64);
        let events = engine.drain_events();
        let Some(Event::Draw { seat, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Draw { .. }))
            .cloned()
        else {
            panic!("次のツモが出ていない: {events:?}");
        };
        assert_eq!(seat, Seat::new(1), "下家がツモる");
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::RequestAction { seat: s, .. } if *s == Seat::new(1))));
    }

    /// 見逃した席には ActionPassed が出る。候補が無かった席には出ない。
    #[test]
    fn only_seats_with_candidates_get_an_action_passed() {
        let mut engine = start_at(0);
        engine.drain_events();
        tsumogiri(&mut engine, 1_000);
        engine.drain_events();
        engine.tick(1_000 + rules().min_reaction_window_ms as u64);

        let events = engine.drain_events();
        for event in &events {
            if let Event::ActionPassed { seat, declined, .. } = event {
                assert!(
                    !declined.is_empty(),
                    "候補が無い席 {seat:?} に ActionPassed を出している"
                );
            }
        }
    }

    /// 打牌した席は自分の打牌に反応できない。
    #[test]
    fn the_discarder_cannot_react_to_itself() {
        let mut engine = start_at(0);
        engine.drain_events();
        tsumogiri(&mut engine, 1_000);
        engine.drain_events();

        assert_eq!(
            engine.apply(
                Seat::new(0),
                Command::CallResponse {
                    window_id: engine.next_window_id() - 1,
                    response: CallResponse::Pass,
                },
                1_100
            ),
            Err(Reject::Window(crate::reaction::Rejection::IsTheDiscarder))
        );
    }

    /// 期限内に答えれば、基準時間の中はバンクが減らない。
    #[test]
    fn answering_within_the_base_time_costs_no_bank() {
        let mut engine = start_at(0);
        engine.drain_events();
        let before = engine.state().seat(Seat::new(0)).think_bank_ms;
        tsumogiri(&mut engine, 1_000);
        assert_eq!(engine.state().seat(Seat::new(0)).think_bank_ms, before);
    }

    /// 基準時間を超えた分はバンクから引かれる。演出と通信猶予は課金しない。
    #[test]
    fn thinking_past_the_base_time_eats_the_bank() {
        let mut engine = start_at(0);
        let events = engine.drain_events();
        let lead_in = timing::lead_in_of(&events[..3]);
        let r = rules();

        // 基準時間を 2 秒超えるまで待って打つ。
        let elapsed = lead_in as u64 + r.network_grace_ms as u64 + r.base_think_ms as u64 + 2_000;
        tsumogiri(&mut engine, elapsed);

        assert_eq!(
            engine.state().seat(Seat::new(0)).think_bank_ms,
            r.think_bank_ms - 2_000
        );
    }

    /// 手番の期限を過ぎたらツモ切りになり、バンクが尽きる。
    #[test]
    fn an_unanswered_turn_is_auto_discarded() {
        let mut engine = start_at(0);
        engine.drain_events();
        let tile = drawn_of(&engine);

        // 期限を大きく過ぎた時刻で tick する。
        engine.tick(10_000_000);
        let events = engine.drain_events();
        let Some(Event::Discard {
            tile: discarded,
            manner,
            ..
        }) = events
            .iter()
            .find(|e| matches!(e, Event::Discard { .. }))
            .cloned()
        else {
            panic!("自動打牌が出ていない");
        };
        assert_eq!(discarded, tile, "ツモ牌を切る");
        assert_eq!(manner, DiscardManner::Tsumogiri);
        assert_eq!(engine.state().seat(Seat::new(0)).think_bank_ms, 0);
    }

    /// 幺九牌以外を切ると流し満貫の資格を失う。
    #[test]
    fn discarding_a_simple_ends_the_nagashi_claim() {
        let mut engine = start_at(0);
        engine.drain_events();
        // 手牌を確実に中張牌だけにしてから切る。
        let seat = Seat::new(0);
        assert!(engine.state().seat(seat).nagashi_alive);
        let simple = *engine
            .state()
            .seat(seat)
            .hand
            .iter()
            .find(|t| !t.kind().is_terminal_or_honor())
            .expect("配牌14枚に中張牌が1枚はある");
        engine
            .apply(
                seat,
                Command::Discard {
                    tile: simple,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        assert!(!engine.state().seat(seat).nagashi_alive);
    }

    /// 打牌のたびに牌の総数は保たれる。
    #[test]
    fn every_discard_conserves_the_tiles() {
        let mut engine = start_at(0);
        engine.drain_events();
        let mut now = 1_000;
        for _ in 0..8 {
            tsumogiri(&mut engine, now);
            engine.drain_events();
            now += rules().min_reaction_window_ms as u64;
            engine.tick(now);
            engine.drain_events();
            crate::invariant::assert_tiles_conserved(engine.state());
            now += 1_000;
        }
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine discard_tests`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
use crate::reaction::{Outcome, Rejection, WindowKind};
use crate::round::reaction_options;
use protocol::command::{CallResponse, Command};
use protocol::event::{DiscardManner, RiichiStep};
use protocol::tile::{Tile, TileKind};

/// コマンドを受け付けなかった理由。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reject {
    /// いま手番ではない。
    NotYourTurn,
    /// その操作は提示していない。
    NotOffered,
    /// 反応ウィンドウが開いていない。
    NoWindow,
    /// 遅れて届いた応答。いまのウィンドウのものではない。
    StaleWindow,
    /// 反応ウィンドウ側の拒否。
    Window(Rejection),
}

impl RoundEngine {
    pub fn apply(&mut self, seat: Seat, command: Command, now_ms: u64) -> Result<(), Reject> {
        match command {
            Command::Discard { tile, riichi } => self.apply_discard(seat, tile, riichi, now_ms),
            Command::CallResponse {
                window_id,
                response,
            } => self.apply_response(seat, window_id, response, now_ms),
            // 槓・ツモ・九種九牌は Wave 2d が扱う。
            _ => Err(Reject::NotOffered),
        }
    }

    /// 時間で進むものを進める。応答が来なくても局が止まらないようにする。
    pub fn tick(&mut self, now_ms: u64) {
        match self.phase.clone() {
            Phase::Turn { seat, start } => {
                let Some(open) = self.outstanding[seat.index()] else {
                    return;
                };
                let absolute = timing::deadline_for(
                    &self.state.rules,
                    open.issued_at_ms,
                    self.state.seat(seat).think_bank_ms,
                    open.lead_in_ms,
                );
                if now_ms <= absolute {
                    return;
                }
                // 無応答はツモ切り。鳴いた直後なら手の先頭を切る。
                let tile = match start {
                    TurnStart::Draw { tile, .. } => tile,
                    TurnStart::AfterCall => self.state.seat(seat).hand[0],
                };
                self.state.seat_mut(seat).think_bank_ms = 0;
                self.outstanding[seat.index()] = None;
                self.discard(seat, tile, now_ms);
            }
            Phase::Reaction => self.resolve_window(now_ms),
            Phase::Done => {}
        }
    }

    fn apply_discard(
        &mut self,
        seat: Seat,
        tile: Tile,
        riichi: bool,
        now_ms: u64,
    ) -> Result<(), Reject> {
        let Phase::Turn {
            seat: turn,
            start,
        } = self.phase.clone()
        else {
            return Err(Reject::NotYourTurn);
        };
        if turn != seat {
            return Err(Reject::NotYourTurn);
        }
        // リーチ宣言は Wave 2d が扱う。
        if riichi {
            return Err(Reject::NotOffered);
        }

        let allowed = discard_options(&self.state, seat, start)
            .into_iter()
            .find_map(|o| match o {
                ActionOption::Discard { allowed, .. } => Some(allowed),
                _ => None,
            })
            .unwrap_or_default();
        if !allowed.contains(&tile) {
            return Err(Reject::NotOffered);
        }

        self.charge(seat, now_ms);
        self.discard(seat, tile, now_ms);
        Ok(())
    }

    /// 打牌を確定させ、反応ウィンドウを開く。
    fn discard(&mut self, seat: Seat, tile: Tile, now_ms: u64) {
        let drawn = match self.phase {
            Phase::Turn {
                start: TurnStart::Draw { tile, .. },
                ..
            } => Some(tile),
            _ => None,
        };
        let manner = if drawn == Some(tile) {
            DiscardManner::Tsumogiri
        } else {
            DiscardManner::Tedashi
        };

        let hand = &mut self.state.seat_mut(seat).hand;
        let position = hand
            .iter()
            .position(|t| *t == tile)
            .expect("合法手として提示した牌は手にある");
        hand.remove(position);

        let riichi_declaration = matches!(
            &self.state.seat(seat).riichi,
            Some(r) if r.step == RiichiStep::Declare
        );
        self.state.seat_mut(seat).river.push(crate::state::Discarded {
            tile,
            manner,
            called_by: None,
            riichi_declaration,
        });

        // 幺九牌以外を切ったら流し満貫は消える。
        if !tile.kind().is_terminal_or_honor() {
            self.state.seat_mut(seat).nagashi_alive = false;
        }

        self.emit(Event::Discard { seat, tile, manner });
        invariant::assert_tiles_conserved(&self.state);
        self.open_reaction(seat, tile, now_ms);
    }

    fn open_reaction(&mut self, from: Seat, tile: Tile, now_ms: u64) {
        let candidates: [Vec<ActionOption>; 4] =
            std::array::from_fn(|i| reaction_options(&self.state, Seat::new(i as u8), tile, from));

        // 候補がある席にだけ要求を出す。
        let window_id = self.take_window_id();
        let mut deadline = now_ms + self.state.rules.min_reaction_window_ms as u64;
        for seat in Seat::ALL {
            if candidates[seat.index()].is_empty() {
                continue;
            }
            let lead_in_ms = timing::lead_in_of(&self.since_request[seat.index()]);
            self.since_request[seat.index()].clear();
            let absolute = timing::deadline_for(
                &self.state.rules,
                now_ms,
                self.state.seat(seat).think_bank_ms,
                lead_in_ms,
            );
            deadline = deadline.max(absolute);
            self.outstanding[seat.index()] = Some(Outstanding {
                window_id,
                issued_at_ms: now_ms,
                lead_in_ms,
            });
            self.pending.push(Event::RequestAction {
                seat,
                window_id,
                options: candidates[seat.index()].clone(),
                deadline_ms: timing::remaining_for_event(absolute, now_ms),
            });
        }

        let window = ReactionWindow::open(
            window_id,
            WindowKind::Discard,
            from,
            tile,
            candidates,
            now_ms,
            deadline,
        );
        invariant::assert_no_simultaneous_non_ron(&window);
        self.window = Some(window);
        self.phase = Phase::Reaction;
    }

    fn apply_response(
        &mut self,
        seat: Seat,
        window_id: u32,
        response: CallResponse,
        now_ms: u64,
    ) -> Result<(), Reject> {
        let Some(window) = self.window.as_mut() else {
            return Err(Reject::NoWindow);
        };
        if window.id() != window_id {
            return Err(Reject::StaleWindow);
        }
        window.respond(seat, response).map_err(Reject::Window)?;
        self.charge(seat, now_ms);
        self.resolve_window(now_ms);
        Ok(())
    }

    fn resolve_window(&mut self, now_ms: u64) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let outcome = window.resolve(now_ms, self.state.rules.min_reaction_window_ms);
        match outcome {
            Outcome::Pending => {}
            Outcome::PassAll => {
                let from = window.from();
                self.close_window_with_passes();
                self.advance_after_pass(from, now_ms);
            }
            // 鳴きと和了は Task 3 と Task 4 で実装する。
            Outcome::Call { .. } | Outcome::Ron(_) => {
                unimplemented!("鳴きと和了は Task 3 / Task 4 で実装する")
            }
        }
    }

    /// 見逃しを記録してウィンドウを閉じる。
    ///
    /// **候補があった席にだけ `ActionPassed` を出す。**候補が無かった席にも
    /// 出すと、誰が鳴けたのかが牌譜から漏れる。
    fn close_window_with_passes(&mut self) {
        let Some(window) = self.window.take() else {
            return;
        };
        for seat in Seat::ALL {
            let declined = std::mem::take(&mut self.offered[seat.index()]);
            if declined.is_empty() {
                continue;
            }
            // ロンを見逃したなら同巡内フリテンになる。
            if declined.iter().any(|o| matches!(o, ActionOption::Ron)) {
                let hand = self.state.seat(seat).hand.clone();
                let melds = self.state.seat(seat).melds.len() as u8;
                let waits = mahjong_core::wait::waiting_tiles(
                    &mahjong_core::hand::HandCounts::from_tiles(&hand),
                    melds,
                );
                self.state.seat_mut(seat).passed_this_turn.extend(waits);
            }
            self.outstanding[seat.index()] = None;
            self.emit(Event::ActionPassed {
                seat,
                window_id: window.id(),
                declined,
            });
        }
    }

    /// 誰も鳴かなかったので下家がツモる。
    fn advance_after_pass(&mut self, from: Seat, now_ms: u64) {
        if self.state.wall.live_remaining() == 0 {
            // 荒牌平局は Task 4 で実装する。
            unimplemented!("荒牌平局は Task 4 で実装する")
        }
        let next = Seat::new(((from.index() + 1) % 4) as u8);
        self.draw_for(next, DrawSource::Wall);
        self.request_turn(now_ms);
    }

    /// 応答を受け取った席のバンクを課金する。
    fn charge(&mut self, seat: Seat, now_ms: u64) {
        let Some(open) = self.outstanding[seat.index()].take() else {
            return;
        };
        let bank = self.state.seat(seat).think_bank_ms;
        let elapsed = now_ms.saturating_sub(open.issued_at_ms);
        self.state.seat_mut(seat).think_bank_ms =
            timing::charge_bank(&self.state.rules, bank, elapsed, open.lead_in_ms);
    }
}
```

**`ReactionWindow` の `candidates` は private であり、読み出す API は無い。**
`reaction.rs` は Wave 2a の凍結物なので getter を足せない。したがって
`RoundEngine` は `open_reaction` で作った候補を**自分で保持する**。

```rust
/// いま開いているウィンドウで、各席へ提示した候補。
/// ActionPassed の declined に使う。ReactionWindow からは読み出せない。
offered: [Vec<ActionOption>; 4],
```

`open_reaction` が `self.offered = candidates.clone();` を置き、ウィンドウを
閉じるときに読む。閉じたら空に戻す。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine discard_tests`
Expected: 14テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 打牌と反応ウィンドウを実装"
```

---

### Task 3: 鳴きの成立

`Outcome::Call` を受けて副露を作り、鳴いた席の手番にする。

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**Interfaces:**
- Consumes: `Outcome::Call { seat, response }`
- 明槓は Wave 2d が扱う。`CallResponse::Kan` が来たら `Reject::NotOffered` を返す

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod call_tests {
    use super::start_tests::{rules, start_at};
    use super::*;
    use protocol::command::{CallResponse, Command};
    use protocol::meld::MeldKind;
    use protocol::notation::parse_tile;

    /// 席1がポンできる局面を作る。配牌に頼らず、手牌を直接置く。
    fn state_where_seat_one_can_pon(engine: &mut RoundEngine, tile: &str) -> Tile {
        let target = parse_tile(tile).expect("正しい記法");
        let hand = &mut engine.state_mut().seat_mut(Seat::new(1)).hand;
        hand[0] = target;
        hand[1] = target;
        target
    }

    /// ポンすると Call が出て、鳴いた席の手番になる。ツモは無い。
    #[test]
    fn a_pon_gives_the_turn_to_the_caller_without_a_draw() {
        let mut engine = start_at(0);
        engine.drain_events();
        let target = state_where_seat_one_can_pon(&mut engine, "5p");

        // 親に 5p を切らせる。
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = target;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: target,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();

        let window_id = engine.next_window_id() - 1;
        engine
            .apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pon {
                        tiles: [target, target],
                    },
                },
                1_400,
            )
            .expect("ポンできる");

        let events = engine.drain_events();
        let Some(Event::Call {
            seat, from, kind, ..
        }) = events
            .iter()
            .find(|e| matches!(e, Event::Call { .. }))
            .cloned()
        else {
            panic!("Call が出ていない: {events:?}");
        };
        assert_eq!(seat, Seat::new(1));
        assert_eq!(from, Seat::new(0));
        assert_eq!(kind, MeldKind::Pon);

        // ツモは無い。
        assert!(!events.iter().any(|e| matches!(e, Event::Draw { .. })));
        assert_eq!(
            *engine.phase(),
            Phase::Turn {
                seat: Seat::new(1),
                start: TurnStart::AfterCall
            }
        );
    }

    /// ポンした牌は手から抜けて副露に入る。総数は変わらない。
    #[test]
    fn a_pon_moves_two_tiles_from_the_hand_into_a_meld() {
        let mut engine = start_at(0);
        engine.drain_events();
        let target = state_where_seat_one_can_pon(&mut engine, "5p");
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = target;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: target,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();

        let window_id = engine.next_window_id() - 1;
        engine
            .apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pon {
                        tiles: [target, target],
                    },
                },
                1_400,
            )
            .expect("ポンできる");
        engine.drain_events();

        let seat = engine.state().seat(Seat::new(1));
        assert_eq!(seat.hand.len(), 11);
        assert_eq!(seat.melds.len(), 1);
        assert_eq!(seat.melds[0].tiles.len(), 3);
        assert_eq!(seat.melds[0].from, Some(Seat::new(0)));
        assert_eq!(seat.melds[0].called_tile, Some(target));
        crate::invariant::assert_tiles_conserved(engine.state());
    }

    /// 鳴かれた牌は河に残り、誰に鳴かれたかが記録される。
    #[test]
    fn a_called_tile_stays_in_the_river_and_records_the_caller() {
        let mut engine = start_at(0);
        engine.drain_events();
        let target = state_where_seat_one_can_pon(&mut engine, "5p");
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = target;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: target,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        let window_id = engine.next_window_id() - 1;
        engine
            .apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pon {
                        tiles: [target, target],
                    },
                },
                1_400,
            )
            .expect("ポンできる");

        let river = &engine.state().seat(Seat::new(0)).river;
        assert_eq!(river.len(), 1);
        assert_eq!(river[0].called_by, Some(Seat::new(1)));
    }

    /// 鳴かれた側は流し満貫の資格を失う。
    #[test]
    fn being_called_ends_the_discarders_nagashi_claim() {
        let mut engine = start_at(0);
        engine.drain_events();
        // 幺九牌をポンさせる。切った側は幺九牌しか切っていない。
        let target = state_where_seat_one_can_pon(&mut engine, "1z");
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = target;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: target,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        assert!(
            engine.state().seat(Seat::new(0)).nagashi_alive,
            "幺九牌を切っただけでは失わない"
        );

        let window_id = engine.next_window_id() - 1;
        engine
            .apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pon {
                        tiles: [target, target],
                    },
                },
                1_400,
            )
            .expect("ポンできる");
        assert!(!engine.state().seat(Seat::new(0)).nagashi_alive);
    }

    /// 鳴きが1回でも入れば any_call_made が立つ。
    #[test]
    fn a_call_marks_the_round_as_opened() {
        let mut engine = start_at(0);
        engine.drain_events();
        assert!(!engine.state().any_call_made);
        let target = state_where_seat_one_can_pon(&mut engine, "5p");
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = target;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: target,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        let window_id = engine.next_window_id() - 1;
        engine
            .apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pon {
                        tiles: [target, target],
                    },
                },
                1_400,
            )
            .expect("ポンできる");
        assert!(engine.state().any_call_made);
    }

    /// 鳴いた席は打牌しかできない。ツモも九種九牌も出ない。
    #[test]
    fn a_caller_is_only_offered_a_discard() {
        let mut engine = start_at(0);
        engine.drain_events();
        let target = state_where_seat_one_can_pon(&mut engine, "5p");
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = target;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: target,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        let window_id = engine.next_window_id() - 1;
        engine
            .apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pon {
                        tiles: [target, target],
                    },
                },
                1_400,
            )
            .expect("ポンできる");

        let events = engine.drain_events();
        let Some(Event::RequestAction { seat, options, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::RequestAction { .. }))
            .cloned()
        else {
            panic!("要求が出ていない");
        };
        assert_eq!(seat, Seat::new(1));
        assert_eq!(options.len(), 1);
        assert!(matches!(options[0], ActionOption::Discard { .. }));
    }

    /// 明槓は Wave 2d の担当。いまは拒否する。
    #[test]
    fn a_minkan_is_not_accepted_yet() {
        let mut engine = start_at(0);
        engine.drain_events();
        let target = state_where_seat_one_can_pon(&mut engine, "5p");
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = target;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: target,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        let window_id = engine.next_window_id() - 1;
        assert_eq!(
            engine.apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Kan,
                },
                1_400
            ),
            Err(Reject::NotOffered)
        );
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine call_tests`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
use protocol::meld::{Meld, MeldKind};

impl RoundEngine {
    /// テストが局面を組み立てるために使う。
    #[cfg(test)]
    pub(crate) fn state_mut(&mut self) -> &mut RoundState {
        &mut self.state
    }

    fn apply_call(&mut self, seat: Seat, response: CallResponse, now_ms: u64) {
        let window = self.window.take().expect("反応ウィンドウが開いている");
        let from = window.from();
        let called = window.tile();

        let (kind, from_hand) = match response {
            CallResponse::Chi { tiles } => (MeldKind::Chi, tiles),
            CallResponse::Pon { tiles } => (MeldKind::Pon, tiles),
            // 明槓は Wave 2d が扱う。apply_response が先に弾く。
            _ => unreachable!("鳴き以外がここへ来ることはない"),
        };

        // 手から2枚抜く。同じ牌が複数あっても1枚ずつ取り除く。
        for tile in from_hand {
            let hand = &mut self.state.seat_mut(seat).hand;
            let position = hand
                .iter()
                .position(|t| *t == tile)
                .expect("候補として提示した牌は手にある");
            hand.remove(position);
        }

        let mut tiles = from_hand.to_vec();
        tiles.push(called);
        self.state.seat_mut(seat).melds.push(Meld {
            kind,
            tiles: tiles.clone(),
            from: Some(from),
            called_tile: Some(called),
        });

        // 鳴かれた側は流し満貫の資格を失う。河にも誰に鳴かれたかを残す。
        self.state.seat_mut(from).nagashi_alive = false;
        if let Some(last) = self.state.seat_mut(from).river.last_mut() {
            last.called_by = Some(seat);
        }
        self.state.any_call_made = true;

        // 見逃した席の記録は、鳴きが成立した場合も残す。
        self.record_passes_after(&window, seat);

        self.emit(Event::Call {
            seat,
            from,
            kind,
            tiles,
        });
        invariant::assert_tiles_conserved(&self.state);

        self.phase = Phase::Turn {
            seat,
            start: TurnStart::AfterCall,
        };
        self.request_turn(now_ms);
        let _ = now_ms;
    }
}
```

`apply_response` の `CallResponse::Kan` を先に弾く。

```rust
        // 明槓は Wave 2d が扱う。
        if response == CallResponse::Kan {
            return Err(Reject::NotOffered);
        }
```

`resolve_window` の `Outcome::Call` を結線する。

```rust
            Outcome::Call { seat, response } => self.apply_call(seat, response, now_ms),
```

`record_passes_after` は `close_window_with_passes` と同じ処理を、鳴いた席を
除いて行う。**鳴いた席には `ActionPassed` を出さない。**

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine call_tests`
Expected: 7テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 鳴きの成立を実装"
```

---

### Task 4: 和了と荒牌平局

局を終わらせる。ツモ和了・ロン和了・荒牌平局・流し満貫。

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**Interfaces:**
- Produces:
  - `pub struct RoundOutcome { pub scores: [i32; 4], pub riichi_sticks: u8, pub dealer_repeats: bool }`
  - `RoundEngine::outcome(&self) -> Option<&RoundOutcome>`
- Consumes: Wave 2b の `settle_agari` / `settle_exhaustive` / `settle_nagashi` / `score_change` / `AgariInput`

**供託の扱い:**

| 終わり方 | 供託 |
|---|---|
| 和了 | 和了者が回収する。`score_change` が足し込み、`riichi_sticks` は0になる |
| 荒牌平局 | 持ち越す。`riichi_sticks` はそのまま次局へ渡す |
| 流し満貫 | 持ち越す。和了ではないため |

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod ending_tests {
    use super::start_tests::{rules, start_at};
    use super::*;
    use protocol::command::{CallResponse, Command};
    use protocol::event::RyuukyokuKind;
    use protocol::notation::{parse_hand, parse_tile};

    /// 席1へ和了形の一歩手前を持たせる。234567m 23478p 22s は 6p/9p 待ち。
    fn make_tenpai(engine: &mut RoundEngine, seat: Seat) {
        engine.state_mut().seat_mut(seat).hand =
            parse_hand("234567m23478p22s").expect("正しい記法");
    }

    /// 山を空にする。荒牌平局を起こすため。
    fn drain_the_wall(engine: &mut RoundEngine) {
        while engine.state().wall.live_remaining() > 0 {
            engine.state_mut().wall.draw().expect("残っている");
        }
    }

    /// ロンすると Agari が出て、局が終わる。
    #[test]
    fn a_ron_ends_the_round() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(&mut engine, Seat::new(1));
        let winning = parse_tile("6p").expect("正しい記法");
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = winning;

        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: winning,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();

        let window_id = engine.next_window_id() - 1;
        engine
            .apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Ron,
                },
                1_400,
            )
            .expect("ロンできる");

        let events = engine.drain_events();
        let Some(Event::Agari {
            results,
            settlement,
        }) = events
            .iter()
            .find(|e| matches!(e, Event::Agari { .. }))
            .cloned()
        else {
            panic!("Agari が出ていない: {events:?}");
        };
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].seat, Seat::new(1));
        assert_eq!(results[0].from, Some(Seat::new(0)));
        assert!(settlement.is_balanced());
        assert_eq!(*engine.phase(), Phase::Done);
        // RoundEnd は出さない。次局を決めるのは Wave 2e である。
        assert!(!events.iter().any(|e| matches!(e, Event::RoundEnd { .. })));
    }

    /// 和了で点棒が動く。合計は変わらない。
    #[test]
    fn a_ron_moves_points_without_creating_any() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(&mut engine, Seat::new(1));
        let winning = parse_tile("6p").expect("正しい記法");
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = winning;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: winning,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        let window_id = engine.next_window_id() - 1;
        engine
            .apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Ron,
                },
                1_400,
            )
            .expect("ロンできる");
        engine.drain_events();

        let outcome = engine.outcome().expect("終わっている");
        assert_eq!(outcome.scores.iter().sum::<i32>(), 100_000);
        assert!(outcome.scores[1] > 25_000, "和了者が増えている");
        assert!(outcome.scores[0] < 25_000, "放銃者が減っている");
    }

    /// 供託は和了者が回収し、残高は0になる。
    #[test]
    fn a_win_collects_the_riichi_sticks() {
        let mut engine = RoundEngine::start(
            rules(),
            Round {
                wind: protocol::seat::Wind::East,
                number: 1,
            },
            Seat::new(0),
            0,
            2, // 供託2本
            [25_000; 4],
            &super::start_tests::seed(),
            1,
            0,
        );
        engine.drain_events();
        make_tenpai(&mut engine, Seat::new(1));
        let winning = parse_tile("6p").expect("正しい記法");
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = winning;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: winning,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        let window_id = engine.next_window_id() - 1;
        engine
            .apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Ron,
                },
                1_400,
            )
            .expect("ロンできる");
        engine.drain_events();

        let outcome = engine.outcome().expect("終わっている");
        assert_eq!(outcome.riichi_sticks, 0, "供託は回収された");
        assert_eq!(
            outcome.scores.iter().sum::<i32>(),
            100_000 + 2_000,
            "供託2本が卓へ戻る"
        );
    }

    /// ツモ和了すると Agari が出て、局が終わる。
    #[test]
    fn a_tsumo_ends_the_round() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(&mut engine, Seat::new(0));
        engine.force_draw_turn(Seat::new(0), parse_tile("6p").expect("正しい記法"));

        engine
            .apply(Seat::new(0), Command::Tsumo, 1_000)
            .expect("ツモ和了できる");
        let events = engine.drain_events();
        let Some(Event::Agari { results, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Agari { .. }))
            .cloned()
        else {
            panic!("Agari が出ていない: {events:?}");
        };
        assert_eq!(results[0].seat, Seat::new(0));
        assert_eq!(results[0].from, None, "ツモなので放銃者はいない");
        assert_eq!(*engine.phase(), Phase::Done);
    }

    /// 親の和了は連荘になる。
    #[test]
    fn a_dealer_win_repeats_the_dealership() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(&mut engine, Seat::new(0));
        engine.force_draw_turn(Seat::new(0), parse_tile("6p").expect("正しい記法"));
        engine
            .apply(Seat::new(0), Command::Tsumo, 1_000)
            .expect("ツモ和了できる");
        engine.drain_events();
        assert!(engine.outcome().expect("終わっている").dealer_repeats);
    }

    /// 和了形でない席はツモ和了できない。
    #[test]
    fn a_seat_without_a_winning_hand_cannot_declare_tsumo() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(&mut engine, Seat::new(0));
        engine.force_draw_turn(Seat::new(0), parse_tile("1z").expect("正しい記法"));
        assert_eq!(
            engine.apply(Seat::new(0), Command::Tsumo, 1_000),
            Err(Reject::NotOffered)
        );
    }

    /// 子の和了は親が流れる。
    #[test]
    fn a_non_dealer_win_moves_the_dealership() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(&mut engine, Seat::new(1));
        let winning = parse_tile("6p").expect("正しい記法");
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = winning;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: winning,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        let window_id = engine.next_window_id() - 1;
        engine
            .apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Ron,
                },
                1_400,
            )
            .expect("ロンできる");
        engine.drain_events();
        assert!(!engine.outcome().expect("終わっている").dealer_repeats);
    }

    /// 山が尽きたら荒牌平局になる。
    #[test]
    fn an_empty_wall_ends_the_round_in_a_draw() {
        let mut engine = start_at(0);
        engine.drain_events();
        drain_the_wall(&mut engine);

        let seat = Seat::new(0);
        let tile = engine.state().seat(seat).hand[0];
        engine
            .apply(
                seat,
                Command::Discard {
                    tile,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        engine.tick(1_000 + rules().min_reaction_window_ms as u64);

        let events = engine.drain_events();
        let Some(Event::Ryuukyoku { kind, tenpai, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Ryuukyoku { .. }))
            .cloned()
        else {
            panic!("Ryuukyoku が出ていない: {events:?}");
        };
        assert_eq!(kind, RyuukyokuKind::Exhaustive);
        assert_eq!(tenpai.len(), 4);
        assert_eq!(*engine.phase(), Phase::Done);
    }

    /// テンパイ料は合計3000点で釣り合う。
    #[test]
    fn the_noten_penalty_balances() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(&mut engine, Seat::new(1));
        drain_the_wall(&mut engine);

        let seat = Seat::new(0);
        let tile = engine.state().seat(seat).hand[0];
        engine
            .apply(
                seat,
                Command::Discard {
                    tile,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        engine.tick(1_000 + rules().min_reaction_window_ms as u64);
        engine.drain_events();

        let outcome = engine.outcome().expect("終わっている");
        assert_eq!(outcome.scores.iter().sum::<i32>(), 100_000);
    }

    /// 荒牌平局では供託が持ち越される。
    #[test]
    fn a_draw_carries_the_riichi_sticks_forward() {
        let mut engine = RoundEngine::start(
            rules(),
            Round {
                wind: protocol::seat::Wind::East,
                number: 1,
            },
            Seat::new(0),
            0,
            1,
            [25_000; 4],
            &super::start_tests::seed(),
            1,
            0,
        );
        engine.drain_events();
        drain_the_wall(&mut engine);
        let seat = Seat::new(0);
        let tile = engine.state().seat(seat).hand[0];
        engine
            .apply(
                seat,
                Command::Discard {
                    tile,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        engine.tick(1_000 + rules().min_reaction_window_ms as u64);
        engine.drain_events();

        assert_eq!(engine.outcome().expect("終わっている").riichi_sticks, 1);
    }

    /// 荒牌平局の直前でも、最後の打牌にロンがあれば和了が優先される。
    #[test]
    fn a_ron_on_the_last_discard_beats_the_draw() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(&mut engine, Seat::new(1));
        drain_the_wall(&mut engine);

        let winning = parse_tile("6p").expect("正しい記法");
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = winning;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: winning,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        let window_id = engine.next_window_id() - 1;
        engine
            .apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Ron,
                },
                1_400,
            )
            .expect("ロンできる");

        let events = engine.drain_events();
        assert!(events.iter().any(|e| matches!(e, Event::Agari { .. })));
        assert!(!events.iter().any(|e| matches!(e, Event::Ryuukyoku { .. })));
    }

    /// 流し満貫が成立すればテンパイ料は発生しない。
    #[test]
    fn a_nagashi_replaces_the_noten_penalty() {
        let mut engine = start_at(0);
        engine.drain_events();
        // 席2だけ幺九牌しか切っていないことにする。
        for seat in Seat::ALL {
            engine.state_mut().seat_mut(seat).nagashi_alive = seat == Seat::new(2);
        }
        drain_the_wall(&mut engine);
        let seat = Seat::new(0);
        let tile = engine.state().seat(seat).hand[0];
        engine
            .apply(
                seat,
                Command::Discard {
                    tile,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        engine.tick(1_000 + rules().min_reaction_window_ms as u64);

        let events = engine.drain_events();
        let Some(Event::Ryuukyoku {
            nagashi_winners,
            settlement,
            ..
        }) = events
            .iter()
            .find(|e| matches!(e, Event::Ryuukyoku { .. }))
            .cloned()
        else {
            panic!("Ryuukyoku が出ていない");
        };
        assert_eq!(nagashi_winners, vec![Seat::new(2)]);
        // 子の流し満貫。親から4000、子から2000ずつ。
        assert_eq!(settlement.delta, [-4_000, -2_000, 8_000, -2_000]);
    }

    /// 局が終わったあとはコマンドを受け付けない。
    #[test]
    fn a_finished_round_rejects_further_commands() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(&mut engine, Seat::new(1));
        let winning = parse_tile("6p").expect("正しい記法");
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = winning;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: winning,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        let window_id = engine.next_window_id() - 1;
        engine
            .apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Ron,
                },
                1_400,
            )
            .expect("ロンできる");
        engine.drain_events();

        assert_eq!(
            engine.apply(
                Seat::new(2),
                Command::Discard {
                    tile: winning,
                    riichi: false
                },
                2_000
            ),
            Err(Reject::NotYourTurn)
        );
    }

    /// ツモ切りだけで局を最後まで回せる。
    #[test]
    fn a_round_of_tsumogiri_reaches_an_ending() {
        let mut engine = start_at(0);
        engine.drain_events();
        let mut now = 1_000u64;

        // 王牌を除く122枚を引き切るまでには必ず終わる。
        for _ in 0..200 {
            if *engine.phase() == Phase::Done {
                break;
            }
            engine.tick(now);
            engine.drain_events();
            crate::invariant::assert_tiles_conserved(engine.state());
            now += 100_000;
        }
        assert_eq!(*engine.phase(), Phase::Done, "局が終わらなかった");
        assert_eq!(
            engine.outcome().expect("終わっている").scores.iter().sum::<i32>()
                + engine.outcome().expect("終わっている").riichi_sticks as i32 * 1_000,
            100_000
        );
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine ending_tests`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
use crate::round::{score_change, settle_agari, settle_exhaustive, settle_nagashi, AgariInput};
use mahjong_core::score::{score, WinType};
use mahjong_core::wait::waiting_tiles;
use mahjong_core::hand::HandCounts;
use protocol::event::{AgariResult, RyuukyokuKind};

/// 局が終わったときの結果。Wave 2e の `MatchEngine` が次局を組み立てるのに使う。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RoundOutcome {
    pub scores: [i32; 4],
    pub riichi_sticks: u8,
    pub dealer_repeats: bool,
}

impl RoundEngine {
    pub fn outcome(&self) -> Option<&RoundOutcome> {
        self.outcome.as_ref()
    }

    /// ロンを確定させる。ダブロンなら席順で並べる。
    fn finish_with_ron(&mut self, winners: Vec<Seat>) {
        let window = self.window.take().expect("反応ウィンドウが開いている");
        let from = window.from();
        let tile = window.tile();

        let mut inputs = Vec::new();
        let mut results = Vec::new();
        for seat in &winners {
            let context = self.state.hand_context(*seat, WinType::Ron);
            let s = self.state.seat(*seat);
            let result = score(&s.hand, &s.melds, tile, &context, &self.state.rules)
                .expect("ロンを提示した以上、役がある");
            inputs.push(AgariInput {
                seat: *seat,
                from: Some(from),
                payment: result.payment,
                liability: None, // 責任払いは Wave 2d で結線する
            });
            results.push(AgariResult {
                seat: *seat,
                from: Some(from),
                hand: s.hand.clone(),
                melds: s.melds.clone(),
                win_tile: tile,
                yaku: result.yaku.clone(),
                fu: result.fu,
                han: result.han,
                score: payment_total(&result.payment),
                liability: None,
                // リーチ和了のみ Some。空配列との使い分けに頼らない設計である。
                ura_indicators: None,
            });
        }

        let settlement = settle_agari(
            &inputs,
            self.state.dealer,
            self.state.honba,
            self.state.riichi_sticks,
        );
        self.emit(Event::Agari {
            results,
            settlement: settlement.clone(),
        });

        let dealer_repeats = winners.contains(&self.state.dealer);
        self.finish(settlement_scores(&self.state, &settlement), 0, dealer_repeats);
    }

    /// 荒牌平局。流し満貫が成立していればテンパイ料は発生しない。
    fn finish_exhaustive(&mut self) {
        let tenpai: [bool; 4] = std::array::from_fn(|i| {
            let seat = self.state.seat(Seat::new(i as u8));
            !waiting_tiles(&HandCounts::from_tiles(&seat.hand), seat.melds.len() as u8).is_empty()
        });
        let nagashi_winners: Vec<Seat> = Seat::ALL
            .iter()
            .copied()
            .filter(|s| self.state.seat(*s).nagashi_alive)
            .collect();

        let settlement = if nagashi_winners.is_empty() {
            settle_exhaustive(tenpai, &self.state.rules)
        } else {
            settle_nagashi(&nagashi_winners, self.state.dealer)
        };

        // テンパイしている席の手牌だけを開く。
        let revealed_hands: Vec<(Seat, Vec<Tile>)> = Seat::ALL
            .iter()
            .copied()
            .filter(|s| tenpai[s.index()])
            .map(|s| (s, self.state.seat(s).hand.clone()))
            .collect();

        self.emit(Event::Ryuukyoku {
            kind: RyuukyokuKind::Exhaustive,
            initiator: None,
            tenpai,
            revealed_hands,
            nagashi_winners,
            settlement: settlement.clone(),
        });

        // 供託は持ち越す。テンパイ料は供託を動かさない。
        let mut scores = self.state.scores;
        for seat in Seat::ALL {
            scores[seat.index()] += settlement.delta[seat.index()];
        }
        let dealer_repeats = tenpai[self.state.dealer.index()];
        self.finish(scores, self.state.riichi_sticks, dealer_repeats);
    }

    /// 局を閉じる。
    fn finish(&mut self, scores: [i32; 4], riichi_sticks: u8, dealer_repeats: bool) {
        let before = self.state.scores;
        let sticks_delta =
            (riichi_sticks as i32 - self.state.riichi_sticks as i32) * crate::state::RIICHI_STICK;
        invariant::assert_scores_conserved(&before, &scores, sticks_delta);

        self.state.scores = scores;
        self.state.riichi_sticks = riichi_sticks;
        self.phase = Phase::Done;
        self.window = None;
        self.outstanding = [None; 4];

        // **`RoundEnd` はここで出さない。**`next: NextRound` を決めるには
        // 半荘全体の状況（西入・アガリ止め・飛び）が要る。それを知るのは
        // Wave 2e の `MatchEngine` だけである。局は結果を `RoundOutcome`
        // で返し、`RoundEnd` の発行は呼び出し側に任せる。
        self.outcome = Some(RoundOutcome {
            scores,
            riichi_sticks,
            dealer_repeats,
        });
    }
}

/// 素点の合計。`AgariResult.score` は供託も本場も含まない。
fn payment_total(payment: &mahjong_core::score::Payment) -> i32 {
    use mahjong_core::score::Payment;
    match payment {
        Payment::Ron { total } => *total,
        Payment::TsumoDealer { from_each } => from_each * 3,
        Payment::TsumoNonDealer {
            from_dealer,
            from_each_non_dealer,
        } => from_dealer + from_each_non_dealer * 2,
    }
}

/// 和了後の持ち点。供託は `score_change` が足し込む。
fn settlement_scores(state: &RoundState, settlement: &protocol::event::Settlement) -> [i32; 4] {
    let change = score_change(settlement);
    std::array::from_fn(|i| state.scores[i] + change[i])
}
```

`resolve_window` と `advance_after_pass` の `unimplemented!` を差し替える。

```rust
            Outcome::Ron(winners) if winners.len() == 3 => {
                // 三家和は Wave 2d が流局にする。
                unimplemented!("三家和は Wave 2d で実装する")
            }
            Outcome::Ron(winners) => self.finish_with_ron(winners),
```

```rust
        if self.state.wall.live_remaining() == 0 {
            self.finish_exhaustive();
            return;
        }
```

`Command::Tsumo` を `apply` へ結線する。手番の席が `ActionOption::Tsumo` を
提示されている場合のみ受け付ける。

**`RoundEngine` へ `outcome: Option<RoundOutcome>` フィールドを足し、
`start` で `None` に初期化すること。**

テストが局面を組み立てるための補助も同じファイルに置く。

```rust
/// 指定した席のツモ番を直接作る。手牌が13枚であることを前提にする。
/// 自然な進行では狙った牌をツモらせられないので、テストだけが使う。
#[cfg(test)]
pub(crate) fn force_draw_turn(&mut self, seat: Seat, tile: Tile) {
    assert_eq!(
        self.state.seat(seat).hand.len(),
        13,
        "13枚の手へ1枚足して14枚にする"
    );
    self.state.seat_mut(seat).hand.push(tile);
    self.state.last_draw = Some((seat, DrawSource::Wall));
    self.state.draw_count[seat.index()] += 1;
    self.phase = Phase::Turn {
        seat,
        start: TurnStart::Draw {
            tile,
            source: DrawSource::Wall,
        },
    };
    self.outstanding[seat.index()] = Some(Outstanding {
        window_id: 0,
        issued_at_ms: 0,
        lead_in_ms: 0,
    });
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine ending_tests`
Expected: 14テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 和了と荒牌平局を実装"
```

---

## Wave 2c 完了の判定

- [ ] `cargo test --workspace` が通る
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] Task 1 の12テスト、Task 2 の14テスト、Task 3 の7テスト、Task 4 の14テストがすべて通る
- [ ] ツモ切りだけで局が最後まで回る
- [ ] 同じシード・同じ時刻列から同じイベント列が出る
- [ ] すべての局面で牌136枚が保たれる
- [ ] 点棒と供託の合計が100000点で保たれる
- [ ] `match_flow.rs` 以外を編集していない

## Wave 2d へ渡すもの

| 部品 | Wave 2d での使われ方 |
|---|---|
| `Phase` | `Chankan` を足す |
| `apply` | `Command::{Ankan, Kakan, Kyuushu}` と `riichi: true` を結線する |
| `resolve_window` | `Outcome::Ron` の3人を三家和にする |
| `finish` | 途中流局の `RyuukyokuKind` を渡す |
| `finish_with_ron` | `liability` と `ura_indicators` を埋める |
| `RoundOutcome` | 途中流局でも同じ形で返す |
