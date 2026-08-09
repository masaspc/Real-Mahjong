# Wave 2d: リーチと槓 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 局の進行にリーチと槓を足す。供託・一発・裏ドラ・嶺上ツモ・槍槓までを扱い、リーチ後の一発ツモや槍槓が成立する状態にする。

**この計画に含まないもの:** 途中流局（九種九牌・四風連打・四家立直・四開槓・三家和）と責任払いは **Wave 2e**、半荘の進行は **Wave 2f** が担当する。

**Architecture:** Wave 2c の `RoundEngine` へ枝を足す。時刻も乱数も外から注入したまま。同じシード・同じコマンド列・同じ時刻列からは必ず同じイベント列が出る。

**Tech Stack:** Rust 1.97.1 / edition 2021 / `protocol`・`mahjong-core`・Wave 2a〜2c の成果物

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
**作業規約:** `AGENTS.md`
**前提:** Wave 2c がマージ済みであること（engine のテストが200件通ること）

## Global Constraints

- **編集してよいのは `crates/mahjong-engine/src/match_flow.rs` だけである**
- **`round.rs` / `settlement.rs` / `wall.rs` / `reaction.rs` / `state.rs` / `timing.rs` / `invariant.rs` を編集しない**
- **`lib.rs` を編集しない。** Wave 0 で凍結済みである
- `crates/protocol` と `crates/mahjong-core` は凍結済み。**編集も追加もしない**
- **時刻を直接読まない。** `Instant::now()` / `SystemTime::now()` / `rand` を呼ばない
- **時間の式をここに書き直さない。** `crate::state` が再公開している4関数を呼ぶだけにする
- 既存の200件のうち、`call_tests::a_minkan_is_not_accepted_yet` **だけ**を削除してよい。他は1つも壊さない
- 完了条件は `cargo test --workspace` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` がすべて通ること

## 既存コードのどこへ足すか

`match_flow.rs` の実装済みの関数に手を入れる。**新しい関数を足す前に、既存の
どの関数から呼ぶかを決めてから書くこと。**

| 既存の関数 | Wave 2d での変更 |
|---|---|
| `apply` | `Command::Ankan` / `Command::Kakan` を振り分ける |
| `apply_discard` | `riichi: true` を受け付ける |
| `discard` | リーチ中の席が打ったら一発を消す |
| `apply_call` | 明槓を扱う。リーチを成立させる。一発を消す |
| `advance_after_pass` | リーチを成立させる |
| `resolve_window` | 槍槓ウィンドウの `Ron` を槍槓の和了にする |
| `finish_with_ron` / `finish_with_tsumo` | `ura_indicators` を埋める |

## コーディネータが確定させたルール

| 項目 | 決定 |
|---|---|
| リーチの宣言 | `Riichi { step: Declare }` を `Discard` より**先に**出す。宣言牌は河で横向きになる |
| リーチの成立 | 宣言牌への反応が「誰も和了しなかった」で終わった時点。`Riichi { step: Accepted }` を出し、1000点を供託へ移す |
| 宣言牌がロンされた場合 | リーチは**成立しない。**供託も出ない。`RiichiState` は `Declare` のまま残る |
| 宣言牌が鳴かれた場合 | リーチは**成立する。**ただし一発は消える |
| ダブルリーチ | その席の最初のツモ（`draw_count == 1`）で、かつ誰も鳴いていないこと |
| 一発の有効範囲 | 成立から、その席の**次の打牌まで**。次の打牌そのものへのロンは一発ではない |
| 一発が消える条件 | 誰かが鳴いた（チー・ポン・槓のいずれか）、またはリーチした席が次に打牌した |
| 裏ドラ | `AgariResult.ura_indicators` は、その和了者のリーチが成立している場合のみ `Some` |
| 槓のドラ表示 | **どの槓でも成立の直後にめくる。**明槓と加槓を打牌後にする流儀もあるが、分岐を増やすだけなので揃える |
| 暗槓の `Event::Call.from` | 自分自身の席を入れる。`Event::Call.from` は `Option` ではなく凍結済みである。`Meld.from` は `None` のままにする |
| 加槓の `Event::Call.from` | 元になったポンの `from` |
| 槓と `any_call_made` | 暗槓を含め、すべての槓で真にする。天和・地和・九種九牌の資格はここで消える |
| 槓と門前 | `is_menzen` は `Meld::is_concealed` を見るので、暗槓は門前のまま |
| 嶺上ツモ | 槓の成立とドラ表示のあとに王牌から引く。`hand_context.rinshan` が自動で立つ |
| 槍槓の対象 | 加槓は誰でも、暗槓は国士無双のみ。`chankan_options` がすでに判定している |
| 槍槓が成立した場合 | 槓は成立しない。手牌も副露も変えず、`pending_kan` を消して和了へ進む |
| 4つ目の槓のあと | 嶺上牌は残り1枚。**四開槓の判定は Wave 2e が足す。**ここでは槓を続けさせる |

---

## タスクの依存関係

```
1 リーチ ─→ 2 槓
```

直列である。槓のテストでリーチ中の暗槓を扱うため、先にリーチが要る。

---

### Task 1: リーチ

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**Interfaces:**
- Consumes: `crate::state::{RiichiState, RIICHI_STICK}`、`protocol::event::RiichiStep`
- Produces: `Command::Discard { riichi: true }` の受理と `Event::Riichi` の発行

**既存のテストモジュールから借りるもの。** `ending_tests` の `make_tenpai` と
`set_dealer_hand` を `pub(super)` にして使う。**新しく書き直さない。**

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod riichi_tests {
    // RiichiState は match_flow.rs の親スコープに入っていない。明示して取り込む。
    use crate::state::RiichiState;
    use super::discard_tests::WAY_PAST_ANY_DEADLINE_MS;
    use super::ending_tests::{make_tenpai, set_dealer_hand};
    use super::start_tests::start_at;
    use super::*;
    use protocol::command::{CallResponse, Command};
    use protocol::notation::{parse_hand, parse_tile};

    /// 親に 6p/9p 待ちのテンパイを持たせ、1z でリーチ宣言する。
    fn declare_riichi(engine: &mut RoundEngine, now_ms: u64) -> Tile {
        set_dealer_hand(engine, "234567m23478p22s1z");
        let tile = parse_tile("1z").expect("正しい記法");
        engine
            .apply(
                Seat::new(0),
                Command::Discard { tile, riichi: true },
                now_ms,
            )
            .expect("リーチできる");
        tile
    }

    fn riichi_of(engine: &RoundEngine, seat: Seat) -> RiichiState {
        engine
            .state()
            .seat(seat)
            .riichi
            .clone()
            .expect("リーチしている")
    }

    /// 宣言は打牌より先に出る。
    #[test]
    fn the_declaration_comes_before_the_discard() {
        let mut engine = start_at(0);
        engine.drain_events();
        declare_riichi(&mut engine, 1_000);

        let events = engine.drain_events();
        let declare = events
            .iter()
            .position(|e| matches!(e, Event::Riichi { step: RiichiStep::Declare, .. }))
            .expect("宣言が出ていない");
        let discard = events
            .iter()
            .position(|e| matches!(e, Event::Discard { .. }))
            .expect("打牌が出ていない");
        assert!(declare < discard, "宣言が打牌より後に出ている");
    }

    /// 宣言牌は河で横向きになる。
    #[test]
    fn the_declaration_tile_is_marked_in_the_river() {
        let mut engine = start_at(0);
        engine.drain_events();
        declare_riichi(&mut engine, 1_000);
        let river = &engine.state().seat(Seat::new(0)).river;
        assert!(river.last().expect("河に1枚ある").riichi_declaration);
    }

    /// 宣言しただけでは供託は出ない。
    #[test]
    fn declaring_alone_does_not_pay_the_stick() {
        let mut engine = start_at(0);
        engine.drain_events();
        declare_riichi(&mut engine, 1_000);
        assert_eq!(riichi_of(&engine, Seat::new(0)).step, RiichiStep::Declare);
        assert_eq!(engine.state().scores[0], 25_000);
        assert_eq!(engine.state().riichi_sticks, 0);
    }

    /// 誰も和了しなければ成立し、1000点が供託へ移る。
    #[test]
    fn a_riichi_is_accepted_once_nobody_wins_on_it() {
        let mut engine = start_at(0);
        engine.drain_events();
        declare_riichi(&mut engine, 1_000);
        engine.drain_events();
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);

        let events = engine.drain_events();
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Riichi {
                step: RiichiStep::Accepted,
                seat
            } if *seat == Seat::new(0)
        )));
        assert_eq!(riichi_of(&engine, Seat::new(0)).step, RiichiStep::Accepted);
        assert_eq!(engine.state().scores[0], 24_000);
        assert_eq!(engine.state().riichi_sticks, 1);
    }

    /// 成立すると一発が立つ。
    #[test]
    fn an_accepted_riichi_starts_with_ippatsu() {
        let mut engine = start_at(0);
        engine.drain_events();
        declare_riichi(&mut engine, 1_000);
        engine.drain_events();
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        assert!(riichi_of(&engine, Seat::new(0)).ippatsu);
    }

    /// 宣言牌をロンされたらリーチは成立しない。供託も出ない。
    #[test]
    fn a_ron_on_the_declaration_cancels_the_riichi() {
        let mut engine = start_at(0);
        engine.drain_events();
        // 席1が 1z の単騎で和了れる形にする。123m456m789m は一気通貫なので
        // 門前ロンに役がある。13枚を13枚へ差し替えるので総数は変わらない。
        engine.state_mut().seat_mut(Seat::new(1)).hand =
            parse_hand("123m456m789m123s1z").expect("正しい記法");
        let tile = declare_riichi(&mut engine, 1_000);
        engine.drain_events();

        let window_id = engine.next_window_id() - 1;
        let responded = engine.apply(
            Seat::new(1),
            Command::CallResponse {
                window_id,
                response: CallResponse::Ron,
            },
            1_400,
        );
        assert_eq!(responded, Ok(()), "1z でロンできる形にしてある");
        let _ = tile;

        assert_eq!(riichi_of(&engine, Seat::new(0)).step, RiichiStep::Declare);
        assert_eq!(engine.state().riichi_sticks, 0, "供託は出ていない");
        // **持ち点そのものは見ない。**ロンの精算で放銃分が動いており、
        // その額は配牌のドラ次第で変わる。供託が出ていないことは
        // 「卓の点棒の合計が減っていない」で見るほうが確実である。
        // 供託が1本出ていれば合計は 99,000 になる。
        assert_eq!(engine.state().scores.iter().sum::<i32>(), 100_000);
    }

    /// 宣言牌を鳴かれてもリーチは成立する。ただし一発は消える。
    #[test]
    fn a_call_on_the_declaration_keeps_the_riichi_but_kills_ippatsu() {
        let mut engine = start_at(0);
        engine.drain_events();
        // 席1が 1z をポンできる形にする。
        let target = parse_tile("1z").expect("正しい記法");
        for seat in [Seat::new(2), Seat::new(3)] {
            for held in engine.state_mut().seat_mut(seat).hand.iter_mut() {
                if held.kind() == target.kind() {
                    *held = parse_tile("9m").expect("正しい記法");
                }
            }
        }
        engine.state_mut().seat_mut(Seat::new(1)).hand[0] = target;
        engine.state_mut().seat_mut(Seat::new(1)).hand[1] = target;

        declare_riichi(&mut engine, 1_000);
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

        let state = riichi_of(&engine, Seat::new(0));
        assert_eq!(state.step, RiichiStep::Accepted, "宣言は生きている");
        assert!(!state.ippatsu, "鳴かれたら一発は消える");
        assert_eq!(engine.state().riichi_sticks, 1);
    }

    /// 最初のツモでのリーチはダブルリーチになる。
    #[test]
    fn a_riichi_on_the_first_draw_is_double() {
        let mut engine = start_at(0);
        engine.drain_events();
        declare_riichi(&mut engine, 1_000);
        assert!(riichi_of(&engine, Seat::new(0)).double);
    }

    /// 2巡目以降のリーチはダブルリーチにならない。
    #[test]
    fn a_later_riichi_is_not_double() {
        let mut engine = start_at(0);
        engine.drain_events();
        engine.state_mut().draw_count[0] = 2;
        declare_riichi(&mut engine, 1_000);
        assert!(!riichi_of(&engine, Seat::new(0)).double);
    }

    /// 誰かが鳴いていればダブルリーチにならない。
    #[test]
    fn a_call_anywhere_cancels_double_riichi() {
        let mut engine = start_at(0);
        engine.drain_events();
        engine.state_mut().any_call_made = true;
        declare_riichi(&mut engine, 1_000);
        assert!(!riichi_of(&engine, Seat::new(0)).double);
    }

    /// リーチできない牌でリーチ宣言はできない。
    #[test]
    fn a_discard_that_breaks_tenpai_cannot_declare_riichi() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "234567m23478p22s1z");
        // 2m を切るとテンパイが崩れる。
        assert_eq!(
            engine.apply(
                Seat::new(0),
                Command::Discard {
                    tile: parse_tile("2m").expect("正しい記法"),
                    riichi: true,
                },
                1_000
            ),
            Err(Reject::NotOffered)
        );
    }

    /// リーチ中は一発ツモが成立する。
    #[test]
    fn a_riichi_seat_can_win_with_ippatsu_tsumo() {
        let mut engine = start_at(0);
        engine.drain_events();
        declare_riichi(&mut engine, 1_000);
        engine.drain_events();
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        engine.drain_events();

        // 親の番を作り直して 6p を引かせる。
        // `force_draw_turn` は締切を「発行時刻0 + 基準 + バンク + 猶予」で
        // 作る。WAY_PAST を渡すと tick が期限切れと見なして自動和了するので、
        // 期限内の小さい時刻で宣言する。
        engine.force_draw_turn(Seat::new(0), parse_tile("6p").expect("正しい記法"));
        engine
            .apply(Seat::new(0), Command::Tsumo, 2_000)
            .expect("ツモ和了できる");

        let events = engine.drain_events();
        let Some(Event::Agari { results, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Agari { .. }))
            .cloned()
        else {
            panic!("Agari が出ていない: {events:?}");
        };
        let ids: Vec<protocol::yaku::YakuId> = results[0].yaku.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&protocol::yaku::YakuId::Ippatsu), "{ids:?}");
        assert!(ids.contains(&protocol::yaku::YakuId::DoubleRiichi), "{ids:?}");
    }

    /// 和了者のリーチが成立していれば裏ドラを渡す。
    #[test]
    fn a_riichi_winner_receives_the_ura_indicators() {
        let mut engine = start_at(0);
        engine.drain_events();
        declare_riichi(&mut engine, 1_000);
        engine.drain_events();
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        engine.drain_events();
        engine.force_draw_turn(Seat::new(0), parse_tile("6p").expect("正しい記法"));
        // 締切の理由は上のテストと同じ。期限内の時刻で宣言する。
        engine
            .apply(Seat::new(0), Command::Tsumo, 2_000)
            .expect("ツモ和了できる");

        let events = engine.drain_events();
        let Some(Event::Agari { results, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Agari { .. }))
            .cloned()
        else {
            panic!("Agari が出ていない");
        };
        assert_eq!(
            results[0].ura_indicators,
            Some(engine.state().wall.ura_indicators().to_vec())
        );
    }

    /// リーチしていない和了者に裏ドラは渡さない。
    #[test]
    fn a_winner_without_riichi_gets_no_ura() {
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
            panic!("Agari が出ていない");
        };
        assert_eq!(results[0].ura_indicators, None);
    }

    /// 供託を出しても点棒の合計は変わらない。
    #[test]
    fn paying_the_stick_keeps_the_table_total() {
        let mut engine = start_at(0);
        engine.drain_events();
        declare_riichi(&mut engine, 1_000);
        engine.drain_events();
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        engine.drain_events();

        let total: i32 = engine.state().scores.iter().sum::<i32>()
            + engine.state().riichi_sticks as i32 * 1_000;
        assert_eq!(total, 100_000);
    }

    /// リーチが成立しても牌の総数は変わらない。
    #[test]
    fn a_riichi_conserves_every_tile() {
        let mut engine = start_at(0);
        engine.drain_events();
        declare_riichi(&mut engine, 1_000);
        engine.drain_events();
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        crate::invariant::assert_tiles_conserved(engine.state());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine riichi_tests`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

