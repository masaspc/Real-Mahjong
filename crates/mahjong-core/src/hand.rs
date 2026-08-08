//! 手牌の集計表現。判定系（Wave 1a）と点数系（Wave 1b）が共有するため
//! Wave 0 で凍結する。**Wave 1 では編集しないこと。**

use protocol::tile::{Tile, TileKind};

/// 34種それぞれの枚数。赤ドラは対応する通常牌として数える。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandCounts([u8; TileKind::COUNT]);

// 配列の Default は32要素までしか導出できないため手で書く。
impl Default for HandCounts {
    fn default() -> Self {
        HandCounts::new()
    }
}

impl HandCounts {
    pub fn new() -> Self {
        HandCounts([0; TileKind::COUNT])
    }

    pub fn from_tiles(tiles: &[Tile]) -> Self {
        let mut counts = HandCounts::new();
        for tile in tiles {
            counts.add(tile.kind());
        }
        counts
    }

    pub fn get(&self, kind: TileKind) -> u8 {
        self.0[kind.index() as usize]
    }

    pub fn add(&mut self, kind: TileKind) {
        self.0[kind.index() as usize] += 1;
    }

    /// 1枚取り除く。0枚なら false を返して何もしない。
    pub fn remove(&mut self, kind: TileKind) -> bool {
        let slot = &mut self.0[kind.index() as usize];
        if *slot == 0 {
            false
        } else {
            *slot -= 1;
            true
        }
    }

    pub fn total(&self) -> u8 {
        self.0.iter().sum()
    }

    /// 1枚以上ある種類だけを、種類の昇順で返す。
    pub fn kinds(&self) -> impl Iterator<Item = (TileKind, u8)> + '_ {
        self.0
            .iter()
            .enumerate()
            .filter(|(_, &count)| count > 0)
            .map(|(index, &count)| (TileKind::from_index(index as u8).expect("範囲内"), count))
    }

    pub fn as_array(&self) -> &[u8; TileKind::COUNT] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::parse_hand;
    use protocol::tile::TileKind;

    #[test]
    fn counts_tiles_by_kind_ignoring_red() {
        // 0p は赤5pであり、5p と同じ種類として数える。
        let counts = HandCounts::from_tiles(&parse_hand("55p0p").unwrap());
        assert_eq!(counts.get(TileKind::from_index(13).unwrap()), 3);
        assert_eq!(counts.total(), 3);
    }

    #[test]
    fn add_and_remove_round_trip() {
        let mut counts = HandCounts::new();
        let east = TileKind::from_index(27).unwrap();
        counts.add(east);
        counts.add(east);
        assert_eq!(counts.get(east), 2);
        assert!(counts.remove(east));
        assert_eq!(counts.get(east), 1);
        assert!(counts.remove(east));
        assert!(!counts.remove(east), "0枚からは取り除けない");
    }

    #[test]
    fn kinds_lists_only_present_tiles() {
        let counts = HandCounts::from_tiles(&parse_hand("111m9s").unwrap());
        let present: Vec<(u8, u8)> = counts.kinds().map(|(k, n)| (k.index(), n)).collect();
        assert_eq!(present, vec![(0, 3), (26, 1)]);
    }

    #[test]
    fn a_full_hand_totals_fourteen() {
        let counts = HandCounts::from_tiles(&parse_hand("123456789m123p11s").unwrap());
        assert_eq!(counts.total(), 14);
    }
}
