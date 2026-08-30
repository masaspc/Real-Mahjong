//! 1卓 = 1 tokio task の Actor。**唯一 I/O と時間を持つ層。**

#[path = "session_time.rs"]
mod time;

pub use time::{Clock, SeedSource};

use crate::persistence::Scribe;
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

/// 局と局のあいだの間。
///
/// **0 にすると、和了の役も点数も読めないまま次の局が配られる。**実際に
/// 遊んだ人から「上がった後、何の役か分からないままいきなり次が始まる」と
/// 言われた。局の結果はクライアントが板に出すが、次の局が同時に始まって
/// しまうと読む時間が無い。
///
/// **この間は誰の持ち時間も減らさない。**この区間ではどの席にも行動要求が
/// 出ていないので、締切そのものが存在しない。演出カタログ（`protocol`）へ
/// 手を入れる必要も無い。あちらは「行動要求の締切に演出時間を足す」ための
/// 表であり、行動要求の無いここには関係しない。
const INTERLUDE_MS: u64 = 6_000;

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

#[cfg(test)]
mod reaction_tests {
    use super::tests::rules;
    use super::*;
    use protocol::client_event::ClientEvent;
    use protocol::command::{ActionOption, CallResponse};
    use protocol::event::PlayerId;

    fn human_at(seat: usize) -> [Occupant; 4] {
        std::array::from_fn(|index| {
            if index == seat {
                Occupant::Human(PlayerId("human".to_owned()))
            } else {
                Occupant::Cpu(PlayerId(format!("cpu{index}")))
            }
        })
    }

    fn all_cpu() -> [Occupant; 4] {
        std::array::from_fn(|index| Occupant::Cpu(PlayerId(format!("cpu{index}"))))
    }

    fn humans_at(first: usize, second: usize) -> [Occupant; 4] {
        std::array::from_fn(|index| {
            if index == first || index == second {
                Occupant::Human(PlayerId(format!("human{index}")))
            } else {
                Occupant::Cpu(PlayerId(format!("cpu{index}")))
            }
        })
    }

    /// その席へ最初に届く「鳴きの要求」まで進める。
    /// 打牌の要求（自分の番）は読み飛ばす。
    async fn advance_to_a_call_window(inbox: &mut Inbound) -> (u32, Vec<ActionOption>) {
        for _ in 0..3_000 {
            let next = tokio::time::timeout(Duration::from_millis(120_000), inbox.recv())
                .await
                .expect("仮想2分のうちに何か届く");
            let Some(envelope) = next else {
                break;
            };
            if let ClientEvent::RequestAction {
                window_id, options, ..
            } = &envelope.event
            {
                if options
                    .iter()
                    .any(|option| !matches!(option, ActionOption::Discard { .. }))
                {
                    return (*window_id, options.clone());
                }
            }
        }
        panic!("鳴きの要求に到達しなかった");
    }

    /// **卓は時間を作らない。**最低待機 350ms を越えさせるのは Actor の tick。
    #[tokio::test(start_paused = true)]
    async fn the_actor_carries_a_call_across_the_minimum_wait() {
        let (handle, _actor) = spawn(rules(), human_at(1), SeedSource::from_master([1u8; 32]));
        let clock = Clock::start();
        let (_, mut inbox) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        let (window_id, options) = advance_to_a_call_window(&mut inbox).await;
        // 実測では 55,200ms だが、そこは CPU の打牌方針しだいで動く。
        // **仕様は「応答から最低待機ぶん後に成立する」であって、
        // 要求が何ミリ秒に届くかではない。**時刻そのものは固定しない。
        let opened_at = clock.now_ms();

        let tiles = options
            .iter()
            .find_map(|option| match option {
                ActionOption::Pon { candidates } if !candidates.is_empty() => Some(candidates[0]),
                _ => None,
            })
            .expect("ポンの候補がある");

        // **同じミリ秒に応じる。**卓が自分で時間を進めるなら、ここで成立して
        // しまう。成立が 350ms 後になることが、Actor が越えさせている証拠。
        let accepted = handle
            .command(
                Seat::new(1),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pon { tiles },
                },
                handle.now_ms(),
            )
            .await
            .expect("卓は生きている");
        assert_eq!(accepted, Ok(()));
        assert_eq!(clock.now_ms(), opened_at, "応答で時間が進んでいる");

