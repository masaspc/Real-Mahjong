//! 局の進行。時刻も乱数も外から受け取る。
//!
//! 同じシード・同じコマンド列・同じ時刻列からは、必ず同じイベント列が出る。

use crate::invariant;
use crate::reaction::{Outcome, ReactionWindow, Rejection, WindowKind};
use crate::round::{chankan_options, discard_options, reaction_options, TurnStart};
use crate::state::{charge_bank, deadline_for, lead_in_of, remaining_for_event, RoundState};
use crate::wall::Seed;
use protocol::command::{ActionOption, CallResponse, Command, KanCandidate};
use protocol::event::{
    DiscardManner, DrawSource, Event, Liability, LiabilityMode, NextRound, PlayerId, RiichiStep,
};
use protocol::meld::{Meld, MeldKind};
use protocol::ruleset::{MatchLength, Ruleset};
use protocol::seat::{Round, Seat, Wind};
use protocol::tile::{Tile, TileKind};
use protocol::yaku::YakuId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reject {
    NotYourTurn,
    NotOffered,
    NoWindow,
    StaleWindow,
    Window(Rejection),
}
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
        let Some(Event::Ryuukyoku {
            kind, initiator, ..
        }) = events
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
        set_dealer_hand(engine.state_mut(), "19m19p19s12345677z");
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
        set_dealer_hand(engine.state_mut(), "19m19p19s12345677z");
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
        set_dealer_hand(engine.state_mut(), "19m345556m19p19s12z");
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
        set_dealer_hand(engine.state_mut(), "234567m23478p22s1z");
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
            engine.state().scores.iter().sum::<i32>() + engine.state().riichi_sticks as i32 * 1_000,
            100_000
        );
    }

    /// 3人のリーチでは流局しない。
    #[test]
    fn three_riichi_do_not_abort() {
        let mut engine = start_at(0);
        engine.drain_events();
        set_dealer_hand(engine.state_mut(), "234567m23478p22s1z");
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
        set_dealer_hand(engine.state_mut(), "19m19p19s12345677z");
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
        set_dealer_hand(engine.state_mut(), "19m19p19s12345677z");
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
        set_dealer_hand(engine.state_mut(), "19m19p19s12345677z");
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
        set_dealer_hand(engine.state_mut(), "19m19p19s12345677z");
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
        engine.state_mut().seat_mut(seat).hand = parse_hand("23m11m").expect("正しい記法");
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
        engine.state_mut().seat_mut(seat).hand = parse_hand("777z23m11m").expect("正しい記法");
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
        engine.state_mut().seat_mut(seat).hand = parse_hand("23m11m").expect("正しい記法");
        engine.state_mut().wall.draw().expect("山に残っている");
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
        let liability = agari_of(&events)[0].liability.expect("責任払いが成立する");
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
        engine.state_mut().seat_mut(seat).hand = parse_hand("23m567m11m").expect("正しい記法");
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
        engine.state_mut().seat_mut(seat).hand = parse_hand("5z").expect("正しい記法");
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
        let liability = agari_of(&events)[0].liability.expect("責任払いが成立する");
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
        assert_eq!(engine.state().scores.iter().sum::<i32>(), 100_000);
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Outstanding {
    window_id: u32,
    issued_at_ms: u64,
    lead_in_ms: u32,
    deadline_ms: u64,
}

/// 局がいまどこにいるか。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 手番の席が打牌などを選ぶ。
    Turn { seat: Seat, start: TurnStart },
    /// 打牌への反応を待つ。
    Reaction,
    /// 槓への槍槓を待つ。
    Chankan,
    /// 局が終わった。
    Done,
}

pub struct RoundEngine {
    state: RoundState,
    phase: Phase,
    pending: Vec<Event>,
    since_request: [Vec<Event>; 4],
    next_window_id: u32,
    first_window_id: u32,
    window: Option<ReactionWindow>,
    outstanding: [Option<Outstanding>; 4],
    offered: [Vec<ActionOption>; 4],
    last_window_id: u32,
    outcome: Option<RoundOutcome>,
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
            pending: Vec::new(),
            since_request: std::array::from_fn(|_| Vec::new()),
            next_window_id: first_window_id,
            first_window_id,
            window: None,
            outstanding: [None; 4],
            offered: std::array::from_fn(|_| Vec::new()),
            last_window_id: 0,
            outcome: None,
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

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    #[cfg(test)]
    pub(crate) fn state_mut(&mut self) -> &mut RoundState {
        &mut self.state
    }

    pub fn apply(&mut self, seat: Seat, command: Command, now_ms: u64) -> Result<(), Reject> {
        self.tick(now_ms);
        match command {
            Command::Discard { tile, riichi } => self.apply_discard(seat, tile, riichi, now_ms),
            Command::Tsumo => self.apply_tsumo(seat, now_ms),
            Command::Kyuushu => self.apply_kyuushu(seat, now_ms),
            Command::Ankan { kind } => self.apply_ankan(seat, kind, now_ms),
            Command::Kakan { tile } => self.apply_kakan(seat, tile, now_ms),
            Command::CallResponse {
                window_id,
                response,
            } => {
                let accepted = self.accept_response(seat, window_id, response, now_ms);
                self.resolve_window(now_ms);
                accepted
            }
        }
    }

    pub fn tick(&mut self, now_ms: u64) {
        match self.phase.clone() {
            Phase::Turn { seat, start } => {
                debug_assert!(self.outstanding[seat.index()].is_some());
                let Some(open) = self.outstanding[seat.index()] else {
                    return;
                };
                if now_ms <= open.deadline_ms {
                    return;
                }
                let options = discard_options(&self.state, seat, start);
                let can_tsumo = options.iter().any(|o| matches!(o, ActionOption::Tsumo));
                if let (true, TurnStart::Draw { tile, .. }) = (can_tsumo, start) {
                    self.state.seat_mut(seat).think_bank_ms = 0;
                    self.outstanding[seat.index()] = None;
                    self.finish_with_tsumo(seat, tile);
                    return;
                }
                let tile = match start {
                    TurnStart::Draw { tile, .. } => tile,
                    TurnStart::AfterCall => self.state.seat(seat).hand[0],
                };
                self.state.seat_mut(seat).think_bank_ms = 0;
                self.outstanding[seat.index()] = None;
                self.discard(seat, tile, now_ms);
            }
            Phase::Reaction => {
                self.pass_expired_seats(now_ms);
                self.resolve_window(now_ms);
            }
            Phase::Chankan => {
                self.pass_expired_seats(now_ms);
                self.resolve_window(now_ms);
            }
            Phase::Done => {}
        }
    }

    fn pass_expired_seats(&mut self, now_ms: u64) {
        for seat in Seat::ALL {
            let Some(open) = self.outstanding[seat.index()] else {
                continue;
            };
            if now_ms <= open.deadline_ms {
                continue;
            }
            self.state.seat_mut(seat).think_bank_ms = 0;
            self.outstanding[seat.index()] = None;
            if let Some(window) = self.window.as_mut() {
                let _ = window.respond(seat, CallResponse::Pass);
            }
        }
    }

    fn apply_discard(
        &mut self,
        seat: Seat,
        tile: Tile,
        riichi: bool,
        now_ms: u64,
    ) -> Result<(), Reject> {
        let Phase::Turn { seat: turn, start } = self.phase.clone() else {
            return Err(Reject::NotYourTurn);
        };
        if turn != seat {
            return Err(Reject::NotYourTurn);
        }
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
    }

