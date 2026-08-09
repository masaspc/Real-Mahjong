# Wave 2e: 途中流局と責任払い 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 局の終わり方を出し切る。途中流局5種と責任払いを足し、`match_flow.rs` から `unimplemented!` を無くす。

**この計画に含まないもの:** 半荘の進行（連荘・西入・アガリ止め・飛び）は **Wave 2f** が担当する。

**Architecture:** Wave 2d までの `RoundEngine` へ枝を足す。時刻も乱数も外から注入したまま。

**Tech Stack:** Rust 1.97.1 / edition 2021 / `protocol`・`mahjong-core`・Wave 2a〜2d の成果物

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
**作業規約:** `AGENTS.md`
**前提:** Wave 2d がマージ済みであること（engine のテストが235件通ること）

## Global Constraints

- **編集してよいのは `crates/mahjong-engine/src/match_flow.rs` だけである**
- **`round.rs` / `settlement.rs` / `wall.rs` / `reaction.rs` / `state.rs` / `timing.rs` / `invariant.rs` を編集しない**
- **`lib.rs` を編集しない。** Wave 0 で凍結済みである
- `crates/protocol` と `crates/mahjong-core` は凍結済み。**編集も追加もしない**
- **時刻を直接読まない。** `Instant::now()` / `SystemTime::now()` / `rand` を呼ばない
- **既存の235件のテストを1つも壊さない**
- 完了条件は `cargo test --workspace` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` がすべて通ること

## 責任払いをどこに持つか

**状態に置き場所がない。**`RoundState` にも `SeatState` にも責任払いのフィールドは
なく、どちらも凍結済みである。

そのかわり `Meld` は鳴いた相手（`from`）を持ち、**副露は鳴いた順に積まれる。**
和了の時点で副露列を見れば、何番目にどの牌を誰から鳴いたかが復元できる。
責任払いはここから導出する。新しい状態を持たない。

## コーディネータが確定させたルール

| 項目 | 決定 |
|---|---|
| 途中流局の点棒 | 移動しない。供託はそのまま次局へ持ち越す |
| 途中流局の連荘 | 親が続く。`ContinuationReason::AbortiveDraw`、`dealer_repeats = true` |
| 途中流局の手牌開示 | 開かない。`tenpai` は全 false、`revealed_hands` は空。**九種九牌だけは宣言者の手を開く**（何を宣言したかが牌譜から分からなくなるため） |
| 九種九牌 | 自分の最初のツモで、誰も鳴いておらず、幺九牌が9種類以上。`discard_options` が判定済み |
| 四風連打 | 4人が最初の打牌で同じ風牌を切り、その間に誰も鳴いていないこと |
| 四風連打の判定時点 | 4人目の打牌への反応が解決した時点 |
| 四家立直 | 4人目のリーチが**成立**した時点。宣言牌にロンがあれば和了が優先されるので、成立の側で数えれば自動的に正しい |
| 四開槓 | 槓の合計が4で、**2人以上に分かれている**こと。1人で4つなら四槓子が確定しているので続行する |
| 四開槓の判定時点 | 4つ目の槓のあとの打牌への反応が解決した時点。**テストは `kan_count` を直接置いて条件だけを見る。**実際に4つの槓を積む局面を組み立てるのは手間に見合わない |
| 三家和 | `Outcome::Ron` が3席を返した時点。頭ハネより先に判定する |
| 責任払いの成立 | 三元牌の副露が3つ揃う、または風牌の副露が4つ揃うこと。**すべて副露であること**が要る |
| 責任払いを負う席 | その最後の副露を鳴かせた席（`Meld.from`） |
| 手の内が混じる場合 | 責任払いは発生しない。暗刻がいつ揃ったかは副露列から復元できず、推測で誰かに負わせられない |
| 責任払いの形式 | ツモは `Full`、ロンは `Split`。`settle_agari` が食い違いを弾く |
| `Ruleset.liability` | 偽なら責任払いを付けない |

---

## タスクの依存関係

```
1 途中流局 → 2 責任払い
```

直列である。同じファイルを触るためで、内容の依存はない。

---

### Task 1: 途中流局

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**Interfaces:**
- Produces: `Command::Kyuushu` の受理、`RyuukyokuKind` の5種すべての発行
- Consumes: `state.first_turn_winds`、`state.kan_count`、`state.any_call_made`

**判定を1か所へ集める。**打牌への反応が解決した直後に、決まった順で見る。

```
三家和   … resolve_window が Outcome::Ron(3席) を受けた時点
九種九牌 … apply が Command::Kyuushu を受けた時点
四風連打 ┐
四家立直 ├ advance_after_pass の中。リーチの成立より後、荒牌平局より前
四開槓   ┘
```

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod abortive_tests {
    use super::discard_tests::WAY_PAST_ANY_DEADLINE_MS;
    use super::ending_tests::set_dealer_hand;
    use super::start_tests::{rules, start_at};
    use super::*;
    use protocol::command::{CallResponse, Command};
    use protocol::event::RyuukyokuKind;
    use protocol::notation::{parse_hand, parse_tile};

    /// 6p/9p 待ちのテンパイ形。13枚を13枚へ差し替えるので総数は変わらない。
    fn set_tenpai(engine: &mut RoundEngine, seat: Seat) {
        assert_eq!(engine.state().seat(seat).hand.len(), 13);
        engine.state_mut().seat_mut(seat).hand =
            parse_hand("234567m23478p22s").expect("正しい記法");
    }

    /// 指定の牌を全席から追い出す。鳴かれるとテストの意図が崩れるため。
    fn evict_everywhere(engine: &mut RoundEngine, kind: protocol::tile::TileKind) {
        let filler = parse_tile("9m").expect("正しい記法");
        for seat in Seat::ALL {
            for held in engine.state_mut().seat_mut(seat).hand.iter_mut() {
                if held.kind() == kind {
                    *held = filler;
                }
            }
        }
    }

    /// 引いた牌を安全牌へ差し替えてからリーチ宣言する。
    ///
    /// **引いた牌をそのまま切ると、他家の待ちに刺さってロンで終わる。**
    /// テンパイ形は数牌の待ちしか持たないので、字牌なら必ず安全である。
    /// 手牌の枚数は変えないので牌の総数も変わらない。
    fn declare_with_safe_tile(engine: &mut RoundEngine, seat: Seat, now_ms: u64) {
        let safe = parse_tile("3z").expect("正しい記法");
        let last = engine.state().seat(seat).hand.len() - 1;
        engine.state_mut().seat_mut(seat).hand[last] = safe;
        engine
            .apply(
                seat,
                Command::Discard {
                    tile: safe,
                    riichi: true,
                },
                now_ms,
            )
            .expect("リーチできる");
    }

    fn ryuukyoku_of(events: &[Event]) -> (RyuukyokuKind, Option<Seat>) {
        let Some(Event::Ryuukyoku { kind, initiator, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Ryuukyoku { .. }))
            .cloned()
        else {
            panic!("Ryuukyoku が出ていない: {events:?}");
        };
        (kind, initiator)
    }

    // ---------- 九種九牌 ----------

    /// 幺九牌が9種類以上あれば宣言できる。
    #[test]
    fn nine_terminals_can_be_declared() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "19m19p19s12345677z");
        engine
            .apply(Seat::new(0), Command::Kyuushu, 1_000)
            .expect("九種九牌を宣言できる");

        let events = engine.drain_events();
        assert_eq!(
            ryuukyoku_of(&events),
            (RyuukyokuKind::NineTerminals, Some(Seat::new(0)))
        );
        assert_eq!(*engine.phase(), Phase::Done);
    }

    /// 宣言者の手だけを開く。何を宣言したかが牌譜から分かるようにする。
    #[test]
    fn nine_terminals_reveals_only_the_declarer() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "19m19p19s12345677z");
        engine
            .apply(Seat::new(0), Command::Kyuushu, 1_000)
            .expect("九種九牌を宣言できる");

        let events = engine.drain_events();
        let Some(Event::Ryuukyoku { revealed_hands, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Ryuukyoku { .. }))
            .cloned()
        else {
            panic!("Ryuukyoku が出ていない");
        };
        assert_eq!(revealed_hands.len(), 1);
        assert_eq!(revealed_hands[0].0, Seat::new(0));
    }

    /// 8種類では宣言できない。
    #[test]
    fn eight_kinds_cannot_declare_nine_terminals() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "19m345556m19p19s12z");
        assert_eq!(
            engine.apply(Seat::new(0), Command::Kyuushu, 1_000),
            Err(Reject::NotOffered)
        );
    }

    /// 手番でない席は宣言できない。
    #[test]
    fn a_seat_out_of_turn_cannot_declare_nine_terminals() {
        let mut engine = start_at(0);
        engine.drain_events();
        assert_eq!(
            engine.apply(Seat::new(1), Command::Kyuushu, 1_000),
            Err(Reject::NotYourTurn)
        );
    }

    // ---------- 四風連打 ----------

    /// 4人が最初の打牌で同じ風牌を切ると流局する。
    #[test]
    fn four_identical_winds_abort_the_round() {
        let mut engine = start_at(0);
        engine.drain_events();
        let wind = parse_tile("1z").expect("正しい記法");
        let mut now = 1_000u64;
        for seat in Seat::ALL {
            // **打つ直前に毎回追い出す。**一度だけだと、そのあと山から
            // 同じ風牌を引いた席が2枚持ちになり、ポンできてしまう。
            // 鳴きが入ると any_call_made が立って四風連打が消える。
            evict_everywhere(&mut engine, wind.kind());
            engine.state_mut().seat_mut(seat).hand[0] = wind;
            engine
                .apply(
                    seat,
                    Command::Discard {
                        tile: wind,
                        riichi: false,
                    },
                    now,
                )
                .expect("切れる");
            now += WAY_PAST_ANY_DEADLINE_MS;
            engine.tick(now);
        }

        let events = engine.drain_events();
        assert_eq!(ryuukyoku_of(&events), (RyuukyokuKind::FourWinds, None));
        assert_eq!(*engine.phase(), Phase::Done);
    }

    /// 風牌が揃わなければ流局しない。
    #[test]
    fn different_winds_do_not_abort() {
        let mut engine = start_at(0);
        engine.drain_events();
        let winds = ["1z", "1z", "1z", "2z"];
        let mut now = 1_000u64;
        for (index, seat) in Seat::ALL.into_iter().enumerate() {
            let wind = parse_tile(winds[index]).expect("正しい記法");
            for name in ["1z", "2z"] {
                evict_everywhere(&mut engine, parse_tile(name).expect("正しい記法").kind());
            }
            engine.state_mut().seat_mut(seat).hand[0] = wind;
            engine
                .apply(
                    seat,
                    Command::Discard {
                        tile: wind,
                        riichi: false,
                    },
                    now,
                )
                .expect("切れる");
            now += WAY_PAST_ANY_DEADLINE_MS;
            engine.tick(now);
        }
        engine.drain_events();
        assert_ne!(*engine.phase(), Phase::Done);
    }

    /// 風牌でなければ数えない。
    #[test]
    fn a_non_wind_discard_breaks_the_four_winds_count() {
        let mut engine = start_at(0);
        engine.drain_events();
        let wind = parse_tile("1z").expect("正しい記法");
        evict_everywhere(&mut engine, wind.kind());
        engine.state_mut().seat_mut(Seat::new(0)).hand[0] = wind;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: wind,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        engine.drain_events();
        assert_eq!(engine.state().first_turn_winds.len(), 1);
    }

    // ---------- 四家立直 ----------

    /// 4人目のリーチが成立した時点で流局する。
    #[test]
    fn four_riichi_abort_the_round() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "234567m23478p22s1z");
        for seat in [Seat::new(1), Seat::new(2), Seat::new(3)] {
            set_tenpai(&mut engine, seat);
        }

        // 親は 1z を切ってリーチ。他家はツモ牌を切ってリーチする。
        let mut now = 1_000u64;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: parse_tile("1z").expect("正しい記法"),
                    riichi: true,
                },
                now,
            )
            .expect("リーチできる");
        for seat in [Seat::new(1), Seat::new(2), Seat::new(3)] {
            now += WAY_PAST_ANY_DEADLINE_MS;
            engine.tick(now);
            now += 1_000;
            declare_with_safe_tile(&mut engine, seat, now);
        }
        now += WAY_PAST_ANY_DEADLINE_MS;
        engine.tick(now);

        let events = engine.drain_events();
        assert_eq!(ryuukyoku_of(&events), (RyuukyokuKind::FourRiichi, None));
        assert_eq!(engine.state().riichi_sticks, 4, "4本とも供託に残る");
        assert_eq!(
            engine.state().scores.iter().sum::<i32>()
                + engine.state().riichi_sticks as i32 * 1_000,
            100_000
        );
    }

    /// 3人のリーチでは流局しない。
    #[test]
    fn three_riichi_do_not_abort() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "234567m23478p22s1z");
        for seat in [Seat::new(1), Seat::new(2)] {
            set_tenpai(&mut engine, seat);
        }
        let mut now = 1_000u64;
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: parse_tile("1z").expect("正しい記法"),
                    riichi: true,
                },
                now,
            )
            .expect("リーチできる");
        for seat in [Seat::new(1), Seat::new(2)] {
            now += WAY_PAST_ANY_DEADLINE_MS;
            engine.tick(now);
            now += 1_000;
            declare_with_safe_tile(&mut engine, seat, now);
        }
        now += WAY_PAST_ANY_DEADLINE_MS;
        engine.tick(now);
        engine.drain_events();
        assert_ne!(*engine.phase(), Phase::Done);
        assert_eq!(engine.state().riichi_sticks, 3);
    }

    // ---------- 四開槓 ----------

    /// 槓が4つで2人以上に分かれていれば流局する。
    #[test]
    fn four_kans_across_two_seats_abort_the_round() {
        let mut engine = start_at(0);
        engine.drain_events();
        engine.state_mut().kan_count = [2, 2, 0, 0];
        let tile = engine.state().seat(Seat::new(0)).hand[0];
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);

        let events = engine.drain_events();
        assert_eq!(ryuukyoku_of(&events), (RyuukyokuKind::FourKans, None));
    }

    /// 1人で4つなら四槓子が確定しているので続行する。
    #[test]
    fn four_kans_by_one_seat_keep_the_round_going() {
        let mut engine = start_at(0);
        engine.drain_events();
        engine.state_mut().kan_count = [4, 0, 0, 0];
        let tile = engine.state().seat(Seat::new(0)).hand[0];
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        engine.drain_events();
        assert_ne!(*engine.phase(), Phase::Done);
    }

    /// 槓が3つでは流局しない。
    #[test]
    fn three_kans_do_not_abort() {
        let mut engine = start_at(0);
        engine.drain_events();
        engine.state_mut().kan_count = [2, 1, 0, 0];
        let tile = engine.state().seat(Seat::new(0)).hand[0];
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        engine.drain_events();
        assert_ne!(*engine.phase(), Phase::Done);
    }

    // ---------- 三家和 ----------

    /// 3人が同時にロンしたら流局する。
    #[test]
    fn three_rons_abort_the_round() {
        let mut engine = start_at(0);
        engine.drain_events();
        for seat in [Seat::new(1), Seat::new(2), Seat::new(3)] {
            set_tenpai(&mut engine, seat);
        }
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
        for seat in [Seat::new(1), Seat::new(2), Seat::new(3)] {
            engine
                .apply(
                    seat,
                    Command::CallResponse {
                        window_id,
                        response: CallResponse::Ron,
                    },
                    1_400,
                )
                .expect("ロンできる");
        }

        let events = engine.drain_events();
        assert_eq!(ryuukyoku_of(&events), (RyuukyokuKind::ThreeRons, None));
        assert!(!events.iter().any(|e| matches!(e, Event::Agari { .. })));
    }

    // ---------- 共通 ----------

    /// 途中流局では点棒が動かず、供託は持ち越す。
    #[test]
    fn an_abortive_draw_moves_no_points() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "19m19p19s12345677z");
        engine
            .apply(Seat::new(0), Command::Kyuushu, 1_000)
            .expect("九種九牌を宣言できる");
        engine.drain_events();

        let outcome = engine.outcome().expect("終わっている");
        assert_eq!(outcome.scores, [25_000; 4]);
        assert_eq!(outcome.riichi_sticks, 0);
    }

    /// 途中流局は連荘になる。
    #[test]
    fn an_abortive_draw_repeats_the_dealership() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "19m19p19s12345677z");
        engine
            .apply(Seat::new(0), Command::Kyuushu, 1_000)
            .expect("九種九牌を宣言できる");
        engine.drain_events();

        let outcome = engine.outcome().expect("終わっている");
        assert!(outcome.dealer_repeats);
        assert_eq!(outcome.reason, ContinuationReason::AbortiveDraw);
    }

    /// 途中流局でも牌の総数は変わらない。
    #[test]
    fn an_abortive_draw_conserves_every_tile() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "19m19p19s12345677z");
        engine
            .apply(Seat::new(0), Command::Kyuushu, 1_000)
            .expect("九種九牌を宣言できる");
        crate::invariant::assert_tiles_conserved(engine.state());
    }

    /// 流局したあとはコマンドを受け付けない。
    #[test]
    fn an_aborted_round_rejects_further_commands() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(&mut engine, "19m19p19s12345677z");
        engine
            .apply(Seat::new(0), Command::Kyuushu, 1_000)
            .expect("九種九牌を宣言できる");
        assert_eq!(
            engine.apply(Seat::new(0), Command::Kyuushu, 2_000),
            Err(Reject::NotYourTurn)
        );
        let _ = rules();
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine abortive_tests`
Expected: テストの失敗。参照している型もコマンドも既にあるのでコンパイルは通り、
`Reject::NotOffered` や `unimplemented!` で落ちる

