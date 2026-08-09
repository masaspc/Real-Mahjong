//! エンジンの不変条件。破れていれば即座に落とす。
//!
//! 麻雀は牌と点棒が増えも減りもしない閉じた系である。
//! 進行のどこかで壊れたら、その場で気づけるようにする。

use crate::reaction::ReactionWindow;
use crate::state::RoundState;
use protocol::seat::Seat;

/// 牌はちょうど136枚。**牌は1箇所にだけ属する。**
pub fn assert_tiles_conserved(state: &RoundState) {
    let mut total = 0usize;
    for seat in Seat::ALL {
        let s = state.seat(seat);
        total += s.hand.len();
        total += s.melds.iter().map(|m| m.tiles.len()).sum::<usize>();
        // 鳴かれた牌は鳴いた者の melds に入っている。両方数えると二重計上。
        total += s.river.iter().filter(|d| d.called_by.is_none()).count();
    }
    // all_tiles() は136枚すべてを返すので使わない。
    total += state.wall.tiles_in_wall().count();

    assert_eq!(total, 136, "牌の総数が136でない: {total}");
}

/// 点棒は卓の中を移動するだけ。供託の増減を含めて合計は変わらない。
///
/// `sticks_delta` は供託の増加量。リーチ棒が1本出れば +1000、
/// 回収されれば -1000 である。
pub fn assert_scores_conserved(before: &[i32; 4], after: &[i32; 4], sticks_delta: i32) {
    let before_total: i32 = before.iter().sum();
    let after_total: i32 = after.iter().sum();
    assert_eq!(
        after_total + sticks_delta,
        before_total,
        "点棒の合計が変わった: {before_total} → {after_total}（供託 {sticks_delta:+}）"
    );
}

/// 非ロンの同順位が同時に成立していないこと。
///
/// 牌は1種4枚しかないため、2人が同じ牌をポンすることも、ポンと明槓が
/// 競合することも起こりえない（仕様 6.4）。席順で解決するロジックを
/// 書かない代わりに、発生しないことをここで主張する。
pub fn assert_no_simultaneous_non_ron(window: &ReactionWindow) {
    let ties = window.non_ron_ties();
    assert!(ties.is_empty(), "非ロンの同順位が同時に成立した: {ties:?}");
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::RoundState;
    use crate::wall::Seed;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::{Round, Seat, Wind};

    fn fresh() -> RoundState {
        RoundState::new(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            Round {
                wind: Wind::East,
                number: 1,
            },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &Seed::from_hex(&"33".repeat(32)).unwrap(),
        )
    }

    #[test]
    fn a_fresh_round_conserves_every_tile() {
        assert_tiles_conserved(&fresh());
    }

    /// 嶺上を引いた後も136枚のまま。
    /// tiles_in_wall が live_end で切っていると135枚になって落ちる。
    #[test]
    fn a_replacement_draw_keeps_the_count() {
        let mut state = fresh();
        let tile = state.wall.draw_replacement().expect("嶺上がある");
        state.seat_mut(Seat::new(0)).hand.push(tile);
        assert_tiles_conserved(&state);
    }

    #[test]
    #[should_panic(expected = "136")]
    fn a_missing_tile_is_caught() {
        let mut state = fresh();
        state.seat_mut(Seat::new(0)).hand.pop();
        assert_tiles_conserved(&state);
    }

    #[test]
    #[should_panic(expected = "136")]
    fn a_duplicated_tile_is_caught() {
        let mut state = fresh();
        let extra = state.seat(Seat::new(0)).hand[0];
        state.seat_mut(Seat::new(0)).hand.push(extra);
        assert_tiles_conserved(&state);
    }

    /// 点棒は卓の中を移動するだけ。供託の増減を含めて合計は変わらない。
    #[test]
    fn scores_and_sticks_balance() {
        assert_scores_conserved(&[25_000; 4], &[24_000, 26_000, 25_000, 25_000], 0);
        // リーチ棒が1本出た局面。手元から1000減り、供託が1000増える。
        assert_scores_conserved(&[25_000; 4], &[24_000, 25_000, 25_000, 25_000], 1_000);
        // 供託を回収した局面。
        assert_scores_conserved(&[25_000; 4], &[26_000, 25_000, 25_000, 25_000], -1_000);
    }

    #[test]
    #[should_panic(expected = "点棒")]
    fn a_score_leak_is_caught() {
        assert_scores_conserved(&[25_000; 4], &[24_000, 25_000, 25_000, 25_000], 0);
    }

    #[test]
    #[should_panic(expected = "点棒")]
    fn a_score_creation_is_caught() {
        assert_scores_conserved(&[25_000; 4], &[26_000, 25_000, 25_000, 25_000], 0);
    }
}