    /// リーチを宣言する。打牌より先に出す。
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
        invariant::assert_scores_conserved(&before, &self.state.scores, crate::state::RIICHI_STICK);
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
        let position = self
            .state
            .seat(seat)
            .hand
            .iter()
            .position(|t| *t == tile)
            .expect("合法手として提示した牌は手にある");
        self.state.seat_mut(seat).hand.remove(position);
        let riichi_declaration =
            matches!(&self.state.seat(seat).riichi, Some(r) if r.step == RiichiStep::Declare);
        self.state
            .seat_mut(seat)
            .river
            .push(crate::state::Discarded {
                tile,
                manner,
                called_by: None,
                riichi_declaration,
            });
        // 四風連打の材料。最初のツモで、誰も鳴いておらず、風牌のときだけ数える。
        // 数えない打牌が1つでもあれば4つに届かないので、それで判定になる。
        if self.state.draw_count[seat.index()] == 1
            && !self.state.any_call_made
            && is_wind(tile.kind())
        {
            self.state.first_turn_winds.push(tile.kind());
        }
        if !tile.kind().is_terminal_or_honor() {
            self.state.seat_mut(seat).nagashi_alive = false;
        }
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
    }

    fn open_reaction(&mut self, from: Seat, tile: Tile, now_ms: u64) {
        let candidates: [Vec<ActionOption>; 4] =
            std::array::from_fn(|i| reaction_options(&self.state, Seat::new(i as u8), tile, from));
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

    fn accept_response(
        &mut self,
        seat: Seat,
        window_id: u32,
        response: CallResponse,
        now_ms: u64,
    ) -> Result<(), Reject> {
        let Some(window) = self.window.as_ref() else {
            let was_issued = self.first_window_id <= window_id && window_id < self.next_window_id;
            return Err(if was_issued {
                Reject::StaleWindow
            } else {
                Reject::NoWindow
            });
        };
        if window.id() != window_id {
            return Err(Reject::StaleWindow);
        }
        let is_discarder = window.from() == seat;
        let Some(open) = self.outstanding[seat.index()] else {
            return Err(if is_discarder {
                Reject::Window(Rejection::IsTheDiscarder)
            } else {
                Reject::StaleWindow
            });
        };
        if open.window_id != window_id {
            return Err(Reject::StaleWindow);
        }
        if !self.response_is_offered(seat, &response) {
            return Err(Reject::NotOffered);
        }
        self.window
            .as_mut()
            .expect("直前に確認した")
            .respond(seat, response)
            .map_err(Reject::Window)?;
        self.charge(seat, now_ms);
        Ok(())
    }

    fn resolve_window(&mut self, now_ms: u64) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        match window.resolve(now_ms, self.state.rules.min_reaction_window_ms) {
            Outcome::Pending => {}
            Outcome::PassAll if self.phase == Phase::Chankan => {
                self.window = None;
                self.record_passes(&[]);
                self.complete_pending_kan(now_ms);
            }
            Outcome::PassAll => {
                let from = window.from();
                self.window = None;
                self.record_passes(&[]);
                self.advance_after_pass(from, now_ms);
            }
            Outcome::Call { seat, response } => self.apply_call(seat, response, now_ms),
            Outcome::Ron(winners) if winners.len() == 3 => {
                self.window = None;
                self.record_passes(&winners);
                self.finish_abortive(RyuukyokuKind::ThreeRons, None, Vec::new());
            }
            Outcome::Ron(winners) => self.finish_with_ron(winners),
        }
    }

    fn response_is_offered(&self, seat: Seat, response: &CallResponse) -> bool {
        let offered = &self.offered[seat.index()];
        match response {
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

    fn record_passes(&mut self, acted: &[Seat]) {
        let window_id = self.last_window_id;
        for seat in Seat::ALL {
            let declined = std::mem::take(&mut self.offered[seat.index()]);
            self.outstanding[seat.index()] = None;
            if declined.is_empty() || acted.contains(&seat) {
                continue;
            }
            if declined.iter().any(|o| matches!(o, ActionOption::Ron)) {
                let hand = self.state.seat(seat).hand.clone();
                let melds = self.state.seat(seat).melds.len() as u8;
                let waits = mahjong_core::wait::waiting_tiles(
                    &mahjong_core::hand::HandCounts::from_tiles(&hand),
                    melds,
                );
                self.state.seat_mut(seat).passed_this_turn.extend(waits);
            }
            self.emit(Event::ActionPassed {
                seat,
                window_id,
                declined,
            });
        }
    }

    fn advance_after_pass(&mut self, from: Seat, now_ms: u64) {
        // 誰も和了しなかったので、宣言していたリーチが成立する。
        self.accept_riichi_of(from);
        if self.check_abortive() {
            return;
        }
        if self.state.wall.live_remaining() == 0 {
            self.finish_exhaustive();
            return;
        }
        let next = Seat::new(((from.index() + 1) % 4) as u8);
        self.draw_for(next, DrawSource::Wall);
        self.request_turn(now_ms);
    }

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
        let revealed = vec![(seat, self.state.seat(seat).hand.clone())];
        self.finish_abortive(RyuukyokuKind::NineTerminals, Some(seat), revealed);
        Ok(())
    }

    /// 打牌への反応が解決した時点で見る途中流局。
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

    fn four_kans_reached(&self) -> bool {
        let total: u32 = self.state.kan_count.iter().map(|c| u32::from(*c)).sum();
        let seats = self.state.kan_count.iter().filter(|c| **c > 0).count();
        total >= 4 && seats >= 2
    }

    fn charge(&mut self, seat: Seat, now_ms: u64) {
        let Some(open) = self.outstanding[seat.index()].take() else {
            return;
        };
        let bank = self.state.seat(seat).think_bank_ms;
        let elapsed = now_ms.saturating_sub(open.issued_at_ms);
        let charged = charge_bank(&self.state.rules, bank, elapsed, open.lead_in_ms);
        self.state.seat_mut(seat).think_bank_ms = charged;
    }

    fn apply_call(&mut self, seat: Seat, response: CallResponse, now_ms: u64) {
        let window = self.window.take().expect("反応ウィンドウが開いている");
        let from = window.from();
        let called = window.tile();
        let (kind, from_hand) = match response {
            CallResponse::Chi { tiles } => (MeldKind::Chi, tiles),
            CallResponse::Pon { tiles } => (MeldKind::Pon, tiles),
            CallResponse::Kan => {
                self.apply_minkan(seat, from, called, now_ms);
                return;
            }
            _ => unreachable!("鳴き以外がここへ来ることはない"),
        };
        for tile in from_hand {
            let position = self
                .state
                .seat(seat)
                .hand
                .iter()
                .position(|t| *t == tile)
                .expect("候補として提示した牌は手にある");
            self.state.seat_mut(seat).hand.remove(position);
        }
        let mut tiles = from_hand.to_vec();
        tiles.push(called);
        self.state.seat_mut(seat).melds.push(Meld {
            kind,
            tiles: tiles.clone(),
            from: Some(from),
            called_tile: Some(called),
        });
        self.state.seat_mut(from).nagashi_alive = false;
        if let Some(last) = self.state.seat_mut(from).river.last_mut() {
            last.called_by = Some(seat);
        }
        // 鳴かれても宣言は生きる。ただし一発は消える。
        self.accept_riichi_of(from);
        self.clear_ippatsu();
        self.state.any_call_made = true;
        self.record_passes(&[seat]);
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
    }

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

    fn check_kan_offered(&self, seat: Seat, wanted: KanCandidate) -> Result<Tile, Reject> {
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

    fn declare_kan(&mut self, seat: Seat, kind: MeldKind, tile: Tile, now_ms: u64) {
        self.state.pending_kan = Some(crate::state::PendingKan { seat, kind, tile });
        self.emit(Event::KanDeclared { seat, kind, tile });
        self.open_chankan(seat, tile, kind, now_ms);
    }

    fn open_chankan(&mut self, from: Seat, tile: Tile, kind: MeldKind, now_ms: u64) {
        let candidates: [Vec<ActionOption>; 4] =
            std::array::from_fn(|i| chankan_options(&self.state, Seat::new(i as u8), tile, kind));
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

    fn complete_pending_kan(&mut self, now_ms: u64) {
        let pending = self.state.pending_kan.take().expect("宣言中の槓がある");
        let seat = pending.seat;
        let kind = pending.kind;
        let (tiles, from) = match kind {
            MeldKind::Ankan => {
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

    fn request_turn(&mut self, now_ms: u64) {
        let Phase::Turn { seat, start } = self.phase.clone() else {
            panic!("手番でないのに要求を出そうとした");
        };
        let options = discard_options(&self.state, seat, start);
        self.request(seat, options, now_ms);
    }

    fn request(&mut self, seat: Seat, options: Vec<ActionOption>, now_ms: u64) {
        let lead_in_ms = lead_in_of(&self.since_request[seat.index()]);
        self.since_request[seat.index()].clear();
        let absolute = deadline_for(
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
            deadline_ms: absolute,
        });
        self.pending.push(Event::RequestAction {
            seat,
            window_id,
            options,
            deadline_ms: remaining_for_event(absolute, now_ms),
        });
    }

    fn take_window_id(&mut self) -> u32 {
        let id = self.next_window_id;
        self.next_window_id += 1;
        id
    }
}

/// 風牌は 1z..4z。
fn is_wind(kind: TileKind) -> bool {
    (27..=30).contains(&kind.index())
}

/// 三元牌は 5z..7z。
fn is_dragon(kind: TileKind) -> bool {
    (31..=33).contains(&kind.index())
}

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

    #[test]
    fn the_seed_is_committed_before_anything_is_dealt() {
        let mut engine = start_at(0);
        let events = engine.drain_events();
        let Event::RoundStart { seed_commit, .. } = &events[0] else {
            panic!("RoundStart が先頭にない");
        };
        assert_eq!(*seed_commit, seed().commitment());
    }

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

    #[test]
    fn the_first_deadline_includes_the_opening_animation() {
        let mut engine = start_at(0);
        let events = engine.drain_events();
        let Event::RequestAction { deadline_ms, .. } = &events[3] else {
            panic!("RequestAction が無い");
        };
        let lead_in = lead_in_of(&events[..3]);
        let absolute = deadline_for(&rules(), 0, rules().think_bank_ms, lead_in);
        assert_eq!(*deadline_ms, remaining_for_event(absolute, 0));
    }

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

    #[test]
    fn the_same_seed_produces_the_same_events() {
        let mut first = start_at(0);
        let mut second = start_at(0);
        assert_eq!(first.drain_events(), second.drain_events());
    }

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

    #[test]
    fn draining_twice_yields_nothing_the_second_time() {
        let mut engine = start_at(0);
        assert_eq!(engine.drain_events().len(), 4);
        assert!(engine.drain_events().is_empty());
    }

    #[test]
    fn the_opening_conserves_every_tile() {
        let mut engine = start_at(0);
        engine.drain_events();
        crate::invariant::assert_tiles_conserved(engine.state());
    }
}

#[cfg(test)]
mod discard_tests {
    use super::start_tests::{rules, start_at};
    use super::*;
    use protocol::command::{CallResponse, Command};
    use protocol::event::DiscardManner;
    use protocol::notation::parse_tile;

    /// どの席の締切も確実に過ぎている時刻。
    ///
    /// **最低待機を過ぎただけではウィンドウは確定しない。**
    /// `ReactionWindow::resolve` は「未応答の席がいまの最高優先度以上を
    /// 出しうる」あいだ `Pending` を返す（`reaction.rs`）。他家の手牌を
    /// 固定していないテストでは、鳴ける席がいるかどうかがシードで変わる。
    /// 反応の締切は基準時間 + バンク + 通信猶予 + lead_in なので、
    /// これを超える時刻で確定させる。
    pub(super) const WAY_PAST_ANY_DEADLINE_MS: u64 = 1_000_000;

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

    /// 席1がポンできる局面を作る。配牌に頼らず、手牌を直接置く。
    ///
    /// **席2と席3から同じ種類の牌を追い出す。**配牌でそこにも同じ牌が
    /// 2枚あると、その席もポンの候補になる。`ReactionWindow::resolve` は
    /// 「未応答の席が同じ優先度以上を出しうる」あいだ確定しないので、
    /// 席1がポンしてもウィンドウが開いたままになり、テストがシードに
    /// 依存して落ちる。
    ///
    /// 追い出し先の牌は種類が違えばよい。席2と席3は席0の下家ではないので
    /// チーもできない。**ロン待ちである可能性までは消せない**が、その場合は
    /// 要求が出る席を数えるテストで気づける。
    pub(super) fn state_where_seat_one_can_pon(engine: &mut RoundEngine, tile: &str) -> Tile {
        let target = parse_tile(tile).expect("正しい記法");
        let filler = {
            let nine_man = parse_tile("9m").expect("正しい記法");
            if nine_man.kind() == target.kind() {
                parse_tile("1m").expect("正しい記法")
            } else {
                nine_man
            }
        };
        for seat in [Seat::new(2), Seat::new(3)] {
            for held in engine.state_mut().seat_mut(seat).hand.iter_mut() {
                if held.kind() == target.kind() {
                    *held = filler;
                }
            }
        }

        let hand = &mut engine.state_mut().seat_mut(Seat::new(1)).hand;
        hand[0] = target;
        hand[1] = target;
        target
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
        // 手に無い牌を探す。14枚では34種を埋められない。
        let missing = (0..34u8)
            .filter_map(protocol::tile::TileKind::from_index)
            .map(Tile::from_kind)
            .find(|t| !engine.state().seat(Seat::new(0)).hand.contains(t))
            .expect("必ず見つかる");
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

        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
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

    /// 候補があった席にだけ ActionPassed が出る。
    ///
    /// 席1にポンの候補を作り、席2と席3からは同じ牌を追い出しておく。
    /// これで「出る席」と「出ない席」の両方を同じ局面で検査できる。
    #[test]
    fn action_passed_goes_exactly_to_the_seats_with_candidates() {
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
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);

        let events = engine.drain_events();
        let passed: Vec<Seat> = events
            .iter()
            .filter_map(|e| match e {
                Event::ActionPassed { seat, declined, .. } => {
                    assert!(!declined.is_empty(), "候補が空の ActionPassed");
                    Some(*seat)
                }
                _ => None,
            })
            .collect();
        assert_eq!(passed, vec![Seat::new(1)], "候補があったのは席1だけ");
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
        let lead_in = lead_in_of(&events[..3]);
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

    /// 締切を過ぎた打牌は、自動打牌のあとに届くので拒否される。
    ///
    /// `apply` が `tick` を通すかどうかで結果が変わってはならない。
    #[test]
    fn a_discard_after_the_deadline_loses_the_turn() {
        let mut engine = start_at(0);
        let events = engine.drain_events();
        let Event::RequestAction { deadline_ms, .. } = &events[3] else {
            panic!("要求が出ていない");
        };
        let tile = drawn_of(&engine);
        let too_late = *deadline_ms as u64 + 1;

        assert_eq!(
            engine.apply(
                Seat::new(0),
                Command::Discard {
                    tile,
                    riichi: false
                },
                too_late
            ),
            Err(Reject::NotYourTurn),
            "自動打牌が済んでいるので手番ではない"
        );
        // 自動打牌そのものは行われている。
        let events = engine.drain_events();
        assert!(events.iter().any(|e| matches!(e, Event::Discard { .. })));
        assert_eq!(engine.state().seat(Seat::new(0)).think_bank_ms, 0);
    }

    /// tick を先に呼んでも、apply に任せても同じ結果になる。
    #[test]
    fn ticking_first_changes_nothing() {
        let build = |explicit_tick: bool| {
            let mut engine = start_at(0);
            let events = engine.drain_events();
            let Event::RequestAction { deadline_ms, .. } = &events[3] else {
                panic!("要求が出ていない");
            };
            let tile = drawn_of(&engine);
            let too_late = *deadline_ms as u64 + 1;
            if explicit_tick {
                engine.tick(too_late);
            }
            let result = engine.apply(
                Seat::new(0),
                Command::Discard {
                    tile,
                    riichi: false,
                },
                too_late,
            );
            (result, engine.drain_events())
        };
        assert_eq!(build(true), build(false));
    }

    /// 反応の期限切れでも、tick を先に呼んでも apply に任せても同じになる。
    #[test]
    fn ticking_first_changes_nothing_for_reactions() {
        let build = |explicit_tick: bool| {
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
            let opened = engine.drain_events();
            let Some(Event::RequestAction {
                window_id,
                deadline_ms,
                ..
            }) = opened
                .iter()
                .find(|e| matches!(e, Event::RequestAction { .. }))
                .cloned()
            else {
                panic!("反応の要求が出ていない: {opened:?}");
            };

            let too_late = 1_000 + deadline_ms as u64 + 1;
            if explicit_tick {
                engine.tick(too_late);
            }
            let result = engine.apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pon {
                        tiles: [target, target],
                    },
                },
                too_late,
            );
            (result, engine.drain_events())
        };
        assert_eq!(build(true), build(false));
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
            // 鳴ける席がいてもウィンドウが確定するよう、締切を越えて進める。
            now += WAY_PAST_ANY_DEADLINE_MS;
            engine.tick(now);
            engine.drain_events();
            crate::invariant::assert_tiles_conserved(engine.state());
            // 次の手番の要求はいま出たところなので、ここからは期限内である。
            now += 1_000;
        }
    }
}

#[cfg(test)]
mod riichi_tests {
    // RiichiState は match_flow.rs の親スコープに入っていない。明示して取り込む。
    use super::discard_tests::WAY_PAST_ANY_DEADLINE_MS;
    use super::ending_tests::{make_tenpai, set_dealer_hand};
    use super::start_tests::start_at;
    use super::*;
    use crate::state::RiichiState;
    use protocol::command::{CallResponse, Command};
    use protocol::notation::{parse_hand, parse_tile};

    /// 親に 6p/9p 待ちのテンパイを持たせ、1z でリーチ宣言する。
    fn declare_riichi(engine: &mut RoundEngine, now_ms: u64) -> Tile {
        set_dealer_hand(engine.state_mut(), "234567m23478p22s1z");
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
        engine.state().seat(seat).riichi.expect("リーチしている")
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
            .position(|e| {
                matches!(
                    e,
                    Event::Riichi {
                        step: RiichiStep::Declare,
                        ..
                    }
                )
            })
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
        set_dealer_hand(engine.state_mut(), "234567m23478p22s1z");
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
        assert!(
            ids.contains(&protocol::yaku::YakuId::DoubleRiichi),
            "{ids:?}"
        );
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
        make_tenpai(engine.state_mut(), Seat::new(0));
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

        let total: i32 =
            engine.state().scores.iter().sum::<i32>() + engine.state().riichi_sticks as i32 * 1_000;
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

#[cfg(test)]
mod call_tests {
    // 兄弟モジュールの項目は use super::*; では入らない。明示して取り込む。
    use super::discard_tests::state_where_seat_one_can_pon;
    use super::start_tests::start_at;
    use super::*;
    use protocol::command::{CallResponse, Command};
    use protocol::meld::MeldKind;

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
        // **意図した局面になっていることを、応答の前に固定する。**
        // 席1以外にも候補があるとウィンドウが確定せず、以降の主張が空振りする。
        // ここで見るのは打牌に対する反応の要求である。
        let opened = engine.drain_events();
        let requested: Vec<Seat> = opened
            .iter()
            .filter_map(|e| match e {
                Event::RequestAction { seat, .. } => Some(*seat),
                _ => None,
            })
            .collect();
        assert_eq!(requested, vec![Seat::new(1)], "反応できるのは席1だけ");

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

    /// 締切を過ぎたコマンドは、拒否されても状態機械を止めない。
    ///
    /// `apply` が先頭で `tick` を通すので、期限切れの反映と解決は
    /// コマンドの種類によらず起こる。ここでは明槓で確かめる。
    #[test]
    fn a_late_command_still_advances_the_round() {
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
        let events = engine.drain_events();
        let Some(Event::RequestAction {
            window_id,
            deadline_ms,
            ..
        }) = events
            .iter()
            .find(|e| matches!(e, Event::RequestAction { seat, .. } if *seat == Seat::new(1)))
            .cloned()
        else {
            panic!("席1へ要求が出ていない: {events:?}");
        };

        // 自分の締切を過ぎてから送る。tick は呼ばない。
        // apply が先に tick を通すので、ウィンドウはもう閉じている。
        let too_late = 1_000 + deadline_ms as u64 + 1;
        assert_eq!(
            engine.apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Kan,
                },
                too_late
            ),
            Err(Reject::StaleWindow)
        );

        let events = engine.drain_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Draw { seat, .. } if *seat == Seat::new(1))),
            "拒否したまま止まっている: {events:?}"
        );
    }
}