        let mut called_at = None;
        for _ in 0..40 {
            let next = tokio::time::timeout(Duration::from_millis(60_000), inbox.recv())
                .await
                .expect("仮想60秒のうちに届く");
            let Some(envelope) = next else {
                break;
            };
            if matches!(envelope.event, ClientEvent::Call { .. }) {
                called_at = Some(clock.now_ms());
                break;
            }
        }
        let called_at = called_at.expect("鳴きが成立しなかった");
        assert!(
            called_at >= opened_at + 350,
            "最低待機を越えずに成立した: +{}ms",
            called_at - opened_at
        );
        assert!(
            called_at <= opened_at + 450,
            "ポーリング1周を超えて遅れた: +{}ms",
            called_at - opened_at
        );
    }

    /// **誰も鳴けない打牌でも一律に待つ。**（仕様 6.4）
    ///
    /// 鳴ける者がいないときだけ次のツモが速いと、待ち時間の長短から
    /// 「誰も鳴けなかった」が読めてしまう。一律に待つのは情報を漏らさない
    /// ためであり、その待機を越えさせるのも Actor の `tick` である。
    ///
    /// 実測では、4人 CPU の対局で打牌から次のツモまでが**30回すべて
    /// ちょうど 400ms**（最低待機 350ms の直後のポーリング）だった。
    #[tokio::test(start_paused = true)]
    async fn even_an_uncalled_discard_waits_out_the_minimum() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let clock = Clock::start();
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let mut discarded_at: Option<u64> = None;
        let mut gaps: Vec<u64> = Vec::new();
        while gaps.len() < 20 {
            let next = tokio::time::timeout(Duration::from_millis(600_000), inbox.recv())
                .await
                .expect("仮想10分のうちに届く");
            let Some(envelope) = next else { break };
            match &envelope.event {
                ClientEvent::Discard { .. } => discarded_at = Some(clock.now_ms()),
                // **鳴きが挟まった区間は数えない。**鳴けた場合の待機と
                // 混ぜると「誰も鳴けなくても待つ」の検証にならない。
                ClientEvent::Call { .. } | ClientEvent::RequestAction { .. } => {
                    discarded_at = None;
                }
                ClientEvent::Draw { .. } => {
                    if let Some(at) = discarded_at.take() {
                        gaps.push(clock.now_ms() - at);
                    }
                }
                _ => {}
            }
        }

        assert_eq!(gaps.len(), 20, "測れた間隔が足りない");
        for gap in &gaps {
            assert!(
                *gap >= 350,
                "誰も鳴かなかった打牌の直後に次のツモが来た: {gap}ms"
            );
            assert!(*gap <= 450, "ポーリング1周を超えて遅れた: {gap}ms");
        }
    }

    /// **同じミリ秒に2席が同じウィンドウへ応じても、両方が受理される。**
    ///
    /// エンジンの判定規則は 298 件のテストが見ているが、「締切前に入口へ
    /// 着いた応答が、解決の tick より先に全部適用される」のは Actor が
    /// 新たに担う輸送の保証であり、エンジン単体では確かめられない。
    #[tokio::test(start_paused = true)]
    async fn two_seats_answer_the_same_window_before_the_tick() {
        let (handle, _actor) = spawn(rules(), humans_at(0, 2), SeedSource::from_master([1u8; 32]));
        let (_, mut east) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let (_, mut west) = handle
            .attach(Seat::new(2), None)
            .await
            .expect("卓は生きている");

        let mut seen_east: Vec<u32> = Vec::new();
        let mut seen_west: Vec<u32> = Vec::new();
        let mut shared = None;
        for _ in 0..6_000 {
            for (inbox, seen) in [(&mut east, &mut seen_east), (&mut west, &mut seen_west)] {
                while let Ok(envelope) = inbox.try_recv() {
                    if let ClientEvent::RequestAction {
                        window_id, options, ..
                    } = &envelope.event
                    {
                        if options
                            .iter()
                            .any(|option| !matches!(option, ActionOption::Discard { .. }))
                        {
                            seen.push(*window_id);
                        }
                    }
                }
            }
            if let Some(window) = seen_east.iter().find(|w| seen_west.contains(w)) {
                shared = Some(*window);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let window_id = shared.expect("2席共通のウィンドウに到達しなかった");

        // **同時に投入する。**逐次に送ると1件ずつ処理されるだけで、
        // 「入口に並んだ複数の応答が tick より先に片づく」を試せない。
        let at = handle.now_ms();
        let (first, second) = tokio::join!(
            handle.command(
                Seat::new(0),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pass,
                },
                at,
            ),
            handle.command(
                Seat::new(2),
                Command::CallResponse {
                    window_id,
                    response: CallResponse::Pass,
                },
                at,
            )
        );
        assert_eq!(first.expect("卓は生きている"), Ok(()), "先の応答が拒まれた");
        assert_eq!(
            second.expect("卓は生きている"),
            Ok(()),
            "同じミリ秒の2席目の応答が拒まれた"
        );
    }

    /// `window_id` は再送や遅れた応答を別のウィンドウへ当てないための鍵。
    #[tokio::test(start_paused = true)]
    async fn a_response_to_an_unknown_window_is_refused() {
        let (handle, _actor) = spawn(rules(), human_at(1), SeedSource::from_master([1u8; 32]));
        let (_, mut inbox) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");
        let (window_id, _) = advance_to_a_call_window(&mut inbox).await;

        assert_eq!(
            handle
                .command(
                    Seat::new(1),
                    Command::CallResponse {
                        window_id: window_id + 999,
                        response: CallResponse::Pass,
                    },
                    handle.now_ms(),
                )
                .await
                .expect("卓は生きている"),
            Err(Reject::StaleWindow),
            "知らないウィンドウへの応答が通った"
        );

        assert_eq!(
            handle
                .command(
                    Seat::new(1),
                    Command::CallResponse {
                        window_id,
                        response: CallResponse::Pass,
                    },
                    handle.now_ms(),
                )
                .await
                .expect("卓は生きている"),
            Ok(()),
            "正しいウィンドウへの応答が通らない"
        );

        assert_eq!(
            handle
                .command(
                    Seat::new(1),
                    Command::CallResponse {
                        window_id,
                        response: CallResponse::Pass,
                    },
                    handle.now_ms(),
                )
                .await
                .expect("卓は生きている"),
            Err(Reject::StaleWindow),
            "同じウィンドウへ二度応えられている"
        );
    }
}

