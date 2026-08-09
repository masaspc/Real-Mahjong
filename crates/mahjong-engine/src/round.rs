#[path = "settlement.rs"]
mod settlement;
pub use settlement::{
    score_change, settle_agari, settle_exhaustive, settle_nagashi, AgariInput, HONBA_PER_STICK,
};

use crate::state::{RoundState, SeatState, RIICHI_STICK};
use mahjong_core::callable::{
    ankan_candidates, chi_candidates, kakan_candidates, minkan_possible, pon_candidates,
};
use mahjong_core::furiten::{is_furiten_by_discards, is_temporary_furiten};
use mahjong_core::hand::HandCounts;
use mahjong_core::score::{score, WinType};
use mahjong_core::wait::waiting_tiles;
use protocol::command::{ActionOption, KanCandidate};
use protocol::event::{DrawSource, RiichiStep};
use protocol::meld::{Meld, MeldKind};
use protocol::seat::Seat;
use protocol::tile::{Tile, TileKind};
use protocol::yaku::YakuId;

/// 打牌の直前に何が起きたか。
///
/// `RoundState.last_draw` は席と出どころしか持たず、牌そのものを持たない。
/// 手牌の並び順にも契約が無いため「末尾がツモ牌」と決めつけられない。
/// 引いた側が知っているので、ここで受け取る。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TurnStart {
    /// ツモ。`tile` は手牌へ加えたあとの、その牌そのもの。
    Draw { tile: Tile, source: DrawSource },
    /// 鳴きの直後。ツモ牌は無い。
    AfterCall,
}

/// 同じ牌をまとめる。赤5と通常の5は別の `Tile` 値なので区別は残る。
fn distinct(tiles: &[Tile]) -> Vec<Tile> {
    let mut out: Vec<Tile> = Vec::with_capacity(tiles.len());
    for tile in tiles {
        if !out.contains(tile) {
            out.push(*tile);
        }
    }
    out
}

/// 手牌から1枚だけ取り除く。`score` は和了牌を除いた手牌を取るため必要。
fn hand_without(hand: &[Tile], tile: Tile) -> Vec<Tile> {
    let mut out = hand.to_vec();
    if let Some(position) = out.iter().position(|t| *t == tile) {
        out.remove(position);
    }
    out
}

fn is_riichi_accepted(seat: &SeatState) -> bool {
    matches!(&seat.riichi, Some(r) if r.step == RiichiStep::Accepted)
}

/// 待ち牌。`waiting_tiles` は牌種の昇順で返すが、待ちどうしを比較する箇所が
/// あるので、順序の前提をこの関数の中に閉じておく。
fn waits_of(hand: &[Tile], melds: usize) -> Vec<TileKind> {
    let mut waits = waiting_tiles(&HandCounts::from_tiles(hand), melds as u8);
    waits.sort_by_key(|k| k.index());
    waits
}

/// 4つの槓が済んでいれば嶺上牌が尽きるので、それ以上は槓できない。
fn kans_left(state: &RoundState) -> bool {
    state.kan_count.iter().map(|c| u32::from(*c)).sum::<u32>() < 4
}

/// 同じ色の、指定した数の牌。
fn same_suit(reference: TileKind, number: u8) -> Option<TileKind> {
    let current = reference.number()?;
    if !(1..=9).contains(&number) {
        return None;
    }
    let base = reference.index() - (current - 1);
    TileKind::from_index(base + number - 1)
}

// ---------- 打牌 ----------

pub fn discard_options(state: &RoundState, seat: Seat, start: TurnStart) -> Vec<ActionOption> {
    let hand = &state.seat(seat).hand;
    let melds = &state.seat(seat).melds;
    let mut out = Vec::new();

    let allowed = allowed_discards(state, seat, start);
    let riichi_allowed = riichi_discards(state, seat, &allowed);
    out.push(ActionOption::Discard {
        allowed,
        riichi_allowed,
    });

    // ツモした番でしかできないもの。鳴いた直後は打牌のみである。
    if let TurnStart::Draw { tile, .. } = start {
        let candidates = turn_kan_candidates(state, seat, tile);
        if !candidates.is_empty() {
            out.push(ActionOption::Kan { candidates });
        }

        // 振聴はツモ和了を妨げない。
        let rest = hand_without(hand, tile);
        let context = state.hand_context(seat, WinType::Tsumo);
        if score(&rest, melds, tile, &context, &state.rules).is_some() {
            out.push(ActionOption::Tsumo);
        }

        if kyuushu_allowed(state, seat) {
            out.push(ActionOption::Kyuushu);
        }
    }

    out
}

