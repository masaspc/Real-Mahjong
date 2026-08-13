//! 鳴きの判断。**役が見込めるときだけ鳴く。**
//!
//! 鳴いて役が無ければ和了れない。v1 は判断できる形を2つに絞り、
//! チーと槓はしない。チーは手が安くなりやすく、槓はドラを増やして
//! 他家を利する場面があるが、その見極めができない。

use crate::discard::View;
use protocol::command::{ActionOption, CallResponse};
use protocol::seat::Wind;
use protocol::tile::{Tile, TileKind};

/// 提示された選択肢から1つ選ぶ。鳴かないときは `Pass` を返す。
pub fn respond(view: &View, options: &[ActionOption]) -> CallResponse {
    if options.iter().any(|o| matches!(o, ActionOption::Ron)) {
        return CallResponse::Ron;
    }
    for option in options {
        let ActionOption::Pon { candidates } = option else {
            continue;
        };
        let Some(tiles) = candidates.first() else {
            continue;
        };
        if worth_ponning(view, tiles[0]) {
            return CallResponse::Pon { tiles: *tiles };
        }
    }
    CallResponse::Pass
}

/// その牌をポンする価値があるか。
fn worth_ponning(view: &View, tile: Tile) -> bool {
    // 役牌なら1翻が確定する。幺九牌が手にあっても関係ない。
    if is_value_tile(view, tile.kind()) {
        return true;
    }
    // 断幺九。鳴く牌が中張牌で、**手にも副露にも**幺九牌が無いこと。
    // 副露を見ないと、幺九牌を含む副露があるのに断幺九を当てにしてしまう。
    if tile.kind().is_terminal_or_honor() {
        return false;
    }
    let hand_is_clean = view.hand.iter().all(|t| !t.kind().is_terminal_or_honor());
    let melds_are_clean = view
        .melds
        .iter()
        .flat_map(|m| m.tiles.iter())
        .all(|t| !t.kind().is_terminal_or_honor());
    hand_is_clean && melds_are_clean
}

/// 自風・場風・三元牌のいずれか。
fn is_value_tile(view: &View, kind: TileKind) -> bool {
    if is_dragon(kind) {
        return true;
    }
    wind_of(kind).is_some_and(|wind| wind == view.seat_wind || wind == view.round_wind)
}

/// 三元牌は 5z..7z。字牌は 27 から東南西北・白發中の順に並ぶ。
fn is_dragon(kind: TileKind) -> bool {
    (31..=33).contains(&kind.index())
}

/// 風牌なら、その風を返す。
fn wind_of(kind: TileKind) -> Option<Wind> {
    match kind.index() {
        27 => Some(Wind::East),
        28 => Some(Wind::South),
        29 => Some(Wind::West),
        30 => Some(Wind::North),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::command::KanCandidate;
    use protocol::meld::{Meld, MeldKind};
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::seat::Seat;

    fn view_with(hand: &str) -> View {
        view_in(Wind::East, Wind::East, hand)
    }

    fn view_in(seat_wind: Wind, round_wind: Wind, hand: &str) -> View {
        View {
            seat: Seat::new(0),
            seat_wind,
            round_wind,
            hand: parse_hand(hand).expect("正しい記法"),
            melds: Vec::new(),
            rivers: std::array::from_fn(|_| Vec::new()),
            riichi: [false; 4],
            dora_indicators: Vec::new(),
            wall_remaining: 70,
            scores: [25_000; 4],
        }
    }

    fn pon_of(notation: &str) -> ActionOption {
        let tiles = parse_hand(notation).expect("正しい記法");
        ActionOption::Pon {
            candidates: vec![[tiles[0], tiles[1]]],
        }
    }

    fn chi_of(notation: &str) -> ActionOption {
        let tiles = parse_hand(notation).expect("正しい記法");
        ActionOption::Chi {
            candidates: vec![[tiles[0], tiles[1]]],
        }
    }

    #[test]
    fn a_ron_is_always_taken() {
        let view = view_with("234567m23478p22s");
        let options = vec![ActionOption::Ron, pon_of("22s")];
        assert_eq!(respond(&view, &options), CallResponse::Ron);
    }

    #[test]
    fn the_round_wind_is_ponned() {
        let view = view_with("234567m234p11z99s");
        let options = vec![pon_of("11z")];
        let tiles = parse_hand("11z").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }

    #[test]
    fn a_dragon_is_ponned() {
        let view = view_with("234567m234p55z99s");
        let options = vec![pon_of("55z")];
        let tiles = parse_hand("55z").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }

    #[test]
    fn a_guest_wind_is_not_ponned() {
        let view = view_with("234567m234p33z99s");
        let options = vec![pon_of("33z")];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    #[test]
    fn a_hand_without_terminals_pons_for_tanyao() {
        let view = view_with("234567m234p55p22s");
        let options = vec![pon_of("55p")];
        let tiles = parse_hand("55p").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }

    #[test]
    fn a_hand_with_a_terminal_does_not_pon_for_tanyao() {
        let view = view_with("134567m234p55p22s");
        let options = vec![pon_of("55p")];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    #[test]
    fn a_terminal_pon_is_never_taken_for_tanyao() {
        let view = view_with("234567m234p11p22s");
        let options = vec![pon_of("11p")];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    #[test]
    fn a_chi_is_never_taken() {
        let view = view_with("234567m234p56p22s");
        let options = vec![chi_of("56p")];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    #[test]
    fn a_kan_is_never_taken() {
        let view = view_with("234567m234p555p2s");
        let options = vec![ActionOption::Kan {
            candidates: vec![KanCandidate::Minkan],
        }];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    #[test]
    fn nothing_offered_means_pass() {
        let view = view_with("234567m23478p22s");
        assert_eq!(respond(&view, &[]), CallResponse::Pass);
    }

    #[test]
    fn a_meld_with_a_terminal_blocks_the_tanyao_pon() {
        let mut view = view_with("234567m55p22s");
        view.melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("111m").expect("正しい記法"),
            from: Some(Seat::new(1)),
            called_tile: Some(parse_tile("1m").expect("正しい記法")),
        });
        let options = vec![pon_of("55p")];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    #[test]
    fn a_clean_meld_keeps_the_tanyao_pon() {
        let mut view = view_with("234567m55p22s");
        view.melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("333m").expect("正しい記法"),
            from: Some(Seat::new(1)),
            called_tile: Some(parse_tile("3m").expect("正しい記法")),
        });
        let options = vec![pon_of("55p")];
        let tiles = parse_hand("55p").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }

    #[test]
    fn the_seat_wind_alone_is_enough() {
        let view = view_in(Wind::South, Wind::East, "234567m234p22z99s");
        let options = vec![pon_of("22z")];
        let tiles = parse_hand("22z").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }

    #[test]
    fn the_round_wind_alone_is_enough() {
        let view = view_in(Wind::South, Wind::East, "234567m234p11z99s");
        let options = vec![pon_of("11z")];
        let tiles = parse_hand("11z").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }

    #[test]
    fn a_wind_that_is_neither_is_passed() {
        let view = view_in(Wind::South, Wind::East, "234567m234p33z99s");
        let options = vec![pon_of("33z")];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    #[test]
    fn the_same_view_always_gives_the_same_response() {
        let view = view_with("234567m234p11z99s");
        let options = vec![pon_of("11z")];
        assert_eq!(respond(&view, &options), respond(&view, &options));
    }

    #[test]
    fn a_value_tile_is_ponned_even_with_terminals() {
        let view = view_with("134567m234p11z99s");
        let options = vec![pon_of("11z")];
        let tiles = parse_hand("11z").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }
}
