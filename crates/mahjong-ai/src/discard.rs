//! 打牌の選択。向聴数を第一に見て、同じなら安全度で選ぶ。

use crate::safety::is_safe_against_riichi;
use mahjong_core::hand::HandCounts;
use mahjong_core::shanten::overall;
use protocol::command::{ActionOption, Command};
use protocol::meld::Meld;
use protocol::seat::{Seat, Wind};
use protocol::tile::Tile;

/// CPU が見てよい情報。**他家の手牌と山の中身は入れない。**
///
/// 呼び出し側がここを守れば、CPU がいかさまをする道は構造的に無くなる。
pub struct View {
    pub seat: Seat,
    pub seat_wind: Wind,
    pub round_wind: Wind,
    /// 自分の手牌。打牌の判断ではツモ牌を含む。
    pub hand: Vec<Tile>,
    /// **自分の副露と暗槓だけ。**他家の副露は入れない。
    pub melds: Vec<Meld>,
    /// 4席の捨て牌。鳴かれた牌も見えているので含める。
    pub rivers: [Vec<Tile>; 4],
    /// リーチが成立している席。
    pub riichi: [bool; 4],
    /// 開いているドラ表示牌。**裏ドラは入れない。**
    pub dora_indicators: Vec<Tile>,
    pub wall_remaining: u8,
    pub scores: [i32; 4],
}

/// 提示された選択肢から1つ選ぶ。
///
/// **提示に無い手は返さない。**和了が出ていれば必ず取る。
pub fn choose(view: &View, options: &[ActionOption]) -> Command {
    if options.iter().any(|o| matches!(o, ActionOption::Tsumo)) {
        return Command::Tsumo;
    }
    if options.iter().any(|o| matches!(o, ActionOption::Kyuushu)) {
        return Command::Kyuushu;
    }

    let (allowed, riichi_allowed) = options
        .iter()
        .find_map(|o| match o {
            ActionOption::Discard {
                allowed,
                riichi_allowed,
            } => Some((allowed.clone(), riichi_allowed.clone())),
            _ => None,
        })
        .expect("手番には必ず打牌の選択肢がある");

    let tile = best_discard(view, &allowed);
    Command::Discard {
        tile,
        // 宣言できるなら必ずする。待ちの良し悪しは見ない。
        riichi: riichi_allowed.contains(&tile),
    }
}

/// 切る牌を選ぶ。
///
/// 第一に向聴数、第二に安全度、第三に提示された順で決める。
/// **乱数を使わない。**同じ入力からは必ず同じ牌が出る。
fn best_discard(view: &View, allowed: &[Tile]) -> Tile {
    let melds = view.melds.len() as u8;
    allowed
        .iter()
        .copied()
        .enumerate()
        .min_by_key(|(index, tile)| {
            let rest = without(&view.hand, *tile);
            let shanten = overall::shanten(&HandCounts::from_tiles(&rest), melds);
            // 安全な牌を先にしたいので、危険なら 1 を足す。
            let risk = u8::from(!is_safe_against_riichi(view, *tile));
            (shanten, risk, *index)
        })
        .map(|(_, tile)| tile)
        .expect("提示された牌が1つ以上ある")
}

