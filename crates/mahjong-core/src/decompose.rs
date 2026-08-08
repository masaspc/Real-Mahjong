//! 和了形の分解。役判定と符計算はすべてこの出力を入力にする。
//!
//! 同じ手が複数通りに分解できる場合はすべて返す。どれを採るかは
//! 点数が最大になるものを選ぶ `score.rs` の責務である。

use protocol::meld::Meld;
use protocol::tile::{Tile, TileKind};

use crate::hand::HandCounts;
use crate::shapes::{Block, Decomposition, WaitShape};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WinForm {
    Standard(Decomposition),
    Chiitoitsu { pairs: Vec<TileKind> },
    Kokushi { pair: TileKind, thirteen_wait: bool },
}

/// 和了牌を含まない手の内の牌から、成立している和了形をすべて挙げる。
/// 和了形でなければ空を返す。
pub fn decompose(hand: &[Tile], melds: &[Meld], win_tile: Tile) -> Vec<WinForm> {
    let mut counts = HandCounts::from_tiles(hand);
    counts.add(win_tile.kind());

    let mut out = Vec::new();

    // 標準形。副露していても成立する。
    for (blocks, pair) in standard_decompositions(&counts, melds.len()) {
        let wait = wait_shape_of(&blocks, pair, win_tile.kind());
        out.push(WinForm::Standard(Decomposition {
            blocks,
            pair,
            melds: melds.to_vec(),
            wait,
        }));
    }

    // 七対子と国士は門前のみ。
    if melds.is_empty() {
        if let Some(pairs) = chiitoitsu_pairs(&counts) {
            out.push(WinForm::Chiitoitsu { pairs });
        }
        if let Some((pair, thirteen_wait)) = kokushi_shape(&counts, win_tile.kind()) {
            out.push(WinForm::Kokushi {
                pair,
                thirteen_wait,
            });
        }
    }

    out
}

/// 4面子1雀頭に分ける全通り。副露分は面子数に数える。
fn standard_decompositions(counts: &HandCounts, called: usize) -> Vec<(Vec<Block>, TileKind)> {
    let needed_sets = 4 - called;
    let mut out = Vec::new();

    for index in 0..TileKind::COUNT as u8 {
        let pair_kind = TileKind::from_index(index).expect("範囲内");
        if counts.get(pair_kind) < 2 {
            continue;
        }
        let mut work = *counts;
        work.remove(pair_kind);
        work.remove(pair_kind);

        let mut blocks = Vec::new();
        collect_sets(&mut work, 0, needed_sets, &mut blocks, &mut |found| {
            out.push((found.to_vec(), pair_kind));
        });
    }

    // 同じ分解が雀頭の選び方違いで重複しうるので、正規化して1つにまとめる。
    out.sort_by_key(|entry| sort_key(&entry.0));
    out.dedup_by(|a, b| sort_key(&a.0) == sort_key(&b.0));
    out
}

fn sort_key(blocks: &[Block]) -> Vec<(u8, u8)> {
    blocks
        .iter()
        .map(|b| (b.kind_index(), b.number_or_zero()))
        .collect()
}

/// `remaining` 個の面子をちょうど取り出す全通りを列挙する。
fn collect_sets(
    counts: &mut HandCounts,
    index: u8,
    remaining: usize,
    blocks: &mut Vec<Block>,
    found: &mut impl FnMut(&[Block]),
) {
    if remaining == 0 {
        if counts.total() == 0 {
            found(blocks);
        }
        return;
    }
    if index >= TileKind::COUNT as u8 {
        return;
    }

    let kind = TileKind::from_index(index).expect("範囲内");
    if counts.get(kind) == 0 {
        collect_sets(counts, index + 1, remaining, blocks, found);
        return;
    }

    // 刻子
    if counts.get(kind) >= 3 {
        for _ in 0..3 {
            counts.remove(kind);
        }
        blocks.push(Block::Triplet(kind));
        collect_sets(counts, index, remaining - 1, blocks, found);
        blocks.pop();
        for _ in 0..3 {
            counts.add(kind);
        }
    }

    // 順子
    if let Some(run) = run_from(kind) {
        if run.iter().all(|k| counts.get(*k) >= 1) {
            for k in &run {
                counts.remove(*k);
            }
            blocks.push(Block::Run(kind));
            collect_sets(counts, index, remaining - 1, blocks, found);
            blocks.pop();
            for k in &run {
                counts.add(*k);
            }
        }
    }
}

