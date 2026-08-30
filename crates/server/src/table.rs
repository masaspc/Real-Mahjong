//! 1つの卓。`MatchEngine` を持ち、イベントに連番を振って席ごとに配る。
//!
//! **非同期も実時間も持たない。**時刻もシードも外から受け取る。
//! tokio の task で包むのは Wave 3c の仕事である。

use mahjong_ai::call;
use mahjong_ai::discard::{self, View};
use mahjong_engine::match_flow::{MatchEngine, Reject};
use mahjong_engine::state::RoundState;
use mahjong_engine::wall::Seed;
use protocol::client_event::ClientEventEnvelope;
use protocol::command::{ActionOption, Command};
use protocol::event::{Event, EventEnvelope, PlayerId};
use protocol::project::project_envelope;
use protocol::ruleset::Ruleset;
use protocol::seat::Seat;

/// 席にいるのが人か CPU か。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Occupant {
    Human(PlayerId),
    Cpu(PlayerId),
}

impl Occupant {
    pub fn player_id(&self) -> PlayerId {
        match self {
            Occupant::Human(id) | Occupant::Cpu(id) => id.clone(),
        }
    }

    fn is_cpu(&self) -> bool {
        matches!(self, Occupant::Cpu(_))
    }
}

/// CPU へ渡す `View` を組み立てる。
///
/// **その席の分だけを読む。**手牌と副露は `seat` のものに限り、
/// 裏ドラは触れない。ここを誤ると CPU が他家の手を見られる。
fn build_view(state: &RoundState, seat: Seat) -> View {
    View {
        seat,
        seat_wind: state.seat_wind(seat),
        round_wind: state.round.wind,
        hand: state.seat(seat).hand.clone(),
        melds: state.seat(seat).melds.clone(),
        rivers: std::array::from_fn(|i| {
            state
                .seat(Seat::new(i as u8))
                .river
                .iter()
                .map(|d| d.tile)
                .collect()
        }),
        riichi: std::array::from_fn(|i| {
            matches!(
                &state.seat(Seat::new(i as u8)).riichi,
                Some(r) if r.step == protocol::event::RiichiStep::Accepted
            )
        }),
        dora_indicators: state.wall.dora_indicators().to_vec(),
        wall_remaining: state.wall.live_remaining(),
        scores: state.scores,
    }
}

/// 応答を待っている要求。CPU が答えるのに要る分だけを持つ。
struct PendingRequest {
    window_id: u32,
    options: Vec<ActionOption>,
}

pub struct Table {
    engine: MatchEngine,
    /// 席にいるのが人か CPU か。CPU の席だけを卓が代打ちする。
    occupants: [Occupant; 4],
    /// 席ごとの、まだ答えていない要求。
    outstanding: [Option<PendingRequest>; 4],
    /// 卓が出した真実。再接続の再送に使う。
    log: Vec<EventEnvelope>,
    next_seq: u32,
    /// 席ごとの、まだ取り出されていない分。`log` への添字を持つ。
    pending: [Vec<usize>; 4],
}

impl Table {
    pub fn new(rules: Ruleset, occupants: [Occupant; 4], now_ms: u64) -> Self {
        let players = std::array::from_fn(|i| occupants[i].player_id());
        let mut table = Table {
            engine: MatchEngine::start(rules, players, now_ms),
            occupants,
            outstanding: std::array::from_fn(|_| None),
            log: Vec::new(),
            next_seq: 0,
            pending: std::array::from_fn(|_| Vec::new()),
        };
        table.collect(now_ms);
        table
    }

    pub fn is_over(&self) -> bool {
        self.engine.is_over()
    }

    pub fn needs_seed(&self) -> bool {
        self.engine.needs_seed()
    }

    /// 動いている局の状態。卓は全席の手牌を持つ。
    /// **ここから CPU へ渡すものは `View` に詰め直す。**
    pub fn round_state(&self) -> Option<&RoundState> {
        self.engine.round_state()
    }

    pub fn begin_round(&mut self, seed: &Seed, now_ms: u64) {
        self.engine.begin_round(seed, now_ms);
        self.collect(now_ms);
    }

    pub fn apply(&mut self, seat: Seat, command: Command, now_ms: u64) -> Result<(), Reject> {
        self.outstanding[seat.index()] = None;
        let result = self.engine.apply(seat, command, now_ms);
        self.collect(now_ms);
        result
    }

