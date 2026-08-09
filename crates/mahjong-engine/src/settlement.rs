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