`ending_tests` の2つの補助を `pub(super)` にする。

```rust
    pub(super) fn make_tenpai(engine: &mut RoundEngine, seat: Seat) {
    pub(super) fn set_dealer_hand(engine: &mut RoundEngine, notation: &str) {
```

`apply_discard` がリーチを受け付ける。

```rust
        // 打牌の候補とリーチ宣言の候補は別に見る。
        let options = discard_options(&self.state, seat, start);
        let (allowed, riichi_allowed) = options
            .iter()
            .find_map(|o| match o {
                ActionOption::Discard {
                    allowed,
                    riichi_allowed,
                } => Some((allowed.clone(), riichi_allowed.clone())),
                _ => None,
            })
            .unwrap_or_default();
        if riichi {
            if !riichi_allowed.contains(&tile) {
                return Err(Reject::NotOffered);
            }
        } else if !allowed.contains(&tile) {
            return Err(Reject::NotOffered);
        }

        self.charge(seat, now_ms);
        if riichi {
            self.declare_riichi(seat);
        }
        self.discard(seat, tile, now_ms);
        Ok(())
```

宣言と成立を分けて実装する。

```rust
    /// リーチを宣言する。**打牌より先に出す。**
    ///
    /// 供託はまだ出さない。宣言牌がロンされたら成立しないためである。
    fn declare_riichi(&mut self, seat: Seat) {
        let double = self.state.draw_count[seat.index()] == 1 && !self.state.any_call_made;
        self.state.seat_mut(seat).riichi = Some(crate::state::RiichiState {
            step: RiichiStep::Declare,
            declared_at_turn: self.state.draw_count[seat.index()],
            ippatsu: false,
            double,
        });
        self.emit(Event::Riichi {
            seat,
            step: RiichiStep::Declare,
        });
    }

    /// 宣言だけのリーチを成立させる。
    ///
    /// 宣言牌がロンされた経路からは呼ばない。鳴かれた経路からは呼ぶ。
    /// 鳴かれた場合の一発は、呼び出し側が `clear_ippatsu` で消す。
    fn accept_riichi_of(&mut self, seat: Seat) {
        let pending = matches!(
            &self.state.seat(seat).riichi,
            Some(r) if r.step == RiichiStep::Declare
        );
        if !pending {
            return;
        }
        let before = self.state.scores;
        if let Some(riichi) = self.state.seat_mut(seat).riichi.as_mut() {
            riichi.step = RiichiStep::Accepted;
            riichi.ippatsu = true;
        }
        self.state.scores[seat.index()] -= crate::state::RIICHI_STICK;
        self.state.riichi_sticks += 1;
        invariant::assert_scores_conserved(
            &before,
            &self.state.scores,
            crate::state::RIICHI_STICK,
        );
        self.emit(Event::Riichi {
            seat,
            step: RiichiStep::Accepted,
        });
    }

    /// 一発を全席から消す。鳴きが入ったときに呼ぶ。
    fn clear_ippatsu(&mut self) {
        for seat in Seat::ALL {
            if let Some(riichi) = self.state.seat_mut(seat).riichi.as_mut() {
                riichi.ippatsu = false;
            }
        }
    }
```