#[cfg(test)]
mod reconnect_tests {
    use super::tests::{humans, one_human_three_cpus, rules, take_ready};
    use super::*;
    use protocol::client_event::ClientEvent;
    use protocol::event::PlayerId;

    fn all_cpu() -> [Occupant; 4] {
        std::array::from_fn(|i| Occupant::Cpu(PlayerId(format!("cpu{i}"))))
    }

    /// 仮想時間でこれを超えたら、卓が終わらない不具合とみなす。
    const A_VIRTUAL_HOUR_MS: u64 = 3_600_000;

    #[test]
    fn the_first_round_starts_without_waiting() {
        // 卓に着いた直後に間を置く理由は無い。
        let mut resume = None;
        assert!(!interlude(0, false, &mut resume));
        assert_eq!(resume, None);
    }

    #[test]
    fn a_finished_round_holds_for_the_interlude() {
        let mut resume = None;
        assert!(interlude(1_000, true, &mut resume));
        assert_eq!(resume, Some(1_000 + INTERLUDE_MS));
        // まだ途中。
        assert!(interlude(1_000 + INTERLUDE_MS - 1, true, &mut resume));
        // 越えたら通す。
        assert!(!interlude(1_000 + INTERLUDE_MS, true, &mut resume));
    }

    #[test]
    fn the_interlude_is_not_pushed_back_by_later_calls() {
        // **呼ぶたびに終わり時刻を決め直すと、永久に次の局が始まらない。**
        // 卓は 100ms ごとに回るので、そのたびに 6 秒足されることになる。
        let mut resume = None;
        assert!(interlude(1_000, true, &mut resume));
        assert!(interlude(2_000, true, &mut resume));
        assert_eq!(resume, Some(1_000 + INTERLUDE_MS));
    }

