//! 1つの席へ配信されるイベント。
//!
//! `Event` と別の型にすることで、隠すべき情報を運ぶ場所の多くが構造として消える。
//! `Deal` に他席の手牌を入れるフィールドは無く、`Draw` の牌は `Option` である。
//! `project()` を通す以外にこの型を作る経路を設けない。

use serde::{Deserialize, Serialize};

use crate::command::ActionOption;
use crate::event::{
    AgariResult, ContinuationReason, DiscardManner, DrawSource, NextRound, PlayerId, RiichiStep,
    RyuukyokuKind, Settlement,
};
use crate::meld::MeldKind;
use crate::ruleset::Ruleset;
use crate::seat::{Round, Seat};
use crate::tile::Tile;

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/")]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    MatchStart {
        players: [PlayerId; 4],
        rules: Ruleset,
        you: Seat,
    },
    RoundStart {
        round: Round,
        dealer: Seat,
        honba: u8,
        riichi_sticks: u8,
        scores: [i32; 4],
        seed_commit: String,
    },
    Deal {
        your_hand: Vec<Tile>,
        hand_sizes: [u8; 4],
        dora_indicator: Tile,
    },
    Draw {
        seat: Seat,
        /// 自席のツモのみ Some。
        tile: Option<Tile>,
        source: DrawSource,
        wall_remaining: u8,
    },
    Discard {
        seat: Seat,
        tile: Tile,
        manner: DiscardManner,
    },
    Riichi {
        seat: Seat,
        step: RiichiStep,
    },
    Call {
        seat: Seat,
        from: Seat,
        kind: MeldKind,
        tiles: Vec<Tile>,
    },
    KanDeclared {
        seat: Seat,
        kind: MeldKind,
        tile: Tile,
    },
    DoraReveal {
        indicator: Tile,
    },
    /// 見逃したこと自体は公開情報だが、見逃した具体的な選択肢は
    /// 待ちの手掛かりになるため運ばない。
    ActionPassed {
        seat: Seat,
        window_id: u32,
    },
    Agari {
        results: Vec<AgariResult>,
        settlement: Settlement,
    },
    Ryuukyoku {
        kind: RyuukyokuKind,
        initiator: Option<Seat>,
        tenpai: [bool; 4],
        /// 射影側で公開資格を再判定した結果のみが入る。
        revealed_hands: Vec<(Seat, Vec<Tile>)>,
        nagashi_winners: Vec<Seat>,
        settlement: Settlement,
    },
    RoundEnd {
        scores: [i32; 4],
        next: NextRound,
        reason: ContinuationReason,
    },
    MatchEnd {
        final_scores: [i32; 4],
        placements: [u8; 4],
    },
    /// 自席宛のみ配信されるため seat を持たない。
    RequestAction {
        window_id: u32,
        options: Vec<ActionOption>,
        deadline_ms: u32,
    },
    SeedReveal {
        seeds: Vec<String>,
    },
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/")]
pub struct ClientEventEnvelope {
    pub seq: u32,
    pub event: ClientEvent,
}
