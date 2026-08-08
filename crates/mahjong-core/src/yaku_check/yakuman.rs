//! 役満の判定。
//!
//! 通常役より先に判定する。役満が成立していれば通常役を数える必要がなく、
//! 点数計算の分岐が単純になる。
//!
//! 成立条件が明確なので、凝った抽象化を入れず1役1関数で並べる。

use protocol::meld::MeldKind;
use protocol::tile::TileKind;
use protocol::yaku::YakuId;

use crate::decompose::WinForm;
use crate::score::{HandContext, WinType};
use crate::shapes::{Block, Decomposition, WaitShape};

/// 成立している役満をすべて返す。無ければ空。
pub fn detect(form: &WinForm, context: &HandContext) -> Vec<YakuId> {
    let mut found = Vec::new();

    match form {
        WinForm::Kokushi { thirteen_wait, .. } => {
            found.push(if *thirteen_wait {
                YakuId::KokushiMusou13
            } else {
                YakuId::KokushiMusou
            });
        }
        WinForm::Chiitoitsu { pairs } => {
            // 七対子形でも字一色は成立する。
            if pairs.iter().all(|k| k.is_honor()) {
                found.push(YakuId::Tsuuiisou);
            }
        }
        WinForm::Standard(d) => {
            detect_standard(d, context, &mut found);
        }
    }

    if context.tenhou {
        found.push(YakuId::Tenhou);
    }
    if context.chiihou {
        found.push(YakuId::Chiihou);
    }

    found
}

fn detect_standard(d: &Decomposition, context: &HandContext, found: &mut Vec<YakuId>) {
    let concealed_triplets = count_concealed_triplets(d, context);

    if concealed_triplets == 4 {
        found.push(YakuId::Suuankou);
        if d.wait == WaitShape::Tanki {
            found.push(YakuId::SuuankouTanki);
        }
    }

    if is_daisangen(d) {
        found.push(YakuId::Daisangen);
    }

    match count_wind_sets(d) {
        (4, _) => found.push(YakuId::Daisuushii),
        (3, true) => found.push(YakuId::Shousuushii),
        _ => {}
    }

    if all_tiles(d).all(|k| k.is_honor()) {
        found.push(YakuId::Tsuuiisou);
    }

    if all_tiles(d).all(is_green) {
        found.push(YakuId::Ryuuiisou);
    }

    if all_tiles(d).all(|k| k.is_terminal()) {
        found.push(YakuId::Chinroutou);
    }

    if count_kans(d) == 4 {
        found.push(YakuId::Suukantsu);
    }

    if let Some(pure) = chuuren_shape(d, context) {
        found.push(if pure {
            YakuId::ChuurenPoutou9
        } else {
            YakuId::ChuurenPoutou
        });
    }
}

/// 暗刻の数。**ロンで和了牌が完成させた刻子は明刻として数える。**
/// 四暗刻とシャンポン待ちのロンを取り違えるのはよくある誤りである。
fn count_concealed_triplets(d: &Decomposition, context: &HandContext) -> usize {
    let ron_completed_a_triplet = context.win_type == WinType::Ron && d.wait == WaitShape::Shanpon;

    let in_hand = d
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Triplet(_)))
        .count();

    let ankan = d.melds.iter().filter(|m| m.kind == MeldKind::Ankan).count();

    let total = in_hand + ankan;
    if ron_completed_a_triplet {
        total.saturating_sub(1)
    } else {
        total
    }
}

fn is_daisangen(d: &Decomposition) -> bool {
    // 白=31, 發=32, 中=33
    [31u8, 32, 33]
        .iter()
        .all(|index| has_triplet_of(d, TileKind::from_index(*index).expect("範囲内")))
}

/// (風牌の刻子数, 風牌の雀頭があるか)
fn count_wind_sets(d: &Decomposition) -> (usize, bool) {
    let winds: Vec<TileKind> = (27u8..31)
        .map(|i| TileKind::from_index(i).expect("範囲内"))
        .collect();
    let triplets = winds.iter().filter(|k| has_triplet_of(d, **k)).count();
    let pair_is_wind = winds.contains(&d.pair);
    (triplets, pair_is_wind)
}

fn has_triplet_of(d: &Decomposition, kind: TileKind) -> bool {
    d.blocks
        .iter()
        .any(|b| matches!(b, Block::Triplet(k) if *k == kind))
        || d.melds.iter().any(|m| {
            matches!(
                m.kind,
                MeldKind::Pon | MeldKind::Ankan | MeldKind::Minkan | MeldKind::Kakan
            ) && m.tiles.first().map(|t| t.kind()) == Some(kind)
        })
}

fn count_kans(d: &Decomposition) -> usize {
    d.melds
        .iter()
        .filter(|m| matches!(m.kind, MeldKind::Ankan | MeldKind::Minkan | MeldKind::Kakan))
        .count()
}

/// 緑一色に使える牌は 2s/3s/4s/6s/8s と發 のみ。
fn is_green(kind: TileKind) -> bool {
    // 索子は 18..=26（1s..9s）、發は 32
    matches!(kind.index(), 19 | 20 | 21 | 23 | 25 | 32)
}

