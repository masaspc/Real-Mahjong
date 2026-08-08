//! 標準形（4面子1雀頭）の向聴数。

use protocol::tile::TileKind;

use crate::hand::HandCounts;

pub fn shanten(counts: &HandCounts, melds: u8) -> i8 {
    let mut work = *counts;
    let mut best = i8::MAX;
    search(&mut work, 0, melds as i8, 0, false, &mut best);
    best
}

fn search(
    counts: &mut HandCounts,
    index: u8,
    sets: i8,
    partials: i8,
    has_pair: bool,
    best: &mut i8,
) {
    // 搭子は完成面子に置き換わる候補なので、両者の合計は4まで。
    // 雀頭は別枠であり、4面子＋1雀頭を構成する。
    if sets + partials > 4 {
        return;
    }
    if index >= TileKind::COUNT as u8 {
        *best = (*best).min(8 - 2 * sets - partials - i8::from(has_pair));
        return;
    }
    let kind = TileKind::from_index(index).expect("範囲内");
    if counts.get(kind) == 0 {
        search(counts, index + 1, sets, partials, has_pair, best);
        return;
    }
    if counts.get(kind) >= 3 {
        take(counts, kind, 3);
        search(counts, index, sets + 1, partials, has_pair, best);
        give(counts, kind, 3);
    }
    if let Some(run) = run_from(kind) {
        if run.iter().all(|k| counts.get(*k) >= 1) {
            for k in run {
                take(counts, k, 1);
            }
            search(counts, index, sets + 1, partials, has_pair, best);
            for k in run {
                give(counts, k, 1);
            }
        }
    }
    if !has_pair && counts.get(kind) >= 2 {
        take(counts, kind, 2);
        search(counts, index, sets, partials, true, best);
        give(counts, kind, 2);
    }
    if counts.get(kind) >= 2 {
        take(counts, kind, 2);
        search(counts, index, sets, partials + 1, has_pair, best);
        give(counts, kind, 2);
    }
    for gap in [1u8, 2] {
        if let Some(other) = offset_within_suit(kind, gap) {
            if counts.get(other) >= 1 {
                take(counts, kind, 1);
                take(counts, other, 1);
                search(counts, index, sets, partials + 1, has_pair, best);
                give(counts, other, 1);
                give(counts, kind, 1);
            }
        }
    }
    take(counts, kind, 1);
    search(counts, index, sets, partials, has_pair, best);
    give(counts, kind, 1);
}

fn take(counts: &mut HandCounts, kind: TileKind, n: u8) {
    for _ in 0..n {
        assert!(counts.remove(kind), "取り出せる枚数を超えている");
    }
}

fn give(counts: &mut HandCounts, kind: TileKind, n: u8) {
    for _ in 0..n {
        counts.add(kind);
    }
}

fn run_from(kind: TileKind) -> Option<[TileKind; 3]> {
    Some([
        kind,
        offset_within_suit(kind, 1)?,
        offset_within_suit(kind, 2)?,
    ])
}

fn offset_within_suit(kind: TileKind, gap: u8) -> Option<TileKind> {
    let number = kind.number()?;
    if number + gap > 9 {
        return None;
    }
    TileKind::from_index(kind.index() + gap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::parse_hand;

    fn counts(notation: &str) -> HandCounts {
        HandCounts::from_tiles(&parse_hand(notation).unwrap())
    }

    #[test]
    fn a_complete_hand_is_minus_one() {
        assert_eq!(shanten(&counts("123456789m123p11s"), 0), -1);
    }

    #[test]
    fn three_melds_a_partial_and_a_pair_is_tenpai() {
        assert_eq!(shanten(&counts("123m456m789m12p11s"), 0), 0);
        assert_eq!(shanten(&counts("123m456m789m13p11s"), 0), 0);
        assert_eq!(shanten(&counts("123m456m789m11p11s"), 0), 0);
    }

    #[test]
    fn isolated_tiles_do_not_form_a_partial_set() {
        assert_eq!(shanten(&counts("123m456m789m14p11s"), 0), 1);
    }

    #[test]
    fn a_hand_with_nothing_usable_is_eight_away() {
        assert_eq!(shanten(&counts("147m258p369s1234z"), 0), 8);
    }

    #[test]
    fn called_melds_count_toward_the_four_sets() {
        assert_eq!(shanten(&counts("123m456m12p11s"), 1), 0);
    }

    /// 順子は色をまたがない。9m と 1p は隣り合わない。
    #[test]
    fn runs_do_not_cross_suits() {
        // 111s 222s 333s の3面子＋44s の雀頭＋孤立した 9m と 1p(13枚)。
        // 9m1p が搭子になれるなら 8-6-1-1 = 0 だが、色をまたぐので搭子にならず
        // 8-6-0-1 = 1 が正しい。
        assert_eq!(shanten(&counts("9m1p11122233344s"), 0), 1);
    }

    /// 字牌は順子を作れない。刻子と対子だけで評価される。
    #[test]
    fn honors_never_form_runs() {
        // 東東東 南南南 西西西 ＋ 北北 ＋ 白白(13枚)
        // 面子3＋雀頭1＋対子1 → 8-6-1-1 = 0(北と白のシャンポン待ち)
        assert_eq!(shanten(&counts("111z222z333z44z55z"), 0), 0);

        // 1z2z3z が順子になれるなら向聴はもっと小さくなるはず。
        // 実際は対子6つ＋孤立1枚として評価され、ブロック上限5で 8-0-4-1 = 3 になる。
        assert_eq!(shanten(&counts("123z456z1234567z"), 0), 3);
    }
}