- [ ] **Step 3: 実装を書く**

`apply` へ九種九牌を振り分ける。

```rust
            Command::Kyuushu => self.apply_kyuushu(seat, now_ms),
```

```rust
    fn apply_kyuushu(&mut self, seat: Seat, now_ms: u64) -> Result<(), Reject> {
        let Phase::Turn { seat: turn, start } = self.phase.clone() else {
            return Err(Reject::NotYourTurn);
        };
        if turn != seat {
            return Err(Reject::NotYourTurn);
        }
        if !discard_options(&self.state, seat, start)
            .iter()
            .any(|o| matches!(o, ActionOption::Kyuushu))
        {
            return Err(Reject::NotOffered);
        }
        self.charge(seat, now_ms);
        // 宣言者の手だけを開く。何を宣言したかが牌譜から分からなくなるため。
        let revealed = vec![(seat, self.state.seat(seat).hand.clone())];
        self.finish_abortive(RyuukyokuKind::NineTerminals, Some(seat), revealed);
        Ok(())
    }
```

`discard` で最初の巡目の風牌を数える。**河へ積んだ直後**に置く。

```rust
        // 四風連打の材料。最初のツモで、誰も鳴いておらず、風牌のときだけ数える。
        // 数えない打牌が1つでもあれば4つに届かないので、それで判定になる。
        if self.state.draw_count[seat.index()] == 1
            && !self.state.any_call_made
            && is_wind(tile.kind())
        {
            self.state.first_turn_winds.push(tile.kind());
        }
```

