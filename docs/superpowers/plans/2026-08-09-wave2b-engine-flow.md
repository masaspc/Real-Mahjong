# Wave 2b: 精算と合法手の生成 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 進行のステートマシンが必要とする2つの土台を作る。点棒の移動を計算する精算と、その局面で取れる手を列挙する合法手生成である。どちらも進行から切り離した形にして、点数とルールの正しさを進行のテストと混ぜずに検証する。

**この計画に局の進行そのものは含まない。** `round.rs` の `RoundEngine`、槓とリーチ、途中流局、半荘は **Wave 2c** が担当する。先に精算と合法手の API を確定させるほうが手戻りが少ないためである。

**Architecture:** 乱数と時間は外から注入されたまま。同じシード・同じコマンド列・同じ時刻列からは必ず同じイベント列が出る。

**Tech Stack:** Rust 1.97.1 / edition 2021 / `protocol`・`mahjong-core`・Wave 2a の部品に依存

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
**作業規約:** `AGENTS.md`
**前提:** `docs/superpowers/plans/2026-08-09-wave2a-engine-parts.md` が完了していること

## Global Constraints

- **編集してよいのは次のみ**
  - `crates/mahjong-engine/src/round.rs`（**合法手の生成だけ**。進行のステートマシンは書かない）
  - `crates/mahjong-engine/src/settlement.rs`（新規。`round.rs` から `#[path]` で読み込む）
- **`match_flow.rs` を編集しない。** Wave 2c の所有である
- **`lib.rs` を編集しない。** Wave 0 で凍結済みである
- **`wall.rs` / `reaction.rs` / `state.rs` / `timing.rs` / `invariant.rs` を編集しない。** Wave 2a の成果物である。足りなければ実装を止めて報告する
- `crates/protocol` と `crates/mahjong-core` は凍結済み。**編集も追加もしない**
- **時刻を直接読まない。** `Instant::now()` を呼ばず、`now_ms: u64` を引数で受け取る
- `Ruleset` に存在する値をハードコードしない。無い定数は名前付き定数として置き、根拠を書く
- 完了条件は `cargo test --workspace` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` がすべて通ること

## `lib.rs` を編集せずに `settlement` を足す

`round.rs` の先頭に書く。

```rust
#[path = "settlement.rs"]
mod settlement;
pub use settlement::{settle_agari, settle_exhaustive, settle_nagashi};
```

## コーディネータが確定させたルール

仕様に明記が無く、実装者が推測すると割れる箇所を先に決める。

| 項目 | 決定 |
|---|---|
| 本場料 | 1本場あたり300点。ロンは放銃者が全額、ツモは各家100点ずつ |
| ダブロンの本場料 | **放銃者から見て最も近い和了者**（下家方向）のみが受け取る |
| ダブロンの供託 | 同じく最も近い和了者が総取り |
| ノーテン罰符 | 合計3000点。テンパイ1人なら各ノーテン1000／2人なら1500／3人なら3000。全員テンパイ・全員ノーテンは移動なし |
| 流し満貫 | 子は満貫（親4000・子2000ずつ＝8000）、親は満貫（子4000ずつ＝12000）。テンパイ料より優先し、テンパイ料は発生しない |
| 四開槓 | 4つの槓が**2人以上に分かれた**場合のみ流局。1人で4つなら四槓子確定として続行 |
| 四開槓の判定時点 | 4つ目の槓の**打牌に対する反応が解決した後** |
| 暗槓への槍槓 | **国士無双のみ可** |
| 西入 | 半荘戦で南4局終了時に誰も `return_score` に達していなければ西入。西4局終了または誰かが到達で終局 |
| アガリ止め | あり。オーラスで親が和了しトップなら続行しない |
| テンパイ止め | あり。オーラス荒牌平局で親がテンパイかつトップなら続行しない |
| 四風連打 | 1巡目に4人が同じ風牌を切ったら流局。鳴きが入っていたら成立しない |
| 四家立直 | 4人目のリーチが**成立**した時点で流局。宣言牌にロンがあればロンを優先 |

## タスクの依存関係

```
1 settlement ──┐
               ├─→ Wave 2c（局の進行・槓とリーチ・途中流局・半荘）
