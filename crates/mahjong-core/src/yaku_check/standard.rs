//! 通常役の判定。
//!
//! 翻数は門前と副露で変わる役があるため、返り値に翻数を含める。
//! 上位役が成立したら下位役は付けない（二盃口なら一盃口を付けない等）。

use protocol::meld::MeldKind;
use protocol::seat::Wind;
use protocol::tile::{Suit, TileKind};
use protocol::yaku::YakuId;

use crate::decompose::WinForm;
use crate::score::{HandContext, WinType};
use crate::shapes::{Block, Decomposition, WaitShape};

/// 成立している通常役と、その翻数を返す。
pub fn detect(form: &WinForm, context: &HandContext) -> Vec<(YakuId, u8)> {
    let mut found = Vec::new();
    let menzen = is_menzen(form);

    situational(context, menzen, &mut found);

    match form {
        WinForm::Chiitoitsu { pairs } => {
            found.push((YakuId::Chiitoitsu, 2));
            if pairs.iter().all(|k| !k.is_terminal_or_honor()) {
                found.push((YakuId::Tanyao, 1));
            }
            if pairs.iter().all(|k| k.is_terminal_or_honor()) {
                found.push((YakuId::Honroutou, 2));
            }
            flush_yaku(
                &pairs.iter().copied().collect::<Vec<_>>(),
                menzen,
                &mut found,
            );
        }
        // 国士は役満側で扱う。
        WinForm::Kokushi { .. } => {}
        WinForm::Standard(d) => detect_standard(d, context, menzen, &mut found),
    }

    supersede(&mut found);
    found
}

/// 手牌の形に依らない、状況で決まる役。
fn situational(context: &HandContext, menzen: bool, found: &mut Vec<(YakuId, u8)>) {
    if menzen {
        if context.double_riichi {
            found.push((YakuId::DoubleRiichi, 2));
        } else if context.riichi {
            found.push((YakuId::Riichi, 1));
        }
        if context.ippatsu {
            found.push((YakuId::Ippatsu, 1));
        }
        if context.win_type == WinType::Tsumo {
            found.push((YakuId::MenzenTsumo, 1));
        }
    }
    if context.chankan {
        found.push((YakuId::Chankan, 1));
    }
    if context.rinshan {
        found.push((YakuId::RinshanKaihou, 1));
    }
    if context.haitei && context.win_type == WinType::Tsumo {
        found.push((YakuId::HaiteiRaoyue, 1));
    }
    if context.houtei && context.win_type == WinType::Ron {
        found.push((YakuId::HouteiRaoyui, 1));
    }
}

fn detect_standard(
    d: &Decomposition,
    context: &HandContext,
    menzen: bool,
    found: &mut Vec<(YakuId, u8)>,
) {
    let tiles = all_tiles(d);

    if menzen && is_pinfu(d, context) {
        found.push((YakuId::Pinfu, 1));
    }

    if tiles.iter().all(|k| !k.is_terminal_or_honor()) {
        found.push((YakuId::Tanyao, 1));
    }

    // 役牌
    for (index, yaku) in [
        (31u8, YakuId::YakuhaiHaku),
        (32, YakuId::YakuhaiHatsu),
        (33, YakuId::YakuhaiChun),
    ] {
        if has_triplet_of(d, TileKind::from_index(index).expect("範囲内")) {
            found.push((yaku, 1));
        }
    }
    if has_triplet_of(d, wind_kind(context.round_wind)) {
        found.push((YakuId::YakuhaiRoundWind, 1));
    }
    if has_triplet_of(d, wind_kind(context.seat_wind)) {
        found.push((YakuId::YakuhaiSeatWind, 1));
    }

    // 順子の並びから決まる役
    let runs: Vec<TileKind> = concealed_and_called_runs(d);
    match count_identical_run_pairs(&runs) {
        2 if menzen => found.push((YakuId::Ryanpeikou, 3)),
        1 if menzen => found.push((YakuId::Iipeiko, 1)),
        _ => {}
    }
    if has_sanshoku_doujun(&runs) {
        found.push((YakuId::SanshokuDoujun, if menzen { 2 } else { 1 }));
    }
    if has_ittsu(&runs) {
        found.push((YakuId::Ittsu, if menzen { 2 } else { 1 }));
    }

    // 刻子の並びから決まる役
    let triplets = all_triplets(d);
    if has_sanshoku_doukou(&triplets) {
        found.push((YakuId::SanshokuDoukou, 2));
    }
    if triplets.len() == 4 {
        found.push((YakuId::Toitoi, 2));
    }
    if count_concealed_triplets(d, context) >= 3 {
        found.push((YakuId::Sanankou, 2));
    }
    if count_kans(d) == 3 {
        found.push((YakuId::Sankantsu, 2));
    }

    // 三元牌2種の刻子＋1種の雀頭
    let dragons: Vec<TileKind> = (31u8..34)
        .map(|i| TileKind::from_index(i).expect("範囲内"))
        .collect();
    let dragon_triplets = dragons.iter().filter(|k| has_triplet_of(d, **k)).count();
    if dragon_triplets == 2 && dragons.contains(&d.pair) {
        found.push((YakuId::Shousangen, 2));
    }

    // 幺九牌の絡み方で決まる役
    if tiles.iter().all(|k| k.is_terminal_or_honor()) {
        found.push((YakuId::Honroutou, 2));
    } else if every_group_touches_terminal(d) {
        let has_honor = tiles.iter().any(|k| k.is_honor());
        if has_honor {
            found.push((YakuId::Chanta, if menzen { 2 } else { 1 }));
        } else {
            found.push((YakuId::Junchan, if menzen { 3 } else { 2 }));
        }
    }

    flush_yaku(&tiles, menzen, found);
}

