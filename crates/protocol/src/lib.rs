//! サーバとクライアントが共有する契約。
//!
//! イベント・コマンド・視界フィルタ・演出カタログをここで凍結する。
//! 実装の各層はこのクレートにのみ依存し、互いには依存しない。

pub mod command;
pub mod event;
pub mod meld;
pub mod notation;
pub mod ruleset;
pub mod seat;
pub mod tile;
pub mod yaku;