2 合法手生成 ──┘
```

Task 1 と 2 は互いに独立で、**並行して実装できる**。

---

### Task 1: 精算

和了・荒牌平局・流し満貫の点棒移動を計算する。**進行から切り離した純粋関数**にして、
点数の正しさを進行のテストと混ぜずに検証する。

**Files:**
- Create: `crates/mahjong-engine/src/settlement.rs`

**Interfaces:**
- Consumes: `protocol::event::{Settlement, SettlementEntry, Liability, LiabilityMode}`、`mahjong_core::score::Payment`
- Produces:
  - `pub struct AgariInput { pub seat: Seat, pub from: Option<Seat>, pub payment: Payment, pub liability: Option<Liability> }`
  - `pub fn settle_agari(winners: &[AgariInput], dealer: Seat, honba: u8, riichi_sticks: u8) -> Settlement`
  - `pub fn settle_exhaustive(tenpai: [bool; 4], rules: &Ruleset) -> Settlement`
  - `pub fn settle_nagashi(winners: &[Seat], dealer: Seat) -> Settlement`
  - `pub fn score_change(settlement: &Settlement) -> [i32; 4]`
  - `pub const HONBA_PER_STICK: i32 = 300;`

**本場と供託の割り当て（ダブロン）:**

放銃者から見て**下家方向に最も近い和了者**が、本場料と供託を総取りする。
点数そのものは各和了者がそれぞれ受け取る。

**供託は `Settlement::delta` に含めない。**`protocol` は `delta` を
「合計は常に0でなければならない」と定義して凍結済みである。供託は宣言時に
すでに各席から引かれて場に出ており、回収は卓の外からの流入になるため、
`delta` に足すとこの不変条件が破れる。供託は `entries[].riichi_sticks` に
記録し、点棒への反映は `score_change()` を通す。

```
delta                … 席と席のあいだの移動。合計は必ず0
entries[].riichi_sticks … 場から回収する分。合計は供託の総額
score_change()        … 上の2つを足した、実際の持ち点の増減
```

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mahjong_core::score::Payment;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::Seat;

    fn rules() -> Ruleset {
        Ruleset::kin_no_ma(MatchLength::Hanchan)
    }

    fn ron(seat: u8, from: u8, total: i32) -> AgariInput {
        AgariInput {
            seat: Seat::new(seat),
            from: Some(Seat::new(from)),
            payment: Payment::Ron { total },
            liability: None,
        }
    }

    /// 子のロン。放銃者が素点を払う。
    #[test]
    fn a_simple_ron_moves_points_from_the_dealer_in() {
        let s = settle_agari(&[ron(1, 0, 3_900)], Seat::new(0), 0, 0);
        assert_eq!(s.delta, [-3_900, 3_900, 0, 0]);
        assert!(s.is_balanced());
    }

    /// 本場は1本300点。ロンなら放銃者が全額払う。
    #[test]
    fn honba_is_paid_by_the_discarder_on_a_ron() {
        let s = settle_agari(&[ron(1, 0, 3_900)], Seat::new(0), 2, 0);
        assert_eq!(s.delta, [-4_500, 4_500, 0, 0]);
    }

    /// 供託は和了者が総取りするが、delta には入らない。
    /// delta は席と席のあいだの移動だけを表し、合計は必ず0である。
    #[test]
    fn riichi_sticks_are_recorded_outside_the_delta() {
        let s = settle_agari(&[ron(1, 0, 3_900)], Seat::new(0), 0, 2);
        assert_eq!(s.delta, [-3_900, 3_900, 0, 0]);
        assert!(s.is_balanced());

        let entry = s.entries.iter().find(|e| e.seat == Seat::new(1)).unwrap();
        assert_eq!(entry.riichi_sticks, 2_000);
    }

    /// 実際の持ち点の増減は delta と供託の和である。
    #[test]
    fn score_change_adds_the_sticks_back_in() {
        let s = settle_agari(&[ron(1, 0, 3_900)], Seat::new(0), 0, 2);
        assert_eq!(score_change(&s), [-3_900, 5_900, 0, 0]);
    }

    /// 子のツモ。親が2倍、子が等分。
    #[test]
    fn a_non_dealer_tsumo_splits_the_payment() {
        let input = AgariInput {
            seat: Seat::new(1),
            from: None,
            payment: Payment::TsumoNonDealer {
                from_dealer: 1_300,
                from_each_non_dealer: 700,
            },
            liability: None,
        };
        let s = settle_agari(&[input], Seat::new(0), 0, 0);
        assert_eq!(s.delta, [-1_300, 2_700, -700, -700]);
        assert!(s.is_balanced());
    }

    /// 親のツモ。子が等分。
    #[test]
    fn a_dealer_tsumo_takes_from_everyone_equally() {
        let input = AgariInput {
            seat: Seat::new(0),
            from: None,
            payment: Payment::TsumoDealer { from_each: 4_000 },
            liability: None,
        };
        let s = settle_agari(&[input], Seat::new(0), 0, 0);
        assert_eq!(s.delta, [12_000, -4_000, -4_000, -4_000]);
        assert!(s.is_balanced());
    }

    /// ツモの本場は各家100点ずつ。
    #[test]
    fn honba_on_a_tsumo_is_split_across_the_others() {
        let input = AgariInput {
            seat: Seat::new(1),
            from: None,
            payment: Payment::TsumoNonDealer {
                from_dealer: 1_300,
                from_each_non_dealer: 700,
            },
            liability: None,
        };
        let s = settle_agari(&[input], Seat::new(0), 1, 0);
        // 本場300を3人で100ずつ
        assert_eq!(s.delta, [-1_400, 3_000, -800, -800]);
        assert!(s.is_balanced());
    }

    /// ダブロン。素点はそれぞれ受け取るが、本場と供託は
    /// 放銃者から見て最も近い和了者だけが取る。
    #[test]
    fn a_double_ron_gives_honba_and_sticks_to_the_nearest_winner() {
        // 放銃は席3。下家方向で最も近い和了者は席0。
        let s = settle_agari(
            &[ron(0, 3, 2_000), ron(2, 3, 8_000)],
            Seat::new(0),
            1,
            1,
        );
        // 席0: 2000 + 本場300 = 2300（供託は delta の外）
        // 席2: 8000
        // 席3: -(2000 + 300 + 8000) = -10300
        assert_eq!(s.delta, [2_300, 0, 8_000, -10_300]);
        assert!(s.is_balanced());

        // 供託は放銃者に最も近い席0だけが受け取る。
        let near = s.entries.iter().find(|e| e.seat == Seat::new(0)).unwrap();
        let far = s.entries.iter().find(|e| e.seat == Seat::new(2)).unwrap();
        assert_eq!(near.riichi_sticks, 1_000);
        assert_eq!(far.riichi_sticks, 0);
        assert_eq!(score_change(&s), [3_300, 0, 8_000, -10_300]);
    }

    /// 責任払い（ツモ）。責任者が全額を負担する。
    #[test]
    fn a_full_liability_makes_one_seat_pay_everything() {
        let input = AgariInput {
            seat: Seat::new(1),
            from: None,
            payment: Payment::TsumoNonDealer {
                from_dealer: 16_000,
                from_each_non_dealer: 8_000,
            },
            liability: Some(Liability {
                seat: Seat::new(2),
                yaku: protocol::yaku::YakuId::Daisangen,
                mode: LiabilityMode::Full,
            }),
        };
        let s = settle_agari(&[input], Seat::new(0), 0, 0);
        assert_eq!(s.delta, [0, 32_000, -32_000, 0]);
        assert!(s.is_balanced());
    }

    /// 責任払い（ロン）。責任者と放銃者で折半する。
    #[test]
    fn a_split_liability_halves_the_payment() {
        let input = AgariInput {
            seat: Seat::new(1),
            from: Some(Seat::new(0)),
            payment: Payment::Ron { total: 32_000 },
            liability: Some(Liability {
                seat: Seat::new(2),
                yaku: protocol::yaku::YakuId::Daisuushii,
                mode: LiabilityMode::Split,
            }),
        };
        let s = settle_agari(&[input], Seat::new(0), 0, 0);
        assert_eq!(s.delta, [-16_000, 32_000, -16_000, 0]);
        assert!(s.is_balanced());
    }

    /// 精算の内訳が復元できる。素点・本場・供託を分けて記録する。
    #[test]
    fn the_settlement_records_its_breakdown() {
        let s = settle_agari(&[ron(1, 0, 3_900)], Seat::new(0), 2, 1);
        let entry = s
            .entries
            .iter()
            .find(|e| e.seat == Seat::new(1))
            .expect("和了者の内訳がある");
        assert_eq!(entry.base, 3_900);
        assert_eq!(entry.honba, 600);
        assert_eq!(entry.riichi_sticks, 1_000);
        assert_eq!(entry.liability, 0);
        assert!(s.is_balanced());
    }

    /// ノーテン罰符は合計3000点。
    #[test]
    fn noten_penalty_is_three_thousand_in_total() {
        let one = settle_exhaustive([true, false, false, false], &rules());
        assert_eq!(one.delta, [3_000, -1_000, -1_000, -1_000]);

        let two = settle_exhaustive([true, true, false, false], &rules());
        assert_eq!(two.delta, [1_500, 1_500, -1_500, -1_500]);

        let three = settle_exhaustive([true, true, true, false], &rules());
        assert_eq!(three.delta, [1_000, 1_000, 1_000, -3_000]);
    }

    /// 全員テンパイ・全員ノーテンは移動なし。
    #[test]
    fn a_uniform_tenpai_state_moves_nothing() {
        assert_eq!(settle_exhaustive([true; 4], &rules()).delta, [0; 4]);
        assert_eq!(settle_exhaustive([false; 4], &rules()).delta, [0; 4]);
    }

    /// 流し満貫。子は満貫、親は親満。
    #[test]
    fn nagashi_pays_a_mangan() {
        let child = settle_nagashi(&[Seat::new(1)], Seat::new(0));
        assert_eq!(child.delta, [-4_000, 8_000, -2_000, -2_000]);
        assert!(child.is_balanced());

        let dealer = settle_nagashi(&[Seat::new(0)], Seat::new(0));
        assert_eq!(dealer.delta, [12_000, -4_000, -4_000, -4_000]);
        assert!(dealer.is_balanced());
    }

    /// 流し満貫が2人成立しても、それぞれが満貫を受け取る。
    #[test]
    fn two_nagashi_winners_are_paid_independently() {
        let s = settle_nagashi(&[Seat::new(1), Seat::new(2)], Seat::new(0));
        // 席1: 親4000 + 子2000×2 = 8000 を受け取る（席2からも2000）
        // 席2: 同様に8000
        assert_eq!(s.delta[1], 8_000 - 2_000);
        assert_eq!(s.delta[2], 8_000 - 2_000);
        assert!(s.is_balanced());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine settlement`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