#[cfg(test)]
mod kan_tests {
    use super::discard_tests::{state_where_seat_one_can_pon, WAY_PAST_ANY_DEADLINE_MS};
    use super::ending_tests::set_dealer_hand;
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
        set_dealer_hand(engine.state_mut(), "1111m234p567p22s78s");
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
        set_dealer_hand(engine.state_mut(), "1111m234p567p22s78s");
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
        set_dealer_hand(engine.state_mut(), "1111m234p567p22s78s");
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
        set_dealer_hand(engine.state_mut(), "1111m234p567p22s78s");
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
        set_dealer_hand(engine.state_mut(), "1111m234p567p22s78s");
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
        set_dealer_hand(engine.state_mut(), "1111m234p567p22s78s");
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
        set_dealer_hand(engine.state_mut(), "1111m234p567p22s78s");
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
        set_dealer_hand(engine.state_mut(), "234567m23478p22s1z");
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
        assert_eq!(
            seat.melds[0].from,
            Some(Seat::new(3)),
            "元のポンの相手を残す"
        );
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
        set_dealer_hand(engine.state_mut(), "1111m234p567p22s78s");
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
        set_dealer_hand(engine.state_mut(), "1111m234p567p22s78s");
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
        assert!(
            ids.contains(&protocol::yaku::YakuId::RinshanKaihou),
            "{ids:?}"
        );
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
        set_dealer_hand(engine.state_mut(), "1111m234p567p22s78s");
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
        engine.state_mut().seat_mut(Seat::new(1)).hand[0] = parse_tile("3p").expect("正しい記法");
        engine.state_mut().seat_mut(Seat::new(1)).hand[1] = parse_tile("4p").expect("正しい記法");
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

use crate::round::{score_change, settle_agari, settle_exhaustive, settle_nagashi, AgariInput};
use mahjong_core::hand::HandCounts;
use mahjong_core::score::{score, WinType};
use mahjong_core::wait::waiting_tiles;
use protocol::event::{AgariResult, ContinuationReason, RyuukyokuKind};

/// 局が終わったときの結果。Wave 2e の `MatchEngine` が次局を組み立てるのに使う。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RoundOutcome {
    pub scores: [i32; 4],
    pub riichi_sticks: u8,
    pub dealer_repeats: bool,
    /// Wave 2e が `RoundEnd.reason` を組み立てるのに使う。
    /// 流し満貫は荒牌平局と区別しなければ牌譜から復元できない。
    pub reason: ContinuationReason,
    /// 流局で終わったか。本場の進み方が和了と違う。
    pub was_draw: bool,
}