```rust
/// 風牌は 1z..4z。字牌は 27 から始まり、東南西北・白發中の順に並ぶ。
fn is_wind(kind: TileKind) -> bool {
    (27..=30).contains(&kind.index())
}

/// 三元牌は 5z..7z。
fn is_dragon(kind: TileKind) -> bool {
    (31..=33).contains(&kind.index())
}
```

`advance_after_pass` へ判定を挟む。**リーチの成立より後、荒牌平局より前。**

```rust
    fn advance_after_pass(&mut self, from: Seat, now_ms: u64) {
        self.accept_riichi_of(from);
        if self.check_abortive() {
            return;
        }
        if self.state.wall.live_remaining() == 0 {
```

```rust
    /// 打牌への反応が解決した時点で見る途中流局。
    ///
    /// 決まった順で見る。同時に条件が立つことは実際には無いが、
    /// 順序を固定しておかないと同じ入力から違う結果が出る。
    fn check_abortive(&mut self) -> bool {
        if self.four_winds_reached() {
            self.finish_abortive(RyuukyokuKind::FourWinds, None, Vec::new());
            return true;
        }
        if self.four_riichi_reached() {
            self.finish_abortive(RyuukyokuKind::FourRiichi, None, Vec::new());
            return true;
        }
        if self.four_kans_reached() {
            self.finish_abortive(RyuukyokuKind::FourKans, None, Vec::new());
            return true;
        }
        false
    }

    fn four_winds_reached(&self) -> bool {
        let winds = &self.state.first_turn_winds;
        winds.len() == 4 && winds.iter().all(|k| *k == winds[0])
    }

    fn four_riichi_reached(&self) -> bool {
        Seat::ALL.iter().all(|s| {
            matches!(
                &self.state.seat(*s).riichi,
                Some(r) if r.step == RiichiStep::Accepted
            )
        })
    }

    /// 槓が4つで2人以上に分かれていること。
    /// 1人で4つなら四槓子が確定しているので続ける。
    fn four_kans_reached(&self) -> bool {
        let total: u32 = self.state.kan_count.iter().map(|c| u32::from(*c)).sum();
        let seats = self.state.kan_count.iter().filter(|c| **c > 0).count();
        total >= 4 && seats >= 2
    }

    /// 途中流局で局を閉じる。点棒は動かず、供託は持ち越す。
    fn finish_abortive(
        &mut self,
        kind: RyuukyokuKind,
        initiator: Option<Seat>,
        revealed_hands: Vec<(Seat, Vec<Tile>)>,
    ) {
        let settlement = protocol::event::Settlement {
            delta: [0; 4],
            entries: Vec::new(),
        };
        self.emit(Event::Ryuukyoku {
            kind,
            initiator,
            tenpai: [false; 4],
            revealed_hands,
            nagashi_winners: Vec::new(),
            settlement,
        });
        let scores = self.state.scores;
        let sticks = self.state.riichi_sticks;
        self.finish(scores, sticks, true, ContinuationReason::AbortiveDraw);
    }
```