//! 点棒の移動。進行から切り離した純粋関数にして、点数の正しさを
//! 進行のテストと混ぜずに検証できるようにする。

use mahjong_core::score::Payment;
use protocol::event::{Liability, LiabilityMode, Settlement, SettlementEntry};
use protocol::ruleset::Ruleset;
use protocol::seat::Seat;

/// 1本場あたりの加算。Ruleset に設定項目として存在しない普遍の値。
pub const HONBA_PER_STICK: i32 = 300;

/// リーチ棒1本の点数。`state.rs` の RIICHI_STICK と同じ値だが、
/// あちらは Wave 2a の所有なので参照しない。
const STICK_VALUE: i32 = 1_000;

/// 流し満貫の点数。子の満貫と同じ扱い。
const NAGASHI_BASE: i32 = 2_000;

pub struct AgariInput {
    pub seat: Seat,
    /// ロンなら放銃者、ツモなら None。
    pub from: Option<Seat>,
    pub payment: Payment,
    pub liability: Option<Liability>,
}

/// 放銃者から見て下家方向に最も近い和了者。
/// ダブロンで本場と供託を受け取る席を決める。
fn nearest_winner(winners: &[AgariInput], from: Seat) -> Seat {
    winners
        .iter()
        .map(|w| w.seat)
        .min_by_key(|s| (s.index() + 4 - from.index()) % 4)
        .expect("和了者が1人はいる")
}