impl RoundEngine {
    pub fn outcome(&self) -> Option<&RoundOutcome> {
        self.outcome.as_ref()
    }

    /// ロンを確定させる。
    ///
    /// `ReactionWindow::resolve` はロン応答をすべて返すので、`double_ron` が
    /// 偽なら**ここで頭ハネにする。**放銃者から下家回りで最も近い1人だけが
    /// 和了する。
    fn finish_with_ron(&mut self, declared: Vec<Seat>) {
        let window = self.window.take().expect("反応ウィンドウが開いている");
        let from = window.from();
        let tile = window.tile();

        // **見逃しの記録は頭ハネの前に行う。**ロンを宣言した席は、頭ハネで
        // 和了できなくても見逃してはいない。頭ハネ後の勝者だけを除くと、
        // 負けた側が見逃し扱いになり、同巡内フリテンまで付いてしまう。
        self.record_passes(&declared);

        let winners = self.head_bump(from, declared);

        let mut inputs = Vec::new();
        let mut results = Vec::new();
        for seat in &winners {
            let liability = self.liability_for(*seat, WinType::Ron);
            let context = self.state.hand_context(*seat, WinType::Ron);
            let hand = self.state.seat(*seat).hand.clone();
            let melds = self.state.seat(*seat).melds.clone();
            let result = score(&hand, &melds, tile, &context, &self.state.rules)
                .expect("ロンを提示した以上、役がある");
            inputs.push(AgariInput {
                seat: *seat,
                from: Some(from),
                payment: result.payment,
                liability,
            });
            results.push(AgariResult {
                seat: *seat,
                from: Some(from),
                hand,
                melds,
                win_tile: tile,
                yaku: result.yaku.clone(),
                fu: result.fu,
                han: result.han,
                score: payment_total(&result.payment),
                liability,
                // リーチ和了のみ Some。空配列との使い分けに頼らない設計である。
                ura_indicators: self.ura_for(*seat),
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
        // 引数の中で &self.state を作ると receiver の &mut self と衝突する。
        let scores = settlement_scores(&self.state, &settlement);
        // 槍槓の1翻は hand_context が pending_kan を見て立てる。
        // 採点が終わるまで消せない。
        self.state.pending_kan = None;
        self.finish(
            scores,
            0,
            dealer_repeats,
            if dealer_repeats {
                ContinuationReason::DealerWin
            } else {
                ContinuationReason::DealerLoss
            },
            false,
        );
    }

    /// 頭ハネ。`double_ron` が真ならそのまま席順で返す。
    fn head_bump(&self, from: Seat, mut winners: Vec<Seat>) -> Vec<Seat> {
        if self.state.rules.double_ron || winners.len() <= 1 {
            winners.sort_by_key(|s| s.index());
            return winners;
        }
        let head = winners
            .into_iter()
            .min_by_key(|s| (s.index() + 4 - from.index()) % 4)
            .expect("1人以上いる");
        vec![head]
    }

    /// ツモ和了を確定させる。
    fn finish_with_tsumo(&mut self, seat: Seat, win_tile: Tile) {
        let context = self.state.hand_context(seat, WinType::Tsumo);
        // `score` は和了牌を除いた手牌を取る。
        let mut hand = self.state.seat(seat).hand.clone();
        let position = hand
            .iter()
            .position(|t| *t == win_tile)
            .expect("ツモ牌は手にある");
        hand.remove(position);
        let melds = self.state.seat(seat).melds.clone();

        let result = score(&hand, &melds, win_tile, &context, &self.state.rules)
            .expect("ツモを提示した以上、役がある");
        let liability = self.liability_for(seat, WinType::Tsumo);
        let input = AgariInput {
            seat,
            from: None,
            payment: result.payment,
            liability,
        };
        let settlement = settle_agari(
            &[input],
            self.state.dealer,
            self.state.honba,
            self.state.riichi_sticks,
        );
        let results = vec![AgariResult {
            seat,
            from: None,
            hand,
            melds,
            win_tile,
            yaku: result.yaku.clone(),
            fu: result.fu,
            han: result.han,
            score: payment_total(&result.payment),
            liability,
            ura_indicators: self.ura_for(seat),
        }];
        self.emit(Event::Agari {
            results,
            settlement: settlement.clone(),
        });

        let dealer_repeats = seat == self.state.dealer;
        let scores = settlement_scores(&self.state, &settlement);
        self.finish(
            scores,
            0,
            dealer_repeats,
            if dealer_repeats {
                ContinuationReason::DealerWin
            } else {
                ContinuationReason::DealerLoss
            },
            false,
        );
    }

    /// 裏ドラは、リーチが成立している和了者にだけ渡す。
    fn ura_for(&self, seat: Seat) -> Option<Vec<Tile>> {
        matches!(
            &self.state.seat(seat).riichi,
            Some(r) if r.step == RiichiStep::Accepted
        )
        .then(|| self.state.wall.ura_indicators().to_vec())
    }

    /// 責任払いを副露列から導く。
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
                    None => has_concealed = true,
                }
            }
            if count == needed && !has_concealed {
                if let Some(from) = last_from {
                    return Some(Liability {
                        seat: from,
                        yaku,
                        mode,
                    });
                }
            }
        }
        None
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