    pub fn tick(&mut self, now_ms: u64) {
        self.engine.tick(now_ms);
        self.collect(now_ms);
    }

    /// その席へまだ届けていない分を取り出す。
    /// まだ書き出していないぶんの真実。**牌譜はここから取る。**
    ///
    /// 射影しない。保存するのはサーバの真実であって、席ごとの見え方では
    /// ない。読み出すときに `project()` を通す。
    pub fn log_from(&self, from: usize) -> &[EventEnvelope] {
        &self.log[from.min(self.log.len())..]
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    pub fn drain_for(&mut self, seat: Seat) -> Vec<ClientEventEnvelope> {
        std::mem::take(&mut self.pending[seat.index()])
            .into_iter()
            .filter_map(|index| project_envelope(&self.log[index], seat))
            .collect()
    }

    /// その連番より後を、視界フィルタを通して返す。
    ///
    /// **卓の状態を変えない。**何度呼んでも同じものが返る。
    /// `None` なら最初から全部返す。
    pub fn since(&self, seat: Seat, last_seq: Option<u32>) -> Vec<ClientEventEnvelope> {
        self.log
            .iter()
            .filter(|envelope| last_seq.is_none_or(|last| envelope.seq > last))
            .filter_map(|envelope| project_envelope(envelope, seat))
            .collect()
    }

    /// 局のイベントを取り込み、連番を振って席ごとの待ち行列へ入れる。
    ///
    /// **射影はここでは行わない。**`drain_for` まで遅らせることで、
    /// `log` には真実だけが残り、再接続の再送でも同じ経路を通る。
    fn collect(&mut self, now_ms: u64) {
        self.take_events();
        self.let_cpus_act(now_ms);
    }

    /// 局のイベントを取り込み、連番を振り、要求を控える。
    fn take_events(&mut self) {
        for event in self.engine.drain_events() {
            if let Event::RequestAction {
                seat,
                window_id,
                options,
                ..
            } = &event
            {
                self.outstanding[seat.index()] = Some(PendingRequest {
                    window_id: *window_id,
                    options: options.clone(),
                });
            }
            let index = self.log.len();
            self.log.push(EventEnvelope {
                seq: self.next_seq,
                event,
            });
            self.next_seq += 1;
            for queue in &mut self.pending {
                queue.push(index);
            }
        }
    }

    /// CPU の席へ出た要求を、その場で処理する。
    ///
    /// **時計は進めない。**反応ウィンドウの最低待機を越えさせるのは、
    /// 呼び出し側の `tick` である。
    fn let_cpus_act(&mut self, now_ms: u64) {
        for _ in 0..1_000 {
            let Some(seat) = self.next_cpu_to_act() else {
                return;
            };
            let request = self.outstanding[seat.index()]
                .take()
                .expect("直前に確認した");
            let Some(state) = self.engine.round_state() else {
                return;
            };
            let view = build_view(state, seat);
            let command = if request
                .options
                .iter()
                .any(|o| matches!(o, ActionOption::Discard { .. }))
            {
                discard::choose(&view, &request.options)
            } else {
                Command::CallResponse {
                    window_id: request.window_id,
                    response: call::respond(&view, &request.options),
                }
            };
            let _ = self.engine.apply(seat, command, now_ms);
            self.take_events();
        }
        panic!("CPU の応答が終わらない");
    }

    /// まだ答えていない CPU の席。席順で先にあるものを返す。
    fn next_cpu_to_act(&self) -> Option<Seat> {
        Seat::ALL.into_iter().find(|seat| {
            self.occupants[seat.index()].is_cpu() && self.outstanding[seat.index()].is_some()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::client_event::ClientEvent;
    use protocol::ruleset::MatchLength;

    pub(super) fn seed_of(index: u8) -> Seed {
        Seed::from_hex(&format!("{index:02x}").repeat(32)).expect("正しい hex")
    }

    pub(super) fn humans() -> [Occupant; 4] {
        std::array::from_fn(|i| Occupant::Human(PlayerId(format!("p{i}"))))
    }

    pub(super) fn table_of(occupants: [Occupant; 4]) -> Table {
        Table::new(Ruleset::kin_no_ma(MatchLength::Hanchan), occupants, 0)
    }

    #[test]
    fn a_new_table_announces_itself_to_everyone() {
        let mut table = table_of(humans());
        for seat in Seat::ALL {
            let events = table.drain_for(seat);
            assert_eq!(events.len(), 1);
            assert!(matches!(events[0].event, ClientEvent::MatchStart { .. }));
        }
    }

    #[test]
    fn the_sequence_starts_at_zero() {
        let mut table = table_of(humans());
        let events = table.drain_for(Seat::new(0));
        assert_eq!(events[0].seq, 0);
    }

    #[test]
    fn each_seat_learns_which_one_it_is() {
        let mut table = table_of(humans());
        for seat in Seat::ALL {
            let events = table.drain_for(seat);
            let ClientEvent::MatchStart { you, .. } = events[0].event else {
                panic!("MatchStart でない");
            };
            assert_eq!(you, seat);
        }
    }

    #[test]
    fn draining_twice_yields_nothing_the_second_time() {
        let mut table = table_of(humans());
        assert!(!table.drain_for(Seat::new(0)).is_empty());
        assert!(table.drain_for(Seat::new(0)).is_empty());
    }

    #[test]
    fn each_seat_has_its_own_queue() {
        let mut table = table_of(humans());
        table.drain_for(Seat::new(0));
        assert!(!table.drain_for(Seat::new(1)).is_empty());
    }

    #[test]
    fn giving_a_seed_starts_the_round() {
        let mut table = table_of(humans());
        assert!(table.needs_seed());
        table.begin_round(&seed_of(1), 0);
        assert!(!table.needs_seed());
        let events = table.drain_for(Seat::new(0));
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::RoundStart { .. })));
    }

    #[test]
    fn only_the_drawer_sees_the_drawn_tile() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        let own = table.drain_for(Seat::new(0));
        let Some(ClientEvent::Draw { tile, .. }) = own.iter().find_map(|e| match e.event {
            ClientEvent::Draw { .. } => Some(e.event.clone()),
            _ => None,
        }) else {
            panic!("親のツモが見えていない");
        };
        assert!(tile.is_some(), "自分のツモ牌は見える");
        let other = table.drain_for(Seat::new(1));
        let Some(ClientEvent::Draw { tile, .. }) = other.iter().find_map(|e| match e.event {
            ClientEvent::Draw { .. } => Some(e.event.clone()),
            _ => None,
        }) else {
            panic!("他家にもツモの事実は見える");
        };
        assert_eq!(tile, None, "他家のツモ牌は見えない");
    }