fn chiitoitsu_pairs(counts: &HandCounts) -> Option<Vec<TileKind>> {
    let mut pairs = Vec::new();
    for (kind, count) in counts.kinds() {
        if count != 2 {
            return None;
        }
        pairs.push(kind);
    }
    (pairs.len() == 7).then_some(pairs)
}

/// 国士無双の形。対子になっている幺九牌と、十三面待ちだったかを返す。
fn kokushi_shape(counts: &HandCounts, win_kind: TileKind) -> Option<(TileKind, bool)> {
    let mut pair = None;
    let mut kinds = 0;
    for (kind, count) in counts.kinds() {
        if !kind.is_terminal_or_honor() {
            return None;
        }
        kinds += 1;
        match count {
            1 => {}
            2 if pair.is_none() => pair = Some(kind),
            _ => return None,
        }
    }
    if kinds != 13 {
        return None;
    }
    let pair = pair?;
    // 和了牌が対子を作ったなら、和了前は13種すべてを1枚ずつ持っていた
    // ことになる（＝十三面待ち）。対子が既にあったなら、欠けていた1種を
    // 待っていた単騎である。
    Some((pair, pair == win_kind))
}

fn run_from(kind: TileKind) -> Option<[TileKind; 3]> {
    let number = kind.number()?;
    if number > 7 {
        return None;
    }
    Some([
        kind,
        TileKind::from_index(kind.index() + 1)?,
        TileKind::from_index(kind.index() + 2)?,
    ])
}

/// その分解のもとで、和了牌がどの役割だったかを答える。
///
/// 待ち形は「どう分解したか」に依存する。同じ手でも分解が違えば待ち形が
/// 変わりうるため、分解と同じ場所に置く。
///
/// 判定の順序に意味がある。雀頭と一致するなら単騎、刻子の一部なら双碰、
/// 順子の一部なら位置で両面／嵌張／辺張を分ける。
pub fn wait_shape_of(blocks: &[Block], pair: TileKind, win_tile: TileKind) -> WaitShape {
    if win_tile == pair {
        return WaitShape::Tanki;
    }

    for block in blocks {
        match *block {
            Block::Triplet(kind) | Block::Pair(kind) if kind == win_tile => {
                return WaitShape::Shanpon;
            }
            Block::Run(start) => {
                let (Some(number), Some(win_number)) = (start.number(), win_tile.number()) else {
                    continue;
                };
                if start.suit() != win_tile.suit() {
                    continue;
                }
                // 順子は start, start+1, start+2。
                match win_number.checked_sub(number) {
                    // 和了牌が真ん中 → 嵌張
                    Some(1) => return WaitShape::Kanchan,
                    // 和了牌が最小。789 の 7 も 123 の 1 も、残り2枚は連続しており両面。
                    Some(0) => return WaitShape::Ryanmen,
                    // 和了牌が最大。123 の 3 は 12 に対する辺張。それ以外は両面。
                    Some(2) => {
                        return if number == 1 {
                            WaitShape::Penchan
                        } else {
                            WaitShape::Ryanmen
                        };
                    }
                    _ => continue,
                }
            }
            _ => continue,
        }
    }

    // ここへ来るのは分解と和了牌が食い違っている場合。呼び出し側のバグ。
    WaitShape::Tanki
}

trait BlockExt {
    fn kind_index(&self) -> u8;
    fn number_or_zero(&self) -> u8;
}

impl BlockExt for Block {
    fn kind_index(&self) -> u8 {
        match self {
            Block::Run(t) => t.index(),
            Block::Triplet(t) => t.index(),
            Block::Pair(t) => t.index(),
        }
    }

