//! 1卓 = 1 tokio task の Actor。**唯一 I/O と時間を持つ層。**

#[path = "session_time.rs"]
mod time;

pub use time::{Clock, SeedSource};

use crate::table::{Occupant, Table};
use mahjong_engine::match_flow::Reject;
use mahjong_engine::wall::Seed;
use protocol::client_event::{ClientEvent, ClientEventEnvelope};
use protocol::command::Command;
use protocol::ruleset::Ruleset;
use protocol::seat::Seat;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::MissedTickBehavior;

/// Actor が目を覚ます間隔。
///
/// `Table` は「次の締切はいつか」を教えてくれないので、締切ちょうどで
/// 起きることはできない。基準思考時間は 5,000ms、反応ウィンドウの
/// 最低待機は 350ms なので、100ms の粒度は人には見えない。
const POLL_MS: u64 = 100;

/// 入口の容量。**`tick` の前に片づける件数の上限でもある。**
///
/// 空になるまで回すと、受信で空いたスロットへ待機中の送信者が補充する
/// ので、コマンドが途切れないかぎり `tick` が永久に来ない。
const INBOX: usize = 32;

/// 1接続ぶんの出口の余裕。
///
/// 追いつきぶんはこれとは別に確保するので、ここは生配信のための余裕
/// である。実測では1席あたりの可視イベントが 1,304 / 1,875 / 1,677 件
/// （シード3種）だった。tokio の mpsc は容量ぶんを先に確保しないので、
/// 健全な接続ではこの数を大きくしても費用はかからない。
const OUTBOX: usize = 8_192;

/// 局のシードを配る。
///
/// **Wave 3d はここで永続化してから返す。**局頭で配った `seed_commit` と、
/// あとで開示するシードが食い違うと、プレイヤーが検算したときに
/// **サーバが山を操作したように見える。**不正の疑いに答えるための仕組みが、
/// 逆に不正の証拠を作ってしまう（仕様 8.3）。
///
/// だから「シードを作る」と「局を始める」のあいだに待てる形にしておく。
/// Wave 3c の `SeedSource` は待たずに返すが、契約は同じである。
pub trait Seeds: Send + 'static {
    fn next_seed(&mut self) -> impl std::future::Future<Output = Seed> + Send;
}

impl Seeds for SeedSource {
    async fn next_seed(&mut self) -> Seed {
        SeedSource::next_seed(self)
    }
}

/// 接続へイベントを押し出す口。
///
/// **有界だが、Actor は決して待たない。**`try_send` で押し込み、溢れたら
/// その接続を切る。切られた側は `attach(seat, last_seq)` で追いつける。
/// **遅い接続への対処は、既に持っている再接続の経路そのものである。**
pub type Outbound = mpsc::Sender<ClientEventEnvelope>;
pub type Inbound = mpsc::Receiver<ClientEventEnvelope>;

/// 卓が既に畳まれている。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gone;

/// 席への接続1本を指す。
///
/// **同じ席に2本目が来たら1本目は無効になる。**この ID がないと、
/// 置き換えられた古い接続の切断が、新しい接続を巻き添えにする。
///
/// **中身は非公開。**外から作れないので、Wave 3d で他人の接続 ID を
/// 騙ることができない。卓が配ったものを返すことしかできない。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConnectionId(u64);

pub enum TableMsg {
    Command {
        seat: Seat,
        command: Command,
        at_ms: u64,
        reply: oneshot::Sender<Result<(), Reject>>,
    },
    Attach {
        seat: Seat,
        last_seq: Option<u32>,
        ack: oneshot::Sender<(ConnectionId, Inbound)>,
    },
    Detach {
        seat: Seat,
        connection: ConnectionId,
    },
}

#[derive(Clone)]
pub struct TableHandle {
    tx: mpsc::Sender<TableMsg>,
    clock: Arc<Clock>,
}

