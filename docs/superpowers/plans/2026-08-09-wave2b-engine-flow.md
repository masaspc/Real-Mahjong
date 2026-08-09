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
pub use settlement::{
    score_change, settle_agari, settle_exhaustive, settle_nagashi, AgariInput, HONBA_PER_STICK,
};
```

## コーディネータが確定させたルール

仕様に明記が無く、実装者が推測すると割れる箇所を先に決める。

| 項目 | 決定 |
|---|---|
| 本場料 | 1本場あたり300点。ロンは放銃者が全額、ツモは各家100点ずつ |
| ダブロンの本場料 | **放銃者から見て最も近い和了者**（下家方向）のみが受け取る |
| ダブロンの供託 | 同じく最も近い和了者が総取り |
| 責任払いの範囲 | **素点にのみ適用する。**本場は責任払いと無関係に、通常どおり負担する（ロンは放銃者が全額、ツモは各家100点ずつ） |
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

### 責任払いを素点に限る理由

「ツモは責任者が全額、ロンは折半」を本場込みの総額に適用すると、ロンで
100点単位に割り切れなくなる。素点32000・1本場なら総額32300、その半分は
16150 であり、点棒として存在しない。したがって責任払いは素点にだけ効かせ、
本場は責任払いと無関係に通常どおり負担させる。

| 局面 | 素点32000 | 本場300 | 結果 |
|---|---|---|---|
| ツモ・責任者=席2・和了=席1 | 席2が32000 | 席0/2/3が各100 | `[-100, +32300, -32100, -100]` |
| ロン・放銃=席0・責任者=席2・和了=席1 | 席0と席2が各16000 | 席0が300 | `[-16300, +32300, -16000, 0]` |

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

    /// 責任払いは素点にだけ効く。本場は通常どおり各家が100点ずつ負担する。
    /// 本場まで責任者に寄せると、ロン側で100点単位に割り切れなくなる。
    #[test]
    fn a_liability_does_not_absorb_the_honba_on_a_tsumo() {
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
        let s = settle_agari(&[input], Seat::new(0), 1, 0);
        assert_eq!(s.delta, [-100, 32_300, -32_100, -100]);
        assert!(s.is_balanced());
    }

    #[test]
    fn a_liability_does_not_absorb_the_honba_on_a_ron() {
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
        let s = settle_agari(&[input], Seat::new(0), 1, 0);
        assert_eq!(s.delta, [-16_300, 32_300, -16_000, 0]);
        assert!(s.is_balanced());
    }

    /// 入力の不変条件。ロンとツモは混ざらず、ダブロンの放銃者は1人である。
    #[test]
    #[should_panic(expected = "和了者がいない")]
    fn an_empty_winner_list_is_rejected() {
        settle_agari(&[], Seat::new(0), 0, 0);
    }

    #[test]
    #[should_panic(expected = "ツモは同時に起こらない")]
    fn two_tsumo_winners_are_rejected() {
        let tsumo = |seat: u8| AgariInput {
            seat: Seat::new(seat),
            from: None,
            payment: Payment::TsumoNonDealer {
                from_dealer: 1_300,
                from_each_non_dealer: 700,
            },
            liability: None,
        };
        settle_agari(&[tsumo(1), tsumo(2)], Seat::new(0), 0, 0);
    }

    #[test]
    #[should_panic(expected = "放銃者が一致しない")]
    fn winners_from_different_discarders_are_rejected() {
        settle_agari(&[ron(0, 3, 2_000), ron(2, 1, 8_000)], Seat::new(0), 0, 0);
    }

    /// 親の和了に子の支払い形式を渡すと、親から取るはずの分を誰も
    /// 払わないまま素点だけが増え、合計が0にならない。
    #[test]
    #[should_panic(expected = "親の和了に TsumoNonDealer を渡している")]
    fn a_dealer_win_with_a_child_payment_is_rejected() {
        let input = AgariInput {
            seat: Seat::new(0),
            from: None,
            payment: Payment::TsumoNonDealer {
                from_dealer: 1_300,
                from_each_non_dealer: 700,
            },
            liability: None,
        };
        settle_agari(&[input], Seat::new(0), 0, 0);
    }

    #[test]
    #[should_panic(expected = "子の和了に TsumoDealer を渡している")]
    fn a_child_win_with_a_dealer_payment_is_rejected() {
        let input = AgariInput {
            seat: Seat::new(1),
            from: None,
            payment: Payment::TsumoDealer { from_each: 4_000 },
            liability: None,
        };
        settle_agari(&[input], Seat::new(0), 0, 0);
    }

    /// protocol は Full をツモ、Split をロンと定義している。
    /// 食い違ったまま通すと、責任払いが黙って無視される。
    #[test]
    #[should_panic(expected = "ロンなのに責任払いが Full である")]
    fn a_ron_with_a_full_liability_is_rejected() {
        let input = AgariInput {
            seat: Seat::new(1),
            from: Some(Seat::new(0)),
            payment: Payment::Ron { total: 32_000 },
            liability: Some(Liability {
                seat: Seat::new(2),
                yaku: protocol::yaku::YakuId::Daisangen,
                mode: LiabilityMode::Full,
            }),
        };
        settle_agari(&[input], Seat::new(0), 0, 0);
    }

    #[test]
    #[should_panic(expected = "ツモなのに責任払いが Split である")]
    fn a_tsumo_with_a_split_liability_is_rejected() {
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
                mode: LiabilityMode::Split,
            }),
        };
        settle_agari(&[input], Seat::new(0), 0, 0);
    }

    #[test]
    #[should_panic(expected = "同じ席が2回和了している")]
    fn the_same_seat_winning_twice_is_rejected() {
        settle_agari(&[ron(1, 0, 2_000), ron(1, 0, 8_000)], Seat::new(0), 0, 0);
    }

    /// 自分の打牌で自分が和了することはない。点棒は釣り合ってしまうので、
    /// 上流の結線ミスを捕まえるにはここで弾く必要がある。
    #[test]
    #[should_panic(expected = "自分の打牌で和了している")]
    fn a_seat_winning_off_its_own_discard_is_rejected() {
        settle_agari(&[ron(1, 1, 3_900)], Seat::new(0), 0, 0);
    }

    /// 責任払いの相手が和了者本人になることもない。
    #[test]
    #[should_panic(expected = "和了者自身が責任を負っている")]
    fn a_winner_liable_for_its_own_hand_is_rejected() {
        let input = AgariInput {
            seat: Seat::new(1),
            from: None,
            payment: Payment::TsumoNonDealer {
                from_dealer: 16_000,
                from_each_non_dealer: 8_000,
            },
            liability: Some(Liability {
                seat: Seat::new(1),
                yaku: protocol::yaku::YakuId::Daisangen,
                mode: LiabilityMode::Full,
            }),
        };
        settle_agari(&[input], Seat::new(0), 0, 0);
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

/// 入力の不変条件。ここを通ったものだけが `Settlement` になる。
///
/// 黙って通すと非ゼロサムの精算が出てしまう。たとえば親のツモを
/// `TsumoNonDealer` で渡すと、親から取るはずの分を誰も払わない。
fn validate(winners: &[AgariInput], dealer: Seat) {
    assert!(!winners.is_empty(), "和了者がいない");

    let mut seen = [false; 4];
    for win in winners {
        assert!(!seen[win.seat.index()], "同じ席が2回和了している");
        seen[win.seat.index()] = true;

        assert_ne!(Some(win.seat), win.from, "自分の打牌で和了している");
        assert_ne!(
            win.liability.map(|l| l.seat),
            Some(win.seat),
            "和了者自身が責任を負っている"
        );

        // 支払い形式は、和了種別と親子に一致していなければならない。
        match win.payment {
            Payment::Ron { .. } => assert!(win.from.is_some(), "ロンなのに放銃者がいない"),
            Payment::TsumoDealer { .. } => {
                assert!(win.from.is_none(), "ツモなのに放銃者がいる");
                assert_eq!(win.seat, dealer, "子の和了に TsumoDealer を渡している");
            }
            Payment::TsumoNonDealer { .. } => {
                assert!(win.from.is_none(), "ツモなのに放銃者がいる");
                assert_ne!(win.seat, dealer, "親の和了に TsumoNonDealer を渡している");
            }
        }

        // 責任払いの形式も和了種別と一致していなければならない。
        // protocol は Full をツモ、Split をロンと定義している。
        match win.liability {
            Some(Liability {
                mode: LiabilityMode::Full,
                ..
            }) => assert!(win.from.is_none(), "ロンなのに責任払いが Full である"),
            Some(Liability {
                mode: LiabilityMode::Split,
                ..
            }) => assert!(win.from.is_some(), "ツモなのに責任払いが Split である"),
            None => {}
        }
    }

    // 同時和了は必ず「同じ牌に対する複数のロン」である。
    // ツモは他家の応答を待たないため、同時には起こりえない。
    if winners[0].from.is_some() {
        assert!(
            winners.iter().all(|w| w.from == winners[0].from),
            "放銃者が一致しない"
        );
    } else {
        assert_eq!(winners.len(), 1, "ツモは同時に起こらない");
    }
}

pub fn settle_agari(
    winners: &[AgariInput],
    dealer: Seat,
    honba: u8,
    riichi_sticks: u8,
) -> Settlement {
    validate(winners, dealer);

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
        // `let mut base = 0` にすると、全分岐で上書きされる初期値が
        // 読まれないため unused_assignments 警告になる。
        let base = match (win.payment, win.from) {
            (Payment::Ron { total }, Some(from)) => {
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
                total
            }
            (Payment::TsumoDealer { from_each }, None) => {
                let base = from_each * 3;
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
                base
            }
            (
                Payment::TsumoNonDealer {
                    from_dealer,
                    from_each_non_dealer,
                },
                None,
            ) => {
                let base = from_dealer + from_each_non_dealer * 2;
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
                base
            }
            // `validate` が弾いている。
            _ => unreachable!("和了種別と支払い形式が食い違っている"),
        };
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

    let settlement = Settlement { delta, entries };
    // 分岐を足したときの安全網。供託を delta から外しているので、
    // ここは常に成り立たなければならない。
    debug_assert!(
        settlement.is_balanced(),
        "点棒の移動が釣り合っていない: {:?}",
        settlement.delta
    );
    settlement
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
Expected: 27テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 和了と流局の精算を実装"
```