    fn number_or_zero(&self) -> u8 {
        match self {
            Block::Run(t) => t.number().unwrap_or(0),
            Block::Triplet(t) => t.number().unwrap_or(0),
            Block::Pair(t) => t.number().unwrap_or(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::{parse_hand, parse_tile};

    fn forms(concealed: &str, win: &str) -> Vec<WinForm> {
        decompose(
            &parse_hand(concealed).unwrap(),
            &[],
            parse_tile(win).unwrap(),
        )
    }

    #[test]
    fn a_standard_hand_decomposes_into_four_sets_and_a_pair() {
        let found = forms("234567m23478p22s", "6p");
        assert_eq!(found.len(), 1);
        let WinForm::Standard(d) = &found[0] else {
            panic!("標準形ではない");
        };
        assert_eq!(d.blocks.len(), 4);
        assert_eq!(d.pair, parse_tile("2s").unwrap().kind());
        assert_eq!(d.wait, WaitShape::Ryanmen);
    }

    #[test]
    fn seven_pairs_is_recognised() {
        let found = forms("1122m3344p5566s7z", "7z");
        assert!(found
            .iter()
            .any(|f| matches!(f, WinForm::Chiitoitsu { pairs } if pairs.len() == 7)));
    }

    #[test]
    fn kokushi_tanki_is_recognised_and_not_a_thirteen_wait() {
        let found = forms("119m19p19s123456z", "7z");
        let kokushi = found
            .iter()
            .find_map(|f| match f {
                WinForm::Kokushi {
                    pair,
                    thirteen_wait,
                } => Some((*pair, *thirteen_wait)),
                _ => None,
            })
            .expect("国士が見つからない");
        assert_eq!(kokushi.0, parse_tile("1m").unwrap().kind());
        assert!(!kokushi.1, "単騎待ちなので十三面ではない");
    }

    #[test]
    fn kokushi_thirteen_wait_is_flagged() {
        let found = forms("19m19p19s1234567z", "1m");
        let thirteen = found
            .iter()
            .any(|f| matches!(f, WinForm::Kokushi { thirteen_wait, .. } if *thirteen_wait));
        assert!(thirteen, "十三面待ちとして検出されるべき");
    }

    /// 同じ手が複数通りに分解できる場合は全部返す。
    /// 111222333m は「123 123 123」とも「111 222 333」とも読める。
    #[test]
    fn ambiguous_hands_yield_every_decomposition() {
        let found = forms("111222333m456p4s", "4s");
        let standards: Vec<_> = found
            .iter()
            .filter_map(|f| match f {
                WinForm::Standard(d) => Some(d),
                _ => None,
            })
            .collect();
        assert!(
            standards.len() >= 2,
            "複数の分解が返るべき。実際は {}",
            standards.len()
        );
    }

    #[test]
    fn a_hand_that_is_not_complete_yields_nothing() {
        assert!(forms("147m258p369s1234z", "1m").is_empty());
    }

    #[test]
    fn identifies_each_wait_shape() {
        let kind = |n: &str| parse_tile(n).unwrap().kind();
        let run = |n: &str| Block::Run(kind(n));

        // 678p の 6p → 残る 78p は両面
        assert_eq!(
            wait_shape_of(&[run("6p")], kind("2s"), kind("6p")),
            WaitShape::Ryanmen
        );
        // 123m の 3m → 残る 12m は辺張
        assert_eq!(
            wait_shape_of(&[run("1m")], kind("5s"), kind("3m")),
            WaitShape::Penchan
        );
        // 789m の 7m → 残る 89m は両面（辺張ではない）
        assert_eq!(
            wait_shape_of(&[run("7m")], kind("5s"), kind("7m")),
            WaitShape::Ryanmen
        );
        // 123p の 2p → 残る 13p は嵌張
        assert_eq!(
            wait_shape_of(&[run("1p")], kind("5s"), kind("2p")),
            WaitShape::Kanchan
        );
        // 和了牌が刻子の一部 → シャンポン
        assert_eq!(
            wait_shape_of(&[Block::Triplet(kind("1p"))], kind("5s"), kind("1p")),
            WaitShape::Shanpon
        );
        // 和了牌が雀頭 → 単騎
        assert_eq!(
            wait_shape_of(&[run("1p")], kind("5s"), kind("5s")),
            WaitShape::Tanki
        );
    }

    #[test]
    fn melds_are_carried_into_the_decomposition() {
        use protocol::meld::{Meld, MeldKind};
        use protocol::seat::Seat;

        let melds = vec![
            Meld {
                kind: MeldKind::Pon,
                tiles: parse_hand("222m").unwrap(),
                from: Some(Seat::new(2)),
                called_tile: Some(parse_tile("2m").unwrap()),
            },
            Meld {
                kind: MeldKind::Chi,
                tiles: parse_hand("345p").unwrap(),
                from: Some(Seat::new(3)),
                called_tile: Some(parse_tile("3p").unwrap()),
            },
        ];
        let found = decompose(
            &parse_hand("88p34678s").unwrap(),
            &melds,
            parse_tile("5s").unwrap(),
        );
        let WinForm::Standard(d) = &found[0] else {
            panic!("標準形ではない");
        };
        assert_eq!(d.melds.len(), 2);
        // 手の内で作った面子は2つ（345s と 678s）、雀頭は 8p
        assert_eq!(d.blocks.len(), 2);
        assert_eq!(d.pair, parse_tile("8p").unwrap().kind());
    }
}