        let nagashi = !nagashi_winners.is_empty();
        let settlement = if nagashi {
            settle_nagashi(&nagashi_winners, self.state.dealer)
        } else {
            settle_exhaustive(tenpai, &self.state.rules)
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
        // 流し満貫も荒牌平局の一種なので、連荘は親のテンパイで決まる。
        // ただし理由は分けないと Wave 2e が RoundEnd.reason を復元できない。
        let dealer_repeats = tenpai[self.state.dealer.index()];
        let reason = if nagashi {
            ContinuationReason::NagashiMangan
        } else if dealer_repeats {
            ContinuationReason::DealerTenpai
        } else {
            ContinuationReason::DealerLoss
        };
        self.finish(
            scores,
            self.state.riichi_sticks,
            dealer_repeats,
            reason,
            true,
        );
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
        self.finish(scores, sticks, true, ContinuationReason::AbortiveDraw, true);
    }

    /// 局を閉じる。
    fn finish(
        &mut self,
        scores: [i32; 4],
        riichi_sticks: u8,
        dealer_repeats: bool,
        reason: ContinuationReason,
        was_draw: bool,
    ) {
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
            reason,
            was_draw,
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

impl RoundEngine {
    fn apply_tsumo(&mut self, seat: Seat, now_ms: u64) -> Result<(), Reject> {
        let Phase::Turn { seat: turn, start } = self.phase.clone() else {
            return Err(Reject::NotYourTurn);
        };
        if turn != seat {
            return Err(Reject::NotYourTurn);
        }
        // 提示していないツモは受け付けない。役の有無は discard_options が見る。
        if !discard_options(&self.state, seat, start)
            .iter()
            .any(|o| matches!(o, ActionOption::Tsumo))
        {
            return Err(Reject::NotOffered);
        }
        let TurnStart::Draw { tile, .. } = start else {
            return Err(Reject::NotOffered);
        };
        self.charge(seat, now_ms);
        self.finish_with_tsumo(seat, tile);
        Ok(())
    }
}

impl RoundEngine {
    /// 指定した席のツモ番を直接作る。
    /// 自然な進行では狙った牌をツモらせられないので、テストだけが使う。
    ///
    /// **手へ1枚足す分、山から1枚抜く。**そうしないと総数が137になり、
    /// `assert_tiles_conserved` が落ちる。
    #[cfg(test)]
    pub(crate) fn force_draw_turn(&mut self, seat: Seat, tile: Tile) {
        let expected = 13 - 3 * self.state.seat(seat).melds.len();
        assert_eq!(
            self.state.seat(seat).hand.len(),
            expected,
            "副露1つにつき手牌は3枚短い"
        );
        self.state.wall.draw().expect("山に残っている");
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
        // **締切は有限にする。**u64::MAX にすると tick が永久に期限切れと
        // 見なさず、自動打牌も自動和了も起きない。
        let bank = self.state.seat(seat).think_bank_ms;
        self.outstanding[seat.index()] = Some(Outstanding {
            window_id: 0,
            issued_at_ms: 0,
            lead_in_ms: 0,
            deadline_ms: deadline_for(&self.state.rules, 0, bank, 0),
        });
    }
}

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
    /// 局ごとのシード。半荘の終わりにまとめて開示する。
    seeds: Vec<Seed>,
    over: bool,
    /// 直前の局の終わり方。`finish_match` が `RoundEnd` に載せる。
    last_reason: ContinuationReason,
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
            seeds: Vec::new(),
            over: false,
            last_reason: ContinuationReason::DealerLoss,
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

    pub fn is_over(&self) -> bool {
        self.over
    }

    /// 次の局を始めるためのシードが要るか。
    pub fn needs_seed(&self) -> bool {
        self.engine.is_none() && !self.over
    }

    /// 動いている局の状態。局と局のあいだは `None`。
    ///
    /// 卓が CPU へ渡す `View` を組み立てるために公開する。
    pub fn round_state(&self) -> Option<&RoundState> {
        self.engine.as_ref().map(|e| e.state())
    }

    pub fn begin_round(&mut self, seed: &Seed, now_ms: u64) {
        assert!(self.needs_seed(), "局が動いている間は始められない");
        self.seeds.push(*seed);
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
    fn close_round(&mut self, outcome: RoundOutcome, now_ms: u64) {
        self.scores = outcome.scores;
        self.riichi_sticks = outcome.riichi_sticks;
        self.honba = if outcome.was_draw || outcome.dealer_repeats {
            self.honba + 1
        } else {
            0
        };
        self.last_reason = outcome.reason;

        // 終局判定は、まだ終わった局の round と dealer を指しているここで行う。
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

    /// 親と局を次へ進める。
    fn advance_seat_and_round(&mut self, outcome: &RoundOutcome) {
        // 本来の最終局で誰も返し点へ届かなければ、親が続く結果でも
        // 次の風へ延長する。終局判定を通過した直後なので、この条件を
        // `dealer_repeats` より先に扱う。
        if self.is_last_round() && !self.reached_return_score() {
            self.dealer = Seat::new(((self.dealer.index() + 1) % 4) as u8);
            self.round = Round {
                wind: next_wind(self.round.wind),
                number: 1,
            };
            return;
        }
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

    /// 終局するか。
    fn should_end(&self, outcome: &RoundOutcome) -> bool {
        if self.rules.busted_ends_match && self.scores.iter().any(|s| *s < 0) {
            return true;
        }
        if self.round.wind == extension_wind(self.rules.length) {
            return self.round.number == 4 || self.reached_return_score();
        }
        if !self.is_last_round() {
            return false;
        }
        if !self.reached_return_score() {
            return false;
        }
        if !outcome.dealer_repeats {
            return true;
        }
        let can_stop = matches!(
            outcome.reason,
            ContinuationReason::DealerWin
                | ContinuationReason::DealerTenpai
                | ContinuationReason::NagashiMangan
        );
        can_stop && placements_of(&self.scores)[self.dealer.index()] == 1
    }

    fn reached_return_score(&self) -> bool {
        self.scores.iter().any(|s| *s >= self.rules.return_score)
    }

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

    #[cfg(test)]
    pub(crate) fn test_round_state(&self) -> &RoundState {
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

    #[cfg(test)]
    pub(crate) fn current_window_id(&self) -> u32 {
        self.next_window_id
    }

    #[cfg(test)]
    pub(crate) fn carried_sticks(&self) -> u8 {
        self.riichi_sticks
    }
}

/// 場風の順。東 → 南 → 西 → 北。
fn next_wind(wind: Wind) -> Wind {
    match wind {
        Wind::East => Wind::South,
        Wind::South => Wind::West,
        Wind::West => Wind::North,
        Wind::North => Wind::East,
    }
}

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
        MatchEngine::start(Ruleset::kin_no_ma(MatchLength::Hanchan), players(), 0)
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

        let tile = game.test_round_state().seat(Seat::new(0)).hand[0];
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
        assert!(!game.last_outcome().expect("終わっている").was_draw);
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
        let dealer = game.test_round_state().dealer;
        make_tenpai(game.round_state_mut(), dealer);
        game.force_draw_turn(dealer, parse_tile("6p").expect("正しい記法"));
        game.apply(dealer, Command::Tsumo, 2_000)
            .expect("ツモ和了できる");
    }

    /// いまの親の下家にツモ和了させる。親が流れる。
    ///
    /// こちらもイベントは drain しない。
    pub(super) fn finish_with_a_child_tsumo(game: &mut MatchEngine) {
        let dealer = game.test_round_state().dealer;
        let child = Seat::new(((dealer.index() + 1) % 4) as u8);
        make_tenpai(game.round_state_mut(), child);
        game.force_draw_turn(child, parse_tile("6p").expect("正しい記法"));
        game.apply(child, Command::Tsumo, 2_000)
            .expect("ツモ和了できる");
    }
}

#[cfg(test)]
mod progression_tests {
    use super::discard_tests::WAY_PAST_ANY_DEADLINE_MS;
    use super::ending_tests::{clear_nagashi, drain_the_wall, set_dealer_hand};
    use super::match_tests::{
        finish_with_a_child_tsumo, finish_with_a_dealer_tsumo, hanchan, seed_of,
    };
    use super::*;
    use protocol::command::Command;
    use protocol::notation::{parse_hand, parse_tile};
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

    /// 全員ノーテンの荒牌平局で局を終える。
    ///
    /// **和了で局を送ると点棒が動く。**その額はドラ次第で変わるので、
    /// 「誰も返し点へ届かないまま何局も進める」テストが配牌に依存する。
    /// 全員ノーテンの流局なら点棒はまったく動かない。
    pub(super) fn finish_with_a_noten_draw(game: &mut MatchEngine) {
        let dealer = game.test_round_state().dealer;
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
            finish_with_a_noten_draw(&mut game);
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
        let tile = game.test_round_state().seat(Seat::new(1)).hand[0];
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
        // 8局ぶん親を流して南4局まで進める。
        // 全員ノーテンの流局なので点棒は動かない。
        for index in 1..=8u8 {
            begin(&mut game, index);
            finish_with_a_noten_draw(&mut game);
        }
        assert_eq!(game.round().wind, Wind::West, "西入した");
        assert_eq!(game.round().number, 1);
    }
}

#[cfg(test)]
mod ending_match_tests {
    use super::discard_tests::WAY_PAST_ANY_DEADLINE_MS;
    use super::ending_tests::{clear_nagashi, drain_the_wall, set_dealer_hand};
    use super::match_tests::{
        finish_with_a_child_tsumo, finish_with_a_dealer_tsumo, players, seed_of,
    };
    use super::progression_tests::finish_with_a_noten_draw;
    use super::*;
    use protocol::command::Command;
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::ruleset::MatchLength;
    use protocol::seat::Wind;

    /// 東風戦。4局で終わるので終局まで回しやすい。
    fn tonpuu() -> MatchEngine {
        MatchEngine::start(Ruleset::kin_no_ma(MatchLength::Tonpuu), players(), 0)
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
        game.force_scores([15_000, 50_000, 20_000, 15_000]);
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
        let mut game = MatchEngine::start(Ruleset::kin_no_ma(MatchLength::Hanchan), players(), 0);
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

#[cfg(test)]
mod ending_tests {
    // 兄弟モジュールの項目は use super::*; では入らない。明示して取り込む。
    use super::discard_tests::WAY_PAST_ANY_DEADLINE_MS;
    use super::start_tests::{rules, start_at};
    use super::*;
    use protocol::command::{CallResponse, Command};
    use protocol::event::{ContinuationReason, RyuukyokuKind};
    use protocol::notation::{parse_hand, parse_tile};

    /// 指定した席へ和了形の一歩手前を持たせる。234567m 23478p 22s は 6p/9p 待ち。
    ///
    /// **枚数を保つ。**親は配牌のあとツモ済みなので14枚である。13枚の形へ
    /// 差し替えると1枚消えるので、あふれた分は捨て台の河へ移す。
    /// 河の牌も `assert_tiles_conserved` の数え上げに入る。
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

    /// 流し満貫の資格を全席から落とす。
    ///
    /// **局の開始時は4席とも `nagashi_alive` が真である**（`state.rs`）。
    /// 資格が消えるのは中張牌を切ったときと、自分の捨て牌を鳴かれたときだけ。
    /// テストでは誰もほとんど打牌しないので、明示的に落とさないと全員が
    /// 流し満貫になり、荒牌平局のテンパイ料が発生しない。
    pub(super) fn clear_nagashi(state: &mut RoundState) {
        for seat in Seat::ALL {
            state.seat_mut(seat).nagashi_alive = false;
        }
    }

    /// 山を空にする。荒牌平局を起こすため。
    ///
    /// **引いた牌を捨てるだけでは牌が消える。**`assert_tiles_conserved` は
    /// 手牌 + 副露 + 鳴かれていない河 + 山 の合計が136であることを要求する
    /// （`invariant.rs`）。山から引いた分は、どこかの河へ入れて数を保つ。
    /// 捨て台には席3を使う。テスト対象にしない席である。
    pub(super) fn drain_the_wall(state: &mut RoundState) {
        while state.wall.live_remaining() > 0 {
            let tile = state.wall.draw().expect("残っている");
            state.seat_mut(sink()).river.push(crate::state::Discarded {
                tile,
                manner: DiscardManner::Tsumogiri,
                called_by: None,
                riichi_declaration: false,
            });
        }
        crate::invariant::assert_tiles_conserved(state);
    }

    /// 捨て台の席。牌の総数を保つための行き先に使う。
    fn sink() -> Seat {
        Seat::new(3)
    }

    /// 牌の退避先。対象の席とは必ず別になる。
    fn sink_for(seat: Seat) -> Seat {
        Seat::new(((seat.index() + 1) % 4) as u8)
    }

    /// ロンすると Agari が出て、局が終わる。
    #[test]
    fn a_ron_ends_the_round() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(engine.state_mut(), Seat::new(1));
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
        make_tenpai(engine.state_mut(), Seat::new(1));
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
        make_tenpai(engine.state_mut(), Seat::new(1));
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
        make_tenpai(engine.state_mut(), Seat::new(0));
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
        make_tenpai(engine.state_mut(), Seat::new(0));
        engine.force_draw_turn(Seat::new(0), parse_tile("6p").expect("正しい記法"));
        engine
            .apply(Seat::new(0), Command::Tsumo, 1_000)
            .expect("ツモ和了できる");
        engine.drain_events();
        assert!(engine.outcome().expect("終わっている").dealer_repeats);
    }

    /// 締切を過ぎても、和了できる状態なら自動で和了る（仕様 8.2）。
    /// 切断した側が和了を取り逃がさないための規則である。
    #[test]
    fn an_unanswered_turn_wins_when_it_can() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(engine.state_mut(), Seat::new(0));
        engine.force_draw_turn(Seat::new(0), parse_tile("6p").expect("正しい記法"));

        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        let events = engine.drain_events();
        assert!(
            events.iter().any(|e| matches!(e, Event::Agari { .. })),
            "自動でツモ和了していない: {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(e, Event::Discard { .. })),
            "和了できるのに打牌している"
        );
        assert_eq!(*engine.phase(), Phase::Done);
    }

    /// 和了形でない席はツモ和了できない。
    #[test]
    fn a_seat_without_a_winning_hand_cannot_declare_tsumo() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(engine.state_mut(), Seat::new(0));
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
        make_tenpai(engine.state_mut(), Seat::new(1));
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
        clear_nagashi(engine.state_mut());
        drain_the_wall(engine.state_mut());

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
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);

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
        make_tenpai(engine.state_mut(), Seat::new(1));
        clear_nagashi(engine.state_mut());
        drain_the_wall(engine.state_mut());

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
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
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
        clear_nagashi(engine.state_mut());
        drain_the_wall(engine.state_mut());
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
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        engine.drain_events();

        assert_eq!(engine.outcome().expect("終わっている").riichi_sticks, 1);
    }

    /// 荒牌平局の直前でも、最後の打牌にロンがあれば和了が優先される。
    #[test]
    fn a_ron_on_the_last_discard_beats_the_draw() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(engine.state_mut(), Seat::new(1));
        clear_nagashi(engine.state_mut());
        drain_the_wall(engine.state_mut());

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
        clear_nagashi(engine.state_mut());
        engine.state_mut().seat_mut(Seat::new(2)).nagashi_alive = true;
        drain_the_wall(engine.state_mut());
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
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);

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