`advance_after_pass` の先頭で成立させる。

```rust
    fn advance_after_pass(&mut self, from: Seat, now_ms: u64) {
        // 誰も和了しなかったので、宣言していたリーチが成立する。
        self.accept_riichi_of(from);
        if self.state.wall.live_remaining() == 0 {
```

`apply_call` でも成立させ、一発を消す。`record_passes` の直前へ置く。

```rust
        // 鳴かれても宣言は生きる。ただし一発は消える。
        self.accept_riichi_of(from);
        self.clear_ippatsu();
        self.state.any_call_made = true;
        self.record_passes(&[seat]);
```

`discard` で、リーチした席の次の打牌が一発を終わらせる。**`open_reaction` より前**に
消す。その打牌へのロンは一発ではない。

```rust
        // 成立済みのリーチの席が打った時点で一発は切れる。
        // 宣言牌のときは step が Declare なので、ここは通らない。
        if let Some(riichi) = self.state.seat_mut(seat).riichi.as_mut() {
            if riichi.step == RiichiStep::Accepted {
                riichi.ippatsu = false;
            }
        }
        self.emit(Event::Discard { seat, tile, manner });
        invariant::assert_tiles_conserved(&self.state);
        self.open_reaction(seat, tile, now_ms);
```

`finish_with_ron` と `finish_with_tsumo` の `ura_indicators: None` を差し替える。

