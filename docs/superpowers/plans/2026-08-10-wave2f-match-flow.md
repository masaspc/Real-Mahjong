# Wave 2f: 半荘の進行 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 局を並べて半荘にする。連荘・本場・西入・アガリ止め・飛び・順位までを扱い、東1局から終局まで通しで回せる状態にする。

**これでエンジンが完成する。** 残るのはサーバとクライアントの結線であり、別のウェーブが担当する。

**Architecture:** `MatchEngine` が `RoundEngine` を局ごとに作り、`RoundOutcome` を受けて次局を決める。乱数はここにも入れない。**シードは局ごとに外から渡す。**

**Tech Stack:** Rust 1.97.1 / edition 2021 / `protocol`・`mahjong-core`・Wave 2a〜2e の成果物

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
**作業規約:** `AGENTS.md`
**前提:** Wave 2e がマージ済みであること（engine のテストが262件通ること）

## Global Constraints

- **編集してよいのは `crates/mahjong-engine/src/match_flow.rs` だけである**
- **`round.rs` / `settlement.rs` / `wall.rs` / `reaction.rs` / `state.rs` / `timing.rs` / `invariant.rs` を編集しない**
- **`lib.rs` を編集しない。** Wave 0 で凍結済みである
- `crates/protocol` と `crates/mahjong-core` は凍結済み。**編集も追加もしない**
- **時刻を直接読まない。** `Instant::now()` / `SystemTime::now()` / `rand` を呼ばない
- **既存の262件のテストを1つも壊さない**
- 完了条件は `cargo test --workspace` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` がすべて通ること

## シードを外から渡す

**局が何回あるかは事前に決まらない。**連荘と本場で伸びるためである。だから
シードの配列を先に受け取る形にはしない。局を始めるたびに1つ受け取る。

```rust
let mut game = MatchEngine::start(rules, players, now_ms);
while !game.is_over() {
    if game.needs_seed() {
        game.begin_round(&next_seed(), now_ms);   // 乱数は呼び出し側
    }
    // apply / tick
}
```

`MatchEngine` は受け取ったシードを溜めておき、**半荘の終わりに `SeedReveal` で
まとめて開示する。**局ごとに開示すると、その局の他家の手牌を遡って復元でき、
同じ半荘の中で不公平が生じる（仕様 5.5）。

## `RoundOutcome` に足りないもの

**`ContinuationReason::DealerLoss` だけでは本場の進み方が決まらない。**

| 終わり方 | `reason` | 本場 |
|---|---|---|
| 子の和了 | `DealerLoss` | **0 に戻す** |
| 荒牌平局・親ノーテン | `DealerLoss` | **+1** |

同じ `DealerLoss` で挙動が違う。局の側は流局かどうかを知っているので、
`RoundOutcome` へ足す。

```rust
    /// 流局で終わったか。本場の進み方が和了と違う。
    pub was_draw: bool,
