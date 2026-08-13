//! 打牌の安全度。v1 は「リーチ者の河にあるか」だけを見る。
//!
//! 筋や壁は見ない。読み違えて放銃するより、通っている牌を切るほうが
//! 分かりやすく、CPU の狙い（大きく負けない）にも合う。

use crate::discard::View;
use protocol::seat::Seat;
use protocol::tile::Tile;

/// リーチしている全員の河に、その牌があるか。
///
/// **誰もリーチしていなければ真を返す。**危険を測る相手がいない。
/// 赤5と通常の5は同じ種類なので、`TileKind` で比べる。
pub fn is_safe_against_riichi(view: &View, tile: Tile) -> bool {
    Seat::ALL.iter().all(|seat| {
        if !view.riichi[seat.index()] {
            return true;
        }
        view.rivers[seat.index()]
            .iter()
            .any(|d| d.kind() == tile.kind())
    })
}

#[cfg(test)]
mod tests {
    // `View` と `Seat` は親モジュールが取り込んでいる。二重に書かない。
    use super::*;
    use protocol::notation::parse_tile;
    use protocol::seat::Wind;

    fn view() -> View {
        View {
            seat: Seat::new(0),
            seat_wind: Wind::East,
            round_wind: Wind::East,
            hand: Vec::new(),
            melds: Vec::new(),
            rivers: std::array::from_fn(|_| Vec::new()),
            riichi: [false; 4],
            dora_indicators: Vec::new(),
            wall_remaining: 70,
            scores: [25_000; 4],
        }
    }

    /// 誰もリーチしていなければ、どの牌も安全とみなす。
    #[test]
    fn everything_is_safe_when_nobody_declared() {
        let view = view();
        assert!(is_safe_against_riichi(
            &view,
            parse_tile("1m").expect("正しい記法")
        ));
    }

    /// リーチ者の河にある牌は通っている。
    #[test]
    fn a_tile_in_the_river_is_safe() {
        let mut view = view();
        view.riichi[2] = true;
        view.rivers[2].push(parse_tile("5p").expect("正しい記法"));
        assert!(is_safe_against_riichi(
            &view,
            parse_tile("5p").expect("正しい記法")
        ));
    }

    /// リーチ者の河に無い牌は通っていない。
    #[test]
    fn a_tile_outside_the_river_is_not_safe() {
        let mut view = view();
        view.riichi[2] = true;
        view.rivers[2].push(parse_tile("5p").expect("正しい記法"));
        assert!(!is_safe_against_riichi(
            &view,
            parse_tile("6p").expect("正しい記法")
        ));
    }

    /// リーチしていない席の河は関係ない。
    #[test]
    fn a_river_without_riichi_does_not_make_a_tile_safe() {
        let mut view = view();
        view.riichi[2] = true;
        view.rivers[1].push(parse_tile("5p").expect("正しい記法"));
        assert!(!is_safe_against_riichi(
            &view,
            parse_tile("5p").expect("正しい記法")
        ));
    }

    /// 複数のリーチには、全員に通っている牌だけが安全である。
    #[test]
    fn every_declarer_must_have_seen_the_tile() {
        let mut view = view();
        view.riichi[1] = true;
        view.riichi[2] = true;
        view.rivers[1].push(parse_tile("5p").expect("正しい記法"));
        assert!(
            !is_safe_against_riichi(&view, parse_tile("5p").expect("正しい記法")),
            "席2には通っていない"
        );
        view.rivers[2].push(parse_tile("5p").expect("正しい記法"));
        assert!(is_safe_against_riichi(
            &view,
            parse_tile("5p").expect("正しい記法")
        ));
    }

    /// 赤5と通常の5は同じ牌として扱う。安全度は種類で決まる。
    #[test]
    fn a_red_five_is_as_safe_as_a_normal_five() {
        let mut view = view();
        view.riichi[1] = true;
        view.rivers[1].push(parse_tile("5p").expect("正しい記法"));
        assert!(is_safe_against_riichi(
            &view,
            parse_tile("0p").expect("正しい記法")
        ));
    }
}
