//! 牌譜記法 `123m456p789s1234567z` の解釈と整形。
//!
//! 赤ドラは `0m` / `0p` / `0s`。字牌は 1z=東, 2z=南, 3z=西, 4z=北, 5z=白, 6z=發, 7z=中。
//! 全エージェントのテストがこの記法を使うため、ここが唯一の定義である。

use crate::tile::{Suit, Tile, RED_5M, RED_5P, RED_5S};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotationError {
    UnexpectedChar(char),
    EmptyRun(char),
    TrailingDigits,
    HonorOutOfRange(u8),
    ExpectedSingleTile,
}

impl std::fmt::Display for NotationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotationError::UnexpectedChar(c) => write!(f, "予期しない文字 '{c}'"),
            NotationError::EmptyRun(c) => write!(f, "'{c}' の前に数字がありません"),
            NotationError::TrailingDigits => write!(f, "末尾の数字に対応する色がありません"),
            NotationError::HonorOutOfRange(n) => {
                write!(f, "字牌は 1z..7z のみ有効です（{n}z）")
            }
            NotationError::ExpectedSingleTile => write!(f, "牌はちょうど1枚である必要があります"),
        }
    }
}

impl std::error::Error for NotationError {}

pub fn parse_hand(input: &str) -> Result<Vec<Tile>, NotationError> {
    let mut out = Vec::new();
    let mut pending: Vec<u8> = Vec::new();

    for ch in input.chars() {
        match ch {
            '0'..='9' => pending.push(ch as u8 - b'0'),
            'm' | 'p' | 's' | 'z' => {
                if pending.is_empty() {
                    return Err(NotationError::EmptyRun(ch));
                }
                for digit in pending.drain(..) {
                    out.push(tile_from_digit(digit, ch)?);
                }
            }
            other => return Err(NotationError::UnexpectedChar(other)),
        }
    }

    if pending.is_empty() {
        Ok(out)
    } else {
        Err(NotationError::TrailingDigits)
    }
}

pub fn parse_tile(input: &str) -> Result<Tile, NotationError> {
    let tiles = parse_hand(input)?;
    match tiles.as_slice() {
        [only] => Ok(*only),
        _ => Err(NotationError::ExpectedSingleTile),
    }
}

fn tile_from_digit(digit: u8, suit: char) -> Result<Tile, NotationError> {
    if suit == 'z' {
        return match digit {
            1..=7 => Ok(Tile::from_encoded(27 + digit - 1).expect("字牌は範囲内")),
            other => Err(NotationError::HonorOutOfRange(other)),
        };
    }

    let (base, red) = match suit {
        'm' => (0u8, RED_5M),
        'p' => (9, RED_5P),
        _ => (18, RED_5S),
    };

    match digit {
        0 => Ok(Tile::from_encoded(red).expect("赤ドラは範囲内")),
        1..=9 => Ok(Tile::from_encoded(base + digit - 1).expect("数牌は範囲内")),
        other => Err(NotationError::HonorOutOfRange(other)),
    }
}

pub fn to_notation(tiles: &[Tile]) -> String {
    let mut sorted = tiles.to_vec();
    sorted.sort_by_key(|t| (t.kind().index(), t.is_red()));

    let mut groups: [Vec<u8>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for tile in sorted {
        let kind = tile.kind();
        let (group, digit) = match kind.suit() {
            Suit::Man => (0usize, digit_for(&tile)),
            Suit::Pin => (1, digit_for(&tile)),
            Suit::Sou => (2, digit_for(&tile)),
            Suit::Honor => (3, kind.index() - 26),
        };
        groups[group].push(digit);
    }

    const SUIT_CHARS: [char; 4] = ['m', 'p', 's', 'z'];
    let mut out = String::new();
    for (index, group) in groups.iter().enumerate() {
        if group.is_empty() {
            continue;
        }
        for digit in group {
            out.push((b'0' + digit) as char);
        }
        out.push(SUIT_CHARS[index]);
    }
    out
}

fn digit_for(tile: &Tile) -> u8 {
    if tile.is_red() {
        0
    } else {
        tile.kind().number().expect("数牌には番号がある")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_suit() {
        let tiles = parse_hand("19m19p19s17z").unwrap();
        let encoded: Vec<u8> = tiles.iter().map(|t| t.encoded()).collect();
        assert_eq!(encoded, vec![0, 8, 9, 17, 18, 26, 27, 33]);
    }

    #[test]
    fn parses_red_fives() {
        let tiles = parse_hand("0m0p0s").unwrap();
        let encoded: Vec<u8> = tiles.iter().map(|t| t.encoded()).collect();
        assert_eq!(encoded, vec![34, 35, 36]);
        assert!(tiles.iter().all(|t| t.is_red()));
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(parse_hand("8z"), Err(NotationError::HonorOutOfRange(8)));
        assert_eq!(parse_hand("0z"), Err(NotationError::HonorOutOfRange(0)));
        assert_eq!(parse_hand("123"), Err(NotationError::TrailingDigits));
        assert_eq!(parse_hand("m"), Err(NotationError::EmptyRun('m')));
        assert_eq!(parse_hand("1x"), Err(NotationError::UnexpectedChar('x')));
    }

    #[test]
    fn parse_tile_requires_exactly_one() {
        assert_eq!(parse_tile("5p").unwrap().encoded(), 13);
        assert_eq!(parse_tile("55p"), Err(NotationError::ExpectedSingleTile));
        assert_eq!(parse_tile(""), Err(NotationError::ExpectedSingleTile));
    }

    /// 手牌は多重集合であり、to_notation は正規順へ並べ替える。
    /// したがって往復で保存されるのは牌の集まりであって、並び順ではない。
    #[test]
    fn round_trips_through_notation() {
        for input in ["123456789m", "0m5m", "1112223334445z", "19m19p19s1234567z"] {
            let mut tiles = parse_hand(input).unwrap();
            let rendered = to_notation(&tiles);
            let mut reparsed = parse_hand(&rendered).unwrap();
            tiles.sort();
            reparsed.sort();
            assert_eq!(tiles, reparsed, "input={input} rendered={rendered}");
        }
    }

    /// to_notation は入力の並び順によらず同じ文字列を返す。
    #[test]
    fn notation_is_canonical_regardless_of_input_order() {
        let a = parse_hand("0m5m").unwrap();
        let b = parse_hand("5m0m").unwrap();
        assert_eq!(to_notation(&a), to_notation(&b));
        assert_eq!(to_notation(&a), "50m");
    }

    #[test]
    fn notation_groups_by_suit_in_canonical_order() {
        let tiles = parse_hand("1z9s1p3m").unwrap();
        assert_eq!(to_notation(&tiles), "3m1p9s1z");
    }
}