---

### Task 2: 合法手の生成

その席がいま取れる操作をすべて列挙する。判定そのものは `mahjong-core` に任せ、
ここは「どの判定をいつ呼ぶか」だけを決める。**新しい麻雀判定をここに書かない。**

**Files:**
- Modify: `crates/mahjong-engine/src/round.rs`

**Interfaces:**
- Consumes: `mahjong_core::{callable, furiten, wait, score, hand}`、`crate::state::{RoundState, SeatState, RIICHI_STICK}`
- Produces:
  - `pub enum TurnStart { Draw { tile: Tile, source: DrawSource }, AfterCall }`
  - `pub fn discard_options(state: &RoundState, seat: Seat, start: TurnStart) -> Vec<ActionOption>`
  - `pub fn reaction_options(state: &RoundState, seat: Seat, discarded: Tile, from: Seat) -> Vec<ActionOption>`
  - `pub fn chankan_options(state: &RoundState, seat: Seat, tile: Tile, kind: MeldKind) -> Vec<ActionOption>`

#### ツモ牌は引数で渡す

`RoundState.last_draw` は `Option<(Seat, DrawSource)>` であり、**牌そのものを持たない。**
`SeatState.hand` も `Vec<Tile>` で、並び順に契約が無い。したがって
「手牌の末尾が直前のツモ牌」と仮定してはならない。

引いた側（Wave 2c の進行）はその牌を知っているので、`TurnStart` で渡す。
これはリーチ中のツモ切り制限・ツモ和了・リーチ中の暗槓の3つすべてに必要である。

```rust
/// 打牌の直前に何が起きたか。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnStart {
    /// ツモ。`tile` は手牌へ加えたあとの、その牌そのもの。
    Draw { tile: Tile, source: DrawSource },
    /// 鳴きの直後。ツモ牌は無い。
    AfterCall,
}
```