/// 実際の持ち点の増減。席間の移動に、場から回収する供託を足したもの。
/// 進行側はこれを持ち点へ加算し、供託の残高を0にする。
pub fn score_change(settlement: &Settlement) -> [i32; 4] {
    let mut out = settlement.delta;
    for entry in &settlement.entries {
        out[entry.seat.index()] += entry.riichi_sticks;
    }
    out
}

pub fn settle_agari(
    winners: &[AgariInput],
    dealer: Seat,
    honba: u8,
    riichi_sticks: u8,
) -> Settlement {
    let mut delta = [0i32; 4];
    let mut entries = Vec::new();

    let honba_total = honba as i32 * HONBA_PER_STICK;
    // 本場と供託を受け取る席。ツモなら和了者本人、ロンなら放銃者に最も近い者。
    let bonus_seat = match winners.first().and_then(|w| w.from) {
        Some(from) => nearest_winner(winners, from),
        None => winners[0].seat,
    };
    let sticks_total = riichi_sticks as i32 * STICK_VALUE;

    for win in winners {
        let mut base = 0i32;
        match (win.payment, win.from) {
            (Payment::Ron { total }, Some(from)) => {
                base = total;
                match win.liability {
                    // ロンの責任払いは放銃者と折半。
                    Some(Liability {
                        seat,
                        mode: LiabilityMode::Split,
                        ..
                    }) => {
                        delta[from.index()] -= total / 2;
                        delta[seat.index()] -= total - total / 2;
                    }
                    _ => delta[from.index()] -= total,
                }
            }
            (Payment::TsumoDealer { from_each }, None) => {
                base = from_each * 3;
                match win.liability {
                    // ツモの責任払いは責任者が全額。
                    Some(Liability {
                        seat,
                        mode: LiabilityMode::Full,
                        ..
                    }) => delta[seat.index()] -= base,
                    _ => {
                        for seat in Seat::ALL {
                            if seat != win.seat {
                                delta[seat.index()] -= from_each;
                            }
                        }
                    }
                }
            }
            (
                Payment::TsumoNonDealer {
                    from_dealer,
                    from_each_non_dealer,
                },
                None,
            ) => {
                base = from_dealer + from_each_non_dealer * 2;
                match win.liability {
                    Some(Liability {
                        seat,
                        mode: LiabilityMode::Full,
                        ..
                    }) => delta[seat.index()] -= base,
                    _ => {
                        for seat in Seat::ALL {
                            if seat == win.seat {
                                continue;
                            }
                            let amount = if seat == dealer {
                                from_dealer
                            } else {
                                from_each_non_dealer
                            };
                            delta[seat.index()] -= amount;
                        }
                    }
                }
            }
            _ => unreachable!("和了種別と支払い形式が食い違っている"),
        }
        delta[win.seat.index()] += base;

        // 本場と供託は最も近い和了者だけが受け取る。
        let (honba_here, sticks_here) = if win.seat == bonus_seat {
            (honba_total, sticks_total)
        } else {
            (0, 0)
        };
        if honba_here > 0 {
            match win.from {
                Some(from) => {
                    delta[from.index()] -= honba_here;
                    delta[win.seat.index()] += honba_here;
                }
                None => {
                    // ツモは各家が等分する。
                    let each = honba_here / 3;
                    for seat in Seat::ALL {
                        if seat != win.seat {
                            delta[seat.index()] -= each;
                            delta[win.seat.index()] += each;
                        }
                    }
                }
            }
        }
        // 供託は delta に足さない。合計0の不変条件を守るためである。

        entries.push(SettlementEntry {
            seat: win.seat,
            base,
            honba: honba_here,
            riichi_sticks: sticks_here,
            // 責任払いで肩代わりされた分。無ければ 0。
            liability: match win.liability {
                Some(Liability { mode: LiabilityMode::Full, .. }) => base,
                Some(Liability { mode: LiabilityMode::Split, .. }) => base - base / 2,
                None => 0,
            },
        });
    }

    Settlement { delta, entries }
}