`resolve_window` の三家和を差し替える。

```rust
            Outcome::Ron(winners) if winners.len() == 3 => {
                self.window = None;
                self.record_passes(&winners);
                self.finish_abortive(RyuukyokuKind::ThreeRons, None, Vec::new());
            }
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine abortive_tests`
Expected: 17テスト PASS

- [ ] **Step 5: 既存のテストを壊していないことを確認する**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: engine 252テスト PASS、警告ゼロ

- [ ] **Step 6: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 途中流局5種を実装"
```

---

### Task 2: 責任払い

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**`force_draw_turn` の前提を広げる。**いまは `hand.len() == 13` を要求するが、
副露があると手牌はそのぶん短い。ツモの責任払いを試すには副露3つの手へ
ツモらせる必要がある。

```rust
    let expected = 13 - 3 * self.state.seat(seat).melds.len();
    assert_eq!(
        self.state.seat(seat).hand.len(),
        expected,
        "副露1つにつき手牌は3枚短い"
    );
```

副露が無ければ13のままなので、既存のテストには影響しない。

**Interfaces:**
- Produces: `AgariInput.liability` と `AgariResult.liability` を埋める
- Consumes: `protocol::event::{Liability, LiabilityMode}`、`protocol::yaku::YakuId`

**副露列から導出する。**状態に置き場所が無いためである。副露は鳴いた順に
積まれるので、三元牌の副露が3つ揃っていれば、そのうち最後のものが3つ目を
確定させた鳴きである。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod liability_tests {
    use super::start_tests::start_at;
    use super::*;
    use protocol::command::{CallResponse, Command};
    use protocol::event::LiabilityMode;
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::yaku::YakuId;

    /// 席1へ大三元の形を作る。三元牌のポン3つと、4枚の手牌。
    /// 副露9枚 + 手牌4枚 = 13枚で、配牌と同じ枚数になる。
    fn set_daisangen(engine: &mut RoundEngine, last_from: Seat) {
        let seat = Seat::new(1);
        engine.state_mut().seat_mut(seat).melds = vec![
            pon("555z", Seat::new(0)),
            pon("666z", Seat::new(2)),
            pon("777z", last_from),
        ];
        engine.state_mut().seat_mut(seat).hand =
            parse_hand("23m11m").expect("正しい記法");
        crate::invariant::assert_tiles_conserved(engine.state());
    }

    fn pon(notation: &str, from: Seat) -> Meld {
        let tiles = parse_hand(notation).expect("正しい記法");
        let called = tiles[0];
        Meld {
            kind: MeldKind::Pon,
            tiles,
            from: Some(from),
            called_tile: Some(called),
        }
    }

    /// 席0に 4m を切らせて席1がロンする。
    ///
    /// **席2と席3を必ずノーテンにする。**配牌のままだと 4m でロンしたり
    /// ポンしたりしうる。ダブロンになると results の順序が変わり、
    /// 責任払いの主張が別の和了者を指してしまう。
    fn ron_on_four_man(engine: &mut RoundEngine) -> Vec<Event> {
        for seat in [Seat::new(2), Seat::new(3)] {
            engine.state_mut().seat_mut(seat).hand =
                parse_hand("147m258p369s1234z").expect("正しい記法");
        }
        let winning = parse_tile("4m").expect("正しい記法");
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
        engine.drain_events()
    }

    fn agari_of(events: &[Event]) -> Vec<protocol::event::AgariResult> {
        let Some(Event::Agari { results, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Agari { .. }))
            .cloned()
        else {
            panic!("Agari が出ていない: {events:?}");
        };
        results
    }

    /// 三元牌の副露が3つ揃うと、最後に鳴かせた席が責任を負う。
    #[test]
    fn the_seat_that_fed_the_third_dragon_is_liable() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_daisangen(&mut engine, Seat::new(3));
        let events = ron_on_four_man(&mut engine);

        let results = agari_of(&events);
        let liability = results[0].liability.expect("責任払いが成立する");
        assert_eq!(liability.seat, Seat::new(3));
        assert_eq!(liability.yaku, YakuId::Daisangen);
        assert_eq!(liability.mode, LiabilityMode::Split, "ロンは折半");
    }

    /// 責任者が変われば結果も変わる。副露の順序を見ている証拠になる。
    #[test]
    fn a_different_last_pon_moves_the_liability() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_daisangen(&mut engine, Seat::new(2));
        let events = ron_on_four_man(&mut engine);
        assert_eq!(
            agari_of(&events)[0]
                .liability
                .expect("責任払いが成立する")
                .seat,
            Seat::new(2)
        );
    }

    /// 手の内の暗刻が混じると責任払いは発生しない。
    #[test]
    fn a_concealed_dragon_cancels_the_liability() {
        let mut engine = start_at(0);
        engine.drain_events();
        let seat = Seat::new(1);
        // 三元牌の副露は2つだけ。残り1つは手の内に持つ。
        engine.state_mut().seat_mut(seat).melds =
            vec![pon("555z", Seat::new(0)), pon("666z", Seat::new(2))];
        engine.state_mut().seat_mut(seat).hand =
            parse_hand("777z23m11m").expect("正しい記法");
        crate::invariant::assert_tiles_conserved(engine.state());
        let events = ron_on_four_man(&mut engine);

        let results = agari_of(&events);
        assert_eq!(results[0].liability, None);
    }

    /// 暗槓が対象の途中にあっても責任払いは発生しない。
    ///
    /// 最後の副露だけを見ると、暗槓のあとに明副露が続いたときに
    /// 責任者がいるように見えてしまう。
    #[test]
    fn a_concealed_kan_in_the_middle_cancels_the_liability() {
        let mut engine = start_at(0);
        engine.drain_events();
        let seat = Seat::new(1);
        // 暗槓 → ポン → ポン の順に積む。暗槓は4枚だが1面子である。
        engine.state_mut().seat_mut(seat).melds = vec![
            Meld {
                kind: MeldKind::Ankan,
                tiles: parse_hand("5555z").expect("正しい記法"),
                from: None,
                called_tile: None,
            },
            pon("666z", Seat::new(2)),
            pon("777z", Seat::new(3)),
        ];
        // 暗槓4枚 + ポン6枚 + 手牌4枚 = 14枚。元の13枚と入れ替えると
        // 卓全体が137枚になる。**暗槓だけは物理4枚で1面子を数えるため、
        // 他の副露と違って1枚増える。**山から1枚抜いて相殺する。
        engine.state_mut().seat_mut(seat).hand =
            parse_hand("23m11m").expect("正しい記法");
        engine
            .state_mut()
            .wall
            .draw()
            .expect("山に残っている");
        crate::invariant::assert_tiles_conserved(engine.state());
        let events = ron_on_four_man(&mut engine);
        assert_eq!(agari_of(&events)[0].liability, None);
    }

    /// ツモの責任払いは責任者が全額を負担する。
    #[test]
    fn a_tsumo_makes_the_liable_seat_pay_everything() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_daisangen(&mut engine, Seat::new(3));
        engine.force_draw_turn(Seat::new(1), parse_tile("4m").expect("正しい記法"));
        engine
            .apply(Seat::new(1), Command::Tsumo, 2_000)
            .expect("ツモ和了できる");

        let events = engine.drain_events();
        let liability = agari_of(&events)[0]
            .liability
            .expect("責任払いが成立する");
        assert_eq!(liability.seat, Seat::new(3));
        assert_eq!(liability.mode, LiabilityMode::Full, "ツモは全額");

        let Some(Event::Agari { settlement, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Agari { .. }))
            .cloned()
        else {
            panic!("Agari が出ていない");
        };
        assert_eq!(settlement.delta[0], 0, "責任者以外は払わない");
        assert_eq!(settlement.delta[2], 0);
        assert!(settlement.delta[3] < 0);
        assert!(settlement.is_balanced());
    }

    /// 三元牌が2つでは責任払いにならない。
    #[test]
    fn two_dragons_are_not_enough() {
        let mut engine = start_at(0);
        engine.drain_events();
        let seat = Seat::new(1);
        engine.state_mut().seat_mut(seat).melds =
            vec![pon("555z", Seat::new(0)), pon("666z", Seat::new(2))];
        engine.state_mut().seat_mut(seat).hand =
            parse_hand("23m567m11m").expect("正しい記法");
        crate::invariant::assert_tiles_conserved(engine.state());
        let events = ron_on_four_man(&mut engine);
        assert_eq!(agari_of(&events)[0].liability, None);
    }

    /// 風牌の副露が4つ揃えば大四喜の責任払いになる。
    #[test]
    fn the_seat_that_fed_the_fourth_wind_is_liable() {
        let mut engine = start_at(0);
        engine.drain_events();
        let seat = Seat::new(1);
        engine.state_mut().seat_mut(seat).melds = vec![
            pon("111z", Seat::new(0)),
            pon("222z", Seat::new(2)),
            pon("333z", Seat::new(3)),
            pon("444z", Seat::new(0)),
        ];
        // 副露12枚 + 手牌1枚 = 13枚。5z の単騎で和了る。
        engine.state_mut().seat_mut(seat).hand =
            parse_hand("5z").expect("正しい記法");
        crate::invariant::assert_tiles_conserved(engine.state());

        // 席2と席3をノーテンにしてから切らせる。理由は ron_on_four_man と同じ。
        for other in [Seat::new(2), Seat::new(3)] {
            engine.state_mut().seat_mut(other).hand =
                parse_hand("147m258p369s1234z").expect("正しい記法");
        }
        let winning = parse_tile("5z").expect("正しい記法");
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
        let liability = agari_of(&events)[0]
            .liability
            .expect("責任払いが成立する");
        assert_eq!(liability.seat, Seat::new(0), "4つ目の風牌を鳴かせた席");
        assert_eq!(liability.yaku, YakuId::Daisuushii);
    }

    /// ルールで切っていれば責任払いを付けない。
    #[test]
    fn a_ruleset_without_liability_never_assigns_it() {
        let mut engine = RoundEngine::start(
            Ruleset {
                liability: false,
                ..Ruleset::kin_no_ma(protocol::ruleset::MatchLength::Hanchan)
            },
            Round {
                wind: protocol::seat::Wind::East,
                number: 1,
            },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &super::start_tests::seed(),
            1,
            0,
        );
        engine.drain_events();
        set_daisangen(&mut engine, Seat::new(3));
        let events = ron_on_four_man(&mut engine);
        assert_eq!(agari_of(&events)[0].liability, None);
    }

    /// 責任払いがあっても点棒の合計は変わらない。
    #[test]
    fn a_liable_settlement_still_balances() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_daisangen(&mut engine, Seat::new(3));
        let events = ron_on_four_man(&mut engine);
        let Some(Event::Agari { settlement, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Agari { .. }))
            .cloned()
        else {
            panic!("Agari が出ていない");
        };
        assert!(settlement.is_balanced());
        assert_eq!(
            engine.state().scores.iter().sum::<i32>(),
            100_000
        );
    }

    /// 責任者と放銃者で折半する。
    #[test]
    fn a_ron_splits_the_payment_with_the_liable_seat() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_daisangen(&mut engine, Seat::new(3));
        let events = ron_on_four_man(&mut engine);
        let Some(Event::Agari { settlement, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Agari { .. }))
            .cloned()
        else {
            panic!("Agari が出ていない");
        };
        // 放銃は席0、責任は席3。どちらも同額を払う。
        assert_eq!(settlement.delta[0], settlement.delta[3]);
        assert!(settlement.delta[0] < 0);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine liability_tests`
