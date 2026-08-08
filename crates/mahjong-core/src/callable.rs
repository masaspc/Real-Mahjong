//! 鳴きの候補を列挙する。ロンの可否は Wave 2 で判定する。

use protocol::meld::{Meld, MeldKind};
use protocol::tile::{Tile, TileKind};

fn tiles_of_kind(hand: &[Tile], kind: TileKind) -> Vec<Tile> {
    let mut found: Vec<Tile> = hand.iter().copied().filter(|t| t.kind() == kind).collect();
    found.sort_by_key(|t| t.is_red());
    found
}

pub fn chi_candidates(hand: &[Tile], discarded: Tile) -> Vec<[Tile; 2]> {
    let Some(number) = discarded.kind().number() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for lowest in [number.wrapping_sub(2), number.wrapping_sub(1), number] {
        if lowest == 0 || lowest > 7 || lowest > number || number > lowest + 2 {
            continue;
        }
        let needed: Vec<u8> = (lowest..lowest + 3).filter(|n| *n != number).collect();

        // 各必要牌について、手にある実物をすべて挙げる。
        // 赤5と通常5では打点が変わるため、どちらを使うかはプレイヤーが選ぶ。
        let mut choices: Vec<Vec<Tile>> = Vec::new();
        for n in &needed {
            let Some(kind) = kind_in_same_suit(discarded.kind(), *n) else {
                choices.clear();
                break;
            };
            let available = tiles_of_kind(hand, kind);
            if available.is_empty() {
                choices.clear();
                break;
            }
            choices.push(available);
        }
        if choices.len() != 2 {
            continue;
        }

        for first in &choices[0] {
            for second in &choices[1] {
                let pair = [*first, *second];
                if !out.contains(&pair) {
                    out.push(pair);
                }
            }
        }
    }
    out
}

pub fn pon_candidates(hand: &[Tile], discarded: Tile) -> Vec<[Tile; 2]> {
    let available = tiles_of_kind(hand, discarded.kind());
    if available.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..available.len() {
        for j in (i + 1)..available.len() {
            let pair = [available[i], available[j]];
            if !out.contains(&pair) {
                out.push(pair);
            }
        }
    }
    out
}

pub fn minkan_possible(hand: &[Tile], discarded: Tile) -> bool {
    tiles_of_kind(hand, discarded.kind()).len() >= 3
}

pub fn ankan_candidates(hand: &[Tile]) -> Vec<TileKind> {
    let mut out = Vec::new();
    for index in 0..TileKind::COUNT as u8 {
        let kind = TileKind::from_index(index).expect("範囲内");
        if tiles_of_kind(hand, kind).len() >= 4 {
            out.push(kind);
        }
    }
    out
}

pub fn kakan_candidates(hand: &[Tile], melds: &[Meld]) -> Vec<Tile> {
    let mut out = Vec::new();
    for meld in melds {
        if meld.kind != MeldKind::Pon {
            continue;
        }
        let Some(kind) = meld.tiles.first().map(|t| t.kind()) else {
            continue;
        };
        for tile in tiles_of_kind(hand, kind) {
            if !out.contains(&tile) {
                out.push(tile);
            }
        }
    }
    out
}

fn kind_in_same_suit(reference: TileKind, number: u8) -> Option<TileKind> {
    let current = reference.number()?;
    if !(1..=9).contains(&number) {
        return None;
    }
    let base = reference.index() - (current - 1);
    TileKind::from_index(base + number - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::meld::{Meld, MeldKind};
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::seat::Seat;

    fn notation_of(pairs: &[[Tile; 2]]) -> Vec<String> {
        pairs
            .iter()
            .map(|p| protocol::notation::to_notation(p))
            .collect()
    }

    #[test]
    fn chi_lists_every_way_to_form_a_run() {
        assert_eq!(
            notation_of(&chi_candidates(
                &parse_hand("1245p").unwrap(),
                parse_tile("3p").unwrap()
            )),
            vec!["12p", "24p", "45p"]
        );
    }
    /// 赤5と通常5では打点が変わる。どちらを使うかを選べるよう別候補で返す。
    #[test]
    fn chi_offers_both_the_red_and_the_normal_five() {
        // 4p を鳴く。手に 3p と 5p(通常) と 0p(赤5) があるので 35p と 30p の2通り。
        let hand = parse_hand("3p5p0p").unwrap();
        let candidates = chi_candidates(&hand, parse_tile("4p").unwrap());
        assert_eq!(
            candidates.len(),
            2,
            "赤と通常の両方が候補になるべき: {:?}",
            notation_of(&candidates)
        );
        let rendered = notation_of(&candidates);
        assert!(rendered.contains(&"35p".to_owned()), "{rendered:?}");
        assert!(rendered.contains(&"30p".to_owned()), "{rendered:?}");
    }

    #[test]
    fn chi_does_not_cross_suits() {
        // 1p を含む順子は 123p だけ。2p3p が揃っていれば鳴ける。
        let hand = parse_hand("89m23p").unwrap();
        assert!(!chi_candidates(&hand, parse_tile("1p").unwrap()).is_empty());

        // 89m があっても 9m と 1p は繋がらない。
        let hand = parse_hand("89m").unwrap();
        assert!(chi_candidates(&hand, parse_tile("1p").unwrap()).is_empty());
    }
    #[test]
    fn honors_cannot_be_chied() {
        assert!(
            chi_candidates(&parse_hand("1234567z").unwrap(), parse_tile("2z").unwrap()).is_empty()
        );
    }
    #[test]
    fn pon_distinguishes_red_fives() {
        assert_eq!(
            notation_of(&pon_candidates(
                &parse_hand("55p0p").unwrap(),
                parse_tile("5p").unwrap()
            )),
            vec!["55p", "50p"]
        );
    }
    #[test]
    fn pon_needs_two_matching_tiles() {
        assert!(pon_candidates(&parse_hand("5p").unwrap(), parse_tile("5p").unwrap()).is_empty());
    }
    #[test]
    fn minkan_needs_three_matching_tiles() {
        assert!(minkan_possible(
            &parse_hand("555p").unwrap(),
            parse_tile("5p").unwrap()
        ));
        assert!(!minkan_possible(
            &parse_hand("55p").unwrap(),
            parse_tile("5p").unwrap()
        ));
    }
    #[test]
    fn ankan_needs_four_in_hand() {
        let candidates: Vec<u8> = ankan_candidates(&parse_hand("5555p111m").unwrap())
            .iter()
            .map(|k| k.index())
            .collect();
        assert_eq!(candidates, vec![parse_tile("5p").unwrap().kind().index()]);
    }
    #[test]
    fn kakan_extends_an_existing_pon() {
        let melds = vec![Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("555p").unwrap(),
            from: Some(Seat::new(1)),
            called_tile: Some(parse_tile("5p").unwrap()),
        }];
        let candidates = kakan_candidates(&parse_hand("5p1m").unwrap(), &melds);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].kind(), parse_tile("5p").unwrap().kind());
    }
    #[test]
    fn kakan_does_not_apply_to_chi() {
        let melds = vec![Meld {
            kind: MeldKind::Chi,
            tiles: parse_hand("345p").unwrap(),
            from: Some(Seat::new(3)),
            called_tile: Some(parse_tile("3p").unwrap()),
        }];
        assert!(kakan_candidates(&parse_hand("5p").unwrap(), &melds).is_empty());
    }
}