`mahjong_core::score::score(hand, melds, win_tile, ..)` は**和了牌を除いた**手牌を
取る。ツモ和了の判定では `hand` から `tile` を1枚抜いて渡す。

#### 8つの `ActionOption` がどこから出るか

| `ActionOption` | 出どころ | 使う `mahjong-core` API |
|---|---|---|
| `Discard { allowed, riichi_allowed }` | `discard_options`（常に1つ） | `wait::waiting_tiles`（リーチ可否） |
| `Kan`（`Ankan` / `Kakan`） | `discard_options`（`Draw` のときのみ） | `callable::ankan_candidates` / `kakan_candidates` |
| `Tsumo` | `discard_options`（`Draw` のときのみ） | `score::score` |
| `Kyuushu` | `discard_options`（`Draw` のときのみ） | なし（下に定義する） |
| `Chi` | `reaction_options` | `callable::chi_candidates` |
| `Pon` | `reaction_options` | `callable::pon_candidates` |
| `Kan`（`Minkan`） | `reaction_options` | `callable::minkan_possible` |
| `Ron` | `reaction_options` / `chankan_options` | `wait::waiting_tiles` + `furiten::*` + `score::score` |
| `Pass` | **どこからも出さない** | — |

`Pass` を候補に入れないのは、Wave 2a の `ReactionWindow::respond` が
「候補を持つ席なら `Pass` を常に許す」と実装済みだからである。候補配列へ入れると
二重管理になる。

#### コーディネータが確定させたルール

| 項目 | 決定 |
|---|---|
| `allowed` の重複 | 同じ `Tile` 値は1つにまとめる。赤5（`0p`）と通常の5（`5p`）は別の値なので区別は残る |
| 食い替え | **現物と筋の両方を禁止する。** ポンは現物のみ。チーで順子の端を鳴いた場合は反対側の隣も禁止 |
| リーチ宣言の条件 | 門前・未リーチ・持ち点1000以上・山の残り4枚以上・その牌を切ってテンパイが保てる |
| 待ちを自分で4枚持つ形 | テンパイと見なさない。`waiting_tiles` は自分が4枚持つ牌を待ちから外すため、この形ではリーチも提示されない |
| リーチ中の打牌 | ツモ牌のみ |
| リーチ中の暗槓 | **いま引いた牌で4枚目が揃い、かつ暗槓の前後で待ちが変わらない**場合のみ（送り槓の禁止） |
| リーチ中の加槓 | 不可（手牌の構成が変わるため） |
| 5つ目の槓 | 不可。`kan_count` の合計が4に達したら槓の候補を出さない |
| 九種九牌 | その席の**最初のツモ**（`draw_count[seat] == 1`）、誰も鳴いていない、手牌の幺九牌が**9種類以上** |
| ツモ和了と振聴 | 振聴はツモ和了を妨げない。振聴の検査はロンにだけ効く |
| 鳴きとリーチ | リーチ成立後はチー・ポン・明槓ができない |
| 暗槓への槍槓 | 国士無双のみ。`score` の役に `KokushiMusou` / `KokushiMusou13` が含まれるかで判定する |

待ちを自分で4枚持つ形（たとえば `1111m 234p 567p 789s` の 1m 単騎）は、
`mahjong_core::wait::waiting_tiles` が `counts.get(kind) >= 4` を除外するため
空の待ちになる。この形をリーチさせない判断はここから自動的に従う。
**荒牌平局のテンパイ料でも同じ扱いになる点を Wave 2c は把握しておくこと。**

**振聴は3種すべてを見る。**

