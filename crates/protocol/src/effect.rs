//! 演出カタログ。サーバとクライアントが同じ表を見ることが要件そのものである。
//!
//! サーバはこの表を使って思考時間の締切に演出時間を足し、
//! クライアントは同じ表の通りに再生する。値がずれると、演出を見ている間に
//! 持ち時間が削られるという理不尽が生まれる。

use serde::{Deserialize, Serialize};

use crate::event::{Event, RiichiStep};
use crate::meld::MeldKind;
use crate::ruleset::Ruleset;

/// 局の進行を止める演出の種類。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    Draw,
    Discard,
    Pon,
    Chi,
    Kan,
    RiichiDeclare,
    DoraReveal,
}

pub const fn effect_duration_ms(kind: EffectKind) -> u32 {
    match kind {
        EffectKind::Draw => 250,
        EffectKind::Discard => 350,
        EffectKind::Pon => 700,
        EffectKind::Chi => 700,
        EffectKind::Kan => 1_100,
        EffectKind::RiichiDeclare => 1_800,
        EffectKind::DoraReveal => 800,
    }
}

/// そのイベントが進行を止める演出を伴うか。伴わないものは None。
pub fn effect_of(event: &Event) -> Option<EffectKind> {
    match event {
        Event::Draw { .. } => Some(EffectKind::Draw),
        Event::Discard { .. } => Some(EffectKind::Discard),
        Event::DoraReveal { .. } => Some(EffectKind::DoraReveal),
        Event::Riichi { step, .. } => match step {
            RiichiStep::Declare => Some(EffectKind::RiichiDeclare),
            RiichiStep::Accepted => None,
        },
        Event::Call { kind, .. } => match kind {
            MeldKind::Chi => Some(EffectKind::Chi),
            MeldKind::Pon => Some(EffectKind::Pon),
            MeldKind::Ankan | MeldKind::Minkan | MeldKind::Kakan => Some(EffectKind::Kan),
        },
        _ => None,
    }
}

/// 直前に配信した一連のイベントの演出時間の合計。
pub fn lead_in_ms(events: &[Event]) -> u32 {
    events
        .iter()
        .filter_map(effect_of)
        .map(effect_duration_ms)
        .sum()
}

/// 行動要求の締切。演出で思考時間が削られないよう lead_in を加算する。
pub fn action_deadline_ms(rules: &Ruleset, bank_remaining_ms: u32, lead_in_ms: u32) -> u32 {
    rules.base_think_ms + bank_remaining_ms + rules.network_grace_ms + lead_in_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{DiscardManner, Event, RiichiStep};
    use crate::meld::MeldKind;
    use crate::notation::parse_tile;
    use crate::ruleset::{MatchLength, Ruleset};
    use crate::seat::Seat;

    #[test]
    fn catalog_matches_the_spec() {
        assert_eq!(effect_duration_ms(EffectKind::Draw), 250);
        assert_eq!(effect_duration_ms(EffectKind::Discard), 350);
        assert_eq!(effect_duration_ms(EffectKind::Pon), 700);
        assert_eq!(effect_duration_ms(EffectKind::Chi), 700);
        assert_eq!(effect_duration_ms(EffectKind::Kan), 1_100);
        assert_eq!(effect_duration_ms(EffectKind::RiichiDeclare), 1_800);
        assert_eq!(effect_duration_ms(EffectKind::DoraReveal), 800);
    }

    #[test]
    fn maps_events_to_their_effects() {
        let discard = Event::Discard {
            seat: Seat::new(0),
            tile: parse_tile("1m").unwrap(),
            manner: DiscardManner::Tsumogiri,
        };
        assert_eq!(effect_of(&discard), Some(EffectKind::Discard));

        let declare = Event::Riichi {
            seat: Seat::new(0),
            step: RiichiStep::Declare,
        };
        assert_eq!(effect_of(&declare), Some(EffectKind::RiichiDeclare));

        // 成立側は点棒の移動のみで、局の進行を止める演出を持たない。
        let accepted = Event::Riichi {
            seat: Seat::new(0),
            step: RiichiStep::Accepted,
        };
        assert_eq!(effect_of(&accepted), None);

        let pon = Event::Call {
            seat: Seat::new(1),
            from: Seat::new(0),
            kind: MeldKind::Pon,
            tiles: vec![parse_tile("1m").unwrap()],
        };
        assert_eq!(effect_of(&pon), Some(EffectKind::Pon));

        let ankan = Event::Call {
            seat: Seat::new(1),
            from: Seat::new(1),
            kind: MeldKind::Ankan,
            tiles: vec![parse_tile("1m").unwrap()],
        };
        assert_eq!(effect_of(&ankan), Some(EffectKind::Kan));
    }

    #[test]
    fn lead_in_sums_only_events_that_have_effects() {
        let events = vec![
            Event::Riichi {
                seat: Seat::new(0),
                step: RiichiStep::Declare,
            },
            Event::Discard {
                seat: Seat::new(0),
                tile: parse_tile("1m").unwrap(),
                manner: DiscardManner::Tedashi,
            },
            Event::Riichi {
                seat: Seat::new(0),
                step: RiichiStep::Accepted,
            },
        ];
        assert_eq!(lead_in_ms(&events), 1_800 + 350);
    }

    #[test]
    fn deadline_adds_the_lead_in_so_effects_do_not_eat_think_time() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);

        let without_effects = action_deadline_ms(&rules, 20_000, 0);
        assert_eq!(without_effects, 5_000 + 20_000 + 500);

        // 直前にリーチ演出が入った分だけ締切が後ろへずれる。
        let after_riichi = action_deadline_ms(&rules, 20_000, 1_800);
        assert_eq!(after_riichi, without_effects + 1_800);
    }

    #[test]
    fn deadline_shrinks_as_the_bank_is_spent() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);
        assert_eq!(action_deadline_ms(&rules, 0, 0), 5_000 + 500);
    }

    /// 最低待機が打牌演出より短いと「次の行動が直前の演出完了より論理的に先行する」
    /// 状態が生まれる。両者の対応を検査で固定しておく。
    #[test]
    fn minimum_reaction_wait_covers_the_discard_effect() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);
        assert!(
            rules.min_reaction_window_ms >= effect_duration_ms(EffectKind::Discard),
            "最低待機{}ms が打牌演出{}ms より短い",
            rules.min_reaction_window_ms,
            effect_duration_ms(EffectKind::Discard)
        );
    }

    /// 進行を止めない種類のイベントに演出時間を割り当てない。
    /// ここが緩むと締切が際限なく延びる。
    #[test]
    fn bookkeeping_events_have_no_effect_time() {
        let events = [
            Event::RoundEnd {
                scores: [25_000; 4],
                next: crate::event::NextRound::MatchOver,
                reason: crate::event::ContinuationReason::DealerLoss,
            },
            Event::ActionPassed {
                seat: Seat::new(0),
                window_id: 1,
                declined: vec![],
            },
            Event::SeedReveal { seeds: vec![] },
        ];
        for event in &events {
            assert_eq!(effect_of(event), None, "{event:?} に演出時間が付いた");
        }
        assert_eq!(lead_in_ms(&events), 0);
    }
}
