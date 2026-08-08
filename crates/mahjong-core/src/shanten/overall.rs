//! 3形を統合した向聴数。呼び出し側は原則これを使う。

use crate::hand::HandCounts;
use crate::shanten::{chiitoitsu, kokushi, standard};

pub fn shanten(counts: &HandCounts, melds: u8) -> i8 {
    standard::shanten(counts, melds)
        .min(chiitoitsu::shanten(counts, melds))
        .min(kokushi::shanten(counts, melds))
}

pub fn is_tenpai(counts: &HandCounts, melds: u8) -> bool {
    shanten(counts, melds) == 0
}

pub fn is_complete(counts: &HandCounts, melds: u8) -> bool {
    shanten(counts, melds) == -1
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::parse_hand;

    fn counts(notation: &str) -> HandCounts {
        HandCounts::from_tiles(&parse_hand(notation).unwrap())
    }

    #[test]
    fn matches_every_fixture() {
        for case in test_fixtures::load_shanten_cases() {
            let hand = counts(&case.concealed);
            let actual = shanten(&hand, case.melds);
            assert_eq!(
                actual, case.expect.overall,
                "{}: {} → overall が {} だが期待は {}（{}）",
                case.id, case.concealed, actual, case.expect.overall, case.note
            );
        }
    }

    #[test]
    fn matches_declared_individual_forms() {
        for case in test_fixtures::load_shanten_cases() {
            let hand = counts(&case.concealed);
            if let Some(expected) = case.expect.chiitoitsu {
                assert_eq!(
                    crate::shanten::chiitoitsu::shanten(&hand, case.melds),
                    expected,
                    "{}: 七対子形",
                    case.id
                );
            }
            if let Some(expected) = case.expect.kokushi {
                assert_eq!(
                    crate::shanten::kokushi::shanten(&hand, case.melds),
                    expected,
                    "{}: 国士形",
                    case.id
                );
            }
        }
    }

    #[test]
    fn tenpai_and_complete_agree_with_shanten() {
        let tenpai = counts("123m456m789m12p11s");
        assert!(is_tenpai(&tenpai, 0));
        assert!(!is_complete(&tenpai, 0));
        let won = counts("123456789m123p11s");
        assert!(is_complete(&won, 0));
        assert!(!is_tenpai(&won, 0));
    }

    #[test]
    fn melded_hands_ignore_menzen_only_forms() {
        assert_eq!(shanten(&counts("123m456m12p11s"), 1), 0);
    }
}