/// 混一色・清一色。七対子形からも呼ぶ。
fn flush_yaku(tiles: &[TileKind], menzen: bool, found: &mut Vec<(YakuId, u8)>) {
    let suits: Vec<Suit> = tiles
        .iter()
        .map(|k| k.suit())
        .filter(|s| *s != Suit::Honor)
        .collect();
    let Some(first) = suits.first() else {
        // 字牌のみ。字一色（役満側）で扱う。
        return;
    };
    if !suits.iter().all(|s| s == first) {
        return;
    }
    if tiles.iter().any(|k| k.is_honor()) {
        found.push((YakuId::Honitsu, if menzen { 3 } else { 2 }));
    } else {
        found.push((YakuId::Chinitsu, if menzen { 6 } else { 5 }));
    }
}

/// 平和。門前・全て順子・雀頭が役牌でない・待ちが両面。
fn is_pinfu(d: &Decomposition, context: &HandContext) -> bool {
    if !d.melds.is_empty() {
        return false;
    }
    if !d.blocks.iter().all(|b| matches!(b, Block::Run(_))) {
        return false;
    }
    if d.wait != WaitShape::Ryanmen {
        return false;
    }
    !is_yakuhai(d.pair, context)
}

fn is_yakuhai(kind: TileKind, context: &HandContext) -> bool {
    kind.index() >= 31
        || kind == wind_kind(context.round_wind)
        || kind == wind_kind(context.seat_wind)
}

fn wind_kind(wind: Wind) -> TileKind {
    let index = match wind {
        Wind::East => 27,
        Wind::South => 28,
        Wind::West => 29,
        Wind::North => 30,
    };
    TileKind::from_index(index).expect("範囲内")
}

/// 同じ順子が2組できている組数。0/1/2 を返す。
fn count_identical_run_pairs(runs: &[TileKind]) -> usize {
    let mut counts: Vec<(TileKind, usize)> = Vec::new();
    for run in runs {
        match counts.iter_mut().find(|(k, _)| k == run) {
            Some((_, n)) => *n += 1,
            None => counts.push((*run, 1)),
        }
    }
    counts.iter().map(|(_, n)| n / 2).sum()
}

fn has_sanshoku_doujun(runs: &[TileKind]) -> bool {
    for run in runs {
        let Some(number) = run.number() else { continue };
        let present: Vec<Suit> = runs
            .iter()
            .filter(|r| r.number() == Some(number))
            .map(|r| r.suit())
            .collect();
        if [Suit::Man, Suit::Pin, Suit::Sou]
            .iter()
            .all(|s| present.contains(s))
        {
            return true;
        }
    }
    false
}

fn has_ittsu(runs: &[TileKind]) -> bool {
    for suit_base in [0u8, 9, 18] {
        let needed = [suit_base, suit_base + 3, suit_base + 6];
        if needed
            .iter()
            .all(|index| runs.iter().any(|r| r.index() == *index))
        {
            return true;
        }
    }
    false
}