| 種類 | 状態 | 解除 |
|---|---|---|
| 自分の河による振聴 | `SeatState.river` の牌 | 解除されない |
| 同巡内振聴 | `SeatState.passed_this_turn` | 自分のツモで解除（`begin_turn`） |
| リーチ後の見逃し | `SeatState.permanent_furiten` | 局の終わりまで解除されない |

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod option_tests {
    use super::*;
    use crate::state::{Discarded, RiichiState, RoundState};
    use crate::wall::Seed;
    use protocol::event::{DiscardManner, DrawSource, RiichiStep};
    use protocol::meld::{Meld, MeldKind};
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
            Round {
                wind: Wind::East,
                number: 1,
            },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &Seed::from_hex(&"44".repeat(32)).unwrap(),
        );
        state.seat_mut(seat).hand = parse_hand(hand).unwrap();
        state.draw_count[seat.index()] = 1;
        state
    }

    /// 13枚の手へ1枚ツモった状態にして、その打牌局面を作る。
    fn after_drawing(state: &mut RoundState, seat: Seat, drawn: &str) -> TurnStart {
        let tile = parse_tile(drawn).unwrap();
        state.seat_mut(seat).hand.push(tile);
        state.last_draw = Some((seat, DrawSource::Wall));
        TurnStart::Draw {
            tile,
            source: DrawSource::Wall,
        }
    }

    fn accept_riichi(state: &mut RoundState, seat: Seat) {
        state.seat_mut(seat).riichi = Some(RiichiState {
            step: RiichiStep::Accepted,
            declared_at_turn: 2,
            ippatsu: false,
            double: false,
        });
    }

    fn discard_of(options: &[ActionOption]) -> (Vec<Tile>, Vec<Tile>) {
        for option in options {
            if let ActionOption::Discard {
                allowed,
                riichi_allowed,
            } = option
            {
                return (allowed.clone(), riichi_allowed.clone());
            }
        }
        panic!("打牌の選択肢がない: {options:?}");
    }

    fn kans_of(options: &[ActionOption]) -> Vec<KanCandidate> {
        for option in options {
            if let ActionOption::Kan { candidates } = option {
                return candidates.clone();
            }
        }
        Vec::new()
    }

    // ---------- 打牌 ----------

    /// リーチしていなければ手のどれでも切れる。同じ牌は1つにまとめる。
    #[test]
    fn a_free_seat_may_discard_any_distinct_tile() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let (allowed, _) = discard_of(&discard_options(&state, Seat::new(1), start));
        // 2s が2枚あるので、重複を除くと13種。
        assert_eq!(allowed.len(), 13);
        assert!(allowed.contains(&parse_tile("2s").unwrap()));
        assert!(allowed.contains(&parse_tile("1z").unwrap()));
    }

    /// 赤5と通常の5は別の牌なので、どちらも別々に切れる。
    #[test]
    fn a_red_five_is_a_separate_discard_choice() {
        let mut state = state_with(Seat::new(1), "234m50p678p234s11z");
        let start = after_drawing(&mut state, Seat::new(1), "9m");
        let (allowed, _) = discard_of(&discard_options(&state, Seat::new(1), start));
        assert!(allowed.contains(&parse_tile("5p").unwrap()));
        assert!(allowed.contains(&parse_tile("0p").unwrap()));
    }

    /// リーチ中はツモ切りしかできない。
    #[test]
    fn a_riichi_seat_can_only_discard_the_drawn_tile() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        accept_riichi(&mut state, Seat::new(1));
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let (allowed, riichi_allowed) = discard_of(&discard_options(&state, Seat::new(1), start));
        assert_eq!(allowed, vec![parse_tile("1z").unwrap()]);
        assert!(riichi_allowed.is_empty(), "既にリーチしている");
    }

    // ---------- リーチ宣言 ----------

    /// テンパイが保てる牌だけがリーチ宣言牌になる。
    #[test]
    fn riichi_is_offered_only_for_discards_that_keep_tenpai() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let (_, riichi_allowed) = discard_of(&discard_options(&state, Seat::new(1), start));
        // 1z を切れば元のテンパイに戻る。他の牌を切ると浮き牌が2枚残る。
        assert_eq!(riichi_allowed, vec![parse_tile("1z").unwrap()]);
    }

    /// 鳴いている席はリーチできない。
    #[test]
    fn an_open_hand_cannot_declare_riichi() {
        let mut state = state_with(Seat::new(1), "234567m78p22s");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("444p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("4p").unwrap()),
        });
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let (_, riichi_allowed) = discard_of(&discard_options(&state, Seat::new(1), start));
        assert!(riichi_allowed.is_empty());
    }

    /// 持ち点が1000点未満なら供託を出せないのでリーチできない。
    #[test]
    fn a_seat_below_one_thousand_points_cannot_declare_riichi() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state.scores[1] = 900;
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let (_, riichi_allowed) = discard_of(&discard_options(&state, Seat::new(1), start));
        assert!(riichi_allowed.is_empty());
    }

    /// 山が尽きかけているとリーチできない。
    #[test]
    fn riichi_needs_tiles_left_in_the_wall() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        while state.wall.live_remaining() >= 4 {
            state.wall.draw().expect("まだ引ける");
        }
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let (_, riichi_allowed) = discard_of(&discard_options(&state, Seat::new(1), start));
        assert!(riichi_allowed.is_empty());
    }

    // ---------- 食い替え ----------

    /// チーで順子の下端を鳴いたら、その牌と上端の隣が打てない。
    ///
    /// 手牌が11枚なのは正しい。配牌13枚から2枚を出してチーすると
    /// 手は11枚になり、副露3枚と合わせて14枚相当になる。ここから1枚
    /// 切って10枚＋副露3枚＝13枚に戻る。
    #[test]
    fn chi_forbids_the_called_tile_and_its_suji() {
        // 4p を上家からチーして 456p。4p（現物）と 7p（筋）が打てない。
        let mut state = state_with(Seat::new(1), "4p7p123m456m789m");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Chi,
            tiles: parse_hand("456p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("4p").unwrap()),
        });
        let (allowed, _) =
            discard_of(&discard_options(&state, Seat::new(1), TurnStart::AfterCall));
        assert!(!allowed.contains(&parse_tile("4p").unwrap()), "現物の食い替え");
        assert!(!allowed.contains(&parse_tile("7p").unwrap()), "筋の食い替え");
        assert_eq!(allowed.len(), 9, "残る9種は打てる");
    }

    /// 嵌張でチーした場合、筋の制限はない。
    #[test]
    fn a_closed_wait_chi_forbids_only_the_called_tile() {
        // 5p を鳴いて 456p。5p だけが打てない。
        let mut state = state_with(Seat::new(1), "5p8p123m456m789m");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Chi,
            tiles: parse_hand("456p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("5p").unwrap()),
        });
        let (allowed, _) =
            discard_of(&discard_options(&state, Seat::new(1), TurnStart::AfterCall));
        assert!(!allowed.contains(&parse_tile("5p").unwrap()));
        assert!(allowed.contains(&parse_tile("8p").unwrap()), "筋の制限は無い");
    }

    /// ポンは現物だけが打てない。
    #[test]
    fn pon_forbids_only_the_called_tile() {
        let mut state = state_with(Seat::new(1), "5p8p123m456m789m");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("555p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("5p").unwrap()),
        });
        let (allowed, _) =
            discard_of(&discard_options(&state, Seat::new(1), TurnStart::AfterCall));
        assert!(!allowed.contains(&parse_tile("5p").unwrap()));
        assert!(allowed.contains(&parse_tile("8p").unwrap()));
    }

    /// 鳴いた直後はツモ和了も九種九牌も槓もできない。
    #[test]
    fn a_call_offers_nothing_but_a_discard() {
        let mut state = state_with(Seat::new(1), "5p8p123m456m789m");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("555p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("5p").unwrap()),
        });
        let options = discard_options(&state, Seat::new(1), TurnStart::AfterCall);
        assert_eq!(options.len(), 1);
        assert!(matches!(options[0], ActionOption::Discard { .. }));
    }

    // ---------- ツモ和了 ----------

    /// 和了形になっていればツモを提示する。
    #[test]
    fn a_completing_draw_is_offered_tsumo() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        let start = after_drawing(&mut state, Seat::new(1), "6p");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(options.iter().any(|o| matches!(o, ActionOption::Tsumo)));
    }

    /// 和了形でなければツモは出ない。
    #[test]
    fn an_unrelated_draw_is_not_offered_tsumo() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(!options.iter().any(|o| matches!(o, ActionOption::Tsumo)));
    }

    /// 振聴でもツモ和了はできる。振聴が縛るのはロンだけである。
    #[test]
    fn a_furiten_seat_may_still_tsumo() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state.seat_mut(Seat::new(1)).river.push(Discarded {
            tile: parse_tile("9p").unwrap(),
            manner: DiscardManner::Tsumogiri,
            called_by: None,
            riichi_declaration: false,
        });
        let start = after_drawing(&mut state, Seat::new(1), "6p");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(options.iter().any(|o| matches!(o, ActionOption::Tsumo)));
    }

    // ---------- 九種九牌 ----------

    /// 境界をまたぐ2件は同じ手牌を使い、ツモ牌だけを変える。
    /// 13枚の時点の幺九牌は 1m 9m 1p 9p 1s 9s 1z 2z の8種類である。
    const EIGHT_KINDS: &str = "19m34555m19p19s12z";

    /// ツモで9種類目が入れば九種九牌を提示する。ちょうど9種類の境界。
    #[test]
    fn exactly_nine_kinds_offer_an_abort() {
        let mut state = state_with(Seat::new(1), EIGHT_KINDS);
        // 3z が9種類目になる。
        let start = after_drawing(&mut state, Seat::new(1), "3z");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(options.iter().any(|o| matches!(o, ActionOption::Kyuushu)));
    }

    /// 8種類のままでは出ない。
    #[test]
    fn eight_kinds_do_not_offer_an_abort() {
        let mut state = state_with(Seat::new(1), EIGHT_KINDS);
        // 6m は幺九牌ではないので種類数は8のままである。
        let start = after_drawing(&mut state, Seat::new(1), "6m");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(!options.iter().any(|o| matches!(o, ActionOption::Kyuushu)));
    }

    /// 2巡目には出ない。
    #[test]
    fn an_abort_is_only_offered_on_the_first_draw() {
        let mut state = state_with(Seat::new(1), EIGHT_KINDS);
        state.draw_count[1] = 2;
        let start = after_drawing(&mut state, Seat::new(1), "3z");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(!options.iter().any(|o| matches!(o, ActionOption::Kyuushu)));
    }

    /// 誰かが鳴いていれば出ない。
    #[test]
    fn a_call_cancels_the_abort() {
        let mut state = state_with(Seat::new(1), EIGHT_KINDS);
        state.any_call_made = true;
        let start = after_drawing(&mut state, Seat::new(1), "3z");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(!options.iter().any(|o| matches!(o, ActionOption::Kyuushu)));
    }

    // ---------- 槓 ----------

    /// 手に4枚あれば暗槓できる。
    #[test]
    fn four_in_hand_offer_an_ankan() {
        let mut state = state_with(Seat::new(1), "1111m234p567p22s9s");
        let start = after_drawing(&mut state, Seat::new(1), "3s");
        let kans = kans_of(&discard_options(&state, Seat::new(1), start));
        assert_eq!(
            kans,
            vec![KanCandidate::Ankan {
                kind: parse_tile("1m").unwrap().kind()
            }]
        );
    }

    /// ポンした牌の4枚目を引けば加槓できる。
    #[test]
    fn the_fourth_tile_of_a_pon_offers_a_kakan() {
        let mut state = state_with(Seat::new(1), "234567m78p22s");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("444p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("4p").unwrap()),
        });
        let start = after_drawing(&mut state, Seat::new(1), "4p");
        let kans = kans_of(&discard_options(&state, Seat::new(1), start));
        assert_eq!(
            kans,
            vec![KanCandidate::Kakan {
                tile: parse_tile("4p").unwrap()
            }]
        );
    }

    /// 4つの槓が済んでいれば、5つ目は提示しない。
    #[test]
    fn a_fifth_kan_is_never_offered() {
        let mut state = state_with(Seat::new(1), "1111m234p567p22s9s");
        state.kan_count = [2, 1, 1, 0];
        let start = after_drawing(&mut state, Seat::new(1), "3s");
        assert!(kans_of(&discard_options(&state, Seat::new(1), start)).is_empty());
    }

    /// リーチ中は、いま引いた牌で揃った暗槓しかできない（送り槓の禁止）。
    #[test]
    fn a_riichi_seat_cannot_kan_a_set_it_was_already_holding() {
        // 1111m 123m(23m+4枚目) 234p 567p ＋ 9s の単騎。リーチ時から 1m を4枚
        // 持っており、9s 待ちのテンパイである。
        let mut state = state_with(Seat::new(1), "1111m23m234p567p9s");
        accept_riichi(&mut state, Seat::new(1));
        // テストデータの意味を固定する。リーチできる形であることを先に示す。
        assert_eq!(
            waits_of(&state.seat(Seat::new(1)).hand, 0),
            vec![parse_tile("9s").unwrap().kind()],
            "9s の単騎テンパイでなければ、この局面はリーチできない"
        );
        // 引いたのは 1m ではないので、いま暗槓すれば送り槓になる。
        let start = after_drawing(&mut state, Seat::new(1), "5z");
        assert!(kans_of(&discard_options(&state, Seat::new(1), start)).is_empty());
    }

    /// リーチ中でも、待ちが変わらない暗槓は認める。
    #[test]
    fn a_riichi_seat_may_kan_when_the_wait_does_not_move() {
        // 111m 234p 567p 22s ＋ 78s で 6s/9s の両面待ち。1m を暗槓すると
        // 手は 234p 567p 22s 78s ＋ 槓1つになるが、待ちは 6s/9s のままである。
        let mut state = state_with(Seat::new(1), "111m234p567p22s78s");
        accept_riichi(&mut state, Seat::new(1));
        let start = after_drawing(&mut state, Seat::new(1), "1m");
        let kans = kans_of(&discard_options(&state, Seat::new(1), start));
        assert_eq!(
            kans,
            vec![KanCandidate::Ankan {
                kind: parse_tile("1m").unwrap().kind()
            }]
        );
    }

    /// リーチ中は加槓できない。手牌の構成が変わるためである。
    /// 鳴いた席はそもそもリーチできないので、この局面は進行上は起こらない。
    /// ガードが効いていることだけを確かめる。
    #[test]
    fn a_riichi_seat_cannot_kakan() {
        let mut state = state_with(Seat::new(1), "234567m78p22s");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("444p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("4p").unwrap()),
        });
        accept_riichi(&mut state, Seat::new(1));
        let start = after_drawing(&mut state, Seat::new(1), "4p");
        assert!(kans_of(&discard_options(&state, Seat::new(1), start)).is_empty());
    }

    // ---------- 反応 ----------

    /// 自分の打牌には反応できない。
    #[test]
    fn a_seat_cannot_react_to_its_own_discard() {
        let state = state_with(Seat::new(1), "234567m23478p22s");
        assert!(reaction_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            Seat::new(1)
        )
        .is_empty());
    }

    /// 同じ牌が2枚あればポンできる。
    #[test]
    fn two_matching_tiles_offer_a_pon() {
        let state = state_with(Seat::new(1), "234567m23478p22s");
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("2s").unwrap(),
            Seat::new(2),
        );
        let Some(ActionOption::Pon { candidates }) = options
            .iter()
            .find(|o| matches!(o, ActionOption::Pon { .. }))
        else {
            panic!("ポンが出ていない: {options:?}");
        };
        assert_eq!(candidates.len(), 1);
    }

    /// 同じ牌が3枚あれば明槓もできる。
    #[test]
    fn three_matching_tiles_offer_a_minkan() {
        let state = state_with(Seat::new(1), "222s234567m2347p");
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("2s").unwrap(),
            Seat::new(2),
        );
        assert_eq!(
            kans_of(&options),
            vec![KanCandidate::Minkan],
            "明槓が出ていない"
        );
        assert!(options.iter().any(|o| matches!(o, ActionOption::Pon { .. })));
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
        assert!(from_kamicha
            .iter()
            .any(|o| matches!(o, ActionOption::Chi { .. })));

        // 席2は上家ではない。
        let from_toimen = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("5p").unwrap(),
            Seat::new(2),
        );
        assert!(!from_toimen
            .iter()
            .any(|o| matches!(o, ActionOption::Chi { .. })));
    }

    /// リーチ中は鳴けない。ロンだけが残る。
    #[test]
    fn a_riichi_seat_can_only_ron() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        accept_riichi(&mut state, Seat::new(1));
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            Seat::new(0),
        );
        assert_eq!(options, vec![ActionOption::Ron]);
    }

    // ---------- ロン ----------

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

    // ---------- 槍槓 ----------

    fn pending(state: &mut RoundState, kind: MeldKind, tile: &str) {
        state.pending_kan = Some(crate::state::PendingKan {
            seat: Seat::new(0),
            kind,
            tile: parse_tile(tile).unwrap(),
        });
    }

    /// 加槓は誰でも槍槓できる。候補はロンだけ。
    #[test]
    fn a_kakan_can_be_robbed_by_any_wait() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        pending(&mut state, MeldKind::Kakan, "6p");
        let options = chankan_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            MeldKind::Kakan,
        );
        assert_eq!(options, vec![ActionOption::Ron]);
    }

    /// 槓を宣言した本人は槍槓できない。
    #[test]
    fn the_declarer_cannot_rob_its_own_kan() {
        let mut state = state_with(Seat::new(0), "234567m23478p22s");
        pending(&mut state, MeldKind::Kakan, "6p");
        assert!(chankan_options(
            &state,
            Seat::new(0),
            parse_tile("6p").unwrap(),
            MeldKind::Kakan,
        )
        .is_empty());
    }

    /// 暗槓を槍槓できるのは国士無双だけ。
    #[test]
    fn an_ankan_can_only_be_robbed_by_kokushi() {
        // 通常の待ちでは暗槓を槍槓できない。
        let mut normal = state_with(Seat::new(1), "234567m23478p22s");
        pending(&mut normal, MeldKind::Ankan, "6p");
        assert!(chankan_options(
            &normal,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            MeldKind::Ankan,
        )
        .is_empty());

        // 国士の待ちなら槍槓できる。
        let mut kokushi = state_with(Seat::new(1), "119m19p19s123456z");
        pending(&mut kokushi, MeldKind::Ankan, "7z");
        assert_eq!(
            chankan_options(
                &kokushi,
                Seat::new(1),
                parse_tile("7z").unwrap(),
                MeldKind::Ankan,
            ),
            vec![ActionOption::Ron]
        );
    }

    /// 振聴なら槍槓もできない。
    #[test]
    fn a_furiten_seat_cannot_rob_a_kan() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state.seat_mut(Seat::new(1)).river.push(Discarded {
            tile: parse_tile("6p").unwrap(),
            manner: DiscardManner::Tsumogiri,
            called_by: None,
            riichi_declaration: false,
        });
        pending(&mut state, MeldKind::Kakan, "6p");
        assert!(chankan_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            MeldKind::Kakan,
        )
        .is_empty());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine option_tests`
Expected: コンパイルエラー（`TurnStart` などが未定義）

- [ ] **Step 3: 実装を書く**

`round.rs` の先頭（`mod settlement;` の直後）へ置く。

```rust
use crate::state::{RoundState, SeatState, RIICHI_STICK};
use mahjong_core::callable::{
    ankan_candidates, chi_candidates, kakan_candidates, minkan_possible, pon_candidates,
};
use mahjong_core::furiten::{is_furiten_by_discards, is_temporary_furiten};
use mahjong_core::hand::HandCounts;
use mahjong_core::score::{score, WinType};
use mahjong_core::wait::waiting_tiles;
use protocol::command::{ActionOption, KanCandidate};
use protocol::event::{DrawSource, RiichiStep};
use protocol::meld::{Meld, MeldKind};
use protocol::seat::Seat;
use protocol::tile::{Tile, TileKind};
use protocol::yaku::YakuId;