fn allowed_discards(state: &RoundState, seat: Seat, start: TurnStart) -> Vec<Tile> {
    let s = state.seat(seat);

    // リーチ成立後はツモ切りのみ。リーチ後は鳴けないので AfterCall は来ない。
    if is_riichi_accepted(s) {
        return match start {
            TurnStart::Draw { tile, .. } => vec![tile],
            TurnStart::AfterCall => Vec::new(),
        };
    }

    let mut allowed = distinct(&s.hand);
    if start == TurnStart::AfterCall {
        for forbidden in kuikae_forbidden(s.melds.last()) {
            allowed.retain(|t| t.kind() != forbidden);
        }
    }
    allowed
}

/// 食い替えで打てなくなる牌の種類。
///
/// 鳴いた牌そのもの（現物）は常に禁止する。チーで順子の端を鳴いた場合は、
/// 反対側の隣（筋）も禁止する。嵌張で鳴いた場合に筋の制限は無い。
fn kuikae_forbidden(last: Option<&Meld>) -> Vec<TileKind> {
    let Some(meld) = last else {
        return Vec::new();
    };
    let Some(called) = meld.called_tile else {
        return Vec::new();
    };
    let mut out = vec![called.kind()];
    if meld.kind != MeldKind::Chi {
        return out;
    }

    let mut numbers: Vec<u8> = meld
        .tiles
        .iter()
        .filter_map(|t| t.kind().number())
        .collect();
    numbers.sort_unstable();
    let Some(called_number) = called.kind().number() else {
        return out;
    };
    if numbers.len() != 3 {
        return out;
    }

    let suji = if called_number == numbers[0] {
        numbers[2].checked_add(1)
    } else if called_number == numbers[2] {
        numbers[0].checked_sub(1)
    } else {
        None
    };
    if let Some(kind) = suji.and_then(|n| same_suit(called.kind(), n)) {
        out.push(kind);
    }
    out
}

/// リーチ宣言牌として選べる牌。
fn riichi_discards(state: &RoundState, seat: Seat, allowed: &[Tile]) -> Vec<Tile> {
    let s = state.seat(seat);
    if !state.is_menzen(seat) || s.riichi.is_some() {
        return Vec::new();
    }
    // 供託を出せない持ち点ではリーチできない。
    if state.scores[seat.index()] < RIICHI_STICK {
        return Vec::new();
    }
    // 宣言したあと一巡もせずに流局するなら、リーチする意味がない。
    if state.wall.live_remaining() < 4 {
        return Vec::new();
    }

    allowed
        .iter()
        .copied()
        .filter(|tile| {
            let rest = hand_without(&s.hand, *tile);
            !waits_of(&rest, s.melds.len()).is_empty()
        })
        .collect()
}

/// 自分の番で宣言できる槓。
fn turn_kan_candidates(state: &RoundState, seat: Seat, drawn: Tile) -> Vec<KanCandidate> {
    if !kans_left(state) {
        return Vec::new();
    }
    let s = state.seat(seat);
    let riichi = is_riichi_accepted(s);
    let mut out = Vec::new();

    for kind in ankan_candidates(&s.hand) {
        if riichi && !riichi_ankan_allowed(s, kind, drawn) {
            continue;
        }
        out.push(KanCandidate::Ankan { kind });
    }

    // 加槓は手牌の構成を変えるので、リーチ中はできない。
    if !riichi {
        for tile in kakan_candidates(&s.hand, &s.melds) {
            out.push(KanCandidate::Kakan { tile });
        }
    }
    out
}

/// リーチ中の暗槓の条件。
///
/// 1. いま引いた牌で4枚目が揃ったこと。手に元からあった4枚を槓する
///    「送り槓」は、待ちを動かさなくても認めない
/// 2. 暗槓の前後で待ちが変わらないこと
fn riichi_ankan_allowed(s: &SeatState, kind: TileKind, drawn: Tile) -> bool {
    if drawn.kind() != kind {
        return false;
    }
    let before = waits_of(&hand_without(&s.hand, drawn), s.melds.len());
    let after = {
        let mut counts = HandCounts::from_tiles(&s.hand);
        for _ in 0..4 {
            counts.remove(kind);
        }
        let mut waits = waiting_tiles(&counts, s.melds.len() as u8 + 1);
        waits.sort_by_key(|k| k.index());
        waits
    };
    !before.is_empty() && before == after
}