impl TableHandle {
    /// 卓が生まれてからの経過ミリ秒。
    ///
    /// **Wave 3d は WebSocket の枠を読んだ直後にこれを呼び、`command` へ渡す。**
    /// 入口の待ち行列に並ぶ前の時刻でなければ、他席の混雑が締切判定に混ざる。
    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    /// 席の操作を送る。`at_ms` は**サーバへ届いた時刻**であり、呼び出し側が
    /// 測る。クライアントが指定するものではない。
    pub async fn command(
        &self,
        seat: Seat,
        command: Command,
        at_ms: u64,
    ) -> Result<Result<(), Reject>, Gone> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(TableMsg::Command {
                seat,
                command,
                at_ms,
                reply,
            })
            .await
            .map_err(|_| Gone)?;
        answer.await.map_err(|_| Gone)
    }

    /// その席の配信を受け取る。
    ///
    /// **受け口は卓が作る。**追いつきぶんの件数を見てから容量を決めるので、
    /// 正当な再接続が容量不足で溢れることが構造的に起きない。
    pub async fn attach(
        &self,
        seat: Seat,
        last_seq: Option<u32>,
    ) -> Result<(ConnectionId, Inbound), Gone> {
        let (ack, done) = oneshot::channel();
        self.tx
            .send(TableMsg::Attach {
                seat,
                last_seq,
                ack,
            })
            .await
            .map_err(|_| Gone)?;
        done.await.map_err(|_| Gone)
    }

    pub async fn detach(&self, seat: Seat, connection: ConnectionId) -> Result<(), Gone> {
        self.tx
            .send(TableMsg::Detach { seat, connection })
            .await
            .map_err(|_| Gone)
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

/// 卓を立ち上げる。
///
/// **`JoinHandle` を返す。**捨てると panic と正常終局が区別できず、どちらも
/// `Gone` に潰れる。Wave 3d の卓の台帳が障害を記録し、局頭から再開するかを
/// 判断するには終了理由が要る（仕様 8.3）。
pub fn spawn<S: Seeds>(
    rules: Ruleset,
    occupants: [Occupant; 4],
    seeds: S,
) -> (TableHandle, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(INBOX);
    let clock = Arc::new(Clock::start());
    let table = Table::new(rules, occupants, clock.now_ms());
    let actor = tokio::spawn(run(table, seeds, Arc::clone(&clock), rx));
    (TableHandle { tx, clock }, actor)
}

struct Sinks {
    out: [Option<Outbound>; 4],
    sent_upto: [Option<u32>; 4],
    live: [Option<ConnectionId>; 4],
    next_id: u64,
    /// 締切を控えたところまで。配信先の有無に関わらず進む。
    noted_upto: [Option<u32>; 4],
    /// `RequestAction` の**絶対**締切。再送のとき残り時間を引き直す。
    ///
    /// **消せない。**`attach(seat, None)` は対局の頭から送り直すので、
    /// 遠い過去の要求も「残り0」に引き直す必要がある。伸び方は卓のログと
    /// 同じで、延長戦を含めても半荘1回ぶんに収まる。卓が畳まれれば消える。
    deadlines: HashMap<u32, u64>,
}

impl Sinks {
    fn new() -> Self {
        Sinks {
            out: std::array::from_fn(|_| None),
            sent_upto: [None; 4],
            live: [None; 4],
            next_id: 1,
            noted_upto: [None; 4],
            deadlines: HashMap::new(),
        }
    }

    fn checked(table: &Table, seat: Seat, last_seq: Option<u32>) -> Option<u32> {
        let claimed = last_seq?;
        table
            .since(seat, None)
            .iter()
            .any(|envelope| envelope.seq == claimed)
            .then_some(claimed)
    }

    /// 新しく出た `RequestAction` の絶対締切を控える。
    ///
    /// **`engine_now_ms` は、そのイベントを生んだ呼び出しへ渡した時刻**で
    /// なければならない。`deadline_ms` はその時刻からの残りなので、Actor が
    /// 別に測り直した時刻を足すと、待ち行列の滞留ぶんだけ絶対締切が
    /// 後ろへずれる。
    ///
    /// **配信先が無い席でも控える。**留守中に出た要求を、戻ってきたときに
    /// 満額の残り時間で見せてしまわないため。
    fn note_deadlines(&mut self, table: &Table, engine_now_ms: u64) {
        for index in 0..4 {
            let seat = Seat::new(index as u8);
            let fresh = table.since(seat, self.noted_upto[index]);
            for envelope in &fresh {
                if let ClientEvent::RequestAction { deadline_ms, .. } = &envelope.event {
                    self.deadlines
                        .entry(envelope.seq)
                        .or_insert(engine_now_ms + u64::from(*deadline_ms));
                }
            }
            if let Some(last) = fresh.last() {
                self.noted_upto[index] = Some(last.seq);
            }
        }
    }

    /// 再送のときに残り時間を引き直す。
    ///
    /// `deadline_ms` は**発行時点からの残り**であって絶対時刻ではない。
    /// 切断から数秒後に同じ包みをそのまま送り直すと、とっくに過ぎた要求が
    /// 満額の持ち時間で表示され、サーバの判定と食い違う。
    fn retimed(&self, mut envelope: ClientEventEnvelope, now_ms: u64) -> ClientEventEnvelope {
        if let ClientEvent::RequestAction { deadline_ms, .. } = &mut envelope.event {
            if let Some(absolute) = self.deadlines.get(&envelope.seq) {
                *deadline_ms = absolute.saturating_sub(now_ms).min(u64::from(u32::MAX)) as u32;
            }
        }
        envelope
    }

    fn flush(&mut self, table: &Table, now_ms: u64) {
        for index in 0..4 {
            if self.out[index].is_none() {
                continue;
            }
            let seat = Seat::new(index as u8);
            let batch = table.since(seat, self.sent_upto[index]);
            let mut highest = self.sent_upto[index];
            let mut alive = true;
            for envelope in batch {
                let seq = envelope.seq;
                let envelope = self.retimed(envelope, now_ms);
                let out = self.out[index].as_ref().expect("直前に確かめた");
                if out.try_send(envelope).is_err() {
                    alive = false;
                    break;
                }
                highest = Some(seq);
            }
            self.sent_upto[index] = highest;
            if !alive {
                self.out[index] = None;
                self.live[index] = None;
            }
        }
    }
}

fn handle(table: &mut Table, sinks: &mut Sinks, message: TableMsg, flush_now_ms: u64) {
    match message {
        TableMsg::Command {
            seat,
            command,
            at_ms,
            reply,
        } => {
            let result = table.apply(seat, command, at_ms);
            // **渡した時刻で控える。**この apply が生んだ要求の残り時間は
            // at_ms からの相対値である。
            sinks.note_deadlines(table, at_ms);
            let _ = reply.send(result);
        }
        TableMsg::Attach {
            seat,
            last_seq,
            ack,
        } => {
            let index = seat.index();
            let start = Sinks::checked(table, seat, last_seq);
            // **追いつきぶんが必ず入る容量にする。**足りないと正当な再接続が
            // 溢れて切られる。OUTBOX は生配信ぶんの余裕として上乗せする。
            let backlog = table.since(seat, start).len();
            let (out, inbox) = mpsc::channel(backlog + OUTBOX);
            sinks.sent_upto[index] = start;
            sinks.out[index] = Some(out);
            let connection = ConnectionId(sinks.next_id);
            sinks.next_id += 1;
            sinks.live[index] = Some(connection);
            sinks.flush(table, flush_now_ms);
            let _ = ack.send((connection, inbox));
        }
        TableMsg::Detach { seat, connection } => {
            if sinks.live[seat.index()] == Some(connection) {
                sinks.out[seat.index()] = None;
                sinks.live[seat.index()] = None;
            }
        }
    }
}

async fn run<S: Seeds>(
    mut table: Table,
    mut seeds: S,
    clock: Arc<Clock>,
    mut rx: mpsc::Receiver<TableMsg>,
) {
    let mut sinks = Sinks::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(POLL_MS));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        let now = clock.now_ms();
        sinks.flush(&table, now);
        if table.is_over() {
            break;
        }
        if table.needs_seed() {
            // **シードを受け取ってから局を始める。**Wave 3d はこの `await` の
            // 中で永続化する。ここで待てないと、`seed_commit` を配ったあとに
            // 落ちて別のシードで再開する経路ができてしまう。
            let seed = seeds.next_seed().await;
            let now = clock.now_ms();
            table.begin_round(&seed, now);
            sinks.note_deadlines(&table, now);
            sinks.flush(&table, now);
        }

        tokio::select! {
            biased;

            _ = ticker.tick() => {
                // **時計を進める前に、既に着いている分を片づける。**
                // 刻印だけでは足りない。tick が締切を越えて自動打牌したあとでは、
                // 締切前の刻印で apply しても局面は巻き戻らない。
                //
                // **上限は入口の容量と同じにする。**空になるまで回すと、受信で
                // 空いたスロットへ待機中の送信者が補充するので、コマンドが
                // 途切れないかぎり tick が永久に来ない。保証するのは
                // 「**この tick を選んだ時点で入口にあった分**は、時計を進める
                // 前に片づける」ところまでである。
                let now = clock.now_ms();
                for _ in 0..INBOX {
                    match rx.try_recv() {
                        Ok(message) => handle(&mut table, &mut sinks, message, now),
                        Err(_) => break,
                    }
                }
                let ticked_at = clock.now_ms();
                table.tick(ticked_at);
                sinks.note_deadlines(&table, ticked_at);
            }
            message = rx.recv() => match message {
                None => break,
                Some(message) => {
                    let now = clock.now_ms();
                    handle(&mut table, &mut sinks, message, now);
                }
            },
        }
    }

    let now = clock.now_ms();
    sinks.flush(&table, now);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Table;
    use mahjong_engine::wall::Seed;
    use protocol::client_event::ClientEvent;
    use protocol::event::PlayerId;
    use protocol::ruleset::MatchLength;
    use protocol::tile::Tile;

    pub(super) fn rules() -> Ruleset {
        Ruleset::kin_no_ma(MatchLength::Hanchan)
    }

    pub(super) fn humans() -> [Occupant; 4] {
        std::array::from_fn(|i| Occupant::Human(PlayerId(format!("p{i}"))))
    }

    pub(super) fn one_human_three_cpus() -> [Occupant; 4] {
        [
            Occupant::Human(PlayerId("human".to_owned())),
            Occupant::Cpu(PlayerId("cpu1".to_owned())),
            Occupant::Cpu(PlayerId("cpu2".to_owned())),
            Occupant::Cpu(PlayerId("cpu3".to_owned())),
        ]
    }

    /// その席へ届いた分をいま取れるだけ取る。
    pub(super) fn take_ready(inbox: &mut Inbound) -> Vec<ClientEventEnvelope> {
        let mut out = Vec::new();
        while let Ok(envelope) = inbox.try_recv() {
            out.push(envelope);
        }
        out
    }

    /// 配牌で配られた自分の手牌。
    pub(super) fn dealt_hand(events: &[ClientEventEnvelope]) -> Vec<Tile> {
        events
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::Deal { your_hand, .. } => Some(your_hand.clone()),
                _ => None,
            })
            .expect("配牌が届いている")
    }

    #[tokio::test(start_paused = true)]
    async fn attaching_delivers_the_opening_without_waiting() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        // yield_now を呼ばない。ack が返った時点で届いていなければならない。
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let events = take_ready(&mut inbox);
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::MatchStart { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::RoundStart { .. })));
        // **配牌は親も子も13枚。**親の14枚目は別の Draw で来る。
        assert_eq!(dealt_hand(&events).len(), 13);
        assert!(
            events.iter().any(|e| matches!(
                &e.event,
                ClientEvent::Draw { seat, tile, .. } if *seat == Seat::new(0) && tile.is_some()
            )),
            "親の14枚目が Draw で来ていない"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_actor_deals_a_seed_without_being_asked() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut inbox) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        let events = take_ready(&mut inbox);
        assert!(
            events
                .iter()
                .any(|e| matches!(e.event, ClientEvent::RoundStart { .. })),
            "外からシードを渡していないのに局が始まっている"
        );
        assert_eq!(dealt_hand(&events).len(), 13);
    }

    #[tokio::test(start_paused = true)]
    async fn two_seats_are_dealt_different_hands() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut east) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let (_, mut south) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        let east_hand = dealt_hand(&take_ready(&mut east));
        let south_hand = dealt_hand(&take_ready(&mut south));
        assert_ne!(east_hand, south_hand, "視界フィルタが効いていない");
    }

    #[tokio::test(start_paused = true)]
    async fn a_seat_never_sees_another_draw() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut south) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        for envelope in take_ready(&mut south) {
            if let ClientEvent::Draw { seat, tile, .. } = &envelope.event {
                assert!(
                    *seat == Seat::new(1) || tile.is_none(),
                    "他家のツモ牌が見えている"
                );
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_discard_reaches_the_other_seats() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut east) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let (_, mut west) = handle
            .attach(Seat::new(2), None)
            .await
            .expect("卓は生きている");

        let tile = dealt_hand(&take_ready(&mut east))[0];
        let _ = take_ready(&mut west);

        handle
            .command(
                Seat::new(0),
                Command::Discard {
                    tile,
                    riichi: false,
                },
                handle.now_ms(),
            )
            .await
            .expect("卓は生きている")
            .expect("親は打てる");

        let seen = tokio::time::timeout(Duration::from_millis(1_000), west.recv())
            .await
            .expect("1秒のうちに届く")
            .expect("卓は生きている");
        assert!(
            matches!(
                &seen.event,
                ClientEvent::Discard { seat, tile: t, .. } if *seat == Seat::new(0) && *t == tile
            ) || take_ready(&mut west).iter().any(|e| matches!(
                &e.event,
                ClientEvent::Discard { seat, tile: t, .. } if *seat == Seat::new(0) && *t == tile
            )),
            "打牌が西家へ届いていない"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_command_from_the_wrong_seat_is_rejected() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut south) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        let tile = dealt_hand(&take_ready(&mut south))[0];
        let rejected = handle
            .command(
                Seat::new(1),
                Command::Discard {
                    tile,
                    riichi: false,
                },
                handle.now_ms(),
            )
            .await
            .expect("卓は生きている");
        assert_eq!(rejected, Err(Reject::NotYourTurn), "親でない席が打てている");
    }

    #[tokio::test(start_paused = true)]
    async fn sequence_numbers_only_go_up() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut inbox) = handle
            .attach(Seat::new(2), None)
            .await
            .expect("卓は生きている");

        let events = take_ready(&mut inbox);
        assert!(events.len() >= 3);
        for pair in events.windows(2) {
            assert!(pair[0].seq < pair[1].seq, "連番が戻っている");
        }
    }

    /// Wave 3d で axum のハンドラへ持たせるには `Send + Sync` が要る。
    /// **ここで確かめておかないと、次のウェーブで型を作り直すことになる。**
    #[test]
    fn the_handle_can_cross_threads() {
        fn assert_send_sync<T: Send + Sync + Clone + 'static>() {}
        assert_send_sync::<TableHandle>();
    }

    /// **刻印だけでは足りないことの証明。**
    ///
    /// 締切前に処理すれば通る。締切を越えた `tick` のあとでは、同じ刻印でも
    /// 通らない。だから Actor は `tick` の前にキューを空にしなければならない。
    #[test]
    fn an_expired_tick_cannot_be_undone() {
        let seed = Seed::from_hex(&"01".repeat(32)).expect("正しい hex");

        let mut early = Table::new(rules(), one_human_three_cpus(), 0);
        early.begin_round(&seed, 0);
        let tile = early
            .round_state()
            .expect("局が動いている")
            .seat(Seat::new(0))
            .hand[0];
        early.tick(25_700);
        assert_eq!(
            early.apply(
                Seat::new(0),
                Command::Discard {
                    tile,
                    riichi: false
                },
                25_700
            ),
            Ok(()),
            "締切前に処理すれば通る"
        );

        let mut late = Table::new(rules(), one_human_three_cpus(), 0);
        late.begin_round(&seed, 0);
        let tile = late
            .round_state()
            .expect("局が動いている")
            .seat(Seat::new(0))
            .hand[0];
        late.tick(25_800);
        assert_eq!(
            late.apply(
                Seat::new(0),
                Command::Discard {
                    tile,
                    riichi: false
                },
                25_700
            ),
            Err(Reject::NotYourTurn),
            "締切を越えた tick は巻き戻らない"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_seat_is_made_to_discard_when_its_bank_runs_out() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let clock = Clock::start();
        let (_, mut east) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let _ = take_ready(&mut east);

        let envelope = tokio::time::timeout(Duration::from_millis(60_000), east.recv())
            .await
            .expect("60秒のうちに何か届く")
            .expect("卓は生きている");

        assert!(
            matches!(&envelope.event, ClientEvent::Discard { seat, .. } if *seat == Seat::new(0)),
            "時間切れで自動打牌されていない"
        );
        // 基準 5,000 + バンク 20,000 + 通信猶予 500 + ツモの lead_in 250 = 25,750。
        // ポーリングは 100ms 刻みなので、実際に切られるのは 25,800。
        let elapsed = clock.now_ms();
        assert!(elapsed > 25_750, "締切より前に切られた: {elapsed}ms");
        assert!(
            elapsed <= 25_850,
            "ポーリング1周を超えて遅れた: {elapsed}ms"
        );
    }
}