    /// **本当に間が空くことを、卓を走らせて確かめる。**
    ///
    /// 純粋な関数の試験は「呼ばれたら待つ」ことしか言えない。呼ぶ場所を
    /// 間違えていても通る。
    #[tokio::test(start_paused = true)]
    async fn the_next_round_is_dealt_after_the_interlude() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        // 1局目が終わり、2局目が配られるまでを見る。
        let mut round_starts: Vec<u64> = Vec::new();
        let mut ended_at: Option<u64> = None;
        let start = tokio::time::Instant::now();
        while round_starts.len() < 2 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            for envelope in take_ready(&mut inbox) {
                let at = start.elapsed().as_millis() as u64;
                match envelope.event {
                    ClientEvent::RoundStart { .. } => round_starts.push(at),
                    ClientEvent::Agari { .. } | ClientEvent::Ryuukyoku { .. } => {
                        ended_at.get_or_insert(at);
                    }
                    _ => {}
                }
            }
            assert!(
                start.elapsed().as_millis() < A_VIRTUAL_HOUR_MS as u128,
                "1局が終わらない"
            );
        }

        let ended = ended_at.expect("局が終わっている");
        let next = round_starts[1];
        assert!(
            next >= ended + INTERLUDE_MS,
            "間が足りない: 終局 {ended}ms → 次局 {next}ms（{INTERLUDE_MS}ms 空けるはず）"
        );
    }

    /// **正直な申告は必ず受理される。**
    ///
    /// 卓を進めてから繋ぎ直すので、受け取る束が空にならない。空のまま
    /// 「戻ってきたものは無かった」で通ると、`checked` が常に最初から
    /// 送り直していても気づけない。
    #[tokio::test(start_paused = true)]
    async fn reattaching_from_a_sequence_skips_what_was_already_seen() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (_, mut first) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        let seen = take_ready(&mut first);
        let last = seen.last().expect("何か届いている").seq;

        // 親は席0の人間。その持ち時間が尽きて卓が進むまで待つ。
        tokio::time::sleep(Duration::from_millis(30_000)).await;

        let (_, mut second) = handle
            .attach(Seat::new(1), Some(last))
            .await
            .expect("卓は生きている");

        let caught_up = take_ready(&mut second);
        assert!(!caught_up.is_empty(), "正直な申告が受理されていない");
        assert_ne!(
            caught_up.first().map(|e| e.seq),
            Some(0),
            "正直な申告なのに最初から送り直している"
        );
        for envelope in &caught_up {
            assert!(envelope.seq > last, "見たものがまた来ている");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn reattaching_from_nothing_replays_the_whole_match() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut first) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let original = take_ready(&mut first);

        let (_, mut second) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let replayed = take_ready(&mut second);

        assert_eq!(
            original.iter().map(|e| e.seq).collect::<Vec<_>>(),
            replayed.iter().map(|e| e.seq).collect::<Vec<_>>(),
            "再送が元と食い違っている"
        );
        assert!(
            replayed
                .iter()
                .any(|e| matches!(e.event, ClientEvent::MatchStart { .. })),
            "最初から再送されていない"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_impossible_sequence_replays_from_the_beginning() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut inbox) = handle
            .attach(Seat::new(0), Some(u32::MAX))
            .await
            .expect("卓は生きている");

        let events = take_ready(&mut inbox);
        assert!(!events.is_empty(), "未来の連番で配信が永久に止まった");
        assert_eq!(
            events.first().map(|e| e.seq),
            Some(0),
            "先頭が seq 0 でない"
        );
        assert!(events
            .iter()
            .any(|e| matches!(e.event, ClientEvent::MatchStart { .. })));
    }

    /// **その席に投影されなかった連番を申告されたら、最初から送り直す。**
    ///
    /// 全体のログを連番で絞ってから席ごとに投影するので、その席に見えない
    /// 連番が正常に飛び飛びで存在する。`RequestAction` は当該席以外へ
    /// 投影されないのが代表例。それを申告されて受理すると、まだ一度も
    /// 見ていない可視イベントが飛ぶ。
    #[tokio::test(start_paused = true)]
    async fn a_sequence_hidden_from_this_seat_is_refused() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let (_, mut inbox) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");

        let mut visible: Vec<u32> = Vec::new();
        while visible.len() < 40 {
            let next = tokio::time::timeout(Duration::from_millis(60_000), inbox.recv())
                .await
                .expect("仮想60秒のうちに届く");
            let Some(envelope) = next else { break };
            visible.push(envelope.seq);
        }
        let highest = *visible.last().expect("何か届いている");
        let hidden: Vec<u32> = (0..highest).filter(|s| !visible.contains(s)).collect();
        assert!(!hidden.is_empty(), "見えない連番が無いと検証にならない");

        let claim = hidden[hidden.len() / 2];
        let (_, mut liar) = handle
            .attach(Seat::new(1), Some(claim))
            .await
            .expect("卓は生きている");
        assert_eq!(
            take_ready(&mut liar).first().map(|e| e.seq),
            Some(0),
            "自席に見えない連番を受理して途中から送っている"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn every_connection_gets_its_own_id() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (first, _a) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let (second, _b) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let (third, _c) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");
        assert_ne!(first, second, "同じ席で ID が使い回されている");
        assert_ne!(second, third, "別の席と ID が衝突している");
    }

    #[tokio::test(start_paused = true)]
    async fn a_stale_detach_does_not_kill_the_new_connection() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (old_id, mut old) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let seen = take_ready(&mut old).last().map(|e| e.seq);
        let (new_id, mut fresh) = handle
            .attach(Seat::new(0), seen)
            .await
            .expect("卓は生きている");
        assert_ne!(old_id, new_id);

        // 置き換えられた古い接続が、遅れて切断を申し出る。
        handle
            .detach(Seat::new(0), old_id)
            .await
            .expect("卓は生きている");
        drop(old);

        tokio::time::sleep(Duration::from_millis(30_000)).await;
        assert!(
            !take_ready(&mut fresh).is_empty(),
            "古い接続の切断が新しい接続を巻き添えにした"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_detached_seat_catches_up_when_it_comes_back() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (id, mut inbox) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");
        let last = take_ready(&mut inbox).last().expect("何か届いている").seq;

        handle
            .detach(Seat::new(1), id)
            .await
            .expect("卓は生きている");
        drop(inbox);

        // 席が居ないあいだも卓は進む。**親は席0の人間**なので、
        // その持ち時間が尽きて自動打牌されるまで待つ必要がある。
        tokio::time::sleep(Duration::from_millis(30_000)).await;

        let (_, mut back) = handle
            .attach(Seat::new(1), Some(last))
            .await
            .expect("卓は生きている");
        let caught_up = take_ready(&mut back);

        assert!(!caught_up.is_empty(), "留守中の分が追いついていない");
        assert!(caught_up.iter().all(|e| e.seq > last));
        for pair in caught_up.windows(2) {
            assert!(pair[0].seq < pair[1].seq);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_a_receiver_does_not_stop_the_table() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (_, inbox) = handle
            .attach(Seat::new(2), None)
            .await
            .expect("卓は生きている");
        drop(inbox);

        tokio::time::sleep(Duration::from_millis(30_000)).await;

        let (_, mut other) = handle
            .attach(Seat::new(3), None)
            .await
            .expect("卓は生きている");
        assert!(!take_ready(&mut other).is_empty(), "卓が止まっている");
    }

    #[tokio::test(start_paused = true)]
    async fn four_cpus_play_a_whole_match_and_the_actor_shuts_down() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let mut saw_match_end = false;
        let mut previous = None;
        loop {
            // 卓が終わらない不具合を、ハングでなく assertion で捕まえる。
            let next =
                tokio::time::timeout(Duration::from_millis(A_VIRTUAL_HOUR_MS), watcher.recv())
                    .await
                    .expect("仮想1時間のうちに終わる");
            let Some(envelope) = next else { break };
            if let Some(prev) = previous {
                assert!(envelope.seq > prev, "連番が戻った");
            }
            previous = Some(envelope.seq);
            if matches!(envelope.event, ClientEvent::MatchEnd { .. }) {
                saw_match_end = true;
            }
        }
        assert!(saw_match_end, "半荘が終わっていない");

        // Actor が落ちるとハンドルの送り口も閉じる。
        for _ in 0..100 {
            if handle.is_closed() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(handle.is_closed(), "卓が終わったのに Actor が生きている");
    }

    /// **何度繋ぎ直しても、残り時間は同じ一本の絶対締切を指す。**
    ///
    /// 絶対締切を Actor が測り直した時刻から作ると、繋ぎ直すたびに
    /// 指す先がずれる。控えるのは「エンジンへ渡した時刻」でなければ
    /// ならない。実測では4回の再送がすべて 25,750ms を指した。
    #[tokio::test(start_paused = true)]
    async fn the_remaining_time_shrinks_exactly_with_the_clock() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let seq = take_ready(&mut inbox)
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::RequestAction { .. } => Some(e.seq),
                _ => None,
            })
            .expect("要求が届いている");

        let mut readings: Vec<(u64, u32)> = Vec::new();
        for _ in 0..4 {
            let (_, mut fresh) = handle
                .attach(Seat::new(0), None)
                .await
                .expect("卓は生きている");
            let now = handle.now_ms();
            let value = take_ready(&mut fresh)
                .iter()
                .find_map(|e| match &e.event {
                    ClientEvent::RequestAction { deadline_ms, .. } if e.seq == seq => {
                        Some(*deadline_ms)
                    }
                    _ => None,
                })
                .expect("同じ要求が再送される");
            readings.push((now, value));
            drop(fresh);
            tokio::time::sleep(Duration::from_millis(3_000)).await;
        }

        let absolute = readings[0].0 + u64::from(readings[0].1);
        for (at, value) in &readings {
            assert_eq!(
                at + u64::from(*value),
                absolute,
                "時刻 {at} の再送が別の絶対締切を指している"
            );
        }
    }

    /// **締切を過ぎた要求は、残り0で再送される。**
    ///
    /// `attach(seat, None)` は対局の頭から送り直すので、とっくに過ぎた
    /// 要求も混ざる。満額の残り時間で送ると、クライアントは終わった
    /// ウィンドウのタイマーを回し始める。
    #[tokio::test(start_paused = true)]
    async fn an_expired_request_is_resent_with_no_time_left() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let seq = take_ready(&mut inbox)
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::RequestAction { .. } => Some(e.seq),
                _ => None,
            })
            .expect("要求が届いている");

        // 締切（5,000 + 20,000 + 500 + 250 = 25,750ms）を大きく越える。
        tokio::time::sleep(Duration::from_millis(40_000)).await;

        let (_, mut back) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        assert_eq!(
            take_ready(&mut back).iter().find_map(|e| match &e.event {
                ClientEvent::RequestAction { deadline_ms, .. } if e.seq == seq =>
                    Some(*deadline_ms),
                _ => None,
            }),
            Some(0),
            "過ぎた要求が残り時間を持ったまま再送されている"
        );
    }

    /// **最新の連番を申告したら、何も返らない。**
    ///
    /// `>` と `>=` を取り違えると、直前に見たものがもう一度届く。
    #[tokio::test(start_paused = true)]
    async fn reattaching_from_the_latest_sequence_sends_nothing() {
        let (handle, _actor) = spawn(rules(), humans(), SeedSource::from_master([1u8; 32]));
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let latest = take_ready(&mut inbox).last().expect("何か届いている").seq;

        let (_, mut again) = handle
            .attach(Seat::new(0), Some(latest))
            .await
            .expect("卓は生きている");
        assert!(
            take_ready(&mut again).is_empty(),
            "最新の連番を申告したのに送り直された"
        );
    }

    /// **一度も読まない接続が切られない。**
    ///
    /// 受け口の容量は追いつきぶんを見てから決めるので、半荘1回ぶんが
    /// たまっても溢れない。溢れて切られると、生きている接続が黙る。
    #[tokio::test(start_paused = true)]
    async fn a_seat_that_never_reads_still_gets_the_whole_match() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([2u8; 32]));
        let (_, mut idle) = handle
            .attach(Seat::new(1), None)
            .await
            .expect("卓は生きている");
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        loop {
            let next =
                tokio::time::timeout(Duration::from_millis(A_VIRTUAL_HOUR_MS), watcher.recv())
                    .await
                    .expect("仮想1時間のうちに終わる");
            if next.is_none() {
                break;
            }
        }
        let piled = take_ready(&mut idle);
        assert!(piled.len() > 500, "溢れて切られている: {} 件", piled.len());
    }

    /// **再送する要求は、いま本当に残っている時間を載せる。**
    ///
    /// `deadline_ms` は発行時点からの残りなので、そのまま送り直すと
    /// クライアントは満額の持ち時間で表示し、サーバは元の絶対締切で
    /// 判定する。実測では初回 25,750ms が5秒後の再送で 20,750ms になる。
    #[tokio::test(start_paused = true)]
    async fn a_resent_request_shows_the_time_that_is_actually_left() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([1u8; 32]),
        );
        let (id, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let (seq, original) = take_ready(&mut inbox)
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::RequestAction { deadline_ms, .. } => Some((e.seq, *deadline_ms)),
                _ => None,
            })
            .expect("要求が届いている");

        handle
            .detach(Seat::new(0), id)
            .await
            .expect("卓は生きている");
        drop(inbox);
        tokio::time::sleep(Duration::from_millis(5_000)).await;

        let (_, mut back) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let resent = take_ready(&mut back)
            .iter()
            .find_map(|e| match &e.event {
                ClientEvent::RequestAction { deadline_ms, .. } if e.seq == seq => {
                    Some(*deadline_ms)
                }
                _ => None,
            })
            .expect("同じ要求が再送されている");

        assert!(
            resent < original,
            "再送された要求が満額の残り時間のまま: {resent} / {original}"
        );
        let drained = original - resent;
        assert!(
            (4_900..=5_100).contains(&drained),
            "引き方がずれている: {drained}ms 減った"
        );
    }

    /// **出口の容量が、半荘1回ぶんの再送に足りている。**
    ///
    /// 足りないと `attach(seat, None)` が溢れ、正当な再接続が切られる。
    /// 実測は 1,304 / 1,875 / 1,677 件（シード3種）だが、ばらつきが4割
    /// あるので、将来 CPU の打ち方が変わったときに気づけるようにしておく。
    #[tokio::test(start_paused = true)]
    async fn a_whole_match_fits_in_one_outbox() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([2u8; 32]));
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let mut count = 0usize;
        loop {
            let next =
                tokio::time::timeout(Duration::from_millis(A_VIRTUAL_HOUR_MS), watcher.recv())
                    .await
                    .expect("仮想1時間のうちに終わる");
            if next.is_none() {
                break;
            }
            count += 1;
        }
        assert!(count > 500, "半荘にしてはイベントが少なすぎる: {count}");
        assert!(
            count < OUTBOX,
            "出口の容量が足りない: {count} 件 / {OUTBOX}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_finished_table_refuses_new_connections() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        loop {
            let next =
                tokio::time::timeout(Duration::from_millis(A_VIRTUAL_HOUR_MS), watcher.recv())
                    .await
                    .expect("仮想1時間のうちに終わる");
            if next.is_none() {
                break;
            }
        }
        for _ in 0..100 {
            if handle.is_closed() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            handle.attach(Seat::new(0), None).await.err(),
            Some(Gone),
            "終わった卓が接続を受けつけている"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn every_round_uses_a_fresh_seed() {
        let (handle, _actor) = spawn(rules(), all_cpu(), SeedSource::from_master([1u8; 32]));
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        let mut commits = Vec::new();
        loop {
            let next =
                tokio::time::timeout(Duration::from_millis(A_VIRTUAL_HOUR_MS), watcher.recv())
                    .await
                    .expect("仮想1時間のうちに終わる");
            let Some(envelope) = next else { break };
            if let ClientEvent::RoundStart { seed_commit, .. } = &envelope.event {
                commits.push(seed_commit.clone());
            }
        }
        assert!(commits.len() >= 2, "局が1つしか立っていない");
        let unique: std::collections::HashSet<_> = commits.iter().collect();
        assert_eq!(unique.len(), commits.len(), "同じシードが2度使われている");
    }
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
    spawn_recorded(rules, occupants, seeds, None)
}