fn has_sanshoku_doukou(triplets: &[TileKind]) -> bool {
    for kind in triplets {
        let Some(number) = kind.number() else {
            continue;
        };
        let present: Vec<Suit> = triplets
            .iter()
            .filter(|t| t.number() == Some(number) && t.suit() != Suit::Honor)
            .map(|t| t.suit())
            .collect();
        if [Suit::Man, Suit::Pin, Suit::Sou]
            .iter()
            .all(|s| present.contains(s))
        {
            return true;
        }
    }
    false
}

/// すべての面子と雀頭が幺九牌を含むか。
fn every_group_touches_terminal(d: &Decomposition) -> bool {
    if !d.pair.is_terminal_or_honor() {
        return false;
    }
    let blocks_ok = d.blocks.iter().all(|b| match b {
        Block::Run(start) => {
            let n = start.number().unwrap_or(0);
            n == 1 || n == 7
        }
        Block::Triplet(k) | Block::Pair(k) => k.is_terminal_or_honor(),
    });
    let melds_ok = d
        .melds
        .iter()
        .all(|m| m.tiles.iter().any(|t| t.kind().is_terminal_or_honor()));
    blocks_ok && melds_ok
}

fn is_menzen(form: &WinForm) -> bool {
    match form {
        WinForm::Standard(d) => d.melds.iter().all(|m| m.kind == MeldKind::Ankan),
        _ => true,
    }
}

fn has_triplet_of(d: &Decomposition, kind: TileKind) -> bool {
    all_triplets(d).contains(&kind)
}

fn all_triplets(d: &Decomposition) -> Vec<TileKind> {
    let mut out: Vec<TileKind> = d
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Triplet(k) => Some(*k),
            _ => None,
        })
        .collect();
    for meld in &d.melds {
        if matches!(
            meld.kind,
            MeldKind::Pon | MeldKind::Ankan | MeldKind::Minkan | MeldKind::Kakan
        ) {
            if let Some(tile) = meld.tiles.first() {
                out.push(tile.kind());
            }
        }
    }
    out
}

fn concealed_and_called_runs(d: &Decomposition) -> Vec<TileKind> {
    let mut out: Vec<TileKind> = d
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Run(k) => Some(*k),
            _ => None,
        })
        .collect();
    for meld in &d.melds {
        if meld.kind == MeldKind::Chi {
            let mut tiles: Vec<TileKind> = meld.tiles.iter().map(|t| t.kind()).collect();
            tiles.sort();
            if let Some(lowest) = tiles.first() {
                out.push(*lowest);
            }
        }
    }
    out
}

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

fn count_kans(d: &Decomposition) -> usize {
    d.melds
        .iter()
        .filter(|m| matches!(m.kind, MeldKind::Ankan | MeldKind::Minkan | MeldKind::Kakan))
        .count()
}

fn all_tiles(d: &Decomposition) -> Vec<TileKind> {
    let mut out = Vec::new();
    for block in &d.blocks {
        match block {
            Block::Run(k) => {
                let base = k.index();
                for offset in 0..3 {
                    out.push(TileKind::from_index(base + offset).expect("範囲内"));
                }
            }
            Block::Triplet(k) => out.extend([*k; 3]),
            Block::Pair(k) => out.extend([*k; 2]),
        }
    }
    for meld in &d.melds {
        out.extend(meld.tiles.iter().map(|t| t.kind()));
    }
    out.push(d.pair);
    out.push(d.pair);
    out
}