/// 九種九牌。自分の最初のツモで、誰も鳴いておらず、幺九牌が9種類以上あること。
fn kyuushu_allowed(state: &RoundState, seat: Seat) -> bool {
    if state.draw_count[seat.index()] != 1 || state.any_call_made {
        return false;
    }
    let mut kinds: Vec<TileKind> = state
        .seat(seat)
        .hand
        .iter()
        .map(|t| t.kind())
        .filter(|k| k.is_terminal_or_honor())
        .collect();
    kinds.sort_by_key(|k| k.index());
    kinds.dedup();
    kinds.len() >= 9
}

// ---------- 反応 ----------

pub fn reaction_options(
    state: &RoundState,
    seat: Seat,
    discarded: Tile,
    from: Seat,
) -> Vec<ActionOption> {
    if seat == from {
        return Vec::new();
    }
    let s = state.seat(seat);
    let mut out = Vec::new();

    // リーチ成立後は鳴けない。
    if !is_riichi_accepted(s) {
        // チーは上家からのみ。
        if (from.index() + 1) % 4 == seat.index() {
            let candidates = chi_candidates(&s.hand, discarded);
            if !candidates.is_empty() {
                out.push(ActionOption::Chi { candidates });
            }
        }

        let candidates = pon_candidates(&s.hand, discarded);
        if !candidates.is_empty() {
            out.push(ActionOption::Pon { candidates });
        }

        if kans_left(state) && minkan_possible(&s.hand, discarded) {
            out.push(ActionOption::Kan {
                candidates: vec![KanCandidate::Minkan],
            });
        }
    }

    if ron_allowed(state, seat, discarded) {
        out.push(ActionOption::Ron);
    }
    out
}

/// ロンできるか。待ちに入っており、振聴でなく、役が1つ以上あること。
fn ron_allowed(state: &RoundState, seat: Seat, tile: Tile) -> bool {
    let s = state.seat(seat);
    let waits = waits_of(&s.hand, s.melds.len());
    if !waits.contains(&tile.kind()) {
        return false;
    }

    let river: Vec<Tile> = s.river.iter().map(|d| d.tile).collect();
    if is_furiten_by_discards(&waits, &river)
        || is_temporary_furiten(&waits, &s.passed_this_turn)
        || is_temporary_furiten(&waits, &s.permanent_furiten)
    {
        return false;
    }

    // 役が1つも無ければ和了できない。ドラは役ではない。
    let context = state.hand_context(seat, WinType::Ron);
    score(&s.hand, &s.melds, tile, &context, &state.rules).is_some()
}

// ---------- 槍槓 ----------

/// 槍槓の候補。ロン以外は出さない。
///
/// **呼ぶ前に `RoundState.pending_kan` を立てておくこと。**
/// `hand_context` はこれを見て `chankan` を立て、槍槓の1翻を付ける。
pub fn chankan_options(
    state: &RoundState,
    seat: Seat,
    tile: Tile,
    kind: MeldKind,
) -> Vec<ActionOption> {
    if state.pending_kan.map(|k| k.seat) == Some(seat) {
        return Vec::new();
    }
    if !ron_allowed(state, seat, tile) {
        return Vec::new();
    }
    // 暗槓を槍槓できるのは国士無双だけ。
    if kind == MeldKind::Ankan && !wins_with_kokushi(state, seat, tile) {
        return Vec::new();
    }
    vec![ActionOption::Ron]
}

fn wins_with_kokushi(state: &RoundState, seat: Seat, tile: Tile) -> bool {
    let s = state.seat(seat);
    let context = state.hand_context(seat, WinType::Ron);
    let Some(result) = score(&s.hand, &s.melds, tile, &context, &state.rules) else {
        return false;
    };
    result
        .yaku
        .iter()
        .any(|(id, _)| matches!(id, YakuId::KokushiMusou | YakuId::KokushiMusou13))
}

#[cfg(test)]
mod option_tests {
    use super::*;
    use crate::state::{Discarded, RiichiState, RoundState};
    use crate::wall::Seed;
    use protocol::event::{DiscardManner, DrawSource, RiichiStep};
    use protocol::meld::{Meld, MeldKind};
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::{Round, Seat, Wind};

