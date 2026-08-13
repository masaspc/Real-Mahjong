//! CPU 4人の半荘を流して、席0から見える範囲を牌譜として出す。
//!
//! **これは動作を目で確かめるための道具であり、製品の一部ではない。**
//! 遊ぶための画面とネットワークは Wave 3d と 3e で入る。
//!
//! ```text
//! cargo run -p server --example watch_a_match           # 仮想時間で一気に流す
//! cargo run -p server --example watch_a_match -- live   # 実時間で眺める（数分かかる）
//! cargo run -p server --example watch_a_match -- 7      # シードを変える
//! ```

use protocol::client_event::ClientEvent;
use protocol::event::PlayerId;
use protocol::notation::to_notation;
use protocol::ruleset::{MatchLength, Ruleset};
use protocol::seat::Seat;
use protocol::tile::Tile;
use server::session::{spawn, SeedSource};
use server::table::Occupant;

fn seat_name(seat: Seat) -> String {
    format!("席{}", seat.index())
}

fn tiles(list: &[Tile]) -> String {
    to_notation(list)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let live = args.iter().any(|a| a == "live");
    let master = args.iter().find_map(|a| a.parse::<u8>().ok()).unwrap_or(1);

    if !live {
        // 仮想時間で回す。数分ぶんの対局が数秒で終わる。
        tokio::time::pause();
    }

    println!("=== CPU 4人の半荘（シード {master}、席0から見える範囲）===");
    if live {
        println!("実時間で流します。実際の対局と同じ速さなので数分かかります。");
    }
    println!();

    let (handle, actor) = spawn(
        Ruleset::kin_no_ma(MatchLength::Hanchan),
        std::array::from_fn(|i| Occupant::Cpu(PlayerId(format!("cpu{i}")))),
        SeedSource::from_master([master; 32]),
    );
    let (_, mut inbox) = handle
        .attach(Seat::new(0), None)
        .await
        .expect("卓は生きている");

    let mut events = 0usize;
    while let Some(envelope) = inbox.recv().await {
        events += 1;
        match &envelope.event {
            ClientEvent::MatchStart { players, you, .. } => {
                println!(
                    "対局開始。あなたは {}。参加者 {:?}",
                    seat_name(*you),
                    players
                );
            }
            ClientEvent::RoundStart {
                round,
                dealer,
                honba,
                riichi_sticks,
                scores,
                seed_commit,
            } => {
                println!();
                println!(
                    "── {:?} {}本場 親={} 供託{}本 点数{:?}",
                    round,
                    honba,
                    seat_name(*dealer),
                    riichi_sticks,
                    scores
                );
                println!(
                    "   山のハッシュ {}…（終局後に開示される）",
                    &seed_commit[..16]
                );
            }
            ClientEvent::Deal {
                your_hand,
                dora_indicator,
                ..
            } => {
                println!(
                    "   配牌 {}   ドラ表示 {}",
                    tiles(your_hand),
                    tiles(&[*dora_indicator])
                );
            }
            ClientEvent::Draw {
                seat,
                tile,
                wall_remaining,
                ..
            } => match tile {
                Some(t) => println!(
                    "   {} ツモ {}  （残り{}）",
                    seat_name(*seat),
                    tiles(&[*t]),
                    wall_remaining
                ),
                None => println!(
                    "   {} ツモ　　  （残り{}）",
                    seat_name(*seat),
                    wall_remaining
                ),
            },
            ClientEvent::Discard { seat, tile, .. } => {
                println!("   {} 打 {}", seat_name(*seat), tiles(&[*tile]));
            }
            ClientEvent::Riichi { seat, step } => {
                println!("   ★ {} リーチ（{:?}）", seat_name(*seat), step);
            }
            ClientEvent::Call {
                seat,
                from,
                kind,
                tiles: used,
            } => {
                println!(
                    "   ● {} が {} から {:?}  {}",
                    seat_name(*seat),
                    seat_name(*from),
                    kind,
                    tiles(used)
                );
            }
            ClientEvent::KanDeclared { seat, kind, tile } => {
                println!(
                    "   ● {} カン {:?} {}",
                    seat_name(*seat),
                    kind,
                    tiles(&[*tile])
                );
            }
            ClientEvent::DoraReveal { indicator } => {
                println!("   新ドラ表示 {}", tiles(&[*indicator]));
            }
            ClientEvent::Agari {
                results,
                settlement,
            } => {
                for r in results {
                    let how = match r.from {
                        Some(f) => format!("ロン（放銃 {}）", seat_name(f)),
                        None => "ツモ".to_owned(),
                    };
                    println!(
                        "   ◆ {} {} {}翻{}符 素点{}",
                        seat_name(r.seat),
                        how,
                        r.han,
                        r.fu,
                        r.score
                    );
                    println!(
                        "      手牌 {}  和了牌 {}",
                        tiles(&r.hand),
                        tiles(&[r.win_tile])
                    );
                    let names: Vec<String> =
                        r.yaku.iter().map(|(y, h)| format!("{y:?}{h}")).collect();
                    println!("      役 {}", names.join(" "));
                }
                println!("      収支 {:?}", settlement.delta);
            }
            ClientEvent::Ryuukyoku {
                kind,
                tenpai,
                nagashi_winners,
                settlement,
                ..
            } => {
                println!("   ◆ 流局 {kind:?}  テンパイ{tenpai:?}");
                if !nagashi_winners.is_empty() {
                    println!("      流し満貫 {nagashi_winners:?}");
                }
                println!("      収支 {:?}", settlement.delta);
            }
            ClientEvent::RoundEnd { scores, reason, .. } => {
                println!("   局終了（{reason:?}） 点数 {scores:?}");
            }
            ClientEvent::MatchEnd {
                final_scores,
                placements,
            } => {
                println!();
                println!("=== 終局 ===");
                for i in 0..4 {
                    println!(
                        "   {} {}位  {}点",
                        seat_name(Seat::new(i as u8)),
                        placements[i],
                        final_scores[i]
                    );
                }
                let total: i32 = final_scores.iter().sum();
                println!("   合計 {total}点（4人×25,000 = 100,000。点棒は増えも減りもしない）");
            }
            ClientEvent::SeedReveal { seeds } => {
                println!();
                println!("   山のシード開示（局頭のハッシュと照合できる）");
                for (i, s) in seeds.iter().enumerate() {
                    println!("     {}局目 {}…", i + 1, &s[..16]);
                }
            }
            _ => {}
        }
    }

    let _ = actor.await;
    println!();
    println!("席0へ届いたイベント {events} 件。");
}
