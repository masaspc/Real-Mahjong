//! 視界フィルタ。サーバの真実を、その席が見てよい形へ落とす。
//!
//! クライアントへ出るバイト列は必ずこの関数を通す。
//! 不正対策の全体重がここにかかるため、負例テストもここへ集中させる。

use crate::client_event::{ClientEvent, ClientEventEnvelope};
use crate::event::{Event, EventEnvelope, RyuukyokuKind};
use crate::seat::Seat;

pub fn project(event: &Event, viewer: Seat) -> Option<ClientEvent> {
    let projected = match event {
        Event::MatchStart { players, rules } => ClientEvent::MatchStart {
            players: players.clone(),
            rules: *rules,
            you: viewer,
        },
        Event::RoundStart {
            round,
            dealer,
            honba,
            riichi_sticks,
            scores,
            seed_commit,
        } => ClientEvent::RoundStart {
            round: *round,
            dealer: *dealer,
            honba: *honba,
            riichi_sticks: *riichi_sticks,
            scores: *scores,
            seed_commit: seed_commit.clone(),
        },
        Event::Deal {
            hands,
            dora_indicator,
        } => {
            let mut hand_sizes = [0u8; 4];
            for (index, hand) in hands.iter().enumerate() {
                hand_sizes[index] = hand.len() as u8;
            }
            ClientEvent::Deal {
                your_hand: hands[viewer.index()].clone(),
                hand_sizes,
                dora_indicator: *dora_indicator,
            }
        }
        Event::Draw {
            seat,
            tile,
            source,
            wall_remaining,
        } => ClientEvent::Draw {
            seat: *seat,
            tile: (*seat == viewer).then_some(*tile),
            source: *source,
            wall_remaining: *wall_remaining,
        },
        Event::Discard { seat, tile, manner } => ClientEvent::Discard {
            seat: *seat,
            tile: *tile,
            manner: *manner,
        },
        Event::Riichi { seat, step } => ClientEvent::Riichi {
            seat: *seat,
            step: *step,
        },
        Event::Call {
            seat,
            from,
            kind,
            tiles,
        } => ClientEvent::Call {
            seat: *seat,
            from: *from,
            kind: *kind,
            tiles: tiles.clone(),
        },
        Event::KanDeclared { seat, kind, tile } => ClientEvent::KanDeclared {
            seat: *seat,
            kind: *kind,
            tile: *tile,
        },
        Event::DoraReveal { indicator } => ClientEvent::DoraReveal {
            indicator: *indicator,
        },
        Event::ActionPassed {
            seat, window_id, ..
        } => ClientEvent::ActionPassed {
            seat: *seat,
            window_id: *window_id,
        },
        Event::Agari {
            results,
            settlement,
        } => ClientEvent::Agari {
            results: results.clone(),
            settlement: settlement.clone(),
        },
        Event::Ryuukyoku {
            kind,
            initiator,
            tenpai,
            revealed_hands,
            nagashi_winners,
            settlement,
        } => ClientEvent::Ryuukyoku {
            kind: *kind,
            initiator: *initiator,
            tenpai: *tenpai,
            // 生成側を信用せず、公開資格をここで再判定する。
            revealed_hands: revealed_hands
                .iter()
                .filter(|(seat, _)| {
                    may_reveal_hand(*kind, *initiator, tenpai, nagashi_winners, *seat)
                })
                .cloned()
                .collect(),
            nagashi_winners: nagashi_winners.clone(),
            settlement: settlement.clone(),
        },
        Event::RoundEnd {
            scores,
            next,
            reason,
        } => ClientEvent::RoundEnd {
            scores: *scores,
            next: *next,
            reason: *reason,
        },
        Event::MatchEnd {
            final_scores,
            placements,
        } => ClientEvent::MatchEnd {
            final_scores: *final_scores,
            placements: *placements,
        },
        Event::RequestAction {
            seat,
            window_id,
            options,
            deadline_ms,
        } => {
            if *seat != viewer {
                return None;
            }
            ClientEvent::RequestAction {
                window_id: *window_id,
                options: options.clone(),
                deadline_ms: *deadline_ms,
            }
        }
        Event::SeedReveal { seeds } => ClientEvent::SeedReveal {
            seeds: seeds.clone(),
        },
    };

    Some(projected)
}