    #[test]
    fn the_deal_shows_only_your_own_hand() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        let events = table.drain_for(Seat::new(2));
        let Some(ClientEvent::Deal {
            your_hand,
            hand_sizes,
            ..
        }) = events.iter().find_map(|e| match &e.event {
            ClientEvent::Deal { .. } => Some(e.event.clone()),
            _ => None,
        })
        else {
            panic!("配牌が届いていない");
        };
        assert_eq!(your_hand.len(), 13, "自分の手牌だけが見える");
        assert_eq!(hand_sizes, [13; 4], "他家は枚数しか見えない");
    }

    #[test]
    fn a_request_reaches_only_its_seat() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        for seat in Seat::ALL {
            let events = table.drain_for(seat);
            let requested = events
                .iter()
                .any(|e| matches!(e.event, ClientEvent::RequestAction { .. }));
            assert_eq!(requested, seat == Seat::new(0), "{seat:?}");
        }
    }

    #[test]
    fn commands_reach_the_engine() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        table.drain_for(Seat::new(0));
        let tile = table
            .round_state()
            .expect("局が動いている")
            .seat(Seat::new(0))
            .hand[0];
        table
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile,
                    riichi: false,
                },
                1_000,
            )
            .expect("切れる");
        let events = table.drain_for(Seat::new(1));
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::Discard { .. })));
    }

    #[test]
    fn the_same_event_carries_the_same_sequence_everywhere() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        let a = table.drain_for(Seat::new(1));
        let b = table.drain_for(Seat::new(2));
        assert_eq!(a[0].seq, b[0].seq);
    }

    #[test]
    fn the_same_input_gives_the_same_output() {
        let build = || {
            let mut table = table_of(humans());
            table.begin_round(&seed_of(1), 0);
            table.drain_for(Seat::new(0))
        };
        assert_eq!(build(), build());
    }
}

