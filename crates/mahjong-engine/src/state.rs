//! 局の状態。進行のステートマシンは持たず、状態と導出だけを担う。
//!
//! 状況役の判定を進行側へ散らさないよう、HandContext の組み立てはここに集約する。

#[path = "timing.rs"]
mod timing;
pub use timing::{charge_bank, deadline_for, lead_in_of, remaining_for_event};

use mahjong_core::hand::HandCounts;
use mahjong_core::score::{HandContext, WinType};
use protocol::event::{DiscardManner, DrawSource, RiichiStep};
use protocol::meld::{Meld, MeldKind};
use protocol::ruleset::Ruleset;
use protocol::seat::{Round, Seat, Wind};
use protocol::tile::{Tile, TileKind};

use crate::wall::{Seed, Wall};

/// リーチ宣言時に供託する点数。リーチ麻雀では普遍の値であり、
/// Ruleset に設定項目として存在しない。
pub const RIICHI_STICK: i32 = 1_000;

/// 河に捨てられた1枚。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Discarded {
    pub tile: Tile,
    pub manner: DiscardManner,
    /// 鳴かれた場合、鳴いた席。**牌の総数を数えるときはこれが Some のものを除く**
    /// （鳴いた者の melds に入っているため）。
    pub called_by: Option<Seat>,
    /// リーチ宣言牌かどうか。横向きに置く演出と、四家立直の判定に使う。
    pub riichi_declaration: bool,
}

/// リーチの状態。宣言と成立を分ける。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RiichiState {
    /// `Declare` は宣言しただけ。`Accepted` で初めて役になり供託が出る。
    pub step: RiichiStep,
    pub declared_at_turn: u32,
    pub ippatsu: bool,
    pub double: bool,
}

/// 槍槓の受付中である槓。成立するまでここに置く。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PendingKan {
    pub seat: Seat,
    pub kind: MeldKind,
    pub tile: Tile,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeatState {
    pub hand: Vec<Tile>,
    pub melds: Vec<Meld>,
    pub river: Vec<Discarded>,
    pub riichi: Option<RiichiState>,
    pub think_bank_ms: u32,
    /// 同巡内フリテン。自分のツモで解除される。
    pub passed_this_turn: Vec<TileKind>,
    /// リーチ後にロンを見逃した待ち。**局の終わりまで解除されない。**
    pub permanent_furiten: Vec<TileKind>,
    /// 自分の捨て牌がすべて幺九牌で、一度も鳴かれていないか（流し満貫）。
    pub nagashi_alive: bool,
}

pub struct RoundState {
    pub rules: Ruleset,
    pub round: Round,
    pub dealer: Seat,
    pub honba: u8,
    pub riichi_sticks: u8,
    pub scores: [i32; 4],
    pub wall: Wall,
    pub seats: [SeatState; 4],
    /// 直前のツモを引いた席と、その出どころ。嶺上開花の判定に使う。
    /// 席を持たせるのは、誰のツモだったかを取り違えないためである。
    pub last_draw: Option<(Seat, DrawSource)>,
    /// 各席が何回ツモしたか。天和・地和の判定に使う。
    pub draw_count: [u32; 4],
    /// 局を通して誰か1人でも鳴いたか。
    pub any_call_made: bool,
    /// 槍槓の受付中かどうか。
    pub pending_kan: Option<PendingKan>,
    /// 席ごとの確定した槓の数。四開槓の判定に使う。
    pub kan_count: [u8; 4],
    /// 1巡目に切られた風牌。四風連打の判定に使う。
    pub first_turn_winds: Vec<TileKind>,
}