Expected: テストの失敗。`liability` がまだ `None` のままなので、
`expect("責任払いが成立する")` で落ちる

- [ ] **Step 3: 実装を書く**

```rust
use protocol::event::{Liability, LiabilityMode};
use protocol::yaku::YakuId;
```

```rust
    /// 責任払いを副露列から導く。
    ///
    /// 状態に置き場所が無いので、和了の時点で組み立て直す。副露は鳴いた順に
    /// 積まれるため、三元牌の副露が3つ揃っていれば最後のものが3つ目を
    /// 確定させた鳴きである。**手の内の暗刻が混じる場合は付けない。**
    /// いつ揃ったかが復元できず、推測で誰かに負わせられない。
    fn liability_for(&self, seat: Seat, win_type: WinType) -> Option<Liability> {
        if !self.state.rules.liability {
            return None;
        }
        let melds = &self.state.seat(seat).melds;
        let mode = match win_type {
            WinType::Tsumo => LiabilityMode::Full,
            WinType::Ron => LiabilityMode::Split,
        };

        for (yaku, needed, matches_kind) in [
            (YakuId::Daisangen, 3usize, is_dragon as fn(TileKind) -> bool),
            (YakuId::Daisuushii, 4, is_wind as fn(TileKind) -> bool),
        ] {
            let mut last_from = None;
            let mut count = 0usize;
            let mut has_concealed = false;
            for meld in melds {
                let Some(kind) = meld.tiles.first().map(|t| t.kind()) else {
                    continue;
                };
                if !matches_kind(kind) {
                    continue;
                }
                count += 1;
                match meld.from {
                    Some(from) => last_from = Some(from),
                    // **暗槓が1つでもあれば責任払いは無い。**
                    // `last_from` を上書きするだけだと、暗槓のあとに明副露が
                    // 続いたときに Some へ戻ってしまう。見つけたことを覚える。
                    None => has_concealed = true,
                }
            }
            if count == needed && !has_concealed {
                if let Some(from) = last_from {
                    return Some(Liability { seat: from, yaku, mode });
                }
            }
        }
        None
    }
```