/// 九蓮宝燈の形かどうか。Some(true) なら純正（九面待ち）。
fn chuuren_shape(d: &Decomposition, context: &HandContext) -> Option<bool> {
    if !d.melds.is_empty() {
        return None;
    }

    let tiles: Vec<TileKind> = all_tiles(d).collect();
    let suit = tiles.first()?.suit();
    if suit == protocol::tile::Suit::Honor || !tiles.iter().all(|k| k.suit() == suit) {
        return None;
    }

    let mut counts = [0u8; 9];
    for kind in &tiles {
        counts[(kind.number()? - 1) as usize] += 1;
    }

    // 1112345678999 に任意の1枚を足した形。
    let base = [3u8, 1, 1, 1, 1, 1, 1, 1, 3];
    let mut extra = None;
    for i in 0..9 {
        match counts[i].checked_sub(base[i]) {
            Some(0) => {}
            Some(1) if extra.is_none() => extra = Some(i),
            _ => return None,
        }
    }
    let extra = extra?;

    // 和了牌を除いた形がちょうど 1112345678999 なら純正九蓮宝燈。
    let win_number = win_tile_number(d, context)?;
    Some(extra + 1 == win_number as usize)
}

/// 和了牌の番号。待ち形と分解から復元する。
fn win_tile_number(d: &Decomposition, _context: &HandContext) -> Option<u8> {
    // 純正判定にのみ使う。単騎なら雀頭、それ以外は復元できないため None。
    match d.wait {
        WaitShape::Tanki => d.pair.number(),
        _ => None,
    }
}

fn all_tiles(d: &Decomposition) -> impl Iterator<Item = TileKind> + '_ {
    let from_blocks = d.blocks.iter().flat_map(|b| match b {
        Block::Run(k) => {
            let base = k.index();
            vec![
                TileKind::from_index(base).expect("範囲内"),
                TileKind::from_index(base + 1).expect("範囲内"),
                TileKind::from_index(base + 2).expect("範囲内"),
            ]
        }
        Block::Triplet(k) => vec![*k; 3],
        Block::Pair(k) => vec![*k; 2],
    });
    let from_melds = d
        .melds
        .iter()
        .flat_map(|m| m.tiles.iter().map(|t| t.kind()).collect::<Vec<_>>());
    from_blocks
        .chain(from_melds)
        .chain(std::iter::once(d.pair))
        .chain(std::iter::once(d.pair))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decompose::decompose;
    use crate::score::{HandContext, WinType};
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::seat::Wind;

    fn detect_for(concealed: &str, win: &str, win_type: WinType) -> Vec<YakuId> {
        let forms = decompose(
            &parse_hand(concealed).unwrap(),
            &[],
            parse_tile(win).unwrap(),
        );
        let context = HandContext::plain(win_type, Wind::South, Wind::East);
        forms.iter().flat_map(|f| detect(f, &context)).collect()
    }

    #[test]
    fn kokushi_tanki_is_single_yakuman() {
        let found = detect_for("119m19p19s123456z", "7z", WinType::Ron);
        assert!(found.contains(&YakuId::KokushiMusou));
        assert!(!found.contains(&YakuId::KokushiMusou13));
    }

    #[test]
    fn kokushi_thirteen_wait_is_double_yakuman() {
        let found = detect_for("19m19p19s1234567z", "1m", WinType::Ron);
        assert!(found.contains(&YakuId::KokushiMusou13));
    }

    #[test]
    fn daisangen_needs_all_three_dragons() {
        let found = detect_for("555z666z777z123m1p", "1p", WinType::Ron);
        assert!(found.contains(&YakuId::Daisangen));
    }

    #[test]
    fn tsuuiisou_needs_every_tile_to_be_an_honor() {
        let found = detect_for("111z222z333z444z5z", "5z", WinType::Ron);
        assert!(found.contains(&YakuId::Tsuuiisou));
    }

    #[test]
    fn ryuuiisou_accepts_only_green_tiles() {
        let found = detect_for("234s234s666s888s6z", "6z", WinType::Ron);
        assert!(found.contains(&YakuId::Ryuuiisou));

        // 5s は緑一色に使えない
        let not_green = detect_for("345s345s666s888s6z", "6z", WinType::Ron);
        assert!(!not_green.contains(&YakuId::Ryuuiisou));
    }

    #[test]
    fn suuankou_requires_four_concealed_triplets() {
        let found = detect_for("111m222m333m444m5p", "5p", WinType::Tsumo);
        assert!(found.contains(&YakuId::Suuankou));
        assert!(found.contains(&YakuId::SuuankouTanki));
    }

    /// ロンで和了牌が刻子を作った場合、その刻子は暗刻にならない。
    #[test]
    fn ron_completing_a_triplet_breaks_suuankou() {
        let found = detect_for("111m222m333m44m55p", "4m", WinType::Ron);
        assert!(!found.contains(&YakuId::Suuankou));
    }

    #[test]
    fn a_normal_hand_has_no_yakuman() {
        let found = detect_for("234567m23478p22s", "6p", WinType::Tsumo);
        assert!(found.is_empty());
    }
}