/// 打牌の直前に何が起きたか。
///
/// `RoundState.last_draw` は席と出どころしか持たず、牌そのものを持たない。
/// 手牌の並び順にも契約が無いため「末尾がツモ牌」と決めつけられない。
/// 引いた側が知っているので、ここで受け取る。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnStart {
    /// ツモ。`tile` は手牌へ加えたあとの、その牌そのもの。
    Draw { tile: Tile, source: DrawSource },
    /// 鳴きの直後。ツモ牌は無い。
    AfterCall,
}

/// 同じ牌をまとめる。赤5と通常の5は別の `Tile` 値なので区別は残る。
fn distinct(tiles: &[Tile]) -> Vec<Tile> {
    let mut out: Vec<Tile> = Vec::with_capacity(tiles.len());
    for tile in tiles {
        if !out.contains(tile) {
            out.push(*tile);
        }
    }
    out
}

/// 手牌から1枚だけ取り除く。`score` は和了牌を除いた手牌を取るため必要。
fn hand_without(hand: &[Tile], tile: Tile) -> Vec<Tile> {
    let mut out = hand.to_vec();
    if let Some(position) = out.iter().position(|t| *t == tile) {
        out.remove(position);
    }
    out
}

fn is_riichi_accepted(seat: &SeatState) -> bool {
    matches!(&seat.riichi, Some(r) if r.step == RiichiStep::Accepted)
}