#[cfg(test)]
mod cpu_tests {
    use super::tests::{seed_of, table_of};
    use super::*;
    use protocol::client_event::ClientEvent;

    fn advance(table: &mut Table, now: &mut u64) {
        *now += 1_000_000;
        table.tick(*now);
    }

    pub(super) fn all_cpu() -> [Occupant; 4] {
        std::array::from_fn(|i| Occupant::Cpu(PlayerId(format!("cpu{i}"))))
    }

    fn mixed() -> [Occupant; 4] {
        [
            Occupant::Human(PlayerId("human".to_owned())),
            Occupant::Cpu(PlayerId("cpu1".to_owned())),
            Occupant::Cpu(PlayerId("cpu2".to_owned())),
            Occupant::Cpu(PlayerId("cpu3".to_owned())),
        ]
    }

    #[test]
    fn the_view_carries_only_its_own_hand() {
        let mut table = table_of(all_cpu());
        table.begin_round(&seed_of(1), 0);
        let state = table.round_state().expect("局が動いている");
        let view = build_view(state, Seat::new(2));

        assert_eq!(view.hand, state.seat(Seat::new(2)).hand);
        for other in [Seat::new(0), Seat::new(1), Seat::new(3)] {
            for tile in &state.seat(other).hand {
                assert!(
                    !view.hand.contains(tile) || state.seat(Seat::new(2)).hand.contains(tile),
                    "他家の手牌が混ざっている"
                );
            }
        }
    }

    #[test]
    fn the_view_never_carries_the_ura_indicators() {
        let mut table = table_of(all_cpu());
        table.begin_round(&seed_of(1), 0);
        let state = table.round_state().expect("局が動いている");
        let view = build_view(state, Seat::new(0));
        assert_eq!(view.dora_indicators, state.wall.dora_indicators().to_vec());
        assert_eq!(view.dora_indicators.len(), 1, "局の頭は1枚だけ");
    }

    #[test]
    fn the_view_carries_every_river() {
        let mut table = table_of(all_cpu());
        table.begin_round(&seed_of(1), 0);
        let state = table.round_state().expect("局が動いている");
        let view = build_view(state, Seat::new(0));
        assert_eq!(view.rivers.len(), 4);
    }

    #[test]
    fn a_cpu_seat_acts_on_its_own() {
        let mut table = table_of(all_cpu());
        table.begin_round(&seed_of(1), 0);
        let events = table.drain_for(Seat::new(1));
        assert!(
            events
                .iter()
                .any(|e| matches!(e.event, ClientEvent::Discard { .. })),
            "CPU が打っていない: {events:?}"
        );
    }

    #[test]
    fn a_human_seat_is_left_alone() {
        let mut table = table_of(mixed());
        table.begin_round(&seed_of(1), 0);
        let events = table.drain_for(Seat::new(0));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e.event, ClientEvent::Discard { .. })),
            "人の席で勝手に打っている"
        );
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::RequestAction { .. })));
    }

    #[test]
    fn the_cpus_continue_after_a_human_move() {
        let mut table = table_of(mixed());
        let mut now = 0u64;
        table.begin_round(&seed_of(1), now);
        for seat in Seat::ALL {
            table.drain_for(seat);
        }
        let tile = table
            .round_state()
            .expect("局が動いている")
            .seat(Seat::new(0))
            .hand[0];
        now += 1_000;
        table
            .apply(
                Seat::new(0),
                Command::Discard {
                    tile,
                    riichi: false,
                },
                now,
            )
            .expect("切れる");
        for _ in 0..3 {
            advance(&mut table, &mut now);
        }
        let events = table.drain_for(Seat::new(0));
        let discards = events
            .iter()
            .filter(|e| matches!(e.event, ClientEvent::Discard { .. }))
            .count();
        assert!(discards >= 2, "CPU が続いていない: {discards}");
    }

    #[test]
    fn a_reaction_window_waits_for_its_minimum() {
        let mut table = table_of(all_cpu());
        table.begin_round(&seed_of(1), 0);
        let events = table.drain_for(Seat::new(0));
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::Discard { .. })));
        let draws = events
            .iter()
            .filter(|e| matches!(e.event, ClientEvent::Draw { .. }))
            .count();
        assert_eq!(draws, 1, "反応が確定する前に次のツモが出ている");
    }

    #[test]
    fn a_cpu_answers_reaction_windows() {
        let mut table = table_of(all_cpu());
        let mut now = 0u64;
        table.begin_round(&seed_of(1), now);
        table.drain_for(Seat::new(0));
        advance(&mut table, &mut now);
        let events = table.drain_for(Seat::new(0));
        assert!(
            events
                .iter()
                .any(|e| matches!(e.event, ClientEvent::Draw { .. })),
            "反応が解決していない: {events:?}"
        );
    }

    #[test]
    fn a_cpu_table_is_reproducible() {
        let build = || {
            let mut table = table_of(all_cpu());
            let mut now = 0u64;
            table.begin_round(&seed_of(1), now);
            for _ in 0..5 {
                advance(&mut table, &mut now);
            }
            table.drain_for(Seat::new(0))
        };
        assert_eq!(build(), build());
    }
}