```

## コーディネータが確定させたルール

| 項目 | 決定 |
|---|---|
| 本場（和了） | 親の和了なら +1、子の和了なら 0 に戻す |
| 本場（流局） | 荒牌平局も途中流局も +1。親が続くかは別に決まる |
| 親の移動 | `RoundOutcome.dealer_repeats` に従う。流れるときは下家へ |
| 局数 | 親が流れたら `round.number += 1`。4 を超えたら次の風の1局へ |
| 最終局 | 半荘は南4局、東風戦は東4局 |
| 西入 | 最終局を終えて誰も `return_score`（30000）に届いていなければ、次の風へ延長する。半荘なら西場、東風戦なら南場 |
| 延長中の終局 | 誰かが返し点へ届いた局の終わりで終局する |
| 延長の打ち切り | 延長した風の4局を終えたら、誰も届いていなくても終局する。**連荘でも打ち切る。**無限に伸ばさない |
| アガリ止め | **終局条件を満たしている最終局**で親が和了し、かつ親がトップなら続行しない |
| テンパイ止め | 同じ条件の荒牌平局で親がテンパイし、かつ親がトップなら続行しない。**流し満貫も荒牌平局の一種なので含める** |
| 止めが効かない場合 | 誰も返し点へ届いていなければ延長する。**そもそも半荘が終わらないので、止める場面ではない。**親がトップでも延長する |
| 飛び | `Ruleset.busted_ends_match` が真で、誰かの持ち点が0未満になったら即終局 |
| `MatchEnd.final_scores` | **素点をそのまま入れる。**ウマとオカはポイントの計算であって点棒ではない。段位とレートの計算は Wave 3 が `placements` から行う |
| 順位 | 持ち点の多い順。同点は席順（起家に近いほう）が上位 |
| 供託の持ち越し | 局をまたいで持ち越す。終局時に残っていてもトップへは渡さない（素点を動かさない） |
| `window_id` | 半荘を通して単調増加する。次局の `first_window_id` に前局の `next_window_id()` を渡す |

---

## タスクの依存関係

```
1 骨格と委譲 → 2 局の進め方 → 3 終局と順位
```

直列である。同じ構造体を段階的に育てる。

---

### Task 1: 骨格と委譲

`MatchStart` を出し、局を1つ動かし、終わったら `RoundEnd` を出すところまで。
**次局へは進まない。**

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**Interfaces:**
- Produces:
  - `pub struct MatchEngine`
  - `MatchEngine::start(rules: Ruleset, players: [PlayerId; 4], now_ms: u64) -> Self`
  - `MatchEngine::needs_seed(&self) -> bool`
  - `MatchEngine::begin_round(&mut self, seed: &Seed, now_ms: u64)`
  - `MatchEngine::apply(&mut self, seat: Seat, command: Command, now_ms: u64) -> Result<(), Reject>`
  - `MatchEngine::tick(&mut self, now_ms: u64)`
  - `MatchEngine::drain_events(&mut self) -> Vec<Event>`
  - `MatchEngine::round(&self) -> Round`
  - `MatchEngine::scores(&self) -> [i32; 4]`
- Consumes: `RoundEngine`、`RoundOutcome`

**`RoundOutcome` へ `was_draw` を足すのもこのタスクで行う。**`finish` の
4つの呼び出しへ値を渡す。和了は `false`、荒牌平局と途中流局は `true`。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod match_tests {
    use super::ending_tests::{make_tenpai, set_dealer_hand};
    use super::*;
    use protocol::command::Command;
    use protocol::event::PlayerId;
    use protocol::notation::parse_tile;
    use protocol::ruleset::MatchLength;
    use protocol::seat::Wind;

    pub(super) fn players() -> [PlayerId; 4] {
        [
            PlayerId("p0".to_owned()),
            PlayerId("p1".to_owned()),
            PlayerId("p2".to_owned()),
            PlayerId("p3".to_owned()),
        ]
    }

    pub(super) fn seed_of(index: u8) -> Seed {
        Seed::from_hex(&format!("{index:02x}").repeat(32)).expect("正しい hex")
    }

    pub(super) fn hanchan() -> MatchEngine {
        MatchEngine::start(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            players(),
            0,
        )
    }

    /// 半荘は MatchStart から始まる。まだ局は始まっていない。
    #[test]
    fn a_match_opens_with_its_own_event() {
        let mut game = hanchan();
        let events = game.drain_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Event::MatchStart { .. }));
        assert!(game.needs_seed());
    }

    /// 東1局から始まる。親は起家。
    #[test]
    fn the_first_round_is_east_one() {
        let game = hanchan();
        assert_eq!(
            game.round(),
            Round {
                wind: Wind::East,
                number: 1
            }
        );
        assert_eq!(game.scores(), [25_000; 4]);
    }

    /// シードを渡すと局が始まる。
    #[test]
    fn giving_a_seed_starts_the_round() {
        let mut game = hanchan();
        game.drain_events();
        game.begin_round(&seed_of(1), 0);

        let events = game.drain_events();
        assert!(matches!(events[0], Event::RoundStart { .. }));
        assert!(!game.needs_seed(), "局が動いている間は要らない");
    }

    /// 局のイベントはそのまま流れてくる。
    #[test]
    fn round_events_pass_through() {
        let mut game = hanchan();
        game.drain_events();
        game.begin_round(&seed_of(1), 0);
        let events = game.drain_events();
        // RoundStart / Deal / Draw / RequestAction
        assert_eq!(events.len(), 4);
        assert!(matches!(events[3], Event::RequestAction { .. }));
    }

    /// コマンドは動いている局へ委譲される。
    #[test]
    fn commands_reach_the_running_round() {
        let mut game = hanchan();
        game.drain_events();
        game.begin_round(&seed_of(1), 0);
        game.drain_events();

        let tile = game.round_state().seat(Seat::new(0)).hand[0];
        game.apply(
            Seat::new(0),
            Command::Discard {
                tile,
                riichi: false,
            },
            1_000,
        )
        .expect("切れる");
        let events = game.drain_events();
        assert!(events.iter().any(|e| matches!(e, Event::Discard { .. })));
        // 打牌のあとは反応の待ちに入るので、局はまだ終わっていない。
        assert!(!game.needs_seed());
    }

    /// 局が始まっていなければコマンドは受け付けない。
    #[test]
    fn commands_before_the_round_are_rejected() {
        let mut game = hanchan();
        assert_eq!(
            game.apply(Seat::new(0), Command::Tsumo, 1_000),
            Err(Reject::NotYourTurn)
        );
    }

    /// 局が終わると RoundEnd が出る。
    #[test]
    fn a_finished_round_emits_its_end() {
        let mut game = hanchan();
        game.drain_events();
        game.begin_round(&seed_of(1), 0);
        game.drain_events();
        finish_with_a_dealer_tsumo(&mut game);

        let events = game.drain_events();
        let Some(Event::RoundEnd { scores, reason, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::RoundEnd { .. }))
            .cloned()
        else {
            panic!("RoundEnd が出ていない: {events:?}");
        };
        assert_eq!(reason, ContinuationReason::DealerWin);
        assert_eq!(scores.iter().sum::<i32>(), 100_000);
    }

    /// RoundEnd のあとは次のシードを待つ。
    #[test]
    fn the_match_waits_for_the_next_seed() {
        let mut game = hanchan();
        game.drain_events();
        game.begin_round(&seed_of(1), 0);
        game.drain_events();
        finish_with_a_dealer_tsumo(&mut game);
        game.drain_events();
        assert!(game.needs_seed());
    }

    /// 局の結果が半荘の持ち点に反映される。
    #[test]
    fn the_match_takes_over_the_round_scores() {
        let mut game = hanchan();
        game.drain_events();
        game.begin_round(&seed_of(1), 0);
        game.drain_events();
        finish_with_a_dealer_tsumo(&mut game);
        game.drain_events();

        assert!(game.scores()[0] > 25_000, "親が和了した");
        assert_eq!(game.scores().iter().sum::<i32>(), 100_000);
    }

    /// 流局かどうかを局が伝える。本場の進み方が和了と違うためである。
    #[test]
    fn the_round_reports_whether_it_was_a_draw() {
        let mut game = hanchan();
        game.drain_events();
        game.begin_round(&seed_of(1), 0);
        game.drain_events();
        finish_with_a_dealer_tsumo(&mut game);
        assert_eq!(game.last_outcome().expect("終わっている").was_draw, false);
    }

    /// 途中流局は was_draw が立つ。
    #[test]
    fn an_abortive_draw_reports_itself_as_a_draw() {
        let mut game = hanchan();
        game.drain_events();
        game.begin_round(&seed_of(1), 0);
        game.drain_events();
        set_dealer_hand(game.round_state_mut(), "19m19p19s12345677z");
        game.apply(Seat::new(0), Command::Kyuushu, 1_000)
            .expect("九種九牌を宣言できる");
        assert!(game.last_outcome().expect("終わっている").was_draw);
    }

    /// いまの親にツモ和了させて局を終わらせる。
    ///
    /// **席0を決め打ちしない。**局が進むと親は移る。
    /// イベントは drain しない。呼び出し側が `RoundEnd` を読むためである。
    pub(super) fn finish_with_a_dealer_tsumo(game: &mut MatchEngine) {
        let dealer = game.round_state().dealer;
        make_tenpai(game.round_state_mut(), dealer);
        game.force_draw_turn(dealer, parse_tile("6p").expect("正しい記法"));
        game.apply(dealer, Command::Tsumo, 2_000)
            .expect("ツモ和了できる");
    }

    /// いまの親の下家にツモ和了させる。親が流れる。
    ///
    /// こちらもイベントは drain しない。
    pub(super) fn finish_with_a_child_tsumo(game: &mut MatchEngine) {
        let dealer = game.round_state().dealer;
        let child = Seat::new(((dealer.index() + 1) % 4) as u8);
        make_tenpai(game.round_state_mut(), child);
        game.force_draw_turn(child, parse_tile("6p").expect("正しい記法"));
        game.apply(child, Command::Tsumo, 2_000)
            .expect("ツモ和了できる");
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine match_tests`
Expected: コンパイルエラー（`MatchEngine` が未定義）

- [ ] **Step 3: 実装を書く**

`RoundOutcome` へ `was_draw` を足し、`finish` の引数を増やす。

```rust
pub struct RoundOutcome {
    pub scores: [i32; 4],
    pub riichi_sticks: u8,
    pub dealer_repeats: bool,
    pub reason: ContinuationReason,
    /// 流局で終わったか。本場の進み方が和了と違う。
    pub was_draw: bool,
}
```

`finish` は `was_draw: bool` を末尾に受け取る。4つの呼び出しは
`finish_with_ron` と `finish_with_tsumo` が `false`、
`finish_exhaustive` と `finish_abortive` が `true` を渡す。