/// 待ち牌。`waiting_tiles` は牌種の昇順で返すが、待ちどうしを比較する箇所が
/// あるので、順序の前提をこの関数の中に閉じておく。
fn waits_of(hand: &[Tile], melds: usize) -> Vec<TileKind> {
    let mut waits = waiting_tiles(&HandCounts::from_tiles(hand), melds as u8);
    waits.sort_by_key(|k| k.index());
    waits
}

/// 4つの槓が済んでいれば嶺上牌が尽きるので、それ以上は槓できない。
fn kans_left(state: &RoundState) -> bool {
    state.kan_count.iter().map(|c| u32::from(*c)).sum::<u32>() < 4
}

/// 同じ色の、指定した数の牌。
fn same_suit(reference: TileKind, number: u8) -> Option<TileKind> {
    let current = reference.number()?;
    if !(1..=9).contains(&number) {
        return None;
    }
    let base = reference.index() - (current - 1);
    TileKind::from_index(base + number - 1)
}

// ---------- 打牌 ----------

pub fn discard_options(state: &RoundState, seat: Seat, start: TurnStart) -> Vec<ActionOption> {
    let hand = &state.seat(seat).hand;
    let melds = &state.seat(seat).melds;
    let mut out = Vec::new();

    let allowed = allowed_discards(state, seat, start);
    let riichi_allowed = riichi_discards(state, seat, &allowed);
    out.push(ActionOption::Discard {
        allowed,
        riichi_allowed,
    });

    // ツモした番でしかできないもの。鳴いた直後は打牌のみである。
    if let TurnStart::Draw { tile, .. } = start {
        let candidates = turn_kan_candidates(state, seat, tile);
        if !candidates.is_empty() {
            out.push(ActionOption::Kan { candidates });
        }

        // 振聴はツモ和了を妨げない。
        let rest = hand_without(hand, tile);
        let context = state.hand_context(seat, WinType::Tsumo);
        if score(&rest, melds, tile, &context, &state.rules).is_some() {
            out.push(ActionOption::Tsumo);
        }

        if kyuushu_allowed(state, seat) {
            out.push(ActionOption::Kyuushu);
        }
    }

    out
}