/// 牌譜を残す卓の宛先。
///
/// **見出しの行は部屋が作る。**席と名前と証明を知っているのは部屋の方で、
/// 卓は自分が誰に配っているかを知らない。卓が受け持つのは出来事だけ。
#[derive(Clone)]
pub struct Recording {
    pub scribe: Scribe,
    pub record_id: String,
}

/// 牌譜を残しながら卓を立ち上げる。
pub fn spawn_recorded<S: Seeds>(
    rules: Ruleset,
    occupants: [Occupant; 4],
    seeds: S,
    recording: Option<Recording>,
) -> (TableHandle, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(INBOX);
    let clock = Arc::new(Clock::start());
    let table = Table::new(rules, occupants, clock.now_ms());
    let actor = tokio::spawn(run(table, seeds, Arc::clone(&clock), rx, recording));
    (TableHandle { tx, clock }, actor)
}

/// まだ書き出していないぶんを書き手へ渡す。
///
/// **何度呼んでも増えない。**局の切れ目を待つあいだ毎周回呼ばれるので、
/// 渡し終えた分をまた渡さないことが要る。
fn hand_over(recording: &Option<Recording>, table: &Table, written_upto: &mut usize) {
    let Some(recording) = recording else { return };
    let pending = table.log_from(*written_upto);
    if pending.is_empty() {
        return;
    }
    recording
        .scribe
        .append(&recording.record_id, pending.to_vec());
    *written_upto = table.log_len();
}

