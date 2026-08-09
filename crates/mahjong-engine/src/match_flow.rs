//! 局の進行。時刻も乱数も外から受け取る。
//!
//! 同じシード・同じコマンド列・同じ時刻列からは、必ず同じイベント列が出る。

use crate::invariant;
use crate::round::{discard_options, TurnStart};
use crate::state::{deadline_for, lead_in_of, remaining_for_event, RoundState};
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
    /// 局が終わった。
    Done,
}

pub struct RoundEngine {
    state: RoundState,
    phase: Phase,
    pending: Vec<Event>,
    since_request: [Vec<Event>; 4],
    next_window_id: u32,
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