/// 手牌から1枚だけ取り除く。
fn without(hand: &[Tile], tile: Tile) -> Vec<Tile> {
    let mut out = hand.to_vec();
    if let Some(position) = out.iter().position(|t| *t == tile) {
        out.remove(position);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::meld::MeldKind;
    use protocol::notation::{parse_hand, parse_tile};

    fn view_with(hand: &str) -> View {
        View {
            seat: Seat::new(0),
            seat_wind: Wind::East,
            round_wind: Wind::East,
            hand: parse_hand(hand).expect("正しい記法"),
            melds: Vec::new(),
            rivers: std::array::from_fn(|_| Vec::new()),
            riichi: [false; 4],
            dora_indicators: Vec::new(),
            wall_remaining: 70,
            scores: [25_000; 4],
        }
    }

    fn discard_option(hand: &str) -> ActionOption {
        ActionOption::Discard {
            allowed: parse_hand(hand).expect("正しい記法"),
            riichi_allowed: Vec::new(),
        }
    }

    #[test]
    fn a_win_is_always_taken() {
        let view = view_with("234567m23478p22s6p");
        let options = vec![discard_option("6p"), ActionOption::Tsumo];
        assert_eq!(choose(&view, &options), Command::Tsumo);
    }

    #[test]
    fn nine_terminals_is_declared() {
        let view = view_with("19m19p19s12345677z");
        let options = vec![discard_option("1z"), ActionOption::Kyuushu];
        assert_eq!(choose(&view, &options), Command::Kyuushu);
    }

    #[test]
    fn a_win_beats_an_abortive_draw() {
        let view = view_with("19m19p19s12345677z");
        let options = vec![
            discard_option("1z"),
            ActionOption::Kyuushu,
            ActionOption::Tsumo,
        ];
        assert_eq!(choose(&view, &options), Command::Tsumo);
    }

    #[test]
    fn the_floating_tile_is_discarded() {
        let view = view_with("234567m23478p22s9m");
        let options = vec![discard_option("234567m23478p22s9m")];
        assert_eq!(
            choose(&view, &options),
            Command::Discard {
                tile: parse_tile("9m").expect("正しい記法"),
                riichi: false
            }
        );
    }

    #[test]
    fn a_closed_tenpai_declares_riichi() {
        let view = view_with("234567m23478p22s9m");
        let options = vec![ActionOption::Discard {
            allowed: parse_hand("234567m23478p22s9m").expect("正しい記法"),
            riichi_allowed: parse_hand("9m").expect("正しい記法"),
        }];
        assert_eq!(
            choose(&view, &options),
            Command::Discard {
                tile: parse_tile("9m").expect("正しい記法"),
                riichi: true
            }
        );
    }

    #[test]
    fn riichi_follows_the_same_choice_as_a_plain_discard() {
        let view = view_with("234567m23478p22s9m");
        let plain = vec![discard_option("234567m23478p22s9m")];
        let with_riichi = vec![ActionOption::Discard {
            allowed: parse_hand("234567m23478p22s9m").expect("正しい記法"),
            riichi_allowed: parse_hand("234567m23478p22s9m").expect("正しい記法"),
        }];
        let Command::Discard { tile: a, .. } = choose(&view, &plain) else {
            panic!("打牌でない");
        };
        let Command::Discard { tile: b, riichi } = choose(&view, &with_riichi) else {
            panic!("打牌でない");
        };
        assert_eq!(a, b);
        assert!(riichi);
    }

    #[test]
    fn an_open_hand_never_declares_riichi() {
        let mut view = view_with("234567m78p22s9m");
        view.melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("444p").expect("正しい記法"),
            from: Some(Seat::new(1)),
            called_tile: Some(parse_tile("4p").expect("正しい記法")),
        });
        let options = vec![discard_option("234567m78p22s9m")];
        let Command::Discard { riichi, .. } = choose(&view, &options) else {
            panic!("打牌でない");
        };
        assert!(!riichi);
    }

    #[test]
    fn the_same_view_always_gives_the_same_command() {
        let view = view_with("234567m23478p22s9m");
        let options = vec![discard_option("234567m23478p22s9m")];
        assert_eq!(choose(&view, &options), choose(&view, &options));
    }

    #[test]
    fn only_offered_tiles_are_chosen() {
        let view = view_with("234567m23478p22s9m");
        let allowed = parse_hand("9m22s").expect("正しい記法");
        let options = vec![ActionOption::Discard {
            allowed: allowed.clone(),
            riichi_allowed: Vec::new(),
        }];
        let Command::Discard { tile, .. } = choose(&view, &options) else {
            panic!("打牌でない");
        };
        assert!(allowed.contains(&tile), "提示外の牌を選んだ: {tile:?}");
    }

    #[test]
    fn a_safe_tile_wins_a_tie() {
        let mut view = view_with("119m19p19s1234567z");
        view.riichi[1] = true;
        view.rivers[1].push(parse_tile("1m").expect("正しい記法"));
        let options = vec![discard_option("1m9m")];
        assert_eq!(
            choose(&view, &options),
            Command::Discard {
                tile: parse_tile("1m").expect("正しい記法"),
                riichi: false
            }
        );
    }
}