    /// テスト用の局面を作る。`RoundState::new` は山から13枚ずつ配るが、
    /// この補助はその手牌を指定の形へ上書きする。山との整合は崩れるものの、
    /// ここで見たいのは「その手でどの選択肢が出るか」だけなので問題ない。
    /// 牌の枚数が合っているかは `invariant.rs` が別に検査する。
    fn state_with(seat: Seat, hand: &str) -> RoundState {
        let mut state = RoundState::new(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            Round {
                wind: Wind::East,
                number: 1,
            },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &Seed::from_hex(&"44".repeat(32)).unwrap(),
        );
        state.seat_mut(seat).hand = parse_hand(hand).unwrap();
        state.draw_count[seat.index()] = 1;
        state
    }

    /// 13枚の手へ1枚ツモった状態にして、その打牌局面を作る。
    fn after_drawing(state: &mut RoundState, seat: Seat, drawn: &str) -> TurnStart {
        let tile = parse_tile(drawn).unwrap();
        state.seat_mut(seat).hand.push(tile);
        state.last_draw = Some((seat, DrawSource::Wall));
        TurnStart::Draw {
            tile,
            source: DrawSource::Wall,
        }
    }

    fn accept_riichi(state: &mut RoundState, seat: Seat) {
        state.seat_mut(seat).riichi = Some(RiichiState {
            step: RiichiStep::Accepted,
            declared_at_turn: 2,
            ippatsu: false,
            double: false,
        });
    }

    fn discard_of(options: &[ActionOption]) -> (Vec<Tile>, Vec<Tile>) {
        for option in options {
            if let ActionOption::Discard {
                allowed,
                riichi_allowed,
            } = option
            {
                return (allowed.clone(), riichi_allowed.clone());
            }
        }
        panic!("打牌の選択肢がない: {options:?}");
    }

    fn kans_of(options: &[ActionOption]) -> Vec<KanCandidate> {
        for option in options {
            if let ActionOption::Kan { candidates } = option {
                return candidates.clone();
            }
        }
        Vec::new()
    }

    // ---------- 打牌 ----------

