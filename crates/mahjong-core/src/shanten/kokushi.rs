//! 国士無双形の向聴数。門前でしか成立しない。

use protocol::tile::TileKind;

use crate::hand::HandCounts;

pub const IMPOSSIBLE: i8 = 127;

/// 国士無双の向聴数。-1 が和了、0 がテンパイ。
pub fn shanten(counts: &HandCounts, melds: u8) -> i8 {
    if melds > 0 {
        return IMPOSSIBLE;
    }

    let mut kinds = 0i8;
    let mut has_pair = false;
    for index in 0..TileKind::COUNT as u8 {
        let kind = TileKind::from_index(index).expect("範囲内");
        if !kind.is_terminal_or_honor() {
            continue;
        }
        let count = counts.get(kind);
        if count >= 1 {
            kinds += 1;
        }
        if count >= 2 {
            has_pair = true;
        }
    }

    13 - kinds - i8::from(has_pair)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::parse_hand;

    fn counts(notation: &str) -> HandCounts {
        HandCounts::from_tiles(&parse_hand(notation).unwrap())
    }

    #[test]
    fn thirteen_kinds_without_a_pair_is_tenpai() {
        assert_eq!(shanten(&counts("19m19p19s1234567z"), 0), 0);
    }

    #[test]
    fn twelve_kinds_with_a_pair_is_tenpai() {
        assert_eq!(shanten(&counts("119m19p19s123456z"), 0), 0);
    }

    #[test]
    fn thirteen_kinds_with_a_pair_is_a_win() {
        assert_eq!(shanten(&counts("119m19p19s1234567z"), 0), -1);
    }

    #[test]
    fn counts_only_terminals_and_honors() {
        assert_eq!(shanten(&counts("147m258p369s1234z"), 0), 7);
    }

    #[test]
    fn melds_make_kokushi_impossible() {
        assert!(shanten(&counts("19m19p19s1234z"), 1) > 13);
    }
}