    /// 山を空にしてから親に指定の牌を切らせ、荒牌平局まで進める。
    ///
    /// 切る牌を引数で受けるのは、テンパイを保つかどうかを狙って決めるためである。
    fn run_to_exhaustive_draw(engine: &mut RoundEngine, discard: Tile) {
        clear_nagashi(engine.state_mut());
        drain_the_wall(engine.state_mut());
        engine
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile: discard,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        engine.drain_events();
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        engine.drain_events();
    }

    /// 親の手を14枚の指定した形にする。親は配牌後にツモ済みなので14枚であり、
    /// 14枚を14枚に差し替えるだけなら牌の総数は変わらない。
    pub(super) fn set_dealer_hand(state: &mut RoundState, notation: &str) {
        let hand = parse_hand(notation).expect("正しい記法");
        assert_eq!(hand.len(), 14, "親の手は14枚である");
        let dealer = state.dealer;
        assert_eq!(state.seat(dealer).hand.len(), 14);
        state.seat_mut(dealer).hand = hand;
    }

    /// 親がテンパイなら連荘する。
    #[test]
    fn a_tenpai_dealer_repeats_after_an_exhaustive_draw() {
        let mut engine = start_at(0);
        engine.drain_events();
        // 1z を切れば 234567m 23478p 22s の 6p/9p 待ちが残る。
        set_dealer_hand(engine.state_mut(), "234567m23478p22s1z");
        run_to_exhaustive_draw(&mut engine, parse_tile("1z").expect("正しい記法"));

        let outcome = engine.outcome().expect("終わっている");
        assert!(outcome.dealer_repeats);
        assert_eq!(outcome.reason, ContinuationReason::DealerTenpai);
    }