pub fn project_envelope(envelope: &EventEnvelope, viewer: Seat) -> Option<ClientEventEnvelope> {
    project(&envelope.event, viewer).map(|event| ClientEventEnvelope {
        seq: envelope.seq,
        event,
    })
}

/// 流局時にその席の手牌を公開してよいか。
///
/// 荒牌平局ではテンパイ者と流し満貫成立者のみ。九種九牌は宣言者のみ。
/// それ以外の途中流局では誰の手牌も公開しない。
fn may_reveal_hand(
    kind: RyuukyokuKind,
    initiator: Option<Seat>,
    tenpai: &[bool; 4],
    nagashi_winners: &[Seat],
    seat: Seat,
) -> bool {
    match kind {
        RyuukyokuKind::Exhaustive => tenpai[seat.index()] || nagashi_winners.contains(&seat),
        RyuukyokuKind::NineTerminals => initiator == Some(seat),
        RyuukyokuKind::FourRiichi
        | RyuukyokuKind::FourWinds
        | RyuukyokuKind::FourKans
        | RyuukyokuKind::ThreeRons => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_event::ClientEvent;
    use crate::event::{DiscardManner, DrawSource, Event, PlayerId, RyuukyokuKind, Settlement};
    use crate::notation::{parse_hand, parse_tile};
    use crate::ruleset::{MatchLength, Ruleset};
    use crate::seat::Seat;
    use crate::tile::Tile;

    /// 各席の手牌を色で完全に分離しておく。漏れた場合にどの席から漏れたか判る。
    fn disjoint_hands() -> [Vec<Tile>; 4] {
        [
            parse_hand("1112223334445m").unwrap(),
            parse_hand("1112223334445p").unwrap(),
            parse_hand("1112223334445s").unwrap(),
            parse_hand("1112223334445z").unwrap(),
        ]
    }

    fn empty_settlement() -> Settlement {
        Settlement {
            delta: [0; 4],
            entries: vec![],
        }
    }

    #[test]
    fn deal_reveals_only_the_viewers_hand() {
        let hands = disjoint_hands();
        let event = Event::Deal {
            hands: hands.clone(),
            dora_indicator: parse_tile("7z").unwrap(),
        };

        let projected = project(&event, Seat::new(0)).expect("配牌は全席に配信される");
        let ClientEvent::Deal {
            your_hand,
            hand_sizes,
            dora_indicator,
        } = projected
        else {
            panic!("Deal が別の variant に射影された");
        };

        assert_eq!(your_hand, hands[0]);
        assert_eq!(hand_sizes, [13, 13, 13, 13]);
        assert_eq!(dora_indicator, parse_tile("7z").unwrap());
    }

    #[test]
    fn deal_json_contains_no_other_seat_tiles() {
        let hands = disjoint_hands();
        let event = Event::Deal {
            hands,
            dora_indicator: parse_tile("7z").unwrap(),
        };

        let projected = project(&event, Seat::new(0)).unwrap();
        let json = serde_json::to_string(&projected).unwrap();
        let back: ClientEvent = serde_json::from_str(&json).unwrap();

        let ClientEvent::Deal { your_hand, .. } = back else {
            panic!("Deal ではない");
        };
        // 自席は萬子のみ。他席の牌が混ざっていれば必ずここで落ちる。
        assert!(
            your_hand
                .iter()
                .all(|t| t.kind().suit() == crate::tile::Suit::Man),
            "自席以外の牌が混入した: {}",
            crate::notation::to_notation(&your_hand)
        );
    }

    #[test]
    fn draw_hides_the_tile_from_other_seats() {
        let event = Event::Draw {
            seat: Seat::new(1),
            tile: parse_tile("5z").unwrap(),
            source: DrawSource::Wall,
            wall_remaining: 69,
        };

        let own = project(&event, Seat::new(1)).unwrap();
        assert!(
            matches!(own, ClientEvent::Draw { tile: Some(_), .. }),
            "自分のツモ牌は見える"
        );

        for viewer in [0u8, 2, 3] {
            let other = project(&event, Seat::new(viewer)).unwrap();
            let ClientEvent::Draw {
                tile,
                wall_remaining,
                ..
            } = other
            else {
                panic!("Draw ではない");
            };
            assert_eq!(tile, None, "席{viewer} にツモ牌が漏れた");
            assert_eq!(wall_remaining, 69, "山の残り枚数は公開情報");
        }
    }

    #[test]
    fn discard_is_public_to_everyone() {
        let event = Event::Discard {
            seat: Seat::new(2),
            tile: parse_tile("3p").unwrap(),
            manner: DiscardManner::Tedashi,
        };

        for viewer in 0u8..4 {
            let projected = project(&event, Seat::new(viewer)).unwrap();
            let ClientEvent::Discard { tile, manner, .. } = projected else {
                panic!("Discard ではない");
            };
            assert_eq!(tile, parse_tile("3p").unwrap());
            assert_eq!(manner, DiscardManner::Tedashi);
        }
    }

    #[test]
    fn request_action_reaches_only_its_own_seat() {
        let event = Event::RequestAction {
            seat: Seat::new(3),
            window_id: 1,
            options: vec![],
            deadline_ms: 5_500,
        };

        assert!(project(&event, Seat::new(3)).is_some());
        for viewer in [0u8, 1, 2] {
            assert!(
                project(&event, Seat::new(viewer)).is_none(),
                "席{viewer} に他家への行動要求が漏れた"
            );
        }
    }

    #[test]
    fn match_start_tells_each_viewer_their_own_seat() {
        let event = Event::MatchStart {
            players: [
                PlayerId("p10".to_owned()),
                PlayerId("p11".to_owned()),
                PlayerId("p12".to_owned()),
                PlayerId("p13".to_owned()),
            ],
            rules: Ruleset::kin_no_ma(MatchLength::Hanchan),
        };

        for viewer in 0u8..4 {
            let projected = project(&event, Seat::new(viewer)).unwrap();
            let ClientEvent::MatchStart { you, .. } = projected else {
                panic!("MatchStart ではない");
            };
            assert_eq!(you, Seat::new(viewer));
        }
    }

    /// 生成側が誤ってノーテン者の手牌を入れても、射影で落ちなければならない。
    /// ここは型では守れないため、負例で守る。
    #[test]
    fn exhaustive_draw_reveals_only_tenpai_hands() {
        let hands = disjoint_hands();
        let event = Event::Ryuukyoku {
            kind: RyuukyokuKind::Exhaustive,
            initiator: None,
            tenpai: [true, false, false, true],
            // 生成側のバグを模して全席分を入れる。
            revealed_hands: (0u8..4)
                .map(|i| (Seat::new(i), hands[i as usize].clone()))
                .collect(),
            nagashi_winners: vec![],
            settlement: empty_settlement(),
        };

        let projected = project(&event, Seat::new(0)).unwrap();
        let ClientEvent::Ryuukyoku { revealed_hands, .. } = projected else {
            panic!("Ryuukyoku ではない");
        };

        let revealed: Vec<usize> = revealed_hands.iter().map(|(s, _)| s.index()).collect();
        assert_eq!(revealed, vec![0, 3], "ノーテン者の手牌が公開された");
    }

    #[test]
    fn nagashi_winner_reveals_even_when_noten() {
        let hands = disjoint_hands();
        let event = Event::Ryuukyoku {
            kind: RyuukyokuKind::Exhaustive,
            initiator: None,
            tenpai: [false, false, false, false],
            revealed_hands: vec![(Seat::new(2), hands[2].clone())],
            nagashi_winners: vec![Seat::new(2)],
            settlement: empty_settlement(),
        };

        let projected = project(&event, Seat::new(0)).unwrap();
        let ClientEvent::Ryuukyoku { revealed_hands, .. } = projected else {
            panic!("Ryuukyoku ではない");
        };
        assert_eq!(revealed_hands.len(), 1);
        assert_eq!(revealed_hands[0].0, Seat::new(2));
    }

    #[test]
    fn abortive_draws_reveal_nothing_but_nine_terminals_shows_the_declarer() {
        let hands = disjoint_hands();
        let all: Vec<(Seat, Vec<Tile>)> = (0u8..4)
            .map(|i| (Seat::new(i), hands[i as usize].clone()))
            .collect();

        for kind in [
            RyuukyokuKind::FourRiichi,
            RyuukyokuKind::FourWinds,
            RyuukyokuKind::FourKans,
            RyuukyokuKind::ThreeRons,
        ] {
            let event = Event::Ryuukyoku {
                kind,
                initiator: Some(Seat::new(1)),
                tenpai: [true; 4],
                revealed_hands: all.clone(),
                nagashi_winners: vec![],
                settlement: empty_settlement(),
            };
            let projected = project(&event, Seat::new(0)).unwrap();
            let ClientEvent::Ryuukyoku { revealed_hands, .. } = projected else {
                panic!("Ryuukyoku ではない");
            };
            assert!(revealed_hands.is_empty(), "{kind:?} で手牌が公開された");
        }

        let event = Event::Ryuukyoku {
            kind: RyuukyokuKind::NineTerminals,
            initiator: Some(Seat::new(1)),
            tenpai: [true; 4],
            revealed_hands: all,
            nagashi_winners: vec![],
            settlement: empty_settlement(),
        };
        let projected = project(&event, Seat::new(0)).unwrap();
        let ClientEvent::Ryuukyoku { revealed_hands, .. } = projected else {
            panic!("Ryuukyoku ではない");
        };
        assert_eq!(revealed_hands.len(), 1, "九種九牌は宣言者のみ公開する");
        assert_eq!(revealed_hands[0].0, Seat::new(1));
    }

    #[test]
    fn request_action_carries_the_window_id() {
        let event = Event::RequestAction {
            seat: Seat::new(2),
            window_id: 77,
            options: vec![],
            deadline_ms: 5_850,
        };
        let projected = project(&event, Seat::new(2)).unwrap();
        let ClientEvent::RequestAction { window_id, .. } = projected else {
            panic!("RequestAction ではない");
        };
        assert_eq!(window_id, 77);
    }

    /// 見逃した具体的な選択肢は、他家に配ると待ちの手掛かりになる。
    #[test]
    fn action_passed_does_not_carry_the_declined_options() {
        let event = Event::ActionPassed {
            seat: Seat::new(1),
            window_id: 3,
            declined: vec![crate::command::ActionOption::Ron],
        };
        let projected = project(&event, Seat::new(0)).unwrap();
        assert!(matches!(
            projected,
            ClientEvent::ActionPassed { window_id: 3, .. }
        ));

        // 配信形には declined を運ぶ場所そのものが無い。
        let json = serde_json::to_string(&projected).unwrap();
        assert!(!json.contains("ron"), "見逃した選択肢が漏れた: {json}");
    }

    /// 半荘終了までシードを出さない。局ごとに出すとその局の他家手牌を
    /// 遡って復元できてしまう。
    #[test]
    fn seed_reveal_carries_every_round_at_once() {
        let event = Event::SeedReveal {
            seeds: vec!["aa".repeat(32), "bb".repeat(32)],
        };
        let projected = project(&event, Seat::new(0)).unwrap();
        let ClientEvent::SeedReveal { seeds } = projected else {
            panic!("SeedReveal ではない");
        };
        assert_eq!(seeds.len(), 2);
    }

    #[test]
    fn envelope_projection_keeps_the_sequence_number() {
        let envelope = EventEnvelope {
            seq: 99,
            event: Event::DoraReveal {
                indicator: parse_tile("1z").unwrap(),
            },
        };
        let projected = project_envelope(&envelope, Seat::new(0)).unwrap();
        assert_eq!(projected.seq, 99);
    }

    /// 他席宛の行動要求は封筒ごと落ちる。
    #[test]
    fn envelope_projection_drops_events_not_meant_for_the_viewer() {
        let envelope = EventEnvelope {
            seq: 100,
            event: Event::RequestAction {
                seat: Seat::new(1),
                window_id: 5,
                options: vec![],
                deadline_ms: 5_500,
            },
        };
        assert!(project_envelope(&envelope, Seat::new(0)).is_none());
    }
}