impl RoundState {
    pub fn new(
        rules: Ruleset,
        round: Round,
        dealer: Seat,
        honba: u8,
        riichi_sticks: u8,
        scores: [i32; 4],
        seed: &Seed,
    ) -> Self {
        let mut wall = Wall::new(seed, &rules);
        let bank = rules.think_bank_ms;

        let seats = std::array::from_fn(|_| {
            let hand = (0..13)
                .map(|_| wall.draw().expect("配牌の分は必ずある"))
                .collect();
            SeatState {
                hand,
                melds: Vec::new(),
                river: Vec::new(),
                riichi: None,
                think_bank_ms: bank,
                passed_this_turn: Vec::new(),
                permanent_furiten: Vec::new(),
                nagashi_alive: true,
            }
        });

        RoundState {
            rules,
            round,
            dealer,
            honba,
            riichi_sticks,
            scores,
            wall,
            seats,
            last_draw: None,
            draw_count: [0; 4],
            any_call_made: false,
            pending_kan: None,
            kan_count: [0; 4],
            first_turn_winds: Vec::new(),
        }
    }

    pub fn seat(&self, seat: Seat) -> &SeatState {
        &self.seats[seat.index()]
    }

    pub fn seat_mut(&mut self, seat: Seat) -> &mut SeatState {
        &mut self.seats[seat.index()]
    }

    pub fn hand_counts(&self, seat: Seat) -> HandCounts {
        HandCounts::from_tiles(&self.seat(seat).hand)
    }

    /// 暗槓は門前を崩さない。
    pub fn is_menzen(&self, seat: Seat) -> bool {
        self.seat(seat).melds.iter().all(|m| m.is_concealed())
    }

    /// **2. 自風は親からの距離で決まる。** 親が東、その下家が南。
    pub fn seat_wind(&self, seat: Seat) -> Wind {
        let offset = (seat.index() + 4 - self.dealer.index()) % 4;
        match offset {
            0 => Wind::East,
            1 => Wind::South,
            2 => Wind::West,
            _ => Wind::North,
        }
    }

    /// **3. `hand_context` は表のとおりに組み立てる。**
    /// 状況役の判定を進行側へ散らさず、ここに集約する。
    pub fn hand_context(&self, seat: Seat, win_type: WinType) -> HandContext {
        let is_tsumo = win_type == WinType::Tsumo;
        let riichi = self
            .seat(seat)
            .riichi
            .as_ref()
            .filter(|r| r.step == RiichiStep::Accepted);
        let exhausted = self.wall.live_remaining() == 0;
        let first_draw_untouched = self.draw_count[seat.index()] == 1 && !self.any_call_made;

        HandContext {
            win_type,
            seat_wind: self.seat_wind(seat),
            round_wind: self.round.wind,
            riichi: riichi.is_some(),
            double_riichi: riichi.map(|r| r.double).unwrap_or(false),
            ippatsu: riichi.map(|r| r.ippatsu).unwrap_or(false),
            rinshan: is_tsumo && self.last_draw == Some((seat, DrawSource::DeadWall)),
            chankan: !is_tsumo && self.pending_kan.is_some(),
            haitei: is_tsumo && exhausted,
            houtei: !is_tsumo && exhausted,
            tenhou: is_tsumo && seat == self.dealer && first_draw_untouched,
            chiihou: is_tsumo && seat != self.dealer && first_draw_untouched,
            dora_indicators: self.wall.dora_indicators().to_vec(),
            // 裏ドラはリーチが成立している席にだけ渡す。
            ura_indicators: if riichi.is_some() {
                self.wall.ura_indicators().to_vec()
            } else {
                Vec::new()
            },
        }
    }