#[cfg(test)]
mod resume_tests {
    use super::cpu_tests::all_cpu;
    use super::tests::{humans, seed_of, table_of};
    use super::*;
    use protocol::client_event::ClientEvent;

    #[test]
    fn a_first_connection_gets_everything() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        table.drain_for(Seat::new(0));
        let all = table.since(Seat::new(0), None);
        assert!(matches!(all[0].event, ClientEvent::MatchStart { .. }));
        assert_eq!(all[0].seq, 0);
    }

    #[test]
    fn a_resume_sends_only_what_came_after() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        let seen = table.drain_for(Seat::new(0));
        let last = seen.last().expect("何か届いている").seq;
        assert!(table.since(Seat::new(0), Some(last)).is_empty());
    }

    #[test]
    fn a_resume_is_filtered_the_same_way() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        let live = table.drain_for(Seat::new(1));
        let resent = table.since(Seat::new(1), None);
        assert_eq!(live, resent, "生と再送で内容が変わってはならない");
    }

    #[test]
    fn a_resume_does_not_advance_anything() {
        let mut table = table_of(humans());
        table.begin_round(&seed_of(1), 0);
        let before = table.since(Seat::new(0), None);
        let after = table.since(Seat::new(0), None);
        assert_eq!(before, after);
    }

    #[test]
    fn four_cpus_finish_a_whole_match() {
        let mut table = table_of(all_cpu());
        let mut now = 1_000u64;
        let mut seed_index = 1u8;
        for _ in 0..5_000 {
            if table.is_over() {
                break;
            }
            if table.needs_seed() {
                table.begin_round(&seed_of(seed_index), now);
                seed_index = seed_index.wrapping_add(1);
                continue;
            }
            now += 1_000_000;
            table.tick(now);
        }
        assert!(table.is_over(), "半荘が終わらなかった");
        let events = table.since(Seat::new(0), None);
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::MatchEnd { .. })));
    }

    #[test]
    fn the_sequence_never_goes_backwards() {
        let mut table = table_of(all_cpu());
        let mut now = 1_000u64;
        let mut seed_index = 1u8;
        for _ in 0..5_000 {
            if table.is_over() {
                break;
            }
            if table.needs_seed() {
                table.begin_round(&seed_of(seed_index), now);
                seed_index = seed_index.wrapping_add(1);
                continue;
            }
            now += 1_000_000;
            table.tick(now);
        }
        assert!(table.is_over(), "半荘が終わらなかった");
        let events = table.since(Seat::new(0), None);
        for pair in events.windows(2) {
            assert!(pair[0].seq < pair[1].seq, "連番が戻っている");
        }
    }

    #[test]
    fn a_finished_table_takes_no_more_commands() {
        let mut table = table_of(all_cpu());
        let mut now = 1_000u64;
        let mut seed_index = 1u8;
        for _ in 0..5_000 {
            if table.is_over() {
                break;
            }
            if table.needs_seed() {
                table.begin_round(&seed_of(seed_index), now);
                seed_index = seed_index.wrapping_add(1);
                continue;
            }
            now += 1_000_000;
            table.tick(now);
        }
        assert!(table.is_over(), "半荘が終わらなかった");
        assert!(table.apply(Seat::new(0), Command::Tsumo, now).is_err());
    }
}
