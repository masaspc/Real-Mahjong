//! 締切と溜め時間バンクの計算。
//!
//! 仕様 6.2.1 と 6.2.2 をそのまま関数にしたもの。状態を持たない。

use protocol::effect::{effect_duration_ms, effect_of};
use protocol::event::Event;
use protocol::ruleset::Ruleset;

/// 直前に配信した一連のイベントの演出時間の合計。
///
/// 呼び出し側は「前回その席へ RequestAction を送ってから今回まで」に
/// 絞ったイベント列を渡す。区間の切り出しは進行側（Wave 2b）の責務である。
pub fn lead_in_of(events: &[Event]) -> u32 {
    events
        .iter()
        .filter_map(effect_of)
        .map(effect_duration_ms)
        .sum()
}

/// 行動要求の絶対締切。演出で思考時間が削られないよう lead_in を加算する。
pub fn deadline_for(rules: &Ruleset, now_ms: u64, bank_remaining_ms: u32, lead_in_ms: u32) -> u64 {
    now_ms
        + rules.base_think_ms as u64
        + bank_remaining_ms as u64
        + rules.network_grace_ms as u64
        + lead_in_ms as u64
}

/// 応答を受け取ったあとのバンク残量。
///
/// **演出を見ていた時間と通信の遅れは課金しない。** 課金すると 6.2 で
/// 締切を後ろへずらした意味が失われる。
pub fn charge_bank(
    rules: &Ruleset,
    bank_remaining_ms: u32,
    elapsed_ms: u64,
    lead_in_ms: u32,
) -> u32 {
    let excluded = lead_in_ms as u64 + rules.network_grace_ms as u64;
    let thinking = elapsed_ms.saturating_sub(excluded);
    let overtime = thinking.saturating_sub(rules.base_think_ms as u64);
    bank_remaining_ms.saturating_sub(overtime.min(u32::MAX as u64) as u32)
}

/// イベントへ載せる残り時間。要求発行時点からの相対値で、既に過ぎていたら 0。
pub fn remaining_for_event(absolute_deadline: u64, now_ms: u64) -> u32 {
    let remaining = absolute_deadline.saturating_sub(now_ms);
    debug_assert!(
        remaining <= u32::MAX as u64,
        "締切が u32 に収まらない: {remaining}"
    );
    remaining.min(u32::MAX as u64) as u32
}
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::event::{DiscardManner, DrawSource, Event};
    use protocol::meld::MeldKind;
    use protocol::notation::parse_tile;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::Seat;

    fn rules() -> Ruleset {
        Ruleset::kin_no_ma(MatchLength::Hanchan)
    }

    /// 槓の演出は宣言が持ち、成立は0。
    /// KanDeclared 1100 + Call 0 + DoraReveal 800 + Draw 250 + Discard 350 = 2500
    #[test]
    fn lead_in_counts_a_kan_animation_once() {
        let events = vec![
            Event::KanDeclared {
                seat: Seat::new(1),
                kind: MeldKind::Kakan,
                tile: parse_tile("5s").unwrap(),
            },
            Event::Call {
                seat: Seat::new(1),
                from: Seat::new(1),
                kind: MeldKind::Kakan,
                tiles: vec![parse_tile("5s").unwrap()],
            },
            Event::DoraReveal {
                indicator: parse_tile("1z").unwrap(),
            },
            Event::Draw {
                seat: Seat::new(1),
                tile: parse_tile("2m").unwrap(),
                source: DrawSource::DeadWall,
                wall_remaining: 60,
            },
            Event::Discard {
                seat: Seat::new(1),
                tile: parse_tile("1m").unwrap(),
                manner: DiscardManner::Tsumogiri,
            },
        ];
        assert_eq!(lead_in_of(&events), 2_500);
    }

    #[test]
    fn an_empty_event_list_has_no_lead_in() {
        assert_eq!(lead_in_of(&[]), 0);
    }

    #[test]
    fn the_deadline_pushes_back_by_the_lead_in() {
        let plain = deadline_for(&rules(), 10_000, 20_000, 0);
        assert_eq!(plain, 10_000 + 5_000 + 20_000 + 500);
        assert_eq!(deadline_for(&rules(), 10_000, 20_000, 1_800), plain + 1_800);
    }

    #[test]
    fn an_empty_bank_leaves_only_the_base_time() {
        assert_eq!(deadline_for(&rules(), 0, 0, 0), 5_500);
    }

    #[test]
    fn answering_within_the_base_time_costs_nothing() {
        assert_eq!(charge_bank(&rules(), 20_000, 4_000, 0), 20_000);
        assert_eq!(charge_bank(&rules(), 20_000, 5_000, 0), 20_000);
    }

    /// 通信猶予はバンクから引かない。基準時間ちょうど＋猶予でも減らない。
    #[test]
    fn the_network_grace_is_not_charged() {
        assert_eq!(charge_bank(&rules(), 20_000, 5_500, 0), 20_000);
        assert_eq!(charge_bank(&rules(), 20_000, 5_501, 0), 19_999);
    }

    /// 演出を見ていた時間も課金しない。
    /// 実時間 8000 − 演出 1800 − 猶予 500 = 思考 5700。超過は 700。
    #[test]
    fn the_lead_in_is_not_charged() {
        assert_eq!(charge_bank(&rules(), 20_000, 8_000, 1_800), 19_300);
    }

    /// 実時間 8000 − 猶予 500 = 思考 7500。超過は 2500。
    #[test]
    fn overtime_comes_out_of_the_bank() {
        assert_eq!(charge_bank(&rules(), 20_000, 8_000, 0), 17_500);
    }

    #[test]
    fn the_bank_never_goes_below_zero() {
        assert_eq!(charge_bank(&rules(), 1_000, 30_000, 0), 0);
    }

    /// イベントへ載せる残り時間は要求発行時点からの相対値。
    #[test]
    fn the_event_carries_a_relative_deadline() {
        assert_eq!(remaining_for_event(35_500, 10_000), 25_500);
        assert_eq!(remaining_for_event(10_000, 10_000), 0);
        assert_eq!(remaining_for_event(9_000, 10_000), 0, "既に過ぎていたら0");
    }
}
