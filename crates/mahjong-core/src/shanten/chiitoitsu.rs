//! 七対子形の向聴数。門前でしか成立しない。

use crate::hand::HandCounts;

/// 成立しえない場合に返す値。overall で min を取るときに選ばれないよう十分大きくする。
pub const IMPOSSIBLE: i8 = 127;

/// 七対子の向聴数。-1 が和了、0 がテンパイ。
///
/// 対子が7つ揃えば和了。種類が7未満だと、足りない種類の分だけ余計に遠くなる
/// （同じ牌を4枚持っていても2対子にはできないため）。
pub fn shanten(counts: &HandCounts, melds: u8) -> i8 {
    if melds > 0 {
        return IMPOSSIBLE;
    }

    let mut pairs = 0i8;
    let mut kinds = 0i8;
    for (_, count) in counts.kinds() {
        kinds += 1;
        if count >= 2 {
            pairs += 1;
        }
    }

    6 - pairs + (7 - kinds).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::parse_hand;

    fn counts(notation: &str) -> HandCounts {
        HandCounts::from_tiles(&parse_hand(notation).unwrap())
    }

    #[test]
    fn six_pairs_and_a_single_is_tenpai() {
        assert_eq!(shanten(&counts("1122m3344p5566s7z"), 0), 0);
    }

    #[test]
    fn seven_pairs_is_a_win() {
        assert_eq!(shanten(&counts("1122m3344p5566s77z"), 0), -1);
    }

    /// 対子が無ければ 6 シャンテン。
    #[test]
    fn no_pairs_is_six_away() {
        assert_eq!(shanten(&counts("147m258p369s1234z"), 0), 6);
    }

    /// 4枚持ちは1対子としてしか数えられない。種類が足りない分だけ余計に遠い。
    #[test]
    fn four_of_a_kind_counts_as_one_pair_only() {
        let hand = counts("1111222233334m");
        assert_eq!(shanten(&hand, 0), 6);
    }

    /// 副露していると七対子は成立しない。
    #[test]
    fn melds_make_chiitoitsu_impossible() {
        assert!(shanten(&counts("1122m3344p55s"), 1) > 8);
    }
}
