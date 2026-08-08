//! 和了形の語彙。**Wave 1 では編集しないこと。**
//!
//! 待ち形は Wave 1a（`wait.rs`）が算出し、Wave 1b（`fu.rs`）が符計算で消費する。
//! 分解結果は Wave 1b（`decompose.rs`）が生成する。
//! 型を先に凍結することで、両者が互いの実装完了を待たずに着手できる。

use protocol::meld::Meld;
use protocol::tile::TileKind;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitShape {
    Ryanmen,
    Penchan,
    Kanchan,
    Shanpon,
    Tanki,
}

impl WaitShape {
    /// 符が付く待ちかどうか。両面と双碰は0符、それ以外は2符。
    pub fn earns_fu(self) -> bool {
        matches!(
            self,
            WaitShape::Penchan | WaitShape::Kanchan | WaitShape::Tanki
        )
    }
}

/// 面子ひとつ。順子は最小の牌で表す（123m なら 1m）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Block {
    Run(TileKind),
    Triplet(TileKind),
    Pair(TileKind),
}

/// 和了形をひととおりに分解した結果。
///
/// 同じ手が複数通りに分解できる場合、どれを採るかは点数が最大になる方を選ぶ
/// （`score.rs` の責務）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Decomposition {
    /// 副露を含まない、手の内で構成した面子。
    pub blocks: Vec<Block>,
    pub pair: TileKind,
    pub melds: Vec<Meld>,
    pub wait: WaitShape,
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::tile::TileKind;

    #[test]
    fn a_decomposition_names_its_blocks_and_wait() {
        let decomposition = Decomposition {
            blocks: vec![
                Block::Run(TileKind::from_index(0).unwrap()),
                Block::Run(TileKind::from_index(3).unwrap()),
                Block::Triplet(TileKind::from_index(27).unwrap()),
                Block::Run(TileKind::from_index(9).unwrap()),
            ],
            pair: TileKind::from_index(18).unwrap(),
            melds: vec![],
            wait: WaitShape::Ryanmen,
        };
        assert_eq!(decomposition.blocks.len(), 4);
        assert_eq!(decomposition.wait, WaitShape::Ryanmen);
    }

    #[test]
    fn wait_shapes_are_distinguishable() {
        let all = [
            WaitShape::Ryanmen,
            WaitShape::Penchan,
            WaitShape::Kanchan,
            WaitShape::Shanpon,
            WaitShape::Tanki,
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    /// 符計算はこの対応表に従う。値そのものは fu.rs（Wave 1b）が持つが、
    /// 「どの待ちが2符か」は語彙として固定しておく。
    #[test]
    fn only_penchan_kanchan_and_tanki_earn_fu() {
        assert!(!WaitShape::Ryanmen.earns_fu());
        assert!(!WaitShape::Shanpon.earns_fu());
        assert!(WaitShape::Penchan.earns_fu());
        assert!(WaitShape::Kanchan.earns_fu());
        assert!(WaitShape::Tanki.earns_fu());
    }
}