    /// リーチしていなければ手のどれでも切れる。同じ牌は1つにまとめる。
    #[test]
    fn a_free_seat_may_discard_any_distinct_tile() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let (allowed, _) = discard_of(&discard_options(&state, Seat::new(1), start));
        // 2s が2枚あるので、重複を除くと13種。
        assert_eq!(allowed.len(), 13);
        assert!(allowed.contains(&parse_tile("2s").unwrap()));
        assert!(allowed.contains(&parse_tile("1z").unwrap()));
    }

    /// 赤5と通常の5は別の牌なので、どちらも別々に切れる。
    #[test]
    fn a_red_five_is_a_separate_discard_choice() {
        let mut state = state_with(Seat::new(1), "234m50p678p234s11z");
        let start = after_drawing(&mut state, Seat::new(1), "9m");
        let (allowed, _) = discard_of(&discard_options(&state, Seat::new(1), start));
        assert!(allowed.contains(&parse_tile("5p").unwrap()));
        assert!(allowed.contains(&parse_tile("0p").unwrap()));
    }

    /// リーチ中はツモ切りしかできない。
    #[test]
    fn a_riichi_seat_can_only_discard_the_drawn_tile() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        accept_riichi(&mut state, Seat::new(1));
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let (allowed, riichi_allowed) = discard_of(&discard_options(&state, Seat::new(1), start));
        assert_eq!(allowed, vec![parse_tile("1z").unwrap()]);
        assert!(riichi_allowed.is_empty(), "既にリーチしている");
    }

    // ---------- リーチ宣言 ----------

    /// テンパイが保てる牌だけがリーチ宣言牌になる。
    #[test]
    fn riichi_is_offered_only_for_discards_that_keep_tenpai() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let (_, riichi_allowed) = discard_of(&discard_options(&state, Seat::new(1), start));
        // 1z を切れば元のテンパイに戻る。他の牌を切ると浮き牌が2枚残る。
        assert_eq!(riichi_allowed, vec![parse_tile("1z").unwrap()]);
    }

    /// 鳴いている席はリーチできない。
    #[test]
    fn an_open_hand_cannot_declare_riichi() {
        let mut state = state_with(Seat::new(1), "234567m78p22s");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("444p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("4p").unwrap()),
        });
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let (_, riichi_allowed) = discard_of(&discard_options(&state, Seat::new(1), start));
        assert!(riichi_allowed.is_empty());
    }

    /// 持ち点が1000点未満なら供託を出せないのでリーチできない。
    #[test]
    fn a_seat_below_one_thousand_points_cannot_declare_riichi() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state.scores[1] = 900;
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let (_, riichi_allowed) = discard_of(&discard_options(&state, Seat::new(1), start));
        assert!(riichi_allowed.is_empty());
    }

    /// 山が尽きかけているとリーチできない。
    #[test]
    fn riichi_needs_tiles_left_in_the_wall() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        while state.wall.live_remaining() >= 4 {
            state.wall.draw().expect("まだ引ける");
        }
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let (_, riichi_allowed) = discard_of(&discard_options(&state, Seat::new(1), start));
        assert!(riichi_allowed.is_empty());
    }

    // ---------- 食い替え ----------

    /// チーで順子の下端を鳴いたら、その牌と上端の隣が打てない。
    ///
    /// 手牌が11枚なのは正しい。配牌13枚から2枚を出してチーすると
    /// 手は11枚になり、副露3枚と合わせて14枚相当になる。ここから1枚
    /// 切って10枚＋副露3枚＝13枚に戻る。
    #[test]
    fn chi_forbids_the_called_tile_and_its_suji() {
        // 4p を上家からチーして 456p。4p（現物）と 7p（筋）が打てない。
        let mut state = state_with(Seat::new(1), "4p7p123m456m789m");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Chi,
            tiles: parse_hand("456p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("4p").unwrap()),
        });
        let (allowed, _) =
            discard_of(&discard_options(&state, Seat::new(1), TurnStart::AfterCall));
        assert!(!allowed.contains(&parse_tile("4p").unwrap()), "現物の食い替え");
        assert!(!allowed.contains(&parse_tile("7p").unwrap()), "筋の食い替え");
        assert_eq!(allowed.len(), 9, "残る9種は打てる");
    }

    /// 嵌張でチーした場合、筋の制限はない。
    #[test]
    fn a_closed_wait_chi_forbids_only_the_called_tile() {
        // 5p を鳴いて 456p。5p だけが打てない。
        let mut state = state_with(Seat::new(1), "5p8p123m456m789m");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Chi,
            tiles: parse_hand("456p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("5p").unwrap()),
        });
        let (allowed, _) =
            discard_of(&discard_options(&state, Seat::new(1), TurnStart::AfterCall));
        assert!(!allowed.contains(&parse_tile("5p").unwrap()));
        assert!(allowed.contains(&parse_tile("8p").unwrap()), "筋の制限は無い");
    }

    /// ポンは現物だけが打てない。
    #[test]
    fn pon_forbids_only_the_called_tile() {
        let mut state = state_with(Seat::new(1), "5p8p123m456m789m");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("555p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("5p").unwrap()),
        });
        let (allowed, _) =
            discard_of(&discard_options(&state, Seat::new(1), TurnStart::AfterCall));
        assert!(!allowed.contains(&parse_tile("5p").unwrap()));
        assert!(allowed.contains(&parse_tile("8p").unwrap()));
    }

    /// 鳴いた直後はツモ和了も九種九牌も槓もできない。
    #[test]
    fn a_call_offers_nothing_but_a_discard() {
        let mut state = state_with(Seat::new(1), "5p8p123m456m789m");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("555p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("5p").unwrap()),
        });
        let options = discard_options(&state, Seat::new(1), TurnStart::AfterCall);
        assert_eq!(options.len(), 1);
        assert!(matches!(options[0], ActionOption::Discard { .. }));
    }

    // ---------- ツモ和了 ----------

    /// 和了形になっていればツモを提示する。
    #[test]
    fn a_completing_draw_is_offered_tsumo() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        let start = after_drawing(&mut state, Seat::new(1), "6p");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(options.iter().any(|o| matches!(o, ActionOption::Tsumo)));
    }

    /// 和了形でなければツモは出ない。
    #[test]
    fn an_unrelated_draw_is_not_offered_tsumo() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        let start = after_drawing(&mut state, Seat::new(1), "1z");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(!options.iter().any(|o| matches!(o, ActionOption::Tsumo)));
    }

    /// 振聴でもツモ和了はできる。振聴が縛るのはロンだけである。
    #[test]
    fn a_furiten_seat_may_still_tsumo() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state.seat_mut(Seat::new(1)).river.push(Discarded {
            tile: parse_tile("9p").unwrap(),
            manner: DiscardManner::Tsumogiri,
            called_by: None,
            riichi_declaration: false,
        });
        let start = after_drawing(&mut state, Seat::new(1), "6p");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(options.iter().any(|o| matches!(o, ActionOption::Tsumo)));
    }

    // ---------- 九種九牌 ----------

    /// 境界をまたぐ2件は同じ手牌を使い、ツモ牌だけを変える。
    /// 13枚の時点の幺九牌は 1m 9m 1p 9p 1s 9s 1z 2z の8種類である。
    const EIGHT_KINDS: &str = "19m34555m19p19s12z";

    /// ツモで9種類目が入れば九種九牌を提示する。ちょうど9種類の境界。
    #[test]
    fn exactly_nine_kinds_offer_an_abort() {
        let mut state = state_with(Seat::new(1), EIGHT_KINDS);
        // 3z が9種類目になる。
        let start = after_drawing(&mut state, Seat::new(1), "3z");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(options.iter().any(|o| matches!(o, ActionOption::Kyuushu)));
    }

    /// 8種類のままでは出ない。
    #[test]
    fn eight_kinds_do_not_offer_an_abort() {
        let mut state = state_with(Seat::new(1), EIGHT_KINDS);
        // 6m は幺九牌ではないので種類数は8のままである。
        let start = after_drawing(&mut state, Seat::new(1), "6m");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(!options.iter().any(|o| matches!(o, ActionOption::Kyuushu)));
    }

    /// 2巡目には出ない。
    #[test]
    fn an_abort_is_only_offered_on_the_first_draw() {
        let mut state = state_with(Seat::new(1), EIGHT_KINDS);
        state.draw_count[1] = 2;
        let start = after_drawing(&mut state, Seat::new(1), "3z");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(!options.iter().any(|o| matches!(o, ActionOption::Kyuushu)));
    }

    /// 誰かが鳴いていれば出ない。
    #[test]
    fn a_call_cancels_the_abort() {
        let mut state = state_with(Seat::new(1), EIGHT_KINDS);
        state.any_call_made = true;
        let start = after_drawing(&mut state, Seat::new(1), "3z");
        let options = discard_options(&state, Seat::new(1), start);
        assert!(!options.iter().any(|o| matches!(o, ActionOption::Kyuushu)));
    }

    // ---------- 槓 ----------

    /// 手に4枚あれば暗槓できる。
    #[test]
    fn four_in_hand_offer_an_ankan() {
        let mut state = state_with(Seat::new(1), "1111m234p567p22s9s");
        let start = after_drawing(&mut state, Seat::new(1), "3s");
        let kans = kans_of(&discard_options(&state, Seat::new(1), start));
        assert_eq!(
            kans,
            vec![KanCandidate::Ankan {
                kind: parse_tile("1m").unwrap().kind()
            }]
        );
    }

    /// ポンした牌の4枚目を引けば加槓できる。
    #[test]
    fn the_fourth_tile_of_a_pon_offers_a_kakan() {
        let mut state = state_with(Seat::new(1), "234567m78p22s");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("444p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("4p").unwrap()),
        });
        let start = after_drawing(&mut state, Seat::new(1), "4p");
        let kans = kans_of(&discard_options(&state, Seat::new(1), start));
        assert_eq!(
            kans,
            vec![KanCandidate::Kakan {
                tile: parse_tile("4p").unwrap()
            }]
        );
    }

    /// 4つの槓が済んでいれば、5つ目は提示しない。
    #[test]
    fn a_fifth_kan_is_never_offered() {
        let mut state = state_with(Seat::new(1), "1111m234p567p22s9s");
        state.kan_count = [2, 1, 1, 0];
        let start = after_drawing(&mut state, Seat::new(1), "3s");
        assert!(kans_of(&discard_options(&state, Seat::new(1), start)).is_empty());
    }

    /// リーチ中は、いま引いた牌で揃った暗槓しかできない（送り槓の禁止）。
    #[test]
    fn a_riichi_seat_cannot_kan_a_set_it_was_already_holding() {
        // 1111m 123m(23m+4枚目) 234p 567p ＋ 9s の単騎。リーチ時から 1m を4枚
        // 持っており、9s 待ちのテンパイである。
        let mut state = state_with(Seat::new(1), "1111m23m234p567p9s");
        accept_riichi(&mut state, Seat::new(1));
        // テストデータの意味を固定する。リーチできる形であることを先に示す。
        assert_eq!(
            waits_of(&state.seat(Seat::new(1)).hand, 0),
            vec![parse_tile("9s").unwrap().kind()],
            "9s の単騎テンパイでなければ、この局面はリーチできない"
        );
        // 引いたのは 1m ではないので、いま暗槓すれば送り槓になる。
        let start = after_drawing(&mut state, Seat::new(1), "5z");
        assert!(kans_of(&discard_options(&state, Seat::new(1), start)).is_empty());
    }

    /// リーチ中でも、待ちが変わらない暗槓は認める。
    #[test]
    fn a_riichi_seat_may_kan_when_the_wait_does_not_move() {
        // 111m 234p 567p 22s ＋ 78s で 6s/9s の両面待ち。1m を暗槓すると
        // 手は 234p 567p 22s 78s ＋ 槓1つになるが、待ちは 6s/9s のままである。
        let mut state = state_with(Seat::new(1), "111m234p567p22s78s");
        accept_riichi(&mut state, Seat::new(1));
        let start = after_drawing(&mut state, Seat::new(1), "1m");
        let kans = kans_of(&discard_options(&state, Seat::new(1), start));
        assert_eq!(
            kans,
            vec![KanCandidate::Ankan {
                kind: parse_tile("1m").unwrap().kind()
            }]
        );
    }

    /// リーチ中は加槓できない。手牌の構成が変わるためである。
    /// 鳴いた席はそもそもリーチできないので、この局面は進行上は起こらない。
    /// ガードが効いていることだけを確かめる。
    #[test]
    fn a_riichi_seat_cannot_kakan() {
        let mut state = state_with(Seat::new(1), "234567m78p22s");
        state.seat_mut(Seat::new(1)).melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("444p").unwrap(),
            from: Some(Seat::new(0)),
            called_tile: Some(parse_tile("4p").unwrap()),
        });
        accept_riichi(&mut state, Seat::new(1));
        let start = after_drawing(&mut state, Seat::new(1), "4p");
        assert!(kans_of(&discard_options(&state, Seat::new(1), start)).is_empty());
    }

    // ---------- 反応 ----------

    /// 自分の打牌には反応できない。
    #[test]
    fn a_seat_cannot_react_to_its_own_discard() {
        let state = state_with(Seat::new(1), "234567m23478p22s");
        assert!(reaction_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            Seat::new(1)
        )
        .is_empty());
    }

    /// 同じ牌が2枚あればポンできる。
    #[test]
    fn two_matching_tiles_offer_a_pon() {
        let state = state_with(Seat::new(1), "234567m23478p22s");
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("2s").unwrap(),
            Seat::new(2),
        );
        let Some(ActionOption::Pon { candidates }) = options
            .iter()
            .find(|o| matches!(o, ActionOption::Pon { .. }))
        else {
            panic!("ポンが出ていない: {options:?}");
        };
        assert_eq!(candidates.len(), 1);
    }

    /// 同じ牌が3枚あれば明槓もできる。
    #[test]
    fn three_matching_tiles_offer_a_minkan() {
        let state = state_with(Seat::new(1), "222s234567m2347p");
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("2s").unwrap(),
            Seat::new(2),
        );
        assert_eq!(
            kans_of(&options),
            vec![KanCandidate::Minkan],
            "明槓が出ていない"
        );
        assert!(options.iter().any(|o| matches!(o, ActionOption::Pon { .. })));
    }

    /// チーは上家からのみ。
    #[test]
    fn chi_is_only_offered_to_the_seat_below() {
        let state = state_with(Seat::new(1), "234567m23478p22s");
        // 席0は席1の上家。
        let from_kamicha = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("5p").unwrap(),
            Seat::new(0),
        );
        assert!(from_kamicha
            .iter()
            .any(|o| matches!(o, ActionOption::Chi { .. })));

        // 席2は上家ではない。
        let from_toimen = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("5p").unwrap(),
            Seat::new(2),
        );
        assert!(!from_toimen
            .iter()
            .any(|o| matches!(o, ActionOption::Chi { .. })));
    }

    /// リーチ中は鳴けない。ロンだけが残る。
    #[test]
    fn a_riichi_seat_can_only_ron() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        accept_riichi(&mut state, Seat::new(1));
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            Seat::new(0),
        );
        assert_eq!(options, vec![ActionOption::Ron]);
    }

    // ---------- ロン ----------

    /// 役なしの完成形はロンできない。ドラだけでは和了できない。
    #[test]
    fn a_yakuless_hand_is_not_offered_ron() {
        // 123m 345m 456p 789s ＋ 西の単騎。門前ロンだが役が無い。
        let state = state_with(Seat::new(1), "123m345m456p789s3z");
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("3z").unwrap(),
            Seat::new(0),
        );
        assert!(
            !options.iter().any(|o| matches!(o, ActionOption::Ron)),
            "役無しにロンを提示した"
        );
    }

    /// 役があればロンを提示する。
    #[test]
    fn a_hand_with_a_yaku_is_offered_ron() {
        // 平和＋断幺九の形。6p でロン。
        let state = state_with(Seat::new(1), "234567m23478p22s");
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            Seat::new(0),
        );
        assert!(options.iter().any(|o| matches!(o, ActionOption::Ron)));
    }

    /// 自分の河に待ち牌があればロンできない。
    #[test]
    fn a_seat_furiten_by_its_own_river_is_not_offered_ron() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state.seat_mut(Seat::new(1)).river.push(Discarded {
            tile: parse_tile("9p").unwrap(),
            manner: DiscardManner::Tsumogiri,
            called_by: None,
            riichi_declaration: false,
        });
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            Seat::new(0),
        );
        assert!(
            !options.iter().any(|o| matches!(o, ActionOption::Ron)),
            "78p の待ちは 6p と 9p。9p を捨てていれば振聴"
        );
    }

    /// 同巡内に見逃していればロンできない。
    #[test]
    fn a_temporary_furiten_seat_is_not_offered_ron() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state
            .seat_mut(Seat::new(1))
            .passed_this_turn
            .push(parse_tile("6p").unwrap().kind());
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            Seat::new(0),
        );
        assert!(!options.iter().any(|o| matches!(o, ActionOption::Ron)));
    }

    /// リーチ後の見逃しは局の終わりまで続く。
    #[test]
    fn a_permanent_furiten_seat_is_not_offered_ron() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state
            .seat_mut(Seat::new(1))
            .permanent_furiten
            .push(parse_tile("6p").unwrap().kind());
        let options = reaction_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            Seat::new(0),
        );
        assert!(!options.iter().any(|o| matches!(o, ActionOption::Ron)));
    }

    // ---------- 槍槓 ----------

    fn pending(state: &mut RoundState, kind: MeldKind, tile: &str) {
        state.pending_kan = Some(crate::state::PendingKan {
            seat: Seat::new(0),
            kind,
            tile: parse_tile(tile).unwrap(),
        });
    }

    /// 加槓は誰でも槍槓できる。候補はロンだけ。
    #[test]
    fn a_kakan_can_be_robbed_by_any_wait() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        pending(&mut state, MeldKind::Kakan, "6p");
        let options = chankan_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            MeldKind::Kakan,
        );
        assert_eq!(options, vec![ActionOption::Ron]);
    }

    /// 槓を宣言した本人は槍槓できない。
    #[test]
    fn the_declarer_cannot_rob_its_own_kan() {
        let mut state = state_with(Seat::new(0), "234567m23478p22s");
        pending(&mut state, MeldKind::Kakan, "6p");
        assert!(chankan_options(
            &state,
            Seat::new(0),
            parse_tile("6p").unwrap(),
            MeldKind::Kakan,
        )
        .is_empty());
    }

    /// 暗槓を槍槓できるのは国士無双だけ。
    #[test]
    fn an_ankan_can_only_be_robbed_by_kokushi() {
        // 通常の待ちでは暗槓を槍槓できない。
        let mut normal = state_with(Seat::new(1), "234567m23478p22s");
        pending(&mut normal, MeldKind::Ankan, "6p");
        assert!(chankan_options(
            &normal,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            MeldKind::Ankan,
        )
        .is_empty());

        // 国士の待ちなら槍槓できる。
        let mut kokushi = state_with(Seat::new(1), "119m19p19s123456z");
        pending(&mut kokushi, MeldKind::Ankan, "7z");
        assert_eq!(
            chankan_options(
                &kokushi,
                Seat::new(1),
                parse_tile("7z").unwrap(),
                MeldKind::Ankan,
            ),
            vec![ActionOption::Ron]
        );
    }

    /// 振聴なら槍槓もできない。
    #[test]
    fn a_furiten_seat_cannot_rob_a_kan() {
        let mut state = state_with(Seat::new(1), "234567m23478p22s");
        state.seat_mut(Seat::new(1)).river.push(Discarded {
            tile: parse_tile("6p").unwrap(),
            manner: DiscardManner::Tsumogiri,
            called_by: None,
            riichi_declaration: false,
        });
        pending(&mut state, MeldKind::Kakan, "6p");
        assert!(chankan_options(
            &state,
            Seat::new(1),
            parse_tile("6p").unwrap(),
            MeldKind::Kakan,
        )
        .is_empty());
    }
}