fn allowed_discards(state: &RoundState, seat: Seat, start: TurnStart) -> Vec<Tile> {
    let s = state.seat(seat);

    // リーチ成立後はツモ切りのみ。リーチ後は鳴けないので AfterCall は来ない。
    if is_riichi_accepted(s) {
        return match start {
            TurnStart::Draw { tile, .. } => vec![tile],
            TurnStart::AfterCall => Vec::new(),
        };
    }

    let mut allowed = distinct(&s.hand);
    if start == TurnStart::AfterCall {
        for forbidden in kuikae_forbidden(s.melds.last()) {
            allowed.retain(|t| t.kind() != forbidden);
        }
    }
    allowed
}

/// 食い替えで打てなくなる牌の種類。
///
/// 鳴いた牌そのもの（現物）は常に禁止する。チーで順子の端を鳴いた場合は、
/// 反対側の隣（筋）も禁止する。嵌張で鳴いた場合に筋の制限は無い。
fn kuikae_forbidden(last: Option<&Meld>) -> Vec<TileKind> {
    let Some(meld) = last else {
        return Vec::new();
    };
    let Some(called) = meld.called_tile else {
        return Vec::new();
    };
    let mut out = vec![called.kind()];
    if meld.kind != MeldKind::Chi {
        return out;
    }

    let mut numbers: Vec<u8> = meld
        .tiles
        .iter()
        .filter_map(|t| t.kind().number())
        .collect();
    numbers.sort_unstable();
    let Some(called_number) = called.kind().number() else {
        return out;
    };
    if numbers.len() != 3 {
        return out;
    }

    let suji = if called_number == numbers[0] {
        numbers[2].checked_add(1)
    } else if called_number == numbers[2] {
        numbers[0].checked_sub(1)
    } else {
        None
    };
    if let Some(kind) = suji.and_then(|n| same_suit(called.kind(), n)) {
        out.push(kind);
    }
    out
}

/// リーチ宣言牌として選べる牌。
fn riichi_discards(state: &RoundState, seat: Seat, allowed: &[Tile]) -> Vec<Tile> {
    let s = state.seat(seat);
    if !state.is_menzen(seat) || s.riichi.is_some() {
        return Vec::new();
    }
    // 供託を出せない持ち点ではリーチできない。
    if state.scores[seat.index()] < RIICHI_STICK {
        return Vec::new();
    }
    // 宣言したあと一巡もせずに流局するなら、リーチする意味がない。
    if state.wall.live_remaining() < 4 {
        return Vec::new();
    }

    allowed
        .iter()
        .copied()
        .filter(|tile| {
            let rest = hand_without(&s.hand, *tile);
            !waits_of(&rest, s.melds.len()).is_empty()
        })
        .collect()
}

/// 自分の番で宣言できる槓。
fn turn_kan_candidates(state: &RoundState, seat: Seat, drawn: Tile) -> Vec<KanCandidate> {
    if !kans_left(state) {
        return Vec::new();
    }
    let s = state.seat(seat);
    let riichi = is_riichi_accepted(s);
    let mut out = Vec::new();

    for kind in ankan_candidates(&s.hand) {
        if riichi && !riichi_ankan_allowed(s, kind, drawn) {
            continue;
        }
        out.push(KanCandidate::Ankan { kind });
    }

    // 加槓は手牌の構成を変えるので、リーチ中はできない。
    if !riichi {
        for tile in kakan_candidates(&s.hand, &s.melds) {
            out.push(KanCandidate::Kakan { tile });
        }
    }
    out
}

