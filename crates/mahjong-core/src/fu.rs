use protocol::meld::MeldKind;
use protocol::seat::Wind;
use protocol::tile::TileKind;

use crate::decompose::WinForm;
use crate::score::{HandContext, WinType};
use crate::shapes::{Block, Decomposition, WaitShape};

/// 和了形と和了時の状況から符を計算する。
pub fn fu_of(form: &WinForm, context: &HandContext, has_pinfu: bool, win_tile: TileKind) -> u8 {
    if has_pinfu && context.win_type == WinType::Tsumo {
        return 20;
    }

    let WinForm::Standard(decomposition) = form else {
        return match form {
            WinForm::Chiitoitsu { .. } => 25,
            WinForm::Kokushi { .. } => 20,
            WinForm::Standard(_) => unreachable!(),
        };
    };

    standard_fu(decomposition, context, win_tile)
}

fn standard_fu(decomposition: &Decomposition, context: &HandContext, win_tile: TileKind) -> u8 {
    let menzen = decomposition.melds.iter().all(|meld| meld.is_concealed());
    let mut fu = 20;

    if context.win_type == WinType::Ron && menzen {
        fu += 10;
    }
    if context.win_type == WinType::Tsumo {
        fu += 2;
    }

    if is_value_pair(decomposition.pair, context) {
        fu += 2;
    }
    if decomposition.wait.earns_fu() {
        fu += 2;
    }

    for block in &decomposition.blocks {
        if let Block::Triplet(kind) = *block {
            let ron_completed = context.win_type == WinType::Ron
                && decomposition.wait == WaitShape::Shanpon
                && kind == win_tile;
            fu += set_fu(kind, !ron_completed, false);
        }
    }

    for meld in &decomposition.melds {
        let Some(kind) = meld.tiles.first().map(|tile| tile.kind()) else {
            continue;
        };
        match meld.kind {
            MeldKind::Chi => {}
            MeldKind::Pon => fu += set_fu(kind, false, false),
            MeldKind::Ankan => fu += set_fu(kind, true, true),
            MeldKind::Minkan | MeldKind::Kakan => fu += set_fu(kind, false, true),
        }
    }

    if !menzen && fu == 20 {
        return 30;
    }

    fu.div_ceil(10) * 10
}

fn set_fu(kind: TileKind, concealed: bool, quad: bool) -> u8 {
    let mut fu = 2;
    if kind.is_terminal_or_honor() {
        fu *= 2;
    }
    if concealed {
        fu *= 2;
    }
    if quad {
        fu *= 4;
    }
    fu
}

fn is_value_pair(pair: TileKind, context: &HandContext) -> bool {
    pair.index() >= 31
        || pair == wind_tile(context.seat_wind)
        || pair == wind_tile(context.round_wind)
}

fn wind_tile(wind: Wind) -> TileKind {
    let index = match wind {
        Wind::East => 27,
        Wind::South => 28,
        Wind::West => 29,
        Wind::North => 30,
    };
    TileKind::from_index(index).expect("風牌のインデックスは有効")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decompose::decompose;
    use crate::score::{HandContext, WinType};
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::seat::Wind;

    fn fu_for(concealed: &str, win: &str, context: &HandContext, pinfu: bool) -> u8 {
        let win_tile = parse_tile(win).unwrap();
        let forms = decompose(&parse_hand(concealed).unwrap(), &[], win_tile);
        forms
            .iter()
            .map(|f| fu_of(f, context, pinfu, win_tile.kind()))
            .max()
            .expect("和了形が無い")
    }

    #[test]
    fn pinfu_tsumo_is_exactly_twenty() {
        let context = HandContext::plain(WinType::Tsumo, Wind::South, Wind::East);
        assert_eq!(fu_for("234567m23478p22s", "6p", &context, true), 20);
    }

    #[test]
    fn menzen_ron_adds_ten() {
        let context = HandContext::plain(WinType::Ron, Wind::South, Wind::East);
        // 副底20＋門前ロン10＝30（平和形なので他に符は付かない）
        assert_eq!(fu_for("234567m23467p55s", "8p", &context, true), 30);
    }

    #[test]
    fn penchan_wait_adds_two_and_rounds_up() {
        let context = HandContext::plain(WinType::Ron, Wind::South, Wind::East);
        // 20＋門前ロン10＋辺張2＝32→40
        assert_eq!(fu_for("12456m234678p55s", "3m", &context, false), 40);
    }

    /// 幺九暗刻8＋嵌張2＋役牌雀頭2＋ツモ2＋副底20＝34→40
    #[test]
    fn terminal_concealed_triplet_yakuhai_pair_and_tsumo() {
        let context = HandContext::plain(WinType::Tsumo, Wind::South, Wind::East);
        assert_eq!(fu_for("111m456m789p13s55z", "2s", &context, false), 40);
    }

    #[test]
    fn chiitoitsu_is_exactly_twenty_five() {
        let context = HandContext::plain(WinType::Tsumo, Wind::South, Wind::East);
        assert_eq!(fu_for("1122m3344p5566s7z", "7z", &context, false), 25);
    }

    /// 副露していて符が一切付かない形は30符とする。
    #[test]
    fn open_hand_with_no_fu_is_thirty() {
        use protocol::meld::{Meld, MeldKind};
        use protocol::seat::Seat;

        let melds = vec![
            Meld {
                kind: MeldKind::Chi,
                tiles: parse_hand("234p").unwrap(),
                from: Some(Seat::new(3)),
                called_tile: Some(parse_tile("2p").unwrap()),
            },
            Meld {
                kind: MeldKind::Chi,
                tiles: parse_hand("345s").unwrap(),
                from: Some(Seat::new(3)),
                called_tile: Some(parse_tile("3s").unwrap()),
            },
        ];
        let context = HandContext::plain(WinType::Ron, Wind::South, Wind::East);
        let win_tile = parse_tile("6p").unwrap();
        let forms = decompose(&parse_hand("234m78p33s").unwrap(), &melds, win_tile);
        let fu = forms
            .iter()
            .map(|f| fu_of(f, &context, false, win_tile.kind()))
            .max()
            .unwrap();
        assert_eq!(fu, 30);
    }

    #[test]
    fn ron_completed_triplet_is_open_but_other_triplets_stay_concealed() {
        let context = HandContext::plain(WinType::Ron, Wind::South, Wind::East);
        let win_tile = parse_tile("2m").unwrap();
        let form = WinForm::Standard(Decomposition {
            blocks: vec![
                Block::Triplet(win_tile.kind()),
                Block::Triplet(parse_tile("1p").unwrap().kind()),
                Block::Run(parse_tile("3p").unwrap().kind()),
                Block::Run(parse_tile("4s").unwrap().kind()),
            ],
            pair: parse_tile("5s").unwrap().kind(),
            melds: vec![],
            wait: WaitShape::Shanpon,
        });

        // 20＋門前ロン10＋中張明刻2＋幺九暗刻8＝40
        assert_eq!(fu_of(&form, &context, false, win_tile.kind()), 40);
    }
}
