//! 1つの卓。`MatchEngine` を持ち、イベントに連番を振って席ごとに配る。
//!
//! **非同期も実時間も持たない。**時刻もシードも外から受け取る。
//! tokio の task で包むのは Wave 3c の仕事である。

use mahjong_engine::match_flow::{MatchEngine, Reject};
use mahjong_engine::state::RoundState;
use mahjong_engine::wall::Seed;
use protocol::client_event::ClientEventEnvelope;
use protocol::command::Command;
use protocol::event::{EventEnvelope, PlayerId};
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
    fn player_id(&self) -> PlayerId {
        match self {
            Occupant::Human(id) | Occupant::Cpu(id) => id.clone(),
        }
    }
}

pub struct Table {
    engine: MatchEngine,
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
            log: Vec::new(),
            next_seq: 0,
            pending: std::array::from_fn(|_| Vec::new()),
        };
        table.collect();
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
        self.collect();
    }

    pub fn apply(&mut self, seat: Seat, command: Command, now_ms: u64) -> Result<(), Reject> {
        let result = self.engine.apply(seat, command, now_ms);
        self.collect();
        result
    }

    pub fn tick(&mut self, now_ms: u64) {
        self.engine.tick(now_ms);
        self.collect();
    }

    /// その席へまだ届けていない分を取り出す。
    pub fn drain_for(&mut self, seat: Seat) -> Vec<ClientEventEnvelope> {
        std::mem::take(&mut self.pending[seat.index()])
            .into_iter()
            .filter_map(|index| project_envelope(&self.log[index], seat))
            .collect()
    }

    /// 局のイベントを取り込み、連番を振って席ごとの待ち行列へ入れる。
    ///
    /// **射影はここでは行わない。**`drain_for` まで遅らせることで、
    /// `log` には真実だけが残り、再接続の再送でも同じ経路を通る。
    fn collect(&mut self) {
        for event in self.engine.drain_events() {
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