```rust
/// 半荘の進行。局を並べ、連荘と本場と終局を決める。
///
/// **乱数を持たない。**シードは局ごとに外から受け取る。局が何回あるかは
/// 連荘で伸びるため事前に決まらない。
pub struct MatchEngine {
    rules: Ruleset,
    round: Round,
    dealer: Seat,
    honba: u8,
    riichi_sticks: u8,
    scores: [i32; 4],
    next_window_id: u32,
    engine: Option<RoundEngine>,
    last_outcome: Option<RoundOutcome>,
    pending: Vec<Event>,
}

impl MatchEngine {
    pub fn start(rules: Ruleset, players: [PlayerId; 4], now_ms: u64) -> Self {
        let mut game = MatchEngine {
            rules,
            round: Round {
                wind: Wind::East,
                number: 1,
            },
            dealer: Seat::new(0),
            honba: 0,
            riichi_sticks: 0,
            scores: [rules.start_score; 4],
            next_window_id: 1,
            engine: None,
            last_outcome: None,
            pending: Vec::new(),
        };
        game.pending.push(Event::MatchStart { players, rules });
        let _ = now_ms;
        game
    }

    pub fn round(&self) -> Round {
        self.round
    }

    pub fn scores(&self) -> [i32; 4] {
        self.scores
    }

    pub fn last_outcome(&self) -> Option<&RoundOutcome> {
        self.last_outcome.as_ref()
    }

    /// 次の局を始めるためのシードが要るか。
    pub fn needs_seed(&self) -> bool {
        self.engine.is_none()
    }

    pub fn begin_round(&mut self, seed: &Seed, now_ms: u64) {
        assert!(self.needs_seed(), "局が動いている間は始められない");
        let mut engine = RoundEngine::start(
            self.rules,
            self.round,
            self.dealer,
            self.honba,
            self.riichi_sticks,
            self.scores,
            seed,
            self.next_window_id,
            now_ms,
        );
        self.pending.extend(engine.drain_events());
        self.engine = Some(engine);
    }

    pub fn apply(&mut self, seat: Seat, command: Command, now_ms: u64) -> Result<(), Reject> {
        let Some(engine) = self.engine.as_mut() else {
            return Err(Reject::NotYourTurn);
        };
        let result = engine.apply(seat, command, now_ms);
        self.collect(now_ms);
        result
    }

    pub fn tick(&mut self, now_ms: u64) {
        if let Some(engine) = self.engine.as_mut() {
            engine.tick(now_ms);
        }
        self.collect(now_ms);
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    /// 局のイベントを取り込み、終わっていれば局を閉じる。
    fn collect(&mut self, now_ms: u64) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        self.pending.extend(engine.drain_events());
        if engine.outcome().is_none() {
            return;
        }
        let engine = self.engine.take().expect("直前に確認した");
        let outcome = engine.outcome().cloned().expect("終わっている");
        self.next_window_id = engine.next_window_id();
        self.close_round(outcome, now_ms);
    }

    /// 局の結果を半荘へ取り込み、`RoundEnd` を出す。
    ///
    /// **次局の組み立ては Task 2 が足す。**ここでは結果を写すだけにする。
    fn close_round(&mut self, outcome: RoundOutcome, now_ms: u64) {
        self.scores = outcome.scores;
        self.riichi_sticks = outcome.riichi_sticks;
        self.pending.push(Event::RoundEnd {
            scores: self.scores,
            next: NextRound::MatchOver,
            reason: outcome.reason,
        });
        self.last_outcome = Some(outcome);
        let _ = now_ms;
    }
}
```

**テストが局面を組み立てるための入口も足す。**`RoundEngine` の
`#[cfg(test)]` な補助を半荘越しに使えるようにする。

```rust
impl MatchEngine {
    #[cfg(test)]
    pub(crate) fn round_state(&self) -> &RoundState {
        self.engine.as_ref().expect("局が動いている").state()
    }

    #[cfg(test)]
    pub(crate) fn round_state_mut(&mut self) -> &mut RoundState {
        self.engine.as_mut().expect("局が動いている").state_mut()
    }

    #[cfg(test)]
    pub(crate) fn force_draw_turn(&mut self, seat: Seat, tile: Tile) {
        self.engine
            .as_mut()
            .expect("局が動いている")
            .force_draw_turn(seat, tile);
    }
}
```

**`ending_tests` の補助4つを `&mut RoundState` を取る形へ変える。**
半荘越しでも使えるようにするためで、既存の呼び出しは `engine.state_mut()` を
渡すだけで済む。あわせて `pub(super)` にする。

```rust
    pub(super) fn make_tenpai(state: &mut RoundState, seat: Seat)
    pub(super) fn set_dealer_hand(state: &mut RoundState, notation: &str)
    pub(super) fn clear_nagashi(state: &mut RoundState)
    pub(super) fn drain_the_wall(state: &mut RoundState)
```

**あわせて2つの前提を外す。**どちらも「親は席0」「捨て台は席3」と決め打ちした
ものであり、局が進んで親が移ると成り立たない。

`make_tenpai` は捨て台を席3に固定し、`assert_ne!(seat, sink())` で席3を
拒んでいる。半荘では東4局の親が席3になるので、そのままでは落ちる。
**対象の席から見た下家を捨て台にする。**

```rust
/// 牌の退避先。対象の席とは必ず別になる。
fn sink_for(seat: Seat) -> Seat {
    Seat::new(((seat.index() + 1) % 4) as u8)
}

pub(super) fn make_tenpai(state: &mut RoundState, seat: Seat) {
    let target = parse_hand("234567m23478p22s").expect("正しい記法");
    let old = std::mem::replace(&mut state.seat_mut(seat).hand, target);
    for tile in old.into_iter().skip(13) {
        state
            .seat_mut(sink_for(seat))
            .river
            .push(crate::state::Discarded {
                tile,
                manner: DiscardManner::Tsumogiri,
                called_by: None,
                riichi_declaration: false,
            });
    }
    crate::invariant::assert_tiles_conserved(state);
}
```

`set_dealer_hand` は席0を決め打ちしている。**`state.dealer` を見る。**
既存の呼び出しはすべて親が席0の局なので、挙動は変わらない。

```rust
pub(super) fn set_dealer_hand(state: &mut RoundState, notation: &str) {
    let hand = parse_hand(notation).expect("正しい記法");
    assert_eq!(hand.len(), 14, "親の手は14枚である");
    let dealer = state.dealer;
    assert_eq!(state.seat(dealer).hand.len(), 14);
    state.seat_mut(dealer).hand = hand;
}
```

`drain_the_wall` の捨て台は席3のままでよい。手牌を触らないので、
どの席の河へ積んでも他のテストに響かない。

インポートへ足すもの。`Ruleset` は既にあるので `MatchLength` を並べる。

```rust
use protocol::event::{NextRound, PlayerId};
use protocol::ruleset::{MatchLength, Ruleset};   // Ruleset は既存。MatchLength を足す
use protocol::seat::Wind;
```

`MatchLength` は Task 3 の `last_wind` / `extension_wind` が使う。
**Task 1 の時点では使わないので、そこで入れると未使用になる。Task 3 で足す。**

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine match_tests`
Expected: 11テスト PASS

- [ ] **Step 5: 既存のテストを壊していないことを確認する**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: engine 273テスト PASS、警告ゼロ

- [ ] **Step 6: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 半荘の骨格と局への委譲を実装"
```

---

### Task 2: 局の進め方