/// 荒牌平局のテンパイ料。合計は常に `rules.noten_penalty`。
pub fn settle_exhaustive(tenpai: [bool; 4], rules: &Ruleset) -> Settlement {
    let winners = tenpai.iter().filter(|t| **t).count() as i32;
    let losers = 4 - winners;
    let mut delta = [0i32; 4];

    if winners > 0 && losers > 0 {
        let pay = rules.noten_penalty / losers;
        let get = rules.noten_penalty / winners;
        for seat in Seat::ALL {
            delta[seat.index()] = if tenpai[seat.index()] { get } else { -pay };
        }
    }

    Settlement {
        delta,
        entries: Vec::new(),
    }
}

/// 流し満貫。テンパイ料より優先し、テンパイ料は発生しない。
pub fn settle_nagashi(winners: &[Seat], dealer: Seat) -> Settlement {
    let mut delta = [0i32; 4];
    for winner in winners {
        let dealer_pays = NAGASHI_BASE * 2;
        for seat in Seat::ALL {
            if seat == *winner {
                continue;
            }
            let amount = if *winner == dealer {
                // 親の流し満貫は子が等分。
                dealer_pays
            } else if seat == dealer {
                dealer_pays
            } else {
                NAGASHI_BASE
            };
            delta[seat.index()] -= amount;
            delta[winner.index()] += amount;
        }
    }
    Settlement {
        delta,
        entries: Vec::new(),
    }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine settlement`
Expected: 16テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 和了と流局の精算を実装"
```

---

### Task 2: 合法手の生成

**Files:**
- Modify: `crates/mahjong-engine/src/round.rs`

**Interfaces:**
- Produces:
  - `pub fn discard_options(state: &RoundState, seat: Seat) -> Vec<ActionOption>`
  - `pub fn reaction_options(state: &RoundState, seat: Seat, discarded: Tile, from: Seat) -> Vec<ActionOption>`
  - `pub fn chankan_options(state: &RoundState, seat: Seat, tile: Tile, kind: MeldKind) -> Vec<ActionOption>`

**`ActionOption::Discard::allowed` は重複を除いた牌の集合とする。**
UI は「押せる牌」を並べるので、同じ牌が2枚あっても選択肢は1つでよい。
赤5（`0p`）と通常の5（`5p`）は別の `Tile` 値なので、この重複除去でも区別は残る。

**ロンの可否は「最低1役」を確認する。** `mahjong_core::score::score()` をロン用の
`HandContext` で呼び、`Some` が返る席だけをロン候補にする。振聴の席は外す。

**振聴の判定は2種類を両方見る。**

```
自分の河に待ち牌がある            → is_furiten_by_discards
同巡内に見逃した                   → is_temporary_furiten(passed_this_turn)
リーチ後に見逃した                 → is_temporary_furiten(permanent_furiten)
```

**リーチ中は打牌が制限される。** ツモ切りのみ（暗槓の条件を満たす場合を除く）。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod option_tests {
    use super::*;
    use crate::state::{Discarded, RoundState};
    use crate::wall::Seed;
    use protocol::event::DiscardManner;
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::{Round, Seat, Wind};

    /// テスト用の局面を作る。`RoundState::new` は山から13枚ずつ配るが、
    /// この補助はその手牌を指定の形へ上書きする。山との整合は崩れるものの、
    /// ここで見たいのは「その手でどの選択肢が出るか」だけなので問題ない。
    /// 牌の枚数が合っているかは `invariant.rs` が別に検査する。
    fn state_with(seat: Seat, hand: &str) -> RoundState {
        let mut state = RoundState::new(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            Round { wind: Wind::East, number: 1 },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &Seed::from_hex(&"44".repeat(32)).unwrap(),
        );
        state.seat_mut(seat).hand = parse_hand(hand).unwrap();
        state
    }

    /// 役なしの完成形はロンできない。ドラだけでは和了できない。
    #[test]
    fn a_yakuless_hand_is_not_offered_ron() {
        // 123m 345m 456p 789s ＋ 西の単騎。門前ロンだが役が無い。
        let state = state_with(Seat::new(1), "123m345m456p789s3z");
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("3z").unwrap(),
            Seat::new(0),
        );
        assert!(
            !options.iter().any(|o| matches!(o, ActionOption::Ron)),
            "役無しにロンを提示した"
        );
    }

    /// 役があればロンを提示する。
    #[test]
    fn a_hand_with_a_yaku_is_offered_ron() {
        // 平和＋断幺九の形。6p でロン。
        let state = state_with(Seat::new(1), "234567m23478p22s");
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            Seat::new(0),
        );
        assert!(options.iter().any(|o| matches!(o, ActionOption::Ron)));
    }

    /// 自分の河に待ち牌があればロンできない。
    #[test]
    fn a_seat_furiten_by_its_own_river_is_not_offered_ron() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state.seat_mut(Seat::new(1)).river.push(Discarded {
            tile: parse_tile("9p").unwrap(),
            manner: DiscardManner::Tsumogiri,
            called_by: None,
            riichi_declaration: false,
        });
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            Seat::new(0),
        );
        assert!(
            !options.iter().any(|o| matches!(o, ActionOption::Ron)),
            "78p の待ちは 6p と 9p。9p を捨てていれば振聴"
        );
    }

    /// 同巡内に見逃していればロンできない。
    #[test]
    fn a_temporary_furiten_seat_is_not_offered_ron() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state
            .seat_mut(Seat::new(1))
            .passed_this_turn
            .push(parse_tile("6p").unwrap().kind());
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            Seat::new(0),
        );
        assert!(!options.iter().any(|o| matches!(o, ActionOption::Ron)));
    }

    /// リーチ後の見逃しは局の終わりまで続く。
    #[test]
    fn a_permanent_furiten_seat_is_not_offered_ron() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state
            .seat_mut(Seat::new(1))
            .permanent_furiten
            .push(parse_tile("6p").unwrap().kind());
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            Seat::new(0),
        );
        assert!(!options.iter().any(|o| matches!(o, ActionOption::Ron)));
    }

    /// チーは上家からのみ。
    #[test]
    fn chi_is_only_offered_to_the_seat_below() {
        let state = state_with(Seat::new(1), "234567m23478p22s");
        // 席0は席1の上家。
        let from_kamicha = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("5p").unwrap(),
            Seat::new(0),
        );
        assert!(from_kamicha.iter().any(|o| matches!(o, ActionOption::Chi { .. })));

        // 席2は上家ではない。
        let from_toimen = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("5p").unwrap(),
            Seat::new(2),
        );
        assert!(!from_toimen.iter().any(|o| matches!(o, ActionOption::Chi { .. })));
    }

    /// リーチ中はツモ切りしかできない。
    #[test]
    fn a_riichi_seat_can_only_discard_the_drawn_tile() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state.seat_mut(Seat::new(1)).riichi = Some(crate::state::RiichiState {
            step: protocol::event::RiichiStep::Accepted,
            declared_at_turn: 2,
            ippatsu: false,
            double: false,
        });
        // 14枚目として 1z を引いた状態にする。
        state.seat_mut(Seat::new(1)).hand.push(parse_tile("1z").unwrap());
        let options = discard_options(&state, Seat::new(1));
        let Some(ActionOption::Discard { allowed, .. }) = options
            .iter()
            .find(|o| matches!(o, ActionOption::Discard { .. }))
        else {
            panic!("打牌の選択肢がない");
        };
        assert_eq!(allowed.len(), 1, "リーチ中はツモ牌しか切れない");
        assert_eq!(allowed[0], parse_tile("1z").unwrap());
    }

    /// リーチしていなければ手のどれでも切れる。
    #[test]
    fn a_free_seat_may_discard_anything() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state.seat_mut(Seat::new(1)).hand.push(parse_tile("1z").unwrap());
        let options = discard_options(&state, Seat::new(1));
        let Some(ActionOption::Discard { allowed, .. }) = options
            .iter()
            .find(|o| matches!(o, ActionOption::Discard { .. }))
        else {
            panic!("打牌の選択肢がない");
        };
        // 2s が2枚あるので、重複を除くと13種。
        assert_eq!(allowed.len(), 13);
        assert!(allowed.contains(&parse_tile("2s").unwrap()));
        assert!(allowed.contains(&parse_tile("1z").unwrap()));
    }

    /// 槍槓の候補はロンだけ。
    #[test]
    fn chankan_options_contain_only_ron() {
        let state = state_with(Seat::new(1), "234567m23478p22s");
        let options = chankan_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            protocol::meld::MeldKind::Kakan,
        );
        assert!(options.iter().all(|o| matches!(o, ActionOption::Ron)));
    }

    /// 暗槓を槍槓できるのは国士無双だけ。
    #[test]
    fn an_ankan_can_only_be_robbed_by_kokushi() {
        // 通常の待ちでは暗槓を槍槓できない。
        let normal = state_with(Seat::new(1), "234567m23478p22s");
        assert!(chankan_options(
            &normal,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            protocol::meld::MeldKind::Ankan,
        )
        .is_empty());

        // 国士の待ちなら槍槓できる。
        let kokushi = state_with(Seat::new(1), "119m19p19s123456z");
        assert!(!chankan_options(
            &kokushi,
            Seat::new(1),
            parse_tile("7z").unwrap(),
            protocol::meld::MeldKind::Ankan,
        )
        .is_empty());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine option_tests`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

`mahjong_core` の `callable` / `furiten` / `wait` / `score` を組み合わせる。
新しい判定ロジックをここで書かない。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine option_tests`
Expected: 10テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 合法手の生成を実装"
```

---

## Wave 2b 完了の判定

- [ ] `cargo test --workspace` が通る
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] 精算16テストと合法手10テストがすべて通る
- [ ] `settle_*` が返す `Settlement` は**すべて** `is_balanced()` を満たす
- [ ] 役なしの完成形にロンを提示しない
- [ ] 振聴の3種（自河・同巡内・リーチ後）すべてでロンを提示しない
- [ ] `wall.rs` / `reaction.rs` / `state.rs` / `timing.rs` / `invariant.rs` を編集していない
- [ ] `match_flow.rs` / `lib.rs` / `protocol` / `mahjong-core` を編集していない
- [ ] `round.rs` に進行のステートマシンを書いていない

## Wave 2c へ渡すもの

| 部品 | Wave 2c での使われ方 |
|---|---|
| `settle_agari` | 和了イベントの `settlement` を組み立てる |
| `settle_exhaustive` | 荒牌平局のテンパイ料 |
| `settle_nagashi` | 流し満貫。テンパイ料より優先する |
| `score_change` | 持ち点へ反映する増減。供託を足し込み、供託残高を0にする |
| `discard_options` | 手番の席へ送る `RequestAction.options` |
| `reaction_options` | 打牌後に開く反応ウィンドウの候補 |
| `chankan_options` | 槓宣言後に開く槍槓ウィンドウの候補 |