/// 終局の点数と順位。牌譜の見出しに入れる。
fn match_result_json(table: &Table) -> Option<String> {
    table
        .log_from(0)
        .iter()
        .rev()
        .find_map(|envelope| match &envelope.event {
            protocol::event::Event::MatchEnd {
                final_scores,
                placements,
            } => serde_json::json!({
                "final_scores": final_scores,
                "placements": placements,
            })
            .to_string()
            .into(),
            _ => None,
        })
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

/// 局と局のあいだで待つべきか。
///
/// 初めて呼ばれたときに終わり時刻を決め、それまでは真を返す。**1局目には
/// 間を置かない。**卓に着いた直後に6秒待たされる理由が無い。
fn interlude(now_ms: u64, played_any: bool, resume_at: &mut Option<u64>) -> bool {
    if !played_any {
        return false;
    }
    let until = *resume_at.get_or_insert(now_ms + INTERLUDE_MS);
    now_ms < until
}

async fn run<S: Seeds>(
    mut table: Table,
    mut seeds: S,
    clock: Arc<Clock>,
    mut rx: mpsc::Receiver<TableMsg>,
    recording: Option<Recording>,
) {
    let mut sinks = Sinks::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(POLL_MS));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // 次の局を始めてよくなる時刻。局が終わった時点で決まる。
    let mut resume_at: Option<u64> = None;
    // 1局でも打ったか。**開始の1局目に間を置かない。**
    let mut played_any = false;
    // 牌譜へ渡し終えたところまで。`table.log` への添字。
    let mut written_upto = 0usize;

    loop {
        let now = clock.now_ms();
        sinks.flush(&table, now);
        if table.is_over() {
            break;
        }
        if table.needs_seed() {
            // **局の切れ目で渡す。**落ちても失うのは進行中の1局だけになる
            // （仕様 8.3）。間を置いているあいだ毎周回呼ばれるが、渡し終えた
            // 分は増えない。
            hand_over(&recording, &table, &mut written_upto);
        }
        if table.needs_seed() && !interlude(now, played_any, &mut resume_at) {
            // **シードを受け取ってから局を始める。**Wave 3d はこの `await` の
            // 中で永続化する。ここで待てないと、`seed_commit` を配ったあとに
            // 落ちて別のシードで再開する経路ができてしまう。
            let seed = seeds.next_seed().await;
            let now = clock.now_ms();
            table.begin_round(&seed, now);
            played_any = true;
            resume_at = None;
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

    // 終局。**残りを渡してから閉じる。**最後の局が丸ごと落ちる。
    hand_over(&recording, &table, &mut written_upto);
    if let Some(recording) = &recording {
        recording
            .scribe
            .finish(&recording.record_id, now, match_result_json(&table));
    }
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

#[cfg(test)]
mod recording_tests {
    use super::tests::{one_human_three_cpus, rules};
    use super::*;
    use crate::persistence::{MatchHead, SeatRow, Store};
    use protocol::client_event::ClientEvent;
    use protocol::seat::Seat;

    fn head(id: &str) -> MatchHead {
        MatchHead {
            id: id.to_owned(),
            rules_json: "{}".to_owned(),
            started_ms: 0,
            ended_ms: None,
            players: vec!["p0".into(), "c1".into(), "c2".into(), "c3".into()],
            result_json: None,
        }
    }

    fn seats() -> Vec<SeatRow> {
        (0..4)
            .map(|seat| SeatRow {
                seat,
                name: format!("p{seat}"),
                is_cpu: seat != 0,
                token_hash: (seat == 0).then(|| crate::persistence::hash_token("t")),
                player_key: (seat == 0).then(|| "key".to_owned()),
            })
            .collect()
    }

    /// 牌譜の宛先を渡さなくても卓は動く。**既存の道を塞がない。**
    #[tokio::test(start_paused = true)]
    async fn a_table_without_a_scribe_still_plays() {
        let (handle, _actor) = spawn(
            rules(),
            one_human_three_cpus(),
            SeedSource::from_master([5; 32]),
        );
        let (_, mut inbox) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        let mut seen = 0;
        while inbox.try_recv().is_ok() {
            seen += 1;
        }
        assert!(seen > 0, "牌譜なしで卓が動いていない");
    }

    /// **局の切れ目で塊が増える。**終局まで待たない。
    #[tokio::test(start_paused = true)]
    async fn chunks_grow_as_rounds_finish() {
        let path = std::env::temp_dir().join(format!("mj-round-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = Store::open(&path).expect("開ける");
        store.begin_match(&head("r"), &seats()).expect("書ける");
        let scribe = Scribe::spawn(store);

        let (handle, _actor) = spawn_recorded(
            rules(),
            std::array::from_fn(|i| Occupant::Cpu(protocol::event::PlayerId(format!("c{i}")))),
            SeedSource::from_master([5; 32]),
            Some(Recording {
                scribe,
                record_id: "r".to_owned(),
            }),
        );
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");

        // 3局ぶん進める。半荘は10局前後なので、途中で測れる。
        let mut rounds = 0;
        while let Some(envelope) = watcher.recv().await {
            if matches!(envelope.event, ClientEvent::RoundEnd { .. }) {
                rounds += 1;
                if rounds == 3 {
                    break;
                }
            }
        }
        // 書き手は別 task なので、届くまで待つ。
        let reader = Store::open(&path).expect("開ける");
        let mut chunks = 0;
        for _ in 0..400 {
            chunks = reader.chunk_count("r").expect("数えられる");
            if chunks >= 3 {
                break;
            }
            // **`yield_now` で書き手に順番を回す。**時計を止めた試験では
            // `sleep` は即座に飛ぶので、待っても書き手は動かない。
            tokio::task::yield_now().await;
        }
        assert!(chunks >= 3, "3局終わったのに塊が {chunks} しかない");

        let _ = std::fs::remove_file(&path);
    }

    /// 終局まで打つと、見出しに終わりと順位が入る。
    #[tokio::test(start_paused = true)]
    async fn finishing_writes_the_result() {
        let path = std::env::temp_dir().join(format!("mj-end-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = Store::open(&path).expect("開ける");
        store.begin_match(&head("e"), &seats()).expect("書ける");
        let scribe = Scribe::spawn(store);

        let (handle, _actor) = spawn_recorded(
            rules(),
            std::array::from_fn(|i| Occupant::Cpu(protocol::event::PlayerId(format!("c{i}")))),
            SeedSource::from_master([9; 32]),
            Some(Recording {
                scribe,
                record_id: "e".to_owned(),
            }),
        );
        let (_, mut watcher) = handle
            .attach(Seat::new(0), None)
            .await
            .expect("卓は生きている");
        while watcher.recv().await.is_some() {}

        let reader = Store::open(&path).expect("開ける");
        let mut done = None;
        for _ in 0..400 {
            let head = reader.head("e").expect("引ける").expect("ある");
            if head.ended_ms.is_some() {
                done = head.result_json;
                break;
            }
            tokio::task::yield_now().await;
        }
        let result = done.expect("終局が書かれていない");
        assert!(result.contains("placements"), "{result}");
        assert!(result.contains("final_scores"), "{result}");

        let _ = std::fs::remove_file(&path);
    }

    /// **渡し終えた分をまた渡さない。**間を置いているあいだ毎周回呼ばれる。
    #[tokio::test]
    async fn handing_over_twice_adds_nothing() {
        let table = Table::new(rules(), one_human_three_cpus(), 0);
        let mut upto = 0usize;
        hand_over(&None, &table, &mut upto);
        assert_eq!(upto, 0, "宛先が無いのに進んでいる");

        // 宛先がある場合は、1度目で全部渡り、2度目は空になる。
        let store = Store::open_in_memory().expect("開ける");
        store.begin_match(&head("x"), &seats()).expect("書ける");
        let recording = Some(Recording {
            scribe: Scribe::spawn(store),
            record_id: "x".to_owned(),
        });
        let mut upto = 0usize;
        hand_over(&recording, &table, &mut upto);
        let after_first = upto;
        assert!(after_first > 0, "1度目で何も渡っていない");
        hand_over(&recording, &table, &mut upto);
        assert_eq!(upto, after_first, "2度目でまた渡している");
    }
}