    /// 親がノーテンなら親が流れる。
    #[test]
    fn a_noten_dealer_loses_the_dealership() {
        let mut engine = start_at(0);
        engine.drain_events();
        // 対子も塔子も無い散らばった形。5z を切っても向聴数は変わらない。
        set_dealer_hand(engine.state_mut(), "147m258p369s12345z");
        run_to_exhaustive_draw(&mut engine, parse_tile("5z").expect("正しい記法"));

        let outcome = engine.outcome().expect("終わっている");
        assert!(!outcome.dealer_repeats);
        assert_eq!(outcome.reason, ContinuationReason::DealerLoss);
    }

    /// 流し満貫は荒牌平局と別の理由になる。
    #[test]
    fn a_nagashi_reports_its_own_reason() {
        let mut engine = start_at(0);
        engine.drain_events();
        clear_nagashi(engine.state_mut());
        drain_the_wall(engine.state_mut());
        engine.state_mut().seat_mut(Seat::new(2)).nagashi_alive = true;
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
        engine.tick(WAY_PAST_ANY_DEADLINE_MS);
        engine.drain_events();
        assert_eq!(
            engine.outcome().expect("終わっている").reason,
            ContinuationReason::NagashiMangan
        );
    }

    /// ダブロンを認めないルールでは頭ハネになる。
    /// 放銃者から下家回りで最も近い1人だけが和了する。
    #[test]
    fn head_bump_keeps_only_the_nearest_winner() {
        let mut engine = RoundEngine::start(
            Ruleset {
                double_ron: false,
                ..rules()
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
        // 席1と席2の両方が 6p でロンできる形にする。
        make_tenpai(engine.state_mut(), Seat::new(1));
        make_tenpai(engine.state_mut(), Seat::new(2));
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
        for seat in [Seat::new(2), Seat::new(1)] {
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
        engine.tick(1_400);

        let events = engine.drain_events();
        let Some(Event::Agari { results, .. }) = events
            .iter()
            .find(|e| matches!(e, Event::Agari { .. }))
            .cloned()
        else {
            panic!("Agari が出ていない: {events:?}");
        };
        assert_eq!(results.len(), 1, "頭ハネなので1人だけ");
        assert_eq!(
            results[0].seat,
            Seat::new(1),
            "放銃者 席0 に最も近いのは席1"
        );

        // 頭ハネで負けた席2はロンを宣言している。見逃してはいない。
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::ActionPassed { seat, .. } if *seat == Seat::new(2))),
            "頭ハネで負けた席を見逃し扱いにしている: {events:?}"
        );
        assert!(
            engine
                .state()
                .seat(Seat::new(2))
                .passed_this_turn
                .is_empty(),
            "頭ハネで負けた席に同巡内フリテンを付けている"
        );
    }

