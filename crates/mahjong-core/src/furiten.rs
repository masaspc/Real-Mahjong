//! 振聴の判定。状態の管理は engine の責務であり、ここは判定だけを持つ。

use protocol::tile::{Tile, TileKind};

pub fn is_furiten_by_discards(waits: &[TileKind], own_discards: &[Tile]) -> bool {
    own_discards.iter().any(|tile| waits.contains(&tile.kind()))
}

pub fn is_temporary_furiten(waits: &[TileKind], passed_since_draw: &[TileKind]) -> bool {
    passed_since_draw.iter().any(|kind| waits.contains(kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::{parse_hand, parse_tile};

    fn kinds(notation: &str) -> Vec<TileKind> {
        parse_hand(notation)
            .unwrap()
            .iter()
            .map(|t| t.kind())
            .collect()
    }

    #[test]
    fn discarding_any_waiting_tile_causes_furiten() {
        assert!(is_furiten_by_discards(
            &kinds("6p9p"),
            &parse_hand("1m9p3s").unwrap()
        ));
    }
    #[test]
    fn unrelated_discards_do_not_cause_furiten() {
        assert!(!is_furiten_by_discards(
            &kinds("6p9p"),
            &parse_hand("1m2m3s").unwrap()
        ));
    }
    #[test]
    fn red_fives_count_as_their_normal_kind() {
        assert!(is_furiten_by_discards(
            &kinds("5p"),
            &[parse_tile("0p").unwrap()]
        ));
    }
    #[test]
    fn passing_on_a_waiting_tile_causes_temporary_furiten() {
        assert!(is_temporary_furiten(&kinds("6p9p"), &kinds("6p")));
        assert!(!is_temporary_furiten(&kinds("6p9p"), &kinds("1m")));
    }
    #[test]
    fn no_waits_means_no_furiten() {
        assert!(!is_furiten_by_discards(&[], &parse_hand("1m").unwrap()));
        assert!(!is_temporary_furiten(&[], &kinds("1m")));
    }
}
