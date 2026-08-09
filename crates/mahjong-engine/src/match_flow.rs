//! 局の進行。時刻も乱数も外から受け取る。
//!
//! 同じシード・同じコマンド列・同じ時刻列からは、必ず同じイベント列が出る。

use crate::invariant;
use crate::reaction::{Outcome, ReactionWindow, Rejection, WindowKind};
use crate::round::{discard_options, reaction_options, TurnStart};
use crate::state::{charge_bank, deadline_for, lead_in_of, remaining_for_event, RoundState};
use crate::wall::Seed;
use protocol::command::{ActionOption, CallResponse, Command};
use protocol::event::{DiscardManner, DrawSource, Event, RiichiStep};
use protocol::ruleset::Ruleset;
use protocol::seat::{Round, Seat};
use protocol::tile::Tile;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reject {
    NotYourTurn,
    NotOffered,
    NoWindow,
    StaleWindow,
    Window(Rejection),
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
            Command::CallResponse {
                window_id,
                response,
            } => {
                let accepted = self.accept_response(seat, window_id, response, now_ms);
                self.resolve_window(now_ms);
                accepted
            }
            _ => Err(Reject::NotOffered),
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
            Outcome::PassAll => {
                let from = window.from();
                self.window = None;
                self.record_passes(&[]);
                self.advance_after_pass(from, now_ms);
            }
            Outcome::Call { .. } | Outcome::Ron(_) => {
                unimplemented!("鳴きと和了は Task 3 / Task 4 で実装する")
            }
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
        if self.state.wall.live_remaining() == 0 {
            unimplemented!("荒牌平局は Task 4 で実装する")
        }
        let next = Seat::new(((from.index() + 1) % 4) as u8);
        self.draw_for(next, DrawSource::Wall);
        self.request_turn(now_ms);
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