```rust
            ura_indicators: self.ura_for(*seat),
```

```rust
    /// 裏ドラは、リーチが成立している和了者にだけ渡す。
    fn ura_for(&self, seat: Seat) -> Option<Vec<Tile>> {
        matches!(
            &self.state.seat(seat).riichi,
            Some(r) if r.step == RiichiStep::Accepted
        )
        .then(|| self.state.wall.ura_indicators().to_vec())
    }
```

`finish_with_tsumo` は `seat` を直接渡す（`*seat` ではない）。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine riichi_tests`
Expected: 16テスト PASS

- [ ] **Step 5: 既存のテストを壊していないことを確認する**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: engine 216テスト PASS、警告ゼロ

- [ ] **Step 6: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): リーチの宣言と成立を実装"
```

---

### Task 2: 槓

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**Interfaces:**
- Produces: `Command::Ankan` / `Command::Kakan` の受理、`CallResponse::Kan` の受理
- Consumes: `crate::state::PendingKan`、`crate::reaction::WindowKind::Chankan`、`crate::round::chankan_options`

**槓の流れ。**どの槓も同じ順序で進む。

```
KanDeclared  … 宣言。ここで pending_kan を立てる
  →
槍槓ウィンドウ … chankan_options で候補を作る。誰も和了しなければ次へ
  →
Call         … 成立。手牌と副露を動かす
  →
DoraReveal   … 新しいドラ表示をめくる
  →
Draw         … 王牌から嶺上牌を引く（DrawSource::DeadWall）
  →
RequestAction … 槓した席の打牌
```

