//! 点数計算。符と翻から支払いを決める。
//!
//! `WinType` / `HandContext` / `Payment` / `ScoreResult` はここで定義し、
//! `yaku_check` と `fu` から参照する。
//!
//! 同じ手が複数通りに分解できる場合は**すべての分解を評価し、最も点数が
//! 高くなるものを採る**。

use protocol::meld::Meld;
use protocol::ruleset::Ruleset;
use protocol::seat::Wind;
use protocol::tile::{Tile, TileKind};
use protocol::yaku::YakuId;

use crate::decompose::{decompose, WinForm};
use crate::fu::fu_of;
use crate::yaku_check::{standard, yakuman};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinType {
    Tsumo,
    Ron,
}

/// 役と符の判定に必要な、手牌の外側の情報。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HandContext {
    pub win_type: WinType,
    pub seat_wind: Wind,
    pub round_wind: Wind,
    pub riichi: bool,
    pub double_riichi: bool,
    pub ippatsu: bool,
    pub rinshan: bool,
    pub chankan: bool,
    pub haitei: bool,
    pub houtei: bool,
    pub tenhou: bool,
    pub chiihou: bool,
    pub dora_indicators: Vec<Tile>,
    pub ura_indicators: Vec<Tile>,
}

impl HandContext {
    /// 状況役がすべて無い文脈。テストの土台に使う。
    pub fn plain(win_type: WinType, seat_wind: Wind, round_wind: Wind) -> Self {
        HandContext {
            win_type,
            seat_wind,
            round_wind,
            riichi: false,
            double_riichi: false,
            ippatsu: false,
            rinshan: false,
            chankan: false,
            haitei: false,
            houtei: false,
            tenhou: false,
            chiihou: false,
            dora_indicators: Vec::new(),
            ura_indicators: Vec::new(),
        }
    }

