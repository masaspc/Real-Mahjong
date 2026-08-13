//! CPU 4人の半荘を席0の視点で JSON 行として書き出す。
//!
//! **クライアントの盤面組み立てを本物のイベント列で検証するための道具。**
//! 出力は `apps/web/src/game/__fixtures__/` へ置く。
//!
//! ```text
//! cargo run -q -p server --example dump_match -- 1 > apps/web/src/game/__fixtures__/match-seed1.jsonl
//! ```

use protocol::event::PlayerId;
use protocol::ruleset::{MatchLength, Ruleset};
use protocol::seat::Seat;
use server::session::{spawn, SeedSource};
use server::table::Occupant;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tokio::time::pause();
    let master: u8 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1);
    let (handle, _actor) = spawn(
        Ruleset::kin_no_ma(MatchLength::Hanchan),
        std::array::from_fn(|i| Occupant::Cpu(PlayerId(format!("cpu{i}")))),
        SeedSource::from_master([master; 32]),
    );
    let (_, mut inbox) = handle
        .attach(Seat::new(0), None)
        .await
        .expect("卓は生きている");
    while let Some(envelope) = inbox.recv().await {
        println!(
            "{}",
            serde_json::to_string(&envelope).expect("直列化できる")
        );
    }
}