`finish_with_ron` の `liability: None` を2か所とも差し替える。

```rust
            let liability = self.liability_for(*seat, WinType::Ron);
```

`AgariInput` と `AgariResult` の両方へ同じ値を入れる。`finish_with_tsumo` も
同様に `WinType::Tsumo` で呼ぶ。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine liability_tests`
Expected: 10テスト PASS

- [ ] **Step 5: 既存のテストを壊していないことを確認する**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: engine 262テスト PASS、警告ゼロ

- [ ] **Step 6: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 責任払いを副露列から導く"
```

---

## Wave 2e 完了の判定

- [ ] `cargo test --workspace` が通る（engine 262テスト）
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] 既存の235件を1つも壊していない
- [ ] **`match_flow.rs` に `unimplemented!` が1つも残っていない**
- [ ] 途中流局5種すべてが生成される
- [ ] 途中流局で点棒が動かず、供託が持ち越される
- [ ] 責任払いが副露の順序から導かれる
- [ ] すべての局面で牌136枚と点棒100000点が保たれる
- [ ] `match_flow.rs` 以外を編集していない

## Wave 2f へ渡すもの

| 部品 | Wave 2f での使われ方 |
|---|---|
| `RoundOutcome` | 連荘・西入・アガリ止め・飛びの判定に使う |
| `ContinuationReason` | `RoundEnd.reason` にそのまま載せる |
| `RoundEngine::next_window_id` | 次局の採番の起点にする |
| `finish_abortive` | 途中流局も `RoundOutcome` を返すので、半荘側は区別せず扱える |
