use serde::{Deserialize, Serialize};

use crate::seat::Seat;
use crate::tile::Tile;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/")]
#[serde(rename_all = "snake_case")]
pub enum MeldKind {
    Chi,
    Pon,
    Ankan,
    Minkan,
    Kakan,
}

/// 副露あるいは暗槓による固定された面子。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/")]
pub struct Meld {
    pub kind: MeldKind,
    pub tiles: Vec<Tile>,
    /// 鳴いた相手。暗槓は None。
    pub from: Option<Seat>,
    /// 鳴きの対象になった牌。暗槓は None。
    pub called_tile: Option<Tile>,
}

impl Meld {
    /// 門前を崩さない面子かどうか（暗槓のみ真）。
    pub fn is_concealed(&self) -> bool {
        self.kind == MeldKind::Ankan
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation::parse_hand;

    #[test]
    fn ankan_is_concealed_and_others_are_not() {
        let ankan = Meld {
            kind: MeldKind::Ankan,
            tiles: parse_hand("1111m").unwrap(),
            from: None,
            called_tile: None,
        };
        assert!(ankan.is_concealed());

        let pon = Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("111m").unwrap(),
            from: Some(crate::seat::Seat::new(2)),
            called_tile: parse_hand("1m").unwrap().first().copied(),
        };
        assert!(!pon.is_concealed());
    }
}