    /// 自分のツモ番の始まり。**同巡内フリテンだけを消す。**
    /// 永続フリテン（リーチ後の見逃し）には触らない。
    pub fn begin_turn(&mut self, seat: Seat) {
        self.seats[seat.index()].passed_this_turn.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::wall::Seed;
    use protocol::notation::parse_hand;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::{Round, Seat, Wind};
    use protocol::tile::TileKind;

    fn fresh() -> RoundState {
        RoundState::new(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            Round {
                wind: Wind::East,
                number: 1,
            },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &Seed::from_hex(&"11".repeat(32)).unwrap(),
        )
    }

    #[test]
    fn every_seat_starts_with_thirteen_tiles() {
        let state = fresh();
        for seat in Seat::ALL {
            assert_eq!(state.seat(seat).hand.len(), 13);
        }
    }

    /// 122 − 13×4 = 70
    #[test]
    fn the_wall_loses_exactly_the_dealt_tiles() {
        assert_eq!(fresh().wall.live_remaining(), 70);
    }

    #[test]
    fn seat_winds_follow_the_dealer() {
        let state = fresh();
        assert_eq!(state.seat_wind(Seat::new(0)), Wind::East);
        assert_eq!(state.seat_wind(Seat::new(1)), Wind::South);
        assert_eq!(state.seat_wind(Seat::new(2)), Wind::West);
        assert_eq!(state.seat_wind(Seat::new(3)), Wind::North);
    }

    /// 親が席2なら、席2が東で席3が南。
    #[test]
    fn seat_winds_rotate_with_a_different_dealer() {
        let state = RoundState::new(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            Round {
                wind: Wind::East,
                number: 3,
            },
            Seat::new(2),
            0,
            0,
            [25_000; 4],
            &Seed::from_hex(&"22".repeat(32)).unwrap(),
        );
        assert_eq!(state.seat_wind(Seat::new(2)), Wind::East);
        assert_eq!(state.seat_wind(Seat::new(3)), Wind::South);
        assert_eq!(state.seat_wind(Seat::new(0)), Wind::West);
    }

    #[test]
    fn a_hand_with_no_melds_is_menzen() {
        assert!(fresh().is_menzen(Seat::new(0)));
    }

    #[test]
    fn every_seat_starts_with_a_full_think_bank() {
        let state = fresh();
        for seat in Seat::ALL {
            assert_eq!(state.seat(seat).think_bank_ms, 20_000);
        }
    }

    #[test]
    fn hand_counts_ignore_red_fives() {
        let mut state = fresh();
        state.seat_mut(Seat::new(0)).hand = parse_hand("0p5p").unwrap();
        let counts = state.hand_counts(Seat::new(0));
        assert_eq!(counts.get(TileKind::from_index(13).unwrap()), 2);
    }

    /// リーチ後の見逃しは局の終わりまで解除されない。
    /// 同巡内フリテンだけが自分のツモで消える。
    #[test]
    fn permanent_furiten_survives_the_next_draw() {
        let mut state = fresh();
        let seat = Seat::new(0);
        let kind = protocol::notation::parse_tile("3p").unwrap().kind();
        state.seat_mut(seat).passed_this_turn.push(kind);
        state.seat_mut(seat).permanent_furiten.push(kind);

        state.begin_turn(seat);
        assert!(
            state.seat(seat).passed_this_turn.is_empty(),
            "同巡内は解除される"
        );
        assert_eq!(
            state.seat(seat).permanent_furiten,
            vec![kind],
            "リーチ後の見逃しは残る"
        );
    }

    #[test]
    fn a_fresh_round_has_no_calls_and_no_kans() {
        let state = fresh();
        assert!(!state.any_call_made);
        assert_eq!(state.kan_count, [0; 4]);
        assert!(state.pending_kan.is_none());
        assert!(state.first_turn_winds.is_empty());
    }

    #[test]
    fn every_seat_starts_eligible_for_nagashi() {
        let state = fresh();
        for seat in Seat::ALL {
            assert!(state.seat(seat).nagashi_alive);
        }
    }

    /// 状況役がすべて偽の文脈を作れる。
    #[test]
    fn a_plain_hand_context_has_no_situational_yaku() {
        let state = fresh();
        let ctx = state.hand_context(Seat::new(1), WinType::Ron);
        assert!(!ctx.riichi);
        assert!(!ctx.ippatsu);
        assert!(!ctx.rinshan);
        assert!(!ctx.chankan);
        assert!(!ctx.haitei);
        assert!(!ctx.houtei);
        assert!(!ctx.tenhou);
        assert!(!ctx.chiihou);
        assert_eq!(ctx.seat_wind, Wind::South);
        assert_eq!(ctx.round_wind, Wind::East);
    }

    /// 親の第一ツモは天和の条件を満たす。
    #[test]
    fn the_dealers_first_draw_qualifies_for_tenhou() {
        let mut state = fresh();
        state.draw_count[0] = 1;
        let ctx = state.hand_context(Seat::new(0), WinType::Tsumo);
        assert!(ctx.tenhou);
        assert!(!ctx.chiihou);
    }

    /// 天和・地和はツモ和了に限る。第一巡のロンでは立たない。
    #[test]
    fn tenhou_and_chiihou_require_a_tsumo() {
        let mut state = fresh();
        state.draw_count[0] = 1;
        state.draw_count[1] = 1;
        assert!(!state.hand_context(Seat::new(0), WinType::Ron).tenhou);
        assert!(!state.hand_context(Seat::new(1), WinType::Ron).chiihou);
    }

    /// 子の第一ツモは地和。
    #[test]
    fn a_non_dealers_first_draw_qualifies_for_chiihou() {
        let mut state = fresh();
        state.draw_count[1] = 1;
        let ctx = state.hand_context(Seat::new(1), WinType::Tsumo);
        assert!(ctx.chiihou);
        assert!(!ctx.tenhou);
    }

    /// 誰かが鳴いていれば天和・地和は成立しない。
    #[test]
    fn a_call_disqualifies_tenhou_and_chiihou() {
        let mut state = fresh();
        state.draw_count[0] = 1;
        state.any_call_made = true;
        assert!(!state.hand_context(Seat::new(0), WinType::Tsumo).tenhou);
    }

    /// 嶺上からのツモは rinshan が立つ。
    #[test]
    fn a_dead_wall_draw_sets_rinshan() {
        let mut state = fresh();
        state.last_draw = Some((Seat::new(1), DrawSource::DeadWall));
        assert!(state.hand_context(Seat::new(1), WinType::Tsumo).rinshan);
        // ロンでは立たない
        assert!(!state.hand_context(Seat::new(1), WinType::Ron).rinshan);
        // 別の席のツモでは立たない
        assert!(!state.hand_context(Seat::new(2), WinType::Tsumo).rinshan);
    }

    /// 槍槓の受付中のロンは chankan が立つ。
    #[test]
    fn a_pending_kan_sets_chankan_on_a_ron() {
        let mut state = fresh();
        state.pending_kan = Some(PendingKan {
            seat: Seat::new(0),
            kind: protocol::meld::MeldKind::Kakan,
            tile: protocol::notation::parse_tile("5s").unwrap(),
        });
        assert!(state.hand_context(Seat::new(1), WinType::Ron).chankan);
        assert!(!state.hand_context(Seat::new(1), WinType::Tsumo).chankan);
    }

    /// 裏ドラはリーチが成立している席にだけ渡す。
    #[test]
    fn ura_indicators_are_only_given_to_a_riichi_seat() {
        let mut state = fresh();
        assert!(state
            .hand_context(Seat::new(1), WinType::Ron)
            .ura_indicators
            .is_empty());

        state.seat_mut(Seat::new(1)).riichi = Some(RiichiState {
            step: protocol::event::RiichiStep::Accepted,
            declared_at_turn: 3,
            ippatsu: false,
            double: false,
        });
        assert!(!state
            .hand_context(Seat::new(1), WinType::Ron)
            .ura_indicators
            .is_empty());
    }

    /// 宣言しただけで成立していないリーチは、役にもならず裏ドラも見られない。
    #[test]
    fn a_declared_but_unaccepted_riichi_is_not_yet_a_yaku() {
        let mut state = fresh();
        state.seat_mut(Seat::new(1)).riichi = Some(RiichiState {
            step: protocol::event::RiichiStep::Declare,
            declared_at_turn: 3,
            ippatsu: true,
            double: false,
        });
        let ctx = state.hand_context(Seat::new(1), WinType::Ron);
        assert!(!ctx.riichi);
        assert!(ctx.ura_indicators.is_empty());
    }
}
