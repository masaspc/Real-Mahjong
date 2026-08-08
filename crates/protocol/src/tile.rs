use serde::{Deserialize, Serialize};

/// 牌の種類（赤ドラを区別しない34種）。
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "../../../apps/web/src/protocol/")]
#[serde(transparent)]
pub struct TileKind(u8);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/")]
pub enum Suit {
    Man,
    Pin,
    Sou,
    Honor,
}

impl TileKind {
    pub const COUNT: usize = 34;

    pub fn from_index(index: u8) -> Option<Self> {
        (index < Self::COUNT as u8).then_some(TileKind(index))
    }

    pub fn index(self) -> u8 {
        self.0
    }

    pub fn suit(self) -> Suit {
        match self.0 {
            0..=8 => Suit::Man,
            9..=17 => Suit::Pin,
            18..=26 => Suit::Sou,
            _ => Suit::Honor,
        }
    }

    /// 数牌なら 1..=9、字牌なら None。
    pub fn number(self) -> Option<u8> {
        (self.0 < 27).then(|| self.0 % 9 + 1)
    }

    pub fn is_honor(self) -> bool {
        self.0 >= 27
    }

    pub fn is_terminal(self) -> bool {
        matches!(self.number(), Some(1) | Some(9))
    }

    pub fn is_terminal_or_honor(self) -> bool {
        self.is_honor() || self.is_terminal()
    }
}

/// 場に存在する1枚の牌。赤ドラを区別する37値のエンコードを持つ。
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "../../../apps/web/src/protocol/")]
#[serde(transparent)]
pub struct Tile(u8);

pub(crate) const RED_5M: u8 = 34;
pub(crate) const RED_5P: u8 = 35;
pub(crate) const RED_5S: u8 = 36;

impl Tile {
    pub const ENCODED_COUNT: usize = 37;

    pub fn from_encoded(encoded: u8) -> Option<Self> {
        (encoded < Self::ENCODED_COUNT as u8).then_some(Tile(encoded))
    }

    pub fn from_kind(kind: TileKind) -> Self {
        Tile(kind.index())
    }

    pub fn encoded(self) -> u8 {
        self.0
    }

    pub fn is_red(self) -> bool {
        self.0 >= RED_5M
    }

    pub fn kind(self) -> TileKind {
        match self.0 {
            RED_5M => TileKind(4),
            RED_5P => TileKind(13),
            RED_5S => TileKind(22),
            other => TileKind(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_classifies_suits_and_numbers() {
        let m1 = TileKind::from_index(0).unwrap();
        assert_eq!(m1.suit(), Suit::Man);
        assert_eq!(m1.number(), Some(1));
        assert!(m1.is_terminal());

        let p5 = TileKind::from_index(13).unwrap();
        assert_eq!(p5.suit(), Suit::Pin);
        assert_eq!(p5.number(), Some(5));
        assert!(!p5.is_terminal_or_honor());

        let s9 = TileKind::from_index(26).unwrap();
        assert_eq!(s9.suit(), Suit::Sou);
        assert_eq!(s9.number(), Some(9));
        assert!(s9.is_terminal());

        let chun = TileKind::from_index(33).unwrap();
        assert_eq!(chun.suit(), Suit::Honor);
        assert_eq!(chun.number(), None);
        assert!(chun.is_honor());
        assert!(chun.is_terminal_or_honor());

        assert_eq!(TileKind::from_index(34), None);
    }

    #[test]
    fn red_five_maps_back_to_its_kind() {
        let red_m = Tile::from_encoded(34).unwrap();
        assert!(red_m.is_red());
        assert_eq!(red_m.kind().index(), 4);

        let red_p = Tile::from_encoded(35).unwrap();
        assert_eq!(red_p.kind().index(), 13);

        let red_s = Tile::from_encoded(36).unwrap();
        assert_eq!(red_s.kind().index(), 22);

        let plain = Tile::from_encoded(4).unwrap();
        assert!(!plain.is_red());
        assert_eq!(plain.kind(), red_m.kind());

        assert_eq!(Tile::from_encoded(37), None);
    }

    #[test]
    fn tile_serializes_as_a_bare_number() {
        let json = serde_json::to_string(&Tile::from_encoded(34).unwrap()).unwrap();
        assert_eq!(json, "34");
        let back: Tile = serde_json::from_str("34").unwrap();
        assert_eq!(back.encoded(), 34);
    }
}