**明槓だけは槍槓ウィンドウを開かない。**打牌への反応ウィンドウで確定しており、
そこで和了できた席はもう答えているためである。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod kan_tests {
    use super::discard_tests::{state_where_seat_one_can_pon, WAY_PAST_ANY_DEADLINE_MS};
    use super::ending_tests::{make_tenpai, set_dealer_hand};
    use super::start_tests::start_at;
    use super::*;
    use protocol::command::{CallResponse, Command};
    use protocol::notation::{parse_hand, parse_tile};

    fn kinds_of(events: &[Event]) -> Vec<&'static str> {
        events
            .iter()
            .map(|e| match e {
                Event::KanDeclared { .. } => "kan_declared",
                Event::Call { .. } => "call",
                Event::DoraReveal { .. } => "dora",
                Event::Draw { .. } => "draw",
                Event::RequestAction { .. } => "request",
                Event::Discard { .. } => "discard",
                Event::ActionPassed { .. } => "passed",
                _ => "other",
            })
            .collect()
    }

    /// 暗槓は宣言・成立・ドラ・嶺上ツモ・要求の順に進む。
    #[test]
    fn an_ankan_runs_through_its_whole_sequence() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "1111m234p567p22s78s");
        engine
            .apply(
                Seat::new(0),
                Command::Ankan {
                    kind: parse_tile("1m").expect("正しい記法").kind(),
                },
                1_000,
            )
            .expect("暗槓できる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);

        let events = engine.drain_events();
        assert_eq!(
            kinds_of(&events),
            vec!["kan_declared", "call", "dora", "draw", "request"],
            "{events:?}"
        );
    }

    /// 暗槓は手から4枚を副露へ移す。総数は変わらない。
    #[test]
    fn an_ankan_moves_four_tiles_into_a_concealed_meld() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "1111m234p567p22s78s");
        engine
            .apply(
                Seat::new(0),
                Command::Ankan {
                    kind: parse_tile("1m").expect("正しい記法").kind(),
                },
                1_000,
            )
            .expect("暗槓できる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        engine.drain_events();

        let seat = engine.state().seat(Seat::new(0));
        assert_eq!(seat.melds.len(), 1);
        assert_eq!(seat.melds[0].kind, MeldKind::Ankan);
        assert_eq!(seat.melds[0].tiles.len(), 4);
        assert_eq!(seat.melds[0].from, None, "暗槓に鳴いた相手はいない");
        // 14枚 - 4枚 + 嶺上1枚 = 11枚
        assert_eq!(seat.hand.len(), 11);
        crate::invariant::assert_tiles_conserved(engine.state());
    }

    /// 暗槓でも門前は保たれる。
    #[test]
    fn an_ankan_keeps_the_hand_closed() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "1111m234p567p22s78s");
        engine
            .apply(
                Seat::new(0),
                Command::Ankan {
                    kind: parse_tile("1m").expect("正しい記法").kind(),
                },
                1_000,
            )
            .expect("暗槓できる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        assert!(engine.state().is_menzen(Seat::new(0)));
    }

    /// 槓は天和・地和・九種九牌の資格を消す。
    #[test]
    fn a_kan_marks_the_round_as_opened() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "1111m234p567p22s78s");
        assert!(!engine.state().any_call_made);
        engine
            .apply(
                Seat::new(0),
                Command::Ankan {
                    kind: parse_tile("1m").expect("正しい記法").kind(),
                },
                1_000,
            )
            .expect("暗槓できる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        assert!(engine.state().any_call_made);
    }

    /// ドラ表示は槓のたびに1枚増える。
    #[test]
    fn each_kan_reveals_one_more_dora() {
        let mut engine = start_at(0);
        engine.drain_events();
        assert_eq!(engine.state().wall.dora_indicators().len(), 1);
        set_dealer_hand(&mut engine, "1111m234p567p22s78s");
        engine
            .apply(
                Seat::new(0),
                Command::Ankan {
                    kind: parse_tile("1m").expect("正しい記法").kind(),
                },
                1_000,
            )
            .expect("暗槓できる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        assert_eq!(engine.state().wall.dora_indicators().len(), 2);
    }

    /// 嶺上牌は王牌から引く。生牌の残りは減らない。
    #[test]
    fn the_replacement_comes_from_the_dead_wall() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "1111m234p567p22s78s");
        let live_before = engine.state().wall.live_remaining();
        engine
            .apply(
                Seat::new(0),
                Command::Ankan {
                    kind: parse_tile("1m").expect("正しい記法").kind(),
                },
                1_000,
            )
            .expect("暗槓できる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);

        let events = engine.drain_events();
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Draw {
                source: DrawSource::DeadWall,
                ..
            }
        )));
        assert_eq!(
            engine.state().wall.live_remaining(),
            live_before - 1,
            "嶺上を引くと生牌の最後の1枚が引けなくなる"
        );
    }

    /// 槓の数を席ごとに数える。
    #[test]
    fn a_kan_is_counted_for_its_seat() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "1111m234p567p22s78s");
        engine
            .apply(
                Seat::new(0),
                Command::Ankan {
                    kind: parse_tile("1m").expect("正しい記法").kind(),
                },
                1_000,
            )
            .expect("暗槓できる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        assert_eq!(engine.state().kan_count, [1, 0, 0, 0]);
    }

    /// 提示していない暗槓は受け付けない。
    #[test]
    fn an_unoffered_ankan_is_rejected() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "234567m23478p22s1z");
        assert_eq!(
            engine.apply(
                Seat::new(0),
                Command::Ankan {
                    kind: parse_tile("1m").expect("正しい記法").kind()
                },
                1_000
            ),
            Err(Reject::NotOffered)
        );
    }

    /// 加槓はポンした副露を槓へ育てる。
    #[test]
    fn a_kakan_grows_an_existing_pon() {
        let mut engine = start_at(0);
        engine.drain_events();
        let target = parse_tile("4p").expect("正しい記法");
        engine.state_mut().seat_mut(Seat::new(0)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("444p").expect("正しい記法"),
            from: Some(Seat::new(3)),
            called_tile: Some(target),
        });
        // 副露3枚のぶん手牌を11枚にし、4枚目を持たせる。
        engine.state_mut().seat_mut(Seat::new(0)).hand =
            parse_hand("234567m78p22s4p").expect("正しい記法");

        engine
            .apply(Seat::new(0), Command::Kakan { tile: target }, 1_000)
            .expect("加槓できる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        engine.drain_events();

        let seat = engine.state().seat(Seat::new(0));
        assert_eq!(seat.melds.len(), 1, "副露は増えない");
        assert_eq!(seat.melds[0].kind, MeldKind::Kakan);
        assert_eq!(seat.melds[0].tiles.len(), 4);
        assert_eq!(seat.melds[0].from, Some(Seat::new(3)), "元のポンの相手を残す");
    }

    /// 明槓は打牌への反応から成立する。槍槓ウィンドウは開かない。
    #[test]
    fn a_minkan_is_called_from_a_discard() {
        let mut engine = start_at(0);
        engine.drain_events();
        let target = state_where_seat_one_can_pon(&mut engine, "5p");
        // 3枚目を持たせて明槓できるようにする。
        engine.state_mut().seat_mut(Seat::new(1)).hand[2] = target;
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
                    response: CallResponse::Kan,
                },
                1_400,
            )
            .expect("明槓できる");

        let events = engine.drain_events();
        assert_eq!(
            kinds_of(&events),
            vec!["call", "dora", "draw", "request"],
            "{events:?}"
        );
        let seat = engine.state().seat(Seat::new(1));
        assert_eq!(seat.melds[0].kind, MeldKind::Minkan);
        assert_eq!(seat.melds[0].from, Some(Seat::new(0)));
        crate::invariant::assert_tiles_conserved(engine.state());
    }

    /// 加槓は槍槓できる。槓は成立せず、手牌も副露も変わらない。
    #[test]
    fn a_kakan_can_be_robbed() {
        let mut engine = start_at(0);
        engine.drain_events();
        // 席1が 4p で和了れる形にする。
        engine.state_mut().seat_mut(Seat::new(1)).hand =
            parse_hand("234567m23478p22s").expect("正しい記法");
        let target = parse_tile("6p").expect("正しい記法");
        engine.state_mut().seat_mut(Seat::new(0)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("666p").expect("正しい記法"),
            from: Some(Seat::new(3)),
            called_tile: Some(target),
        });
        engine.state_mut().seat_mut(Seat::new(0)).hand =
            parse_hand("234567m78p22s6p").expect("正しい記法");

        engine
            .apply(Seat::new(0), Command::Kakan { tile: target }, 1_000)
            .expect("加槓できる");
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
            .expect("槍槓できる");

        let events = engine.drain_events();
        assert!(events.iter().any(|e| matches!(e, Event::Agari { .. })));
        assert!(
            !events.iter().any(|e| matches!(e, Event::Call { .. })),
            "槍槓されたら槓は成立しない"
        );
        assert_eq!(
            engine.state().seat(Seat::new(0)).melds[0].kind,
            MeldKind::Pon,
            "副露はポンのまま"
        );
        assert!(engine.state().pending_kan.is_none());
    }

    /// 槍槓は1翻つく。
    #[test]
    fn a_robbed_kan_scores_its_yaku() {
        let mut engine = start_at(0);
        engine.drain_events();
        engine.state_mut().seat_mut(Seat::new(1)).hand =
            parse_hand("234567m23478p22s").expect("正しい記法");
        let target = parse_tile("6p").expect("正しい記法");
        engine.state_mut().seat_mut(Seat::new(0)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("666p").expect("正しい記法"),
            from: Some(Seat::new(3)),
            called_tile: Some(target),
        });
        engine.state_mut().seat_mut(Seat::new(0)).hand =
            parse_hand("234567m78p22s6p").expect("正しい記法");
        engine
            .apply(Seat::new(0), Command::Kakan { tile: target }, 1_000)
            .expect("加槓できる");
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
            .expect("槍槓できる");

        let events = engine.drain_events();
        let Some(Event::Agari { results, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Agari { .. }))
            .cloned()
        else {
            panic!("Agari が出ていない");
        };
        let ids: Vec<protocol::yaku::YakuId> = results[0].yaku.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&protocol::yaku::YakuId::Chankan), "{ids:?}");
    }

    /// 暗槓は通常の待ちでは槍槓できない。
    #[test]
    fn an_ankan_is_not_robbed_by_an_ordinary_wait() {
        let mut engine = start_at(0);
        engine.drain_events();
        // 席1は 1m でも和了れないが、待ちがあっても暗槓は槍槓できない。
        engine.state_mut().seat_mut(Seat::new(1)).hand =
            parse_hand("234567m23478p22s").expect("正しい記法");
        set_dealer_hand(&mut engine, "1111m234p567p22s78s");
        engine
            .apply(
                Seat::new(0),
                Command::Ankan {
                    kind: parse_tile("1m").expect("正しい記法").kind(),
                },
                1_000,
            )
            .expect("暗槓できる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);

        let events = engine.drain_events();
        assert!(events.iter().any(|e| matches!(e, Event::Call { .. })));
        assert!(!events.iter().any(|e| matches!(e, Event::Agari { .. })));
    }

    /// 嶺上ツモで和了れば嶺上開花になる。
    #[test]
    fn winning_on_the_replacement_scores_rinshan() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "1111m234p567p22s78s");
        engine
            .apply(
                Seat::new(0),
                Command::Ankan {
                    kind: parse_tile("1m").expect("正しい記法").kind(),
                },
                1_000,
            )
            .expect("暗槓できる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        engine.drain_events();

        // 嶺上で引いた牌を 6s に差し替えて和了形にする。
        // 234p / 567p / 678s / 22s ＋ 暗槓 1111m で4面子1雀頭になる。
        let hand = &mut engine.state_mut().seat_mut(Seat::new(0)).hand;
        let last = hand.len() - 1;
        hand[last] = parse_tile("6s").expect("正しい記法");
        engine
            .apply(Seat::new(0), Command::Tsumo, WAY_PAST_ANY_DEADLINE_MS + 1)
            .expect("ツモ和了できる");

        let events = engine.drain_events();
        let Some(Event::Agari { results, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Agari { .. }))
            .cloned()
        else {
            panic!("Agari が出ていない: {events:?}");
        };
        let ids: Vec<protocol::yaku::YakuId> = results[0].yaku.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&protocol::yaku::YakuId::RinshanKaihou), "{ids:?}");
    }

    /// 鳴きが入ると一発が消える。槓も鳴きである。
    #[test]
    fn a_kan_kills_everyones_ippatsu() {
        let mut engine = start_at(0);
        engine.drain_events();
        // 席1にリーチ成立の状態を直接作る。
        engine.state_mut().seat_mut(Seat::new(1)).riichi = Some(crate::state::RiichiState {
            step: RiichiStep::Accepted,
            declared_at_turn: 1,
            ippatsu: true,
            double: false,
        });
        set_dealer_hand(&mut engine, "1111m234p567p22s78s");
        engine
            .apply(
                Seat::new(0),
                Command::Ankan {
                    kind: parse_tile("1m").expect("正しい記法").kind(),
                },
                1_000,
            )
            .expect("暗槓できる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);

        let riichi = engine
            .state()
            .seat(Seat::new(1))
            .riichi
            .clone()
            .expect("リーチしている");
        assert!(!riichi.ippatsu);
    }

    /// ポンしか提示されていない席は明槓できない。
    ///
    /// `ReactionWindow` は優先度しか見ず、`Pon` と `Kan` は同順位である。
    /// 進行側で候補そのものと照合しないと、3枚目を探して落ちる。
    #[test]
    fn a_seat_offered_only_a_pon_cannot_call_a_kan() {
        let mut engine = start_at(0);
        engine.drain_events();
        // 手には2枚しかない。明槓の候補は出ない。
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

    /// チーも提示していない牌では鳴けない。ポンと同じ扱いにする。
    #[test]
    fn a_chi_with_tiles_that_were_not_offered_is_rejected() {
        let mut engine = start_at(0);
        engine.drain_events();
        // 席0は席1の上家なので、席1はチーの候補を持ちうる。
        let target = parse_tile("5p").expect("正しい記法");
        engine.state_mut().seat_mut(Seat::new(1)).hand[0] =
            parse_tile("3p").expect("正しい記法");
        engine.state_mut().seat_mut(Seat::new(1)).hand[1] =
            parse_tile("4p").expect("正しい記法");
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
        // 34p は提示されているが、89p は提示されていない。
        let bogus = [
            parse_tile("8p").expect("正しい記法"),
            parse_tile("9p").expect("正しい記法"),
        ];
        assert_eq!(
            engine.apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Chi { tiles: bogus },
                },
                1_400
            ),
            Err(Reject::NotOffered)
        );
    }

    /// 提示していない牌の組み合わせでは鳴けない。
    #[test]
    fn a_call_with_tiles_that_were_not_offered_is_rejected() {
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
        let bogus = parse_tile("9m").expect("正しい記法");
        assert_eq!(
            engine.apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pon {
                        tiles: [bogus, bogus],
                    },
                },
                1_400
            ),
            Err(Reject::NotOffered)
        );
    }

    /// 別の牌種で先に加槓していても、鳴いた相手を取り違えない。
    #[test]
    fn a_second_kakan_reports_its_own_pon_partner() {
        let mut engine = start_at(0);
        engine.drain_events();
        let first = parse_tile("4p").expect("正しい記法");
        let second = parse_tile("6p").expect("正しい記法");
        // 副露は 4p の加槓（4枚・席1から）と 6p のポン（3枚・席3から）。
        // 合わせて7枚。手牌は 14 - 7 = 7枚にする。
        engine.state_mut().seat_mut(Seat::new(0)).melds = vec![
            Meld {
                kind: MeldKind::Kakan,
                tiles: parse_hand("4444p").expect("正しい記法"),
                from: Some(Seat::new(1)),
                called_tile: Some(first),
            },
            Meld {
                kind: MeldKind::Pon,
                tiles: parse_hand("666p").expect("正しい記法"),
                from: Some(Seat::new(3)),
                called_tile: Some(second),
            },
        ];
        engine.state_mut().seat_mut(Seat::new(0)).hand =
            parse_hand("234m567m6p").expect("正しい記法");

        engine
            .apply(Seat::new(0), Command::Kakan { tile: second }, 1_000)
            .expect("加槓できる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);

        let events = engine.drain_events();
        let Some(Event::Call { from, kind, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Call { .. }))
            .cloned()
        else {
            panic!("Call が出ていない: {events:?}");
        };
        assert_eq!(kind, MeldKind::Kakan);
        assert_eq!(from, Seat::new(3), "6p のポンは席3から鳴いている");
        crate::invariant::assert_tiles_conserved(engine.state());
    }

    /// 手番でない席は槓を宣言できない。
    #[test]
    fn a_seat_out_of_turn_cannot_declare_a_kan() {
        let mut engine = start_at(0);
        engine.drain_events();
        assert_eq!(
            engine.apply(
                Seat::new(1),
                Command::Ankan {
                    kind: parse_tile("1m").expect("正しい記法").kind()
                },
                1_000
            ),
            Err(Reject::NotYourTurn)
        );
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine kan_tests`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

`Phase` へ槍槓待ちを足す。

```rust
    /// 槓への槍槓を待つ。
    Chankan,
```

`apply` に槓を振り分ける。

```rust
            Command::Ankan { kind } => self.apply_ankan(seat, kind, now_ms),
            Command::Kakan { tile } => self.apply_kakan(seat, tile, now_ms),
```

宣言を受け付ける。

```rust
    fn apply_ankan(&mut self, seat: Seat, kind: TileKind, now_ms: u64) -> Result<(), Reject> {
        let tile = self.check_kan_offered(seat, KanCandidate::Ankan { kind })?;
        self.charge(seat, now_ms);
        self.declare_kan(seat, MeldKind::Ankan, tile, now_ms);
        Ok(())
    }

    fn apply_kakan(&mut self, seat: Seat, tile: Tile, now_ms: u64) -> Result<(), Reject> {
        self.check_kan_offered(seat, KanCandidate::Kakan { tile })?;
        self.charge(seat, now_ms);
        self.declare_kan(seat, MeldKind::Kakan, tile, now_ms);
        Ok(())
    }

    /// その槓が提示されているか確かめ、宣言する牌を返す。
    ///
    /// 暗槓は種類しか来ないので、手にある実物から1枚選ぶ。赤5を含む場合も
    /// 4枚まとめて副露へ移すため、どれを選んでも副露の中身は変わらない。
    fn check_kan_offered(
        &self,
        seat: Seat,
        wanted: KanCandidate,
    ) -> Result<Tile, Reject> {
        let Phase::Turn { seat: turn, start } = self.phase.clone() else {
            return Err(Reject::NotYourTurn);
        };
        if turn != seat {
            return Err(Reject::NotYourTurn);
        }
        let offered = discard_options(&self.state, seat, start)
            .into_iter()
            .find_map(|o| match o {
                ActionOption::Kan { candidates } => Some(candidates),
                _ => None,
            })
            .unwrap_or_default();
        if !offered.contains(&wanted) {
            return Err(Reject::NotOffered);
        }
        match wanted {
            KanCandidate::Kakan { tile } => Ok(tile),
            KanCandidate::Ankan { kind } => self
                .state
                .seat(seat)
                .hand
                .iter()
                .copied()
                .find(|t| t.kind() == kind)
                .ok_or(Reject::NotOffered),
            KanCandidate::Minkan => Err(Reject::NotOffered),
        }
    }

    /// 槓を宣言し、槍槓のウィンドウを開く。
    fn declare_kan(&mut self, seat: Seat, kind: MeldKind, tile: Tile, now_ms: u64) {
        self.state.pending_kan = Some(crate::state::PendingKan { seat, kind, tile });
        self.emit(Event::KanDeclared { seat, kind, tile });
        self.open_chankan(seat, tile, kind, now_ms);
    }

    fn open_chankan(&mut self, from: Seat, tile: Tile, kind: MeldKind, now_ms: u64) {
        let candidates: [Vec<ActionOption>; 4] = std::array::from_fn(|i| {
            chankan_options(&self.state, Seat::new(i as u8), tile, kind)
        });
        let window_id = self.take_window_id();
        let mut deadline = now_ms + self.state.rules.min_reaction_window_ms as u64;
        for seat in Seat::ALL {
            if candidates[seat.index()].is_empty() {
                continue;
            }
            let lead_in_ms = lead_in_of(&self.since_request[seat.index()]);
            self.since_request[seat.index()].clear();
            let absolute = deadline_for(
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
                deadline_ms: absolute,
            });
            self.pending.push(Event::RequestAction {
                seat,
                window_id,
                options: candidates[seat.index()].clone(),
                deadline_ms: remaining_for_event(absolute, now_ms),
            });
        }
        self.offered = candidates.clone();
        self.last_window_id = window_id;
        self.window = Some(ReactionWindow::open(
            window_id,
            WindowKind::Chankan,
            from,
            tile,
            candidates,
            now_ms,
            deadline,
        ));
        self.phase = Phase::Chankan;
    }
```

`tick` に `Phase::Chankan` を足す。反応と同じ扱いでよい。

```rust
            Phase::Chankan => {
                self.pass_expired_seats(now_ms);
                self.resolve_window(now_ms);
            }
```

`resolve_window` を槍槓へ分岐させる。**`PassAll` の行き先が変わる。**

```rust
            Outcome::PassAll if self.phase == Phase::Chankan => {
                self.window = None;
                self.record_passes(&[]);
                self.complete_pending_kan(now_ms);
            }
            Outcome::PassAll => { /* 既存のまま */ }
```

槍槓で和了した場合は `finish_with_ron` がそのまま使える。**`pending_kan` は
`hand_context` が `chankan` を立てるのに要るので、和了の後に消す。**

既存の `finish_with_ron` は末尾で `let scores = settlement_scores(&self.state, &settlement);`
を作り、続けて `self.finish(...)` を呼ぶ。**その2行のあいだへ置く。**

```rust
        let scores = settlement_scores(&self.state, &settlement);
        // 槍槓の1翻は hand_context が pending_kan を見て立てる。
        // 採点が終わるまで消せない。
        self.state.pending_kan = None;
        self.finish(
```

槓を成立させる。

```rust
    /// 槍槓されなかった槓を成立させる。
    fn complete_pending_kan(&mut self, now_ms: u64) {
        let pending = self.state.pending_kan.take().expect("宣言中の槓がある");
        let seat = pending.seat;
        let kind = pending.kind;

        let (tiles, from) = match kind {
            MeldKind::Ankan => {
                // 手から同じ種類を4枚抜く。
                let mut taken = Vec::with_capacity(4);
                for _ in 0..4 {
                    let position = self
                        .state
                        .seat(seat)
                        .hand
                        .iter()
                        .position(|t| t.kind() == pending.tile.kind())
                        .expect("4枚あることは提示時に確かめている");
                    taken.push(self.state.seat_mut(seat).hand.remove(position));
                }
                self.state.seat_mut(seat).melds.push(Meld {
                    kind: MeldKind::Ankan,
                    tiles: taken.clone(),
                    from: None,
                    called_tile: None,
                });
                // 暗槓に鳴いた相手はいないが、`Event::Call.from` は Option
                // ではない。自分自身を入れる。`Meld.from` は None のままである。
                (taken, seat)
            }
            MeldKind::Kakan => {
                let position = self
                    .state
                    .seat(seat)
                    .hand
                    .iter()
                    .position(|t| *t == pending.tile)
                    .expect("4枚目は手にある");
                let fourth = self.state.seat_mut(seat).hand.remove(position);
                let meld = self
                    .state
                    .seat_mut(seat)
                    .melds
                    .iter_mut()
                    .find(|m| {
                        m.kind == MeldKind::Pon
                            && m.tiles.first().map(|t| t.kind()) == Some(pending.tile.kind())
                    })
                    .expect("元になるポンがある");
                meld.kind = MeldKind::Kakan;
                meld.tiles.push(fourth);
                // **いま育てた副露から取る。**副露を後から走査して最初の
                // Kakan を選ぶと、別の牌種で先に加槓していた副露の相手が
                // 入ってしまう。
                (meld.tiles.clone(), meld.from.unwrap_or(seat))
            }
            _ => unreachable!("宣言できるのは暗槓と加槓だけである"),
        };
        self.emit(Event::Call {
            seat,
            from,
            kind,
            tiles,
        });
        self.after_kan(seat, now_ms);
    }

    /// 槓が成立したあとの共通処理。ドラをめくり、嶺上を引く。
    fn after_kan(&mut self, seat: Seat, now_ms: u64) {
        self.state.kan_count[seat.index()] += 1;
        self.state.any_call_made = true;
        self.clear_ippatsu();

        if let Some(indicator) = self.state.wall.reveal_dora() {
            self.emit(Event::DoraReveal { indicator });
        }
        invariant::assert_tiles_conserved(&self.state);

        self.draw_for(seat, DrawSource::DeadWall);
        self.request_turn(now_ms);
    }
```

明槓を `apply_call` へ足す。**打牌への反応で確定しているので槍槓は開かない。**

```rust
        let (kind, from_hand) = match response {
            CallResponse::Chi { tiles } => (MeldKind::Chi, tiles),
            CallResponse::Pon { tiles } => (MeldKind::Pon, tiles),
            CallResponse::Kan => {
                self.apply_minkan(seat, from, called, now_ms);
                return;
            }
            _ => unreachable!("鳴き以外がここへ来ることはない"),
        };
```

```rust
    fn apply_minkan(&mut self, seat: Seat, from: Seat, called: Tile, now_ms: u64) {
        let mut tiles = Vec::with_capacity(4);
        for _ in 0..3 {
            let position = self
                .state
                .seat(seat)
                .hand
                .iter()
                .position(|t| t.kind() == called.kind())
                .expect("3枚あることは提示時に確かめている");
            tiles.push(self.state.seat_mut(seat).hand.remove(position));
        }
        tiles.push(called);
        self.state.seat_mut(seat).melds.push(Meld {
            kind: MeldKind::Minkan,
            tiles: tiles.clone(),
            from: Some(from),
            called_tile: Some(called),
        });
        self.state.seat_mut(from).nagashi_alive = false;
        if let Some(last) = self.state.seat_mut(from).river.last_mut() {
            last.called_by = Some(seat);
        }
        self.accept_riichi_of(from);
        self.record_passes(&[seat]);
        self.emit(Event::Call {
            seat,
            from,
            kind: MeldKind::Minkan,
            tiles,
        });
        self.after_kan(seat, now_ms);
    }
```

`accept_response` の `CallResponse::Kan` を弾く行を**外し、代わりに応答と候補を
照合する。**

**外すだけでは穴が開く。**`ReactionWindow::respond` は応答と候補を優先度でしか
比べず、`Pon` と `Kan` はどちらも `Priority::Pon` である（`reaction.rs`）。
同じ牌を2枚しか持たない席が `CallResponse::Kan` を送ると素通りし、
`apply_minkan` が3枚目を探して落ちる。

牌の照合も同じ理由で要る。`ReactionWindow` はチーとポンの牌を見ないので、
持っていない牌の組み合わせで鳴けてしまう。**Wave 2b の計画で「Wave 2c が
自分で検査する」と書いたまま結線していなかった。**ここで入れる。

`accept_response` の `window.respond(...)` を呼ぶ直前へ置く。

```rust
        if !self.response_is_offered(seat, &response) {
            return Err(Reject::NotOffered);
        }
```

```rust
    /// 応答が、その席へ実際に提示した候補と一致するか。
    ///
    /// `ReactionWindow::respond` は優先度しか見ない。種別と牌の照合は
    /// 進行側の責務である。`offered` はウィンドウを閉じるまで残っている。
    fn response_is_offered(&self, seat: Seat, response: &CallResponse) -> bool {
        let offered = &self.offered[seat.index()];
        match response {
            // パスは候補を持つ席なら常に許す。
            CallResponse::Pass => true,
            CallResponse::Ron => offered.iter().any(|o| matches!(o, ActionOption::Ron)),
            CallResponse::Kan => offered
                .iter()
                .any(|o| matches!(o, ActionOption::Kan { .. })),
            CallResponse::Chi { tiles } => offered.iter().any(
                |o| matches!(o, ActionOption::Chi { candidates } if candidates.contains(tiles)),
            ),
            CallResponse::Pon { tiles } => offered.iter().any(
                |o| matches!(o, ActionOption::Pon { candidates } if candidates.contains(tiles)),
            ),
        }
    }
```

**既存の `call_tests::a_minkan_is_not_accepted_yet` を削除する。**Wave 2c が
「明槓はまだ受け付けない」ことを固定したテストであり、本タスクでその前提が
変わる。残すと `Err(Reject::NotOffered)` を期待して落ちる。代わりになるのは
本タスクの `a_minkan_is_called_from_a_discard` である。

削除にともない `call_tests` は 8 → 7 テストになる。

インポートへ足すもの。**`WindowKind` は既に取り込まれているので足さない。**
`match_flow.rs` の先頭は `use crate::reaction::{Outcome, ReactionWindow, Rejection, WindowKind};`
である。

```rust
use crate::round::chankan_options;
use protocol::command::KanCandidate;
use protocol::tile::TileKind;
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine kan_tests`
Expected: 20テスト PASS

- [ ] **Step 5: 既存のテストを壊していないことを確認する**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: engine 235テスト PASS、警告ゼロ

- [ ] **Step 6: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 槓と槍槓を実装"
```

---

## Wave 2d 完了の判定

- [ ] `cargo test --workspace` が通る（engine 235テスト）
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] 既存の200件のうち、削除したのは `a_minkan_is_not_accepted_yet` の1件だけである
- [ ] リーチが宣言・成立の2段階で進み、宣言牌をロンされたら供託が出ない
- [ ] 一発が鳴きと次の打牌で消える
- [ ] 裏ドラがリーチ成立者にだけ渡る
- [ ] 槓が宣言・槍槓・成立・ドラ・嶺上ツモの順に進む
- [ ] 槍槓されたら槓が成立しない
- [ ] すべての局面で牌136枚と点棒100000点が保たれる
- [ ] `match_flow.rs` 以外を編集していない

## Wave 2e へ渡すもの

| 部品 | Wave 2e での使われ方 |
|---|---|
| `Phase::Chankan` | 三家和の判定を足す |
| `state.kan_count` | 四開槓の判定に使う |
| `state.first_turn_winds` | 四風連打の判定に使う |
| `accept_riichi_of` | 四家立直は成立した時点で数える |
| `declare_kan` / `after_kan` | 四開槓は4つ目の槓の打牌への反応が解決した後に判定する |
| `finish_with_ron` | 責任払いを `liability` に埋める |