    pub fn is_dealer(&self) -> bool {
        self.seat_wind == Wind::East
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Payment {
    Ron {
        total: i32,
    },
    TsumoDealer {
        from_each: i32,
    },
    TsumoNonDealer {
        from_dealer: i32,
        from_each_non_dealer: i32,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ScoreResult {
    pub yaku: Vec<(YakuId, u8)>,
    pub fu: u8,
    pub han: u8,
    pub payment: Payment,
}

/// 和了形として成立し、かつ役があれば点数を返す。
///
/// **ドラは役ではないので、ドラだけでは和了できない。** 役が1つも無ければ None。
pub fn score(
    hand: &[Tile],
    melds: &[Meld],
    win_tile: Tile,
    context: &HandContext,
    rules: &Ruleset,
) -> Option<ScoreResult> {
    let forms = decompose(hand, melds, win_tile);
    if forms.is_empty() {
        return None;
    }

    let all_tiles = collect_tiles(hand, melds, win_tile);

    forms
        .iter()
        .filter_map(|form| evaluate(form, &all_tiles, win_tile, context, rules))
        .max_by_key(|result| {
            // 点数が最大になる分解を採る。同点なら翻の多い方。
            (payment_total(&result.payment), result.han as i32)
        })
}

fn evaluate(
    form: &WinForm,
    all_tiles: &[Tile],
    win_tile: Tile,
    context: &HandContext,
    rules: &Ruleset,
) -> Option<ScoreResult> {
    let yakuman_found = yakuman::detect(form, context);

    let (mut yaku, han, fu): (Vec<(YakuId, u8)>, u8, u8) = if !yakuman_found.is_empty() {
        // 役満は符を使わない。重なり数だけを見る。
        let list: Vec<(YakuId, u8)> = yakuman_found.iter().map(|id| (*id, 13u8)).collect();
        let total = list.iter().map(|(_, h)| *h as u32).sum::<u32>() as u8;
        (list, total, 0)
    } else {
        let normal = standard::detect(form, context);
        if normal.is_empty() {
            // 役無し。ドラだけでは和了できない。
            return None;
        }
        let has_pinfu = normal.iter().any(|(id, _)| *id == YakuId::Pinfu);
        let fu = fu_of(form, context, has_pinfu, win_tile.kind());
        let base_han: u8 = normal.iter().map(|(_, h)| *h).sum();
        (normal, base_han, fu)
    };

    // ドラは役の有無を判定した後に足す。
    let mut han = han;
    if fu > 0 || yaku.iter().all(|(id, _)| !id.is_yakuman()) {
        for (id, count) in count_dora(all_tiles, context) {
            if count > 0 {
                yaku.push((id, count));
                han += count;
            }
        }
    }

    let base = base_points(fu, han, rules);
    let payment = payout(base, context);

    Some(ScoreResult {
        yaku,
        fu,
        han,
        payment,
    })
}

/// ドラ・赤ドラ・裏ドラの枚数。
fn count_dora(tiles: &[Tile], context: &HandContext) -> Vec<(YakuId, u8)> {
    let mut out = Vec::new();

    let dora = tiles
        .iter()
        .filter(|t| {
            context
                .dora_indicators
                .iter()
                .any(|ind| next_of(ind.kind()) == t.kind())
        })
        .count() as u8;
    out.push((YakuId::Dora, dora));

    let aka = tiles.iter().filter(|t| t.is_red()).count() as u8;
    out.push((YakuId::AkaDora, aka));

    // 裏ドラはリーチしていたときだけ数える。
    if context.riichi || context.double_riichi {
        let ura = tiles
            .iter()
            .filter(|t| {
                context
                    .ura_indicators
                    .iter()
                    .any(|ind| next_of(ind.kind()) == t.kind())
            })
            .count() as u8;
        out.push((YakuId::UraDora, ura));
    }

    out
}

/// ドラ表示牌の次の牌。数牌は9の次が1、風牌は北の次が東、三元牌は中の次が白。
fn next_of(indicator: TileKind) -> TileKind {
    let index = indicator.index();
    let next = match index {
        0..=26 => {
            let base = (index / 9) * 9;
            base + (index - base + 1) % 9
        }
        27..=30 => 27 + (index - 27 + 1) % 4,
        _ => 31 + (index - 31 + 1) % 3,
    };
    TileKind::from_index(next).expect("範囲内")
}

fn collect_tiles(hand: &[Tile], melds: &[Meld], win_tile: Tile) -> Vec<Tile> {
    let mut out = hand.to_vec();
    out.push(win_tile);
    for meld in melds {
        out.extend(meld.tiles.iter().copied());
    }
    out
}

/// 基本点。切り上げ満貫が無いため、4翻以下は基本点が2000を超えたときだけ満貫になる。
pub fn base_points(fu: u8, han: u8, _rules: &Ruleset) -> i32 {
    if han >= 13 {
        // 13翻ごとに役満1つ分。
        return 8000 * (han as i32 / 13);
    }
    match han {
        11..=12 => 6000,
        8..=10 => 4000,
        6..=7 => 3000,
        5 => 2000,
        _ => {
            let raw = fu as i32 * 2i32.pow(2 + han as u32);
            raw.min(2000)
        }
    }
}

pub fn round_up_to_hundred(value: i32) -> i32 {
    (value + 99) / 100 * 100
}

fn payout(base: i32, context: &HandContext) -> Payment {
    match (context.win_type, context.is_dealer()) {
        (WinType::Ron, true) => Payment::Ron {
            total: round_up_to_hundred(base * 6),
        },
        (WinType::Ron, false) => Payment::Ron {
            total: round_up_to_hundred(base * 4),
        },
        (WinType::Tsumo, true) => Payment::TsumoDealer {
            from_each: round_up_to_hundred(base * 2),
        },
        (WinType::Tsumo, false) => Payment::TsumoNonDealer {
            from_dealer: round_up_to_hundred(base * 2),
            from_each_non_dealer: round_up_to_hundred(base),
        },
    }
}

fn payment_total(payment: &Payment) -> i32 {
    match payment {
        Payment::Ron { total } => *total,
        Payment::TsumoDealer { from_each } => from_each * 3,
        Payment::TsumoNonDealer {
            from_dealer,
            from_each_non_dealer,
        } => from_dealer + from_each_non_dealer * 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::meld::MeldKind;
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::ruleset::MatchLength;
    use protocol::seat::Seat;

    fn wind_of(name: &str) -> Wind {
        match name {
            "east" => Wind::East,
            "south" => Wind::South,
            "west" => Wind::West,
            "north" => Wind::North,
            other => panic!("未知の風: {other}"),
        }
    }

    fn meld_of(spec: &test_fixtures::MeldSpec) -> Meld {
        let kind = match spec.kind.as_str() {
            "chi" => MeldKind::Chi,
            "pon" => MeldKind::Pon,
            "ankan" => MeldKind::Ankan,
            "minkan" => MeldKind::Minkan,
            "kakan" => MeldKind::Kakan,
            other => panic!("未知の副露: {other}"),
        };
        Meld {
            kind,
            tiles: parse_hand(&spec.tiles).unwrap(),
            from: Some(Seat::new(spec.from)),
            called_tile: spec.called_tile.as_ref().map(|t| parse_tile(t).unwrap()),
        }
    }

    /// 期待値テーブルの全ケースを通す。これが Wave 1b の合否条件。
    #[test]
    fn matches_every_scoring_fixture() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);

        for case in test_fixtures::load_scoring_cases() {
            let hand = parse_hand(&case.concealed).unwrap();
            let melds: Vec<Meld> = case.melds.iter().map(meld_of).collect();
            let win_tile = parse_tile(&case.win_tile).unwrap();

            let mut context = HandContext::plain(
                match case.win_type {
                    test_fixtures::WinType::Tsumo => WinType::Tsumo,
                    test_fixtures::WinType::Ron => WinType::Ron,
                },
                wind_of(&case.context.seat_wind),
                wind_of(&case.context.round_wind),
            );
            context.riichi = case.context.riichi;
            context.double_riichi = case.context.double_riichi;
            context.ippatsu = case.context.ippatsu;
            context.rinshan = case.context.rinshan;
            context.chankan = case.context.chankan;
            context.haitei = case.context.haitei;
            context.houtei = case.context.houtei;
            context.dora_indicators = case
                .context
                .dora_indicators
                .iter()
                .map(|t| parse_tile(t).unwrap())
                .collect();
            context.ura_indicators = case
                .context
                .ura_indicators
                .iter()
                .map(|t| parse_tile(t).unwrap())
                .collect();

            let result = score(&hand, &melds, win_tile, &context, &rules)
                .unwrap_or_else(|| panic!("{}: 和了形として認識されなかった", case.id));

            let mut actual: Vec<_> = result
                .yaku
                .iter()
                .filter(|(_, han)| *han > 0)
                .map(|(y, h)| (*y, *h))
                .collect();
            actual.sort();
            let mut expected: Vec<_> = case.expect.yaku.iter().map(|y| (y.id, y.han)).collect();
            expected.sort();
            assert_eq!(
                actual, expected,
                "{}: 役の一覧が食い違う（{}）",
                case.id, case.note
            );

            assert_eq!(
                result.fu, case.expect.fu,
                "{}: 符が {} だが期待は {}（{}）",
                case.id, result.fu, case.expect.fu, case.note
            );
            assert_eq!(
                result.han, case.expect.han,
                "{}: 翻が {} だが期待は {}（{}）",
                case.id, result.han, case.expect.han, case.note
            );

            let expected_payment = match case.expect.payment {
                test_fixtures::Payment::Ron { total } => Payment::Ron { total },
                test_fixtures::Payment::TsumoDealer { from_each } => {
                    Payment::TsumoDealer { from_each }
                }
                test_fixtures::Payment::TsumoNonDealer {
                    from_dealer,
                    from_each_non_dealer,
                } => Payment::TsumoNonDealer {
                    from_dealer,
                    from_each_non_dealer,
                },
            };
            assert_eq!(
                result.payment, expected_payment,
                "{}: 支払いが食い違う（{}）",
                case.id, case.note
            );
        }
    }

    #[test]
    fn base_points_cap_at_mangan_from_five_han() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);
        assert_eq!(base_points(20, 5, &rules), 2000);
        assert_eq!(
            base_points(70, 4, &rules),
            2000,
            "4翻でも基本点2000を超えたら満貫"
        );
        assert_eq!(
            base_points(30, 4, &rules),
            1920,
            "切り上げ満貫は無いので7700のまま"
        );
    }

    #[test]
    fn payments_round_up_to_the_hundred() {
        assert_eq!(round_up_to_hundred(1280), 1300);
        assert_eq!(round_up_to_hundred(1300), 1300);
        assert_eq!(round_up_to_hundred(1), 100);
    }

    #[test]
    fn a_hand_that_is_not_complete_scores_nothing() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);
        let context = HandContext::plain(WinType::Tsumo, Wind::South, Wind::East);
        assert!(score(
            &parse_hand("147m258p369s1234z").unwrap(),
            &[],
            parse_tile("1m").unwrap(),
            &context,
            &rules
        )
        .is_none());
    }

    /// ドラは役ではない。役が無ければドラがあっても和了できない。
    #[test]
    fn dora_alone_does_not_make_a_winning_hand() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);
        let mut context = HandContext::plain(WinType::Ron, Wind::South, Wind::East);
        context.dora_indicators = vec![parse_tile("8m").unwrap()];

        // 123m 345m 456p 789s ＋ 西の単騎をロン。
        // 1m と 9s があるので断幺九にならず、単騎待ちなので平和にもならない。
        // 一通・三色・一盃口・チャンタ・役牌のいずれも成立しない役無しの手。
        let result = score(
            &parse_hand("123m345m456p789s3z").unwrap(),
            &[],
            parse_tile("3z").unwrap(),
            &context,
            &rules,
        );
        assert!(result.is_none(), "役が無いのに和了になった: {result:?}");
    }

    /// ドラ表示牌の次の牌がドラ。9の次は1、北の次は東、中の次は白。
    #[test]
    fn dora_wraps_around_within_its_group() {
        let k = |n: &str| parse_tile(n).unwrap().kind();
        assert_eq!(next_of(k("1m")), k("2m"));
        assert_eq!(next_of(k("9m")), k("1m"));
        assert_eq!(next_of(k("9s")), k("1s"));
        assert_eq!(next_of(k("4z")), k("1z"));
        assert_eq!(next_of(k("7z")), k("5z"));
    }
}
