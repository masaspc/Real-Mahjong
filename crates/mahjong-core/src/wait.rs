//! 待ち牌の列挙。

use protocol::tile::TileKind;

use crate::hand::HandCounts;
use crate::shanten::overall;

pub fn waiting_tiles(counts: &HandCounts, melds: u8) -> Vec<TileKind> {
    if !overall::is_tenpai(counts, melds) {
        return Vec::new();
    }
    let mut waits = Vec::new();
    for index in 0..TileKind::COUNT as u8 {
        let kind = TileKind::from_index(index).expect("範囲内");
        if counts.get(kind) >= 4 {
            continue;
        }
        let mut probe = *counts;
        probe.add(kind);
        if overall::is_complete(&probe, melds) {
            waits.push(kind);
        }
    }
    waits
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::{parse_hand, parse_tile};

    fn counts(notation: &str) -> HandCounts {
        HandCounts::from_tiles(&parse_hand(notation).unwrap())
    }
    fn kind(notation: &str) -> TileKind {
        parse_tile(notation).unwrap().kind()
    }

    #[test]
    fn lists_the_tiles_that_complete_the_hand() {
        let waits: Vec<u8> = waiting_tiles(&counts("234567m23478p22s"), 0)
            .iter()
            .map(|k| k.index())
            .collect();
        assert_eq!(waits, vec![kind("6p").index(), kind("9p").index()]);
    }
    #[test]
    fn penchan_waits_on_a_single_tile() {
        let waits: Vec<u8> = waiting_tiles(&counts("12456m234678p55s"), 0)
            .iter()
            .map(|k| k.index())
            .collect();
        assert_eq!(waits, vec![kind("3m").index()]);
    }
    #[test]
    fn kokushi_thirteen_wait_lists_all_terminals_and_honors() {
        assert_eq!(waiting_tiles(&counts("19m19p19s1234567z"), 0).len(), 13);
    }
    #[test]
    fn a_hand_that_is_not_tenpai_has_no_waits() {
        assert!(waiting_tiles(&counts("147m258p369s1234z"), 0).is_empty());
    }
    #[test]
    fn melded_hands_still_report_their_waits() {
        let waits: Vec<u8> = waiting_tiles(&counts("123m456m12p11s"), 1)
            .iter()
            .map(|k| k.index())
            .collect();
        assert_eq!(waits, vec![kind("3p").index()]);
    }
    #[test]
    fn shanpon_waits_on_both_pairs() {
        let waits: Vec<u8> = waiting_tiles(&counts("123m456m789m11p11s"), 0)
            .iter()
            .map(|k| k.index())
            .collect();
        assert_eq!(waits, vec![kind("1p").index(), kind("1s").index()]);
    }
    /// 自分で4枚持っている牌は待ちに含めない。
    #[test]
    fn a_tile_already_held_four_times_is_not_a_wait() {
        let hand = counts("234567m1111p999s");
        assert!(
            waiting_tiles(&hand, 0).is_empty(),
            "残り0枚の牌を待ちとして返した"
        );
    }
}