/// リーチ中の暗槓の条件。
///
/// 1. いま引いた牌で4枚目が揃ったこと。手に元からあった4枚を槓する
///    「送り槓」は、待ちを動かさなくても認めない
/// 2. 暗槓の前後で待ちが変わらないこと
fn riichi_ankan_allowed(s: &SeatState, kind: TileKind, drawn: Tile) -> bool {
    if drawn.kind() != kind {
        return false;
    }
    let before = waits_of(&hand_without(&s.hand, drawn), s.melds.len());
    let after = {
        let mut counts = HandCounts::from_tiles(&s.hand);
        for _ in 0..4 {
            counts.remove(kind);
        }
        let mut waits = waiting_tiles(&counts, s.melds.len() as u8 + 1);
        waits.sort_by_key(|k| k.index());
        waits
    };
    !before.is_empty() && before == after
}

/// 九種九牌。自分の最初のツモで、誰も鳴いておらず、幺九牌が9種類以上あること。
fn kyuushu_allowed(state: &RoundState, seat: Seat) -> bool {
    if state.draw_count[seat.index()] != 1 || state.any_call_made {
        return false;
    }
    let mut kinds: Vec<TileKind> = state
        .seat(seat)
        .hand
        .iter()
        .map(|t| t.kind())
        .filter(|k| k.is_terminal_or_honor())
        .collect();
    kinds.sort_by_key(|k| k.index());
    kinds.dedup();
    kinds.len() >= 9
}

// ---------- 反応 ----------

pub fn reaction_options(
    state: &RoundState,
    seat: Seat,
    discarded: Tile,
    from: Seat,
) -> Vec<ActionOption> {
    if seat == from {
        return Vec::new();
    }
    let s = state.seat(seat);
    let mut out = Vec::new();

    // リーチ成立後は鳴けない。
    if !is_riichi_accepted(s) {
        // チーは上家からのみ。
        if (from.index() + 1) % 4 == seat.index() {
            let candidates = chi_candidates(&s.hand, discarded);
            if !candidates.is_empty() {
                out.push(ActionOption::Chi { candidates });
            }
        }

        let candidates = pon_candidates(&s.hand, discarded);
        if !candidates.is_empty() {
            out.push(ActionOption::Pon { candidates });
        }

        if kans_left(state) && minkan_possible(&s.hand, discarded) {
            out.push(ActionOption::Kan {
                candidates: vec![KanCandidate::Minkan],
            });
        }
    }

    if ron_allowed(state, seat, discarded) {
        out.push(ActionOption::Ron);
    }
    out
}

/// ロンできるか。待ちに入っており、振聴でなく、役が1つ以上あること。
fn ron_allowed(state: &RoundState, seat: Seat, tile: Tile) -> bool {
    let s = state.seat(seat);
    let waits = waits_of(&s.hand, s.melds.len());
    if !waits.contains(&tile.kind()) {
        return false;
    }

    let river: Vec<Tile> = s.river.iter().map(|d| d.tile).collect();
    if is_furiten_by_discards(&waits, &river)
        || is_temporary_furiten(&waits, &s.passed_this_turn)
        || is_temporary_furiten(&waits, &s.permanent_furiten)
    {
        return false;
    }

    // 役が1つも無ければ和了できない。ドラは役ではない。
    let context = state.hand_context(seat, WinType::Ron);
    score(&s.hand, &s.melds, tile, &context, &state.rules).is_some()
}

// ---------- 槍槓 ----------

/// 槍槓の候補。ロン以外は出さない。
///
/// **呼ぶ前に `RoundState.pending_kan` を立てておくこと。**
/// `hand_context` はこれを見て `chankan` を立て、槍槓の1翻を付ける。
pub fn chankan_options(
    state: &RoundState,
    seat: Seat,
    tile: Tile,
    kind: MeldKind,
) -> Vec<ActionOption> {
    if state.pending_kan.map(|k| k.seat) == Some(seat) {
        return Vec::new();
    }
    if !ron_allowed(state, seat, tile) {
        return Vec::new();
    }
    // 暗槓を槍槓できるのは国士無双だけ。
    if kind == MeldKind::Ankan && !wins_with_kokushi(state, seat, tile) {
        return Vec::new();
    }
    vec![ActionOption::Ron]
}

fn wins_with_kokushi(state: &RoundState, seat: Seat, tile: Tile) -> bool {
    let s = state.seat(seat);
    let context = state.hand_context(seat, WinType::Ron);
    let Some(result) = score(&s.hand, &s.melds, tile, &context, &state.rules) else {
        return false;
    };
    result
        .yaku
        .iter()
        .any(|(id, _)| matches!(id, YakuId::KokushiMusou | YakuId::KokushiMusou13))
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine option_tests`
Expected: 38テスト PASS

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
- [ ] 精算27テストと合法手38テストがすべて通る
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
| `TurnStart` | 進行が「ツモしたのか鳴いた直後なのか」と、ツモ牌そのものを渡す |
| `discard_options` | 手番の席へ送る `RequestAction.options`。打牌・槓・ツモ・九種九牌を含む |
| `reaction_options` | 打牌後に開く反応ウィンドウの候補 |
| `chankan_options` | 槓宣言後に開く槍槓ウィンドウの候補。**呼ぶ前に `pending_kan` を立てる** |

`ActionOption::Pass` はどの関数も出さない。Wave 2a の `ReactionWindow::respond` が
候補を持つ席へ常に許しているためである。

**Wave 2c は、受け取ったコマンドの牌が候補に含まれることを自分で検査する。**
`ReactionWindow::respond` が見るのは優先度だけで（`reaction.rs` の
`respond`）、`Command::Chi { tiles }` の牌が `ActionOption::Chi { candidates }`
のどれかと一致するかまでは照合しない。ここを抜くと、持っていない牌で
鳴けてしまう。
