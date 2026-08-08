//! 点数計算。符と翻から支払いを決める。
//!
//! `WinType` / `HandContext` / `Payment` / `ScoreResult` はここで定義し、
//! `yaku_check` と `fu` から参照する。

use protocol::seat::Wind;
use protocol::tile::Tile;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinType {
    Tsumo,
    Ron,
}

/// 役と符の判定に必要な、手牌の外側の情報。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HandContext {
    pub win_type: WinType,
    pub seat_wind: Wind,
    pub round_wind: Wind,
    pub riichi: bool,
    pub double_riichi: bool,
    pub ippatsu: bool,
    pub rinshan: bool,
    pub chankan: bool,
    pub haitei: bool,
    pub houtei: bool,
    pub tenhou: bool,
    pub chiihou: bool,
    pub dora_indicators: Vec<Tile>,
    pub ura_indicators: Vec<Tile>,
}

impl HandContext {
    /// 状況役がすべて無い文脈。テストの土台に使う。
    pub fn plain(win_type: WinType, seat_wind: Wind, round_wind: Wind) -> Self {
        HandContext {
            win_type,
            seat_wind,
            round_wind,
            riichi: false,
            double_riichi: false,
            ippatsu: false,
            rinshan: false,
            chankan: false,
            haitei: false,
            houtei: false,
            tenhou: false,
            chiihou: false,
            dora_indicators: Vec::new(),
            ura_indicators: Vec::new(),
        }
    }

    pub fn is_dealer(&self) -> bool {
        self.seat_wind == Wind::East
    }
}