    /// 自分の締切を過ぎた応答は、tick を待たずに拒否される。
    /// 受理するかどうかが tick の呼び出し順で変わってはならない。
    #[test]
    fn a_response_after_its_own_deadline_is_rejected() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(engine.state_mut(), Seat::new(1));
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
        let events = engine.drain_events();
        let Some(Event::RequestAction {
            seat,
            window_id,
            deadline_ms,
            ..
        }) = events
            .iter()
            .find(|e| matches!(e, Event::RequestAction { .. }))
            .cloned()
        else {
            panic!("反応の要求が出ていない: {events:?}");
        };
        assert_eq!(seat, Seat::new(1));

        // 締切の1ミリ秒あと。tick は呼ばない。
        let too_late = 1_000 + deadline_ms as u64 + 1;
        assert_eq!(
            engine.apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Ron,
                },
                too_late
            ),
            Err(Reject::StaleWindow)
        );
        assert_eq!(
            engine.state().seat(Seat::new(1)).think_bank_ms,
            0,
            "時間切れはバンクを使い切る"
        );

        // **拒否しただけで止まってはならない。**締切の反映でウィンドウは
        // 確定しており、tick を呼んだ場合と同じところまで進んでいる。
        let events = engine.drain_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Draw { seat, .. } if *seat == Seat::new(1))),
            "次のツモまで進んでいない: {events:?}"
        );
        assert!(matches!(engine.phase(), Phase::Turn { .. }));
    }

    /// 別のウィンドウ宛の応答は受け付けない。
    #[test]
    fn a_response_for_another_window_is_rejected() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(engine.state_mut(), Seat::new(1));
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
        assert_ne!(window_id, 99);
        assert_eq!(
            engine.apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id: 99,
                    response: CallResponse::Ron,
                },
                1_200
            ),
            Err(Reject::StaleWindow)
        );
        // 拒否でウィンドウが壊れていないこと。正しい id なら受理される。
        assert_eq!(
            engine.apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Ron,
                },
                1_300
            ),
            Ok(())
        );
    }

    /// 見逃した側にも ActionPassed が出る。和了しても記録は残す。
    #[test]
    fn a_declined_ron_is_recorded_even_when_someone_else_wins() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(engine.state_mut(), Seat::new(1));
        make_tenpai(engine.state_mut(), Seat::new(2));
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
                Seat::new(2),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pass,
                },
                1_200,
            )
            .expect("見逃せる");
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
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::ActionPassed { seat, .. } if *seat == Seat::new(2))),
            "見逃した席の記録が失われている: {events:?}"
        );
    }

    /// 局の途中から採番を始めても、未使用の id は受け付けない。
    ///
    /// window_id は半荘を通して単調増加するので、東1局以外では 1 から
    /// 始まらない。ここではウィンドウが開いているので `window.id()` との
    /// 不一致で `StaleWindow` になる。`first_window_id` を持つ意味は、
    /// ウィンドウが閉じたあとに同じ id が来たとき `NoWindow` と区別する
    /// ためであり、その判定の存在をこのテストで固定しておく。
    #[test]
    fn an_unissued_id_is_not_accepted() {
        let mut engine = RoundEngine::start(
            rules(),
            Round {
                wind: protocol::seat::Wind::East,
                number: 1,
            },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &super::start_tests::seed(),
            100,
            0,
        );
        engine.drain_events();
        make_tenpai(engine.state_mut(), Seat::new(1));
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

        // 1 はこの局では一度も採番していない。
        assert_eq!(
            engine.apply(
                Seat::new(1),
                Command::CallResponse {
                    window_id: 1,
                    response: CallResponse::Ron,
                },
                1_200
            ),
            Err(Reject::StaleWindow),
            "開いているウィンドウとの id 不一致なので Stale"
        );
    }

    /// 局が終わったあとはコマンドを受け付けない。
    #[test]
    fn a_finished_round_rejects_further_commands() {
        let mut engine = start_at(0);
        engine.drain_events();
        make_tenpai(engine.state_mut(), Seat::new(1));
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

    /// 牌の種類ごとの在庫。自然に進めた局では各種4枚のままである。
    ///
    /// `assert_tiles_conserved` は枚数の合計しか見ないので、局面を直接
    /// 差し替えるテストでは在庫が崩れる。ここは配牌から一度も差し替えずに
    /// 進めるので、種類まで検査できる。
    fn assert_every_kind_has_four(state: &crate::state::RoundState) {
        let mut held: Vec<Tile> = Vec::with_capacity(136);
        for seat in Seat::ALL {
            let s = state.seat(seat);
            held.extend(s.hand.iter().copied());
            for meld in &s.melds {
                held.extend(meld.tiles.iter().copied());
            }
            held.extend(
                s.river
                    .iter()
                    .filter(|d| d.called_by.is_none())
                    .map(|d| d.tile),
            );
        }
        held.extend(state.wall.tiles_in_wall());

        let mut counts = [0u8; 34];
        for tile in &held {
            counts[tile.kind().index() as usize] += 1;
        }
        for (index, count) in counts.iter().enumerate() {
            assert_eq!(*count, 4, "牌種 {index} が {count} 枚になっている");
        }
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
            assert_every_kind_has_four(engine.state());
            now += 100_000;
        }
        assert_eq!(*engine.phase(), Phase::Done, "局が終わらなかった");
        assert_eq!(
            engine
                .outcome()
                .expect("終わっている")
                .scores
                .iter()
                .sum::<i32>()
                + engine.outcome().expect("終わっている").riichi_sticks as i32 * 1_000,
            100_000
        );
    }
}