/// 上位役が成立したら下位役を落とす。
fn supersede(found: &mut Vec<(YakuId, u8)>) {
    let has = |list: &Vec<(YakuId, u8)>, y: YakuId| list.iter().any(|(id, _)| *id == y);

    if has(found, YakuId::Ryanpeikou) {
        found.retain(|(id, _)| *id != YakuId::Iipeiko);
    }
    if has(found, YakuId::Junchan) {
        found.retain(|(id, _)| *id != YakuId::Chanta);
    }
    if has(found, YakuId::Chinitsu) {
        found.retain(|(id, _)| *id != YakuId::Honitsu);
    }
    if has(found, YakuId::Honroutou) {
        found.retain(|(id, _)| *id != YakuId::Chanta && *id != YakuId::Junchan);
    }
    if has(found, YakuId::DoubleRiichi) {
        found.retain(|(id, _)| *id != YakuId::Riichi);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decompose::decompose;
    use crate::score::{HandContext, WinType};
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::seat::Wind;

    fn detect_best(concealed: &str, win: &str, context: &HandContext) -> Vec<(YakuId, u8)> {
        let forms = decompose(
            &parse_hand(concealed).unwrap(),
            &[],
            parse_tile(win).unwrap(),
        );
        forms
            .iter()
            .map(|f| detect(f, context))
            .max_by_key(|list| list.iter().map(|(_, han)| *han as u32).sum::<u32>())
            .unwrap_or_default()
    }

    fn plain_tsumo() -> HandContext {
        HandContext::plain(WinType::Tsumo, Wind::South, Wind::East)
    }

    /// 全牌が中張牌の平和形は、平和と断幺九が両方成立する。
    /// 片方だけ検出する誤りが実際に起きたため、明示的に固定する。
    #[test]
    fn pinfu_and_tanyao_both_apply_to_an_all_simples_run_hand() {
        let found = detect_best("234567m23478p22s", "6p", &plain_tsumo());
        assert!(found.iter().any(|(y, _)| *y == YakuId::Pinfu));
        assert!(found.iter().any(|(y, _)| *y == YakuId::Tanyao));
        assert!(found.iter().any(|(y, _)| *y == YakuId::MenzenTsumo));
    }

    #[test]
    fn penchan_wait_breaks_pinfu() {
        let found = detect_best("12456m234678p55s", "3m", &plain_tsumo());
        assert!(!found.iter().any(|(y, _)| *y == YakuId::Pinfu));
    }

    #[test]
    fn a_terminal_breaks_tanyao() {
        let found = detect_best("12456m234678p55s", "3m", &plain_tsumo());
        assert!(!found.iter().any(|(y, _)| *y == YakuId::Tanyao));
    }

    #[test]
    fn a_yakuhai_pair_breaks_pinfu() {
        let found = detect_best("234567m23478p55z", "6p", &plain_tsumo());
        assert!(!found.iter().any(|(y, _)| *y == YakuId::Pinfu));
    }

    #[test]
    fn chiitoitsu_is_two_han() {
        let found = detect_best("1122m3344p5566s7z", "7z", &plain_tsumo());
        assert!(found.contains(&(YakuId::Chiitoitsu, 2)));
    }

    #[test]
    fn ittsu_needs_all_three_runs_in_one_suit() {
        let found = detect_best("123456789m22p34s", "5s", &plain_tsumo());
        assert!(found.iter().any(|(y, _)| *y == YakuId::Ittsu));
    }

    #[test]
    fn sanshoku_needs_the_same_numbers_in_three_suits() {
        let found = detect_best("234m234p234s56m11z", "7m", &plain_tsumo());
        assert!(found.iter().any(|(y, _)| *y == YakuId::SanshokuDoujun));
    }

    #[test]
    fn ryanpeikou_supersedes_iipeiko() {
        let found = detect_best("112233m44556p11s", "6p", &plain_tsumo());
        assert!(found.iter().any(|(y, _)| *y == YakuId::Ryanpeikou));
        assert!(
            !found.iter().any(|(y, _)| *y == YakuId::Iipeiko),
            "二盃口が成立したら一盃口は付けない"
        );
    }

    #[test]
    fn junchan_supersedes_chanta() {
        let found = detect_best("123m789m123p789p1s", "1s", &plain_tsumo());
        assert!(found.iter().any(|(y, _)| *y == YakuId::Junchan));
        assert!(!found.iter().any(|(y, _)| *y == YakuId::Chanta));
    }

    #[test]
    fn chinitsu_supersedes_honitsu() {
        let found = detect_best("1112234567899m", "9m", &plain_tsumo());
        assert!(found.iter().any(|(y, _)| *y == YakuId::Chinitsu));
        assert!(!found.iter().any(|(y, _)| *y == YakuId::Honitsu));
    }

    #[test]
    fn seat_and_round_wind_stack_when_they_match() {
        let context = HandContext::plain(WinType::Tsumo, Wind::East, Wind::East);
        let found = detect_best("111z234m567m234p1s", "1s", &context);
        assert!(found.iter().any(|(y, _)| *y == YakuId::YakuhaiRoundWind));
        assert!(found.iter().any(|(y, _)| *y == YakuId::YakuhaiSeatWind));
    }

    #[test]
    fn double_riichi_supersedes_riichi() {
        let mut context = plain_tsumo();
        context.riichi = true;
        context.double_riichi = true;
        let found = detect_best("234567m23478p22s", "6p", &context);
        assert!(found.iter().any(|(y, _)| *y == YakuId::DoubleRiichi));
        assert!(!found.iter().any(|(y, _)| *y == YakuId::Riichi));
    }
}