連荘・本場・親の移動・局数・西入。**まだ終局しない。**

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**Interfaces:**
- Produces: `RoundEnd.next` に `NextRound::Next` を入れる

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod progression_tests {
    use super::discard_tests::WAY_PAST_ANY_DEADLINE_MS;
    use super::ending_tests::{clear_nagashi, drain_the_wall, set_dealer_hand};
    use super::match_tests::{
        finish_with_a_child_tsumo, finish_with_a_dealer_tsumo, hanchan, seed_of,
    };
    use super::*;
    use protocol::command::Command;
    use protocol::notation::parse_tile;
    use protocol::seat::Wind;

    /// 局を1つ始める。
    fn begin(game: &mut MatchEngine, index: u8) {
        game.begin_round(&seed_of(index), 0);
        game.drain_events();
    }

    fn next_of(game: &mut MatchEngine) -> NextRound {
        let events = game.drain_events();
        let Some(Event::RoundEnd { next, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::RoundEnd { .. }))
            .cloned()
        else {
            panic!("RoundEnd が出ていない: {events:?}");
        };
        next
    }

    /// 親の和了は連荘。局は進まず本場が増える。
    #[test]
    fn a_dealer_win_repeats_the_round_with_one_more_honba() {
        let mut game = hanchan();
        game.drain_events();
        begin(&mut game, 1);
        finish_with_a_dealer_tsumo(&mut game);

        assert_eq!(
            next_of(&mut game),
            NextRound::Next {
                round: Round {
                    wind: Wind::East,
                    number: 1
                },
                dealer: Seat::new(0),
                honba: 1,
                riichi_sticks: 0,
            }
        );
    }

    /// 子の和了は親流れ。局が進み本場は0に戻る。
    #[test]
    fn a_child_win_moves_the_dealership_and_clears_the_honba() {
        let mut game = hanchan();
        game.drain_events();
        begin(&mut game, 1);
        finish_with_a_dealer_tsumo(&mut game);
        next_of(&mut game);
        begin(&mut game, 2);
        finish_with_a_child_tsumo(&mut game);

        assert_eq!(
            next_of(&mut game),
            NextRound::Next {
                round: Round {
                    wind: Wind::East,
                    number: 2
                },
                dealer: Seat::new(1),
                honba: 0,
                riichi_sticks: 0,
            }
        );
    }

    /// 流局は親が流れても本場が増える。
    #[test]
    fn a_draw_adds_a_honba_even_when_the_dealership_moves() {
        let mut game = hanchan();
        game.drain_events();
        begin(&mut game, 1);
        // 親をノーテンにして荒牌平局へ持ち込む。
        set_dealer_hand(game.round_state_mut(), "147m258p369s12345z");
        clear_nagashi(game.round_state_mut());
        drain_the_wall(game.round_state_mut());
        game.apply(
            Seat::new(0),
            Command::Discard {
                tile: parse_tile("5z").expect("正しい記法"),
                riichi: false,
            },
            1_000,
        )
        .expect("切れる");
        game.tick(WAY_PAST_ANY_DEADLINE_MS);

        let NextRound::Next { honba, dealer, .. } = next_of(&mut game) else {
            panic!("次局が決まっていない");
        };
        assert_eq!(honba, 1, "流局は本場が増える");
        assert_eq!(dealer, Seat::new(1), "親ノーテンなので流れる");
    }

    /// 途中流局は親が続き、本場も増える。
    #[test]
    fn an_abortive_draw_repeats_with_one_more_honba() {
        let mut game = hanchan();
        game.drain_events();
        begin(&mut game, 1);
        set_dealer_hand(game.round_state_mut(), "19m19p19s12345677z");
        game.apply(Seat::new(0), Command::Kyuushu, 1_000)
            .expect("九種九牌を宣言できる");

        assert_eq!(
            next_of(&mut game),
            NextRound::Next {
                round: Round {
                    wind: Wind::East,
                    number: 1
                },
                dealer: Seat::new(0),
                honba: 1,
                riichi_sticks: 0,
            }
        );
    }

    /// 東4局で親が流れると南1局になる。
    #[test]
    fn the_round_wind_turns_after_the_fourth_round() {
        let mut game = hanchan();
        game.drain_events();
        for index in 1..=4u8 {
            begin(&mut game, index);
            finish_with_a_child_tsumo(&mut game);
            next_of(&mut game);
        }
        assert_eq!(
            game.round(),
            Round {
                wind: Wind::South,
                number: 1
            }
        );
        assert_eq!(game.round_dealer(), Seat::new(0), "一周して起家へ戻る");
    }

    /// 供託は局をまたいで持ち越される。
    #[test]
    fn riichi_sticks_carry_into_the_next_round() {
        let mut game = hanchan();
        game.drain_events();
        begin(&mut game, 1);
        // 親がリーチしてから流局させる。供託1本が残る。
        set_dealer_hand(game.round_state_mut(), "234567m23478p22s1z");
        game.apply(
            Seat::new(0),
            Command::Discard {
                tile: parse_tile("1z").expect("正しい記法"),
                riichi: true,
            },
            1_000,
        )
        .expect("リーチできる");
        game.tick(WAY_PAST_ANY_DEADLINE_MS);
        game.drain_events();
        clear_nagashi(game.round_state_mut());
        drain_the_wall(game.round_state_mut());
        let tile = game.round_state().seat(Seat::new(1)).hand[0];
        game.apply(
            Seat::new(1),
            Command::Discard {
                tile,
                riichi: false,
            },
            WAY_PAST_ANY_DEADLINE_MS + 1_000,
        )
        .expect("切れる");
        game.tick(WAY_PAST_ANY_DEADLINE_MS * 2);

        let NextRound::Next { riichi_sticks, .. } = next_of(&mut game) else {
            panic!("次局が決まっていない");
        };
        assert_eq!(riichi_sticks, 1);
    }

    /// 次局は前局の続きの window_id から始まる。
    #[test]
    fn the_window_id_keeps_increasing_across_rounds() {
        let mut game = hanchan();
        game.drain_events();
        begin(&mut game, 1);
        finish_with_a_dealer_tsumo(&mut game);
        next_of(&mut game);
        let first_end = game.current_window_id();

        // **`begin` は使わない。**開始イベントを捨ててしまうと、
        // このテストが読みたい `RequestAction` が消える。
        game.begin_round(&seed_of(2), 0);
        let events = game.drain_events();
        let Some(Event::RequestAction { window_id, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::RequestAction { .. }))
            .cloned()
        else {
            panic!("要求が出ていない: {events:?}");
        };
        assert_eq!(window_id, first_end, "採番が続いている");
    }

    /// 南4局を終えても誰も返し点に届かなければ西入する。
    #[test]
    fn nobody_reaching_the_return_score_forces_an_extension() {
        let mut game = hanchan();
        game.drain_events();
        // 8局ぶん親を流して南4局まで進める。**持ち点は検査しない。**
        // 和了の額はドラ次第で変わるので、局ごとにいくら動くかは決まらない。
        // このタスクではまだ終局を判定しないため、局の進み方だけを見る。
        for index in 1..=8u8 {
            begin(&mut game, index);
            finish_with_a_child_tsumo(&mut game);
            next_of(&mut game);
        }
        assert_eq!(game.round().wind, Wind::West, "西入した");
        assert_eq!(game.round().number, 1);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine progression_tests`
Expected: テストの失敗（`next` が `MatchOver` のまま）

- [ ] **Step 3: 実装を書く**

`close_round` を書き換える。

```rust
    fn close_round(&mut self, outcome: RoundOutcome, now_ms: u64) {
        self.scores = outcome.scores;
        self.riichi_sticks = outcome.riichi_sticks;

        // 本場。和了は親が続いたときだけ増え、子の和了で0へ戻る。
        // 流局は親が流れても増える。
        self.honba = if outcome.was_draw || outcome.dealer_repeats {
            self.honba + 1
        } else {
            0
        };

        self.advance_seat_and_round(&outcome);

        let next = NextRound::Next {
            round: self.round,
            dealer: self.dealer,
            honba: self.honba,
            riichi_sticks: self.riichi_sticks,
        };
        self.pending.push(Event::RoundEnd {
            scores: self.scores,
            next,
            reason: outcome.reason,
        });
        self.last_outcome = Some(outcome);
        let _ = now_ms;
    }

    /// 親と局を次へ進める。
    ///
    /// **切り出しておく。**Task 3 が終局を判定する位置は、これを呼ぶ前で
    /// なければならない。先に進めると、東4局の子和了を南1局として
    /// 判定してしまう。
    fn advance_seat_and_round(&mut self, outcome: &RoundOutcome) {
        if outcome.dealer_repeats {
            return;
        }
        self.dealer = Seat::new(((self.dealer.index() + 1) % 4) as u8);
        if self.round.number < 4 {
            self.round.number += 1;
            return;
        }
        self.round = Round {
            wind: next_wind(self.round.wind),
            number: 1,
        };
    }
```

```rust
/// 場風の順。東 → 南 → 西 → 北。
fn next_wind(wind: Wind) -> Wind {
    match wind {
        Wind::East => Wind::South,
        Wind::South => Wind::West,
        Wind::West => Wind::North,
        Wind::North => Wind::East,
    }
}
```

テストが使う入口を足す。

```rust
    #[cfg(test)]
    pub(crate) fn round_dealer(&self) -> Seat {
        self.dealer
    }

    /// テストが持ち点を直接置くための入口。半荘側と局側の両方へ書く。
    #[cfg(test)]
    pub(crate) fn force_scores(&mut self, scores: [i32; 4]) {
        self.scores = scores;
        self.engine
            .as_mut()
            .expect("局が動いている")
            .state_mut()
            .scores = scores;
    }

    /// フィールドと同じ名前にすると、どちらを指しているか読めなくなる。
    #[cfg(test)]
    pub(crate) fn current_window_id(&self) -> u32 {
        self.next_window_id
    }
```

**西入はこのタスクでは「最終局を越えたら次の風へ進む」だけで足りる。**
`advance_round` が東4局の次を南1局、南4局の次を西1局にするので、
半荘の南4局を終えると自動的に西入した状態になる。終局の判定は Task 3 が
足すので、ここでは伸び続ける。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine progression_tests`
Expected: 8テスト PASS

- [ ] **Step 5: 既存のテストを壊していないことを確認する**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: engine 281テスト PASS、警告ゼロ

- [ ] **Step 6: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 連荘と本場と局の進み方を実装"
```

---

### Task 3: 終局と順位

アガリ止め・テンパイ止め・飛び・西入の打ち切り・`MatchEnd`・`SeedReveal`。

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**Interfaces:**
- Produces: `MatchEngine::is_over(&self) -> bool`、`Event::MatchEnd`、`Event::SeedReveal`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod ending_match_tests {
    use super::discard_tests::WAY_PAST_ANY_DEADLINE_MS;
    use super::ending_tests::{clear_nagashi, drain_the_wall, set_dealer_hand};
    use super::match_tests::{
        finish_with_a_child_tsumo, finish_with_a_dealer_tsumo, players, seed_of,
    };
    use super::*;
    use protocol::command::Command;
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::ruleset::MatchLength;
    use protocol::seat::Wind;

    /// 東風戦。4局で終わるので終局まで回しやすい。
    fn tonpuu() -> MatchEngine {
        MatchEngine::start(
            Ruleset::kin_no_ma(MatchLength::Tonpuu),
            players(),
            0,
        )
    }

    /// 全員ノーテンの荒牌平局で局を終える。
    ///
    /// **和了で局を送ると点棒が動く。**その額はドラ次第で変わるので、
    /// 「誰も返し点へ届かないまま何局も進める」テストが配牌に依存する。
    /// 全員ノーテンの流局なら点棒はまったく動かない。
    fn finish_with_a_noten_draw(game: &mut MatchEngine) {
        let dealer = game.round_state().dealer;
        // 親は14枚、子は13枚。どちらも対子も塔子も無い散らばった形にする。
        game.round_state_mut().seat_mut(dealer).hand =
            parse_hand("147m258p369s12345z").expect("正しい記法");
        for seat in Seat::ALL {
            if seat == dealer {
                continue;
            }
            game.round_state_mut().seat_mut(seat).hand =
                parse_hand("147m258p369s1234z").expect("正しい記法");
        }
        clear_nagashi(game.round_state_mut());
        drain_the_wall(game.round_state_mut());
        game.apply(
            dealer,
            Command::Discard {
                tile: parse_tile("5z").expect("正しい記法"),
                riichi: false,
            },
            1_000,
        )
        .expect("切れる");
        game.tick(WAY_PAST_ANY_DEADLINE_MS);
        game.drain_events();
    }

    fn match_end_of(events: &[Event]) -> ([i32; 4], [u8; 4]) {
        let Some(Event::MatchEnd {
            final_scores,
            placements,
        }) = events
            .iter()
            .find(|e| matches!(e, Event::MatchEnd { .. }))
            .cloned()
        else {
            panic!("MatchEnd が出ていない: {events:?}");
        };
        (final_scores, placements)
    }

    /// 東4局まで進め、親をトップにしてから親に和了らせる。
    ///
    /// **持ち点を毎局そろえる。**配牌任せにすると、途中で誰かが返し点へ
    /// 届いて予定より早く終局し、テストがシードに依存する。
    fn run_to_the_end(game: &mut MatchEngine) {
        for index in 1..=3u8 {
            game.begin_round(&seed_of(index), 0);
            game.drain_events();
            finish_with_a_noten_draw(game);
        }
        game.begin_round(&seed_of(4), 0);
        game.drain_events();
        // 3回親が流れたので、東4局の親は席3。トップにしてアガリ止めを起こす。
        assert_eq!(game.round_dealer(), Seat::new(3));
        game.force_scores([20_000, 20_000, 20_000, 40_000]);
        finish_with_a_dealer_tsumo(game);
    }

    /// 東4局で親がトップのまま和了れば終局する。
    #[test]
    fn the_match_ends_after_its_last_round() {
        let mut game = tonpuu();
        game.drain_events();
        run_to_the_end(&mut game);
        let events = game.drain_events();

        assert!(game.is_over());
        let (scores, _) = match_end_of(&events);
        assert_eq!(scores.iter().sum::<i32>(), 100_000);
    }

    /// 終局すればシードをまとめて開示する。
    #[test]
    fn the_seeds_are_revealed_only_at_the_end() {
        let mut game = tonpuu();
        game.drain_events();
        for index in 1..=3u8 {
            game.begin_round(&seed_of(index), 0);
            let events = game.drain_events();
            assert!(
                !events.iter().any(|e| matches!(e, Event::SeedReveal { .. })),
                "局の途中で開示してはならない"
            );
            finish_with_a_noten_draw(&mut game);
            let ended = game.drain_events();
            assert!(
                ended.is_empty() || !ended.iter().any(|e| matches!(e, Event::SeedReveal { .. })),
                "局の終わりにも開示してはならない"
            );
        }
        game.begin_round(&seed_of(4), 0);
        game.drain_events();
        game.force_scores([20_000, 20_000, 20_000, 40_000]);
        finish_with_a_dealer_tsumo(&mut game);

        let events = game.drain_events();
        let Some(Event::SeedReveal { seeds }) = events
            .iter()
            .find(|e| matches!(e, Event::SeedReveal { .. }))
            .cloned()
        else {
            panic!("SeedReveal が出ていない");
        };
        assert_eq!(seeds.len(), 4, "4局ぶん");
        assert_eq!(seeds[0], seed_of(1).to_hex());
    }

    /// 終局後はコマンドもシードも受け付けない。
    #[test]
    fn a_finished_match_takes_nothing_more() {
        let mut game = tonpuu();
        game.drain_events();
        run_to_the_end(&mut game);
        game.drain_events();

        assert!(!game.needs_seed(), "終局したのでシードは要らない");
        assert_eq!(
            game.apply(Seat::new(0), Command::Tsumo, 9_000),
            Err(Reject::NotYourTurn)
        );
    }

    /// 順位は持ち点の多い順。
    #[test]
    fn placements_follow_the_scores() {
        let mut game = tonpuu();
        game.drain_events();
        run_to_the_end(&mut game);
        let events = game.drain_events();

        let (scores, placements) = match_end_of(&events);
        let mut sorted = placements;
        sorted.sort_unstable();
        assert_eq!(sorted, [1, 2, 3, 4], "順位は1から4まで1つずつ");
        for a in 0..4 {
            for b in 0..4 {
                if scores[a] > scores[b] {
                    assert!(placements[a] < placements[b], "{scores:?} {placements:?}");
                }
            }
        }
    }

    /// 同点は席順で決まる。起家に近いほうが上位。
    ///
    /// `run_to_the_end` は親を40,000点にしてから和了らせるので、
    /// 和了額がドラで変わっても親は単独トップのまま終局する。
    /// 子3人は同じ20,000点から同じ額を払うので同点になる。
    #[test]
    fn a_tie_is_broken_by_seat_order() {
        let mut game = tonpuu();
        game.drain_events();
        run_to_the_end(&mut game);
        let events = game.drain_events();

        let (scores, placements) = match_end_of(&events);
        assert_eq!(placements[3], 1, "和了した親が単独トップ");
        assert_eq!(scores[0], scores[1], "子は同点");
        assert_eq!(scores[1], scores[2]);
        // 同点なので席順。
        assert_eq!(placements[0], 2);
        assert_eq!(placements[1], 3);
        assert_eq!(placements[2], 4);
    }

    /// 誰かが0点未満になったら即終局する。
    #[test]
    fn a_busted_seat_ends_the_match_immediately() {
        let mut game = tonpuu();
        game.drain_events();
        game.begin_round(&seed_of(1), 0);
        game.drain_events();
        // 席1を大きく減らしてから親に和了らせる。
        // 親のツモは最低でも子から1300点ずつ取る。額はドラで増えるが、
        // 500点しかない席1が負になることは変わらない。
        game.force_scores([25_000, 500, 25_000, 49_500]);
        finish_with_a_dealer_tsumo(&mut game);
        let events = game.drain_events();

        assert!(game.is_over(), "飛びで終局する");
        assert!(events.iter().any(|e| matches!(e, Event::MatchEnd { .. })));
        assert!(game.scores()[1] < 0);
    }

    /// 飛びを切っていれば続行する。
    #[test]
    fn a_ruleset_without_busting_keeps_playing() {
        let mut game = MatchEngine::start(
            Ruleset {
                busted_ends_match: false,
                ..Ruleset::kin_no_ma(MatchLength::Tonpuu)
            },
            players(),
            0,
        );
        game.drain_events();
        game.begin_round(&seed_of(1), 0);
        game.drain_events();
        game.force_scores([25_000, 500, 25_000, 49_500]);
        finish_with_a_dealer_tsumo(&mut game);
        game.drain_events();
        assert!(!game.is_over());
    }

    /// 最終局で親が和了ってもトップでなければ続行する。
    #[test]
    fn a_dealer_win_without_the_lead_keeps_the_match_going() {
        let mut game = tonpuu();
        game.drain_events();
        run_to_the_last_round(&mut game, 4);
        // 親（席3）を最下位にしてから和了らせる。小さな手ではトップに届かない。
        game.force_scores([1_000, 50_000, 25_000, 24_000]);
        finish_with_a_dealer_tsumo(&mut game);
        game.drain_events();
        assert!(!game.is_over(), "アガリ止めはトップのときだけ");
    }

    /// 延長は無限に伸びない。
    ///
    /// 点棒を動かさない流局だけで送るので、誰も返し点へ届かない。
    /// それでも東風戦は南4局で打ち切られる。
    #[test]
    fn the_extension_does_not_go_on_forever() {
        let mut game = tonpuu();
        game.drain_events();
        let mut rounds = 0u32;
        while !game.is_over() {
            rounds += 1;
            assert!(rounds < 40, "終局しない");
            game.begin_round(&seed_of((rounds % 200) as u8), 0);
            game.drain_events();
            finish_with_a_noten_draw(&mut game);
        }
        assert_eq!(rounds, 8, "東4局＋南4局で打ち切る");
        assert_eq!(game.round().wind, Wind::South);
        assert_eq!(game.scores(), [25_000; 4], "点棒は動いていない");
    }

    /// 3局を点棒の動かない流局で送り、東4局へ入る。親は席3になる。
    fn run_to_the_last_round(game: &mut MatchEngine, seed_index: u8) {
        for index in 1..=3u8 {
            game.begin_round(&seed_of(index), 0);
            game.drain_events();
            finish_with_a_noten_draw(game);
        }
        game.begin_round(&seed_of(seed_index), 0);
        game.drain_events();
        assert_eq!(game.round_dealer(), Seat::new(3));
        assert_eq!(game.round().number, 4);
    }

    /// 最終局で親がテンパイの荒牌平局なら、親がトップのとき終局する。
    #[test]
    fn a_tenpai_dealer_on_top_stops_the_match() {
        let mut game = tonpuu();
        game.drain_events();
        run_to_the_last_round(&mut game, 4);

        set_dealer_hand(game.round_state_mut(), "234567m23478p22s1z");
        for child in [Seat::new(0), Seat::new(1), Seat::new(2)] {
            game.round_state_mut().seat_mut(child).hand =
                parse_hand("147m258p369s1234z").expect("正しい記法");
        }
        game.force_scores([20_000, 20_000, 20_000, 40_000]);
        clear_nagashi(game.round_state_mut());
        drain_the_wall(game.round_state_mut());
        game.apply(
            Seat::new(3),
            Command::Discard {
                tile: parse_tile("1z").expect("正しい記法"),
                riichi: false,
            },
            1_000,
        )
        .expect("切れる");
        game.tick(WAY_PAST_ANY_DEADLINE_MS);
        game.drain_events();
        assert!(game.is_over(), "テンパイ止め");
    }

    /// 誰も返し点へ届いていなければ、親がトップでも延長する。
    ///
    /// アガリ止めは「半荘が終わる場面で親が連荘を選ばない」規則である。
    /// 延長するなら終わらないので、止める場面ではない。
    ///
    /// **和了ではなく荒牌平局で作る。**和了の点数はドラ次第で変わるので、
    /// 「30000点に届かない」という境界を置けない。テンパイ料なら
    /// 親 +3000 / 子 各 -1000 と決まる。
    #[test]
    fn a_top_dealer_below_the_return_score_still_extends() {
        let mut game = tonpuu();
        game.drain_events();
        run_to_the_last_round(&mut game, 4);

        set_dealer_hand(game.round_state_mut(), "234567m23478p22s1z");
        for child in [Seat::new(0), Seat::new(1), Seat::new(2)] {
            game.round_state_mut().seat_mut(child).hand =
                parse_hand("147m258p369s1234z").expect("正しい記法");
        }
        game.force_scores([25_000; 4]);
        clear_nagashi(game.round_state_mut());
        drain_the_wall(game.round_state_mut());
        game.apply(
            Seat::new(3),
            Command::Discard {
                tile: parse_tile("1z").expect("正しい記法"),
                riichi: false,
            },
            1_000,
        )
        .expect("切れる");
        game.tick(WAY_PAST_ANY_DEADLINE_MS);
        game.drain_events();

        assert_eq!(game.scores(), [24_000, 24_000, 24_000, 28_000]);
        assert!(!game.is_over(), "誰も返し点へ届いていないので延長する");
        assert_eq!(game.round().wind, Wind::South, "南入した");
    }

    /// 途中流局では止まらない。親が続いてもアガリ止めではない。
    #[test]
    fn an_abortive_draw_never_stops_the_match() {
        let mut game = tonpuu();
        game.drain_events();
        run_to_the_last_round(&mut game, 4);

        set_dealer_hand(game.round_state_mut(), "19m19p19s12345677z");
        game.force_scores([20_000, 20_000, 20_000, 40_000]);
        game.apply(Seat::new(3), Command::Kyuushu, 1_000)
            .expect("九種九牌を宣言できる");
        game.drain_events();
        assert!(!game.is_over(), "途中流局は止めない");
    }

    /// 最終局で親が流れれば、その時点で終局する。
    #[test]
    fn the_dealership_moving_on_the_last_round_ends_the_match() {
        let mut game = tonpuu();
        game.drain_events();
        run_to_the_last_round(&mut game, 4);

        game.force_scores([40_000, 20_000, 20_000, 20_000]);
        finish_with_a_child_tsumo(&mut game);
        game.drain_events();
        assert!(game.is_over(), "最終局で親が流れたら終わり");
    }

    /// 延長の途中でも、誰かが返し点へ届けばその局で終わる。
    #[test]
    fn reaching_the_return_score_ends_the_extension_early() {
        let mut game = tonpuu();
        game.drain_events();
        for index in 1..=4u8 {
            game.begin_round(&seed_of(index), 0);
            game.drain_events();
            finish_with_a_noten_draw(&mut game);
        }
        assert_eq!(
            game.round(),
            Round {
                wind: Wind::South,
                number: 1
            },
            "南入している"
        );

        game.begin_round(&seed_of(5), 0);
        game.drain_events();
        game.force_scores([40_000, 20_000, 20_000, 20_000]);
        finish_with_a_child_tsumo(&mut game);
        game.drain_events();
        assert!(game.is_over(), "南1局でも返し点に届いていれば終わる");
    }

    /// 延長した風の4局目は、親が連荘しても打ち切る。
    #[test]
    fn the_extension_stops_even_on_a_dealer_repeat() {
        let mut game = tonpuu();
        game.drain_events();
        for index in 1..=7u8 {
            game.begin_round(&seed_of(index), 0);
            game.drain_events();
            finish_with_a_noten_draw(&mut game);
        }
        assert_eq!(
            game.round(),
            Round {
                wind: Wind::South,
                number: 4
            }
        );
        assert_eq!(game.round_dealer(), Seat::new(3));

        game.begin_round(&seed_of(8), 0);
        game.drain_events();
        // 親をトップにしない。アガリ止めの条件は満たさないが、
        // 延長の4局目なので打ち切られる。
        game.force_scores([40_000, 20_000, 20_000, 20_000]);
        finish_with_a_dealer_tsumo(&mut game);
        game.drain_events();
        assert!(game.is_over(), "延長の4局目は連荘でも打ち切る");
    }

    /// 半荘でも同じ条件で終局する。最終局の場風が違うだけである。
    #[test]
    fn a_hanchan_ends_on_its_own_last_round() {
        let mut game = MatchEngine::start(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            players(),
            0,
        );
        game.drain_events();
        for index in 1..=7u8 {
            game.begin_round(&seed_of(index), 0);
            game.drain_events();
            finish_with_a_noten_draw(&mut game);
        }
        assert_eq!(
            game.round(),
            Round {
                wind: Wind::South,
                number: 4
            }
        );

        game.begin_round(&seed_of(8), 0);
        game.drain_events();
        game.force_scores([40_000, 20_000, 20_000, 20_000]);
        finish_with_a_child_tsumo(&mut game);
        game.drain_events();
        assert!(game.is_over(), "南4局で誰かが届いていれば終わる");
    }

    /// 東1局から終局まで、ツモ切りだけで通せる。
    #[test]
    fn a_whole_match_runs_on_tsumogiri() {
        let mut game = tonpuu();
        game.drain_events();
        let mut now = 1_000u64;
        let mut seed_index = 1u8;
        // 1局は最大で 70 ツモ前後あり、`tick` は打牌と反応を別々に進める。
        // 東風戦が延長まで伸びると8局になるので、余裕をもって上限を置く。
        for _ in 0..5_000 {
            if game.is_over() {
                break;
            }
            if game.needs_seed() {
                game.begin_round(&seed_of(seed_index), now);
                seed_index = seed_index.wrapping_add(1);
                game.drain_events();
                continue;
            }
            now += WAY_PAST_ANY_DEADLINE_MS;
            game.tick(now);
            game.drain_events();
        }
        assert!(game.is_over(), "終局しなかった");
        assert_eq!(
            game.scores().iter().sum::<i32>() + game.carried_sticks() as i32 * 1_000,
            100_000
        );
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine ending_match_tests`
Expected: コンパイルエラー（`is_over` などが未定義）

- [ ] **Step 3: 実装を書く**

構造体へ足すもの。

```rust
    /// 局ごとのシード。半荘の終わりにまとめて開示する。
    seeds: Vec<Seed>,
    over: bool,
    /// 直前の局の終わり方。`finish_match` が `RoundEnd` に載せる。
    last_reason: ContinuationReason,
```

`start` の初期化では `last_reason: ContinuationReason::DealerLoss` を置く。
局が1つも終わっていない状態では読まれない。

`start` の初期化へ `seeds: Vec::new()` と `over: false` を足す。
`begin_round` の先頭で `self.seeds.push(*seed)` する。`Seed` は `Copy` なので
`clone()` を呼ぶと clippy の `clone_on_copy` で落ちる。

```rust
    pub fn is_over(&self) -> bool {
        self.over
    }

    #[cfg(test)]
    pub(crate) fn carried_sticks(&self) -> u8 {
        self.riichi_sticks
    }

    /// テストが持ち点を直接置くための入口。
    #[cfg(test)]
    pub(crate) fn force_scores(&mut self, scores: [i32; 4]) {
        self.scores = scores;
        self.engine
            .as_mut()
            .expect("局が動いている")
            .state_mut()
            .scores = scores;
    }
```

`needs_seed` を終局で閉じる。

```rust
    pub fn needs_seed(&self) -> bool {
        self.engine.is_none() && !self.over
    }
```

**`close_round` を書き換える。**判定は**終わった局の `round` と `dealer` で
行い、`advance_seat_and_round` を呼ぶ前**に置く。先に進めると、東4局の
子和了が南1局として判定され、最終局を見失う。

```rust
    fn close_round(&mut self, outcome: RoundOutcome, now_ms: u64) {
        self.scores = outcome.scores;
        self.riichi_sticks = outcome.riichi_sticks;
        self.honba = if outcome.was_draw || outcome.dealer_repeats {
            self.honba + 1
        } else {
            0
        };
        self.last_reason = outcome.reason;

        // **ここで判定する。**round と dealer はまだ終わった局のものである。
        if self.should_end(&outcome) {
            self.finish_match();
            self.last_outcome = Some(outcome);
            let _ = now_ms;
            return;
        }

        self.advance_seat_and_round(&outcome);
        self.pending.push(Event::RoundEnd {
            scores: self.scores,
            next: NextRound::Next {
                round: self.round,
                dealer: self.dealer,
                honba: self.honba,
                riichi_sticks: self.riichi_sticks,
            },
            reason: outcome.reason,
        });
        self.last_outcome = Some(outcome);
        let _ = now_ms;
    }

    /// 終局するか。
    ///
    /// 判定の順を固定する。同時に立ちうるので、決めておかないと同じ入力から
    /// 違う結果が出る。
    fn should_end(&self, outcome: &RoundOutcome) -> bool {
        // 飛び。最優先で、どの局でも終わる。
        if self.rules.busted_ends_match && self.scores.iter().any(|s| *s < 0) {
            return true;
        }

        // **延長した風は、返し点の有無より先に見る。**
        // 誰かが届いた局で終わり、4局目なら届いていなくても終わる。
        // ここを返し点の枝の中に置くと、届いた者がいて親が連荘した場合に
        // 打ち切りへ到達せず、同じ4局目を繰り返し続ける。
        if self.round.wind == extension_wind(self.rules.length) {
            return self.round.number == 4 || self.reached_return_score();
        }

        // ここからは本来の最終局だけの話。
        if !self.is_last_round() {
            return false;
        }
        if !self.reached_return_score() {
            // 誰も届いていない。次の風へ延長する。
            return false;
        }
        // 親が流れるなら、この風の4局目なのでもう局は無い。
        if !outcome.dealer_repeats {
            return true;
        }
        // 親が続く場合に止められるのは、親の和了と、親テンパイの荒牌平局だけ。
        // 流し満貫も荒牌平局の一種なので含める。**途中流局は止めない。**
        let can_stop = matches!(
            outcome.reason,
            ContinuationReason::DealerWin
                | ContinuationReason::DealerTenpai
                | ContinuationReason::NagashiMangan
        );
        // **同点は席順で決まる。**最高点と並んでいても、席順で下なら
        // トップではない。
        can_stop && placements_of(&self.scores)[self.dealer.index()] == 1
    }

    fn reached_return_score(&self) -> bool {
        self.scores.iter().any(|s| *s >= self.rules.return_score)
    }

    /// 本来の最終局。半荘なら南4局、東風戦なら東4局。
    fn is_last_round(&self) -> bool {
        self.round.number == 4 && self.round.wind == last_wind(self.rules.length)
    }

    /// 半荘を閉じる。
    fn finish_match(&mut self) {
        self.over = true;
        self.engine = None;
        self.pending.push(Event::RoundEnd {
            scores: self.scores,
            next: NextRound::MatchOver,
            reason: self.last_reason,
        });
        self.pending.push(Event::MatchEnd {
            final_scores: self.scores,
            placements: placements_of(&self.scores),
        });
        self.pending.push(Event::SeedReveal {
            seeds: self.seeds.iter().map(|s| s.to_hex()).collect(),
        });
    }
```

```rust
/// 最終局の場風。半荘は南、東風戦は東。
fn last_wind(length: MatchLength) -> Wind {
    match length {
        MatchLength::Hanchan => Wind::South,
        MatchLength::Tonpuu => Wind::East,
    }
}

/// 延長した場合の場風。半荘は西、東風戦は南。
fn extension_wind(length: MatchLength) -> Wind {
    next_wind(last_wind(length))
}

/// 順位。持ち点の多い順で、同点は席順が上位。
fn placements_of(scores: &[i32; 4]) -> [u8; 4] {
    let mut order: Vec<usize> = (0..4).collect();
    order.sort_by(|a, b| scores[*b].cmp(&scores[*a]).then(a.cmp(b)));
    let mut placements = [0u8; 4];
    for (rank, seat) in order.into_iter().enumerate() {
        placements[seat] = rank as u8 + 1;
    }
    placements
}
```

`close_round` の先頭で `self.last_reason = outcome.reason;` と置く。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine ending_match_tests`
Expected: 17テスト PASS

- [ ] **Step 5: 既存のテストを壊していないことを確認する**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: engine 298テスト PASS、警告ゼロ

- [ ] **Step 6: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 終局と順位を実装"
```

---

## Wave 2f 完了の判定

- [ ] `cargo test --workspace` が通る（engine 298テスト）
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] 既存の262件を1つも壊していない
- [ ] `unimplemented!` / `todo!` が1つも無い
- [ ] 東1局から終局まで、ツモ切りだけで通る
- [ ] 連荘・本場・親の移動・西入・アガリ止め・飛びがそれぞれ検査されている
- [ ] `window_id` が半荘を通して単調増加する
- [ ] シードが半荘の終わりにだけ開示される
- [ ] すべての局面で牌136枚と点棒100000点が保たれる
- [ ] `match_flow.rs` 以外を編集していない

## 次のウェーブへ渡すもの

| 部品 | 使われ方 |
|---|---|
| `MatchEngine` | サーバの卓 Actor が1つ持つ。コマンドを流し、イベントを射影して配る |
| `needs_seed` / `begin_round` | 乱数はサーバ側。シードを作って渡し、`RoundStart` の commitment で縛る |
| `drain_events` | `project()` を通してから各席へ送る |
| `MatchEnd.placements` | 段位とレートの計算に使う |
