//! サーバが持つ真実。クライアントへはそのまま出さず、必ず `project()` を通す。

use serde::{Deserialize, Serialize};

use crate::command::ActionOption;
use crate::meld::{Meld, MeldKind};
use crate::ruleset::Ruleset;
use crate::seat::{Round, Seat};
use crate::tile::Tile;
use crate::yaku::YakuId;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlayerId(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawSource {
    Wall,
    DeadWall,
}

/// 手出しかツモ切りか。演出と河の表示の双方が必要とするため、
/// 差分からの逆算ではなくイベント自身が持つ。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscardManner {
    Tedashi,
    Tsumogiri,
}

/// 宣言（牌を横に倒す）と成立（棒が出て1000点減る）を分ける。
/// 宣言牌そのものは直後の Discard イベントが運ぶ。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiichiStep {
    Declare,
    Accepted,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RyuukyokuKind {
    Exhaustive,
    NineTerminals,
    FourRiichi,
    FourWinds,
    FourKans,
    ThreeRons,
}

/// 責任払い（パオ）。大三元・大四喜の確定牌を鳴かせた者が負う。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Liability {
    pub seat: Seat,
    pub yaku: YakuId,
    pub mode: LiabilityMode,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiabilityMode {
    /// ツモ和了。責任者が全額を負担する。
    Full,
    /// ロン和了。責任者と放銃者で折半する。
    Split,
}

/// 点棒移動の内訳。最終差分だけでは、ダブロン時に供託を誰が取ったか、
/// 本場をどちらに付けたかを牌譜から復元できないため分けて持つ。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SettlementEntry {
    pub seat: Seat,
    /// 符と翻から決まる素点。
    pub base: i32,
    pub honba: i32,
    pub riichi_sticks: i32,
    /// 責任払いによる肩代わり分。
    pub liability: i32,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Settlement {
    /// 各席の最終増減。合計は常に0でなければならない。
    pub delta: [i32; 4],
    pub entries: Vec<SettlementEntry>,
}

impl Settlement {
    /// 点棒は卓の中で移動するだけであり、増減の合計は必ず0になる。
    pub fn is_balanced(&self) -> bool {
        self.delta.iter().sum::<i32>() == 0
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct AgariResult {
    pub seat: Seat,
    /// ロンなら放銃者、ツモなら None。
    pub from: Option<Seat>,
    pub hand: Vec<Tile>,
    pub melds: Vec<Meld>,
    pub win_tile: Tile,
    pub yaku: Vec<(YakuId, u8)>,
    pub fu: u8,
    pub han: u8,
    /// 供託と積み棒を含まない素点。
    pub score: i32,
    /// 責任払いが成立した場合のみ Some。
    pub liability: Option<Liability>,
    /// リーチ和了があった場合のみ Some。空配列との使い分けに頼らない。
    pub ura_indicators: Option<Vec<Tile>>,
}

/// 局と本場の進み方が決まった理由。エンジンの判断を牌譜から監査するために残す。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationReason {
    DealerWin,
    DealerTenpai,
    DealerLoss,
    AbortiveDraw,
    NagashiMangan,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NextRound {
    Next {
        round: Round,
        dealer: Seat,
        honba: u8,
        riichi_sticks: u8,
    },
    MatchOver,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    MatchStart {
        players: [PlayerId; 4],
        rules: Ruleset,
    },
    RoundStart {
        round: Round,
        dealer: Seat,
        honba: u8,
        riichi_sticks: u8,
        scores: [i32; 4],
        /// 山のシードのハッシュ（hex）。半荘終了後に SeedReveal で開示する。
        seed_commit: String,
    },
    Deal {
        hands: [Vec<Tile>; 4],
        dora_indicator: Tile,
    },
    Draw {
        seat: Seat,
        tile: Tile,
        source: DrawSource,
        wall_remaining: u8,
    },
    Discard {
        seat: Seat,
        tile: Tile,
        manner: DiscardManner,
    },
    Riichi {
        seat: Seat,
        step: RiichiStep,
    },
    Call {
        seat: Seat,
        from: Seat,
        kind: MeldKind,
        tiles: Vec<Tile>,
    },
    /// 加槓・暗槓の宣言。成立（Call）とは別イベントにすることで、
    /// この間に槍槓の反応ウィンドウを開ける。
    KanDeclared {
        seat: Seat,
        kind: MeldKind,
        tile: Tile,
    },
    DoraReveal {
        indicator: Tile,
    },
    /// 反応ウィンドウでの見逃し。同巡内フリテンとリーチ後の制約に必要なため、
    /// コマンドではなくサーバ側イベントとして牌譜に残す。
    ActionPassed {
        seat: Seat,
        window_id: u32,
        declined: Vec<ActionOption>,
    },
    Agari {
        results: Vec<AgariResult>,
        settlement: Settlement,
    },
    Ryuukyoku {
        kind: RyuukyokuKind,
        /// 九種九牌などの宣言者。荒牌平局では None。
        initiator: Option<Seat>,
        tenpai: [bool; 4],
        /// 公開資格のある席の手牌のみ。射影側でも再検査する。
        revealed_hands: Vec<(Seat, Vec<Tile>)>,
        nagashi_winners: Vec<Seat>,
        settlement: Settlement,
    },
    RoundEnd {
        scores: [i32; 4],
        next: NextRound,
        reason: ContinuationReason,
    },
    MatchEnd {
        final_scores: [i32; 4],
        placements: [u8; 4],
    },
    RequestAction {
        seat: Seat,
        /// どの打牌・宣言に対する要求か。遅延した応答や再送の取り違えを防ぐ。
        window_id: u32,
        options: Vec<ActionOption>,
        deadline_ms: u32,
    },
    /// 半荘終了後にまとめて開示する。局ごとに出すと、その局の他家手牌を
    /// 遡って復元できてしまい、同じ半荘の中で不公平が生じる。
    SeedReveal {
        seeds: Vec<String>,
    },
}

/// 牌譜と再接続のための連番付きイベント。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub seq: u32,
    pub event: Event,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation::{parse_hand, parse_tile};
    use crate::ruleset::{MatchLength, Ruleset};
    use crate::seat::Wind;

    #[test]
    fn envelope_round_trips_through_json() {
        let envelope = EventEnvelope {
            seq: 42,
            event: Event::Discard {
                seat: Seat::new(1),
                tile: parse_tile("3p").unwrap(),
                manner: DiscardManner::Tedashi,
            },
        };
        let json = serde_json::to_string(&envelope).unwrap();
        let back: EventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(envelope, back);
    }

    #[test]
    fn deal_carries_every_seat_hand() {
        let hands = [
            parse_hand("1112223334445m").unwrap(),
            parse_hand("1112223334445p").unwrap(),
            parse_hand("1112223334445s").unwrap(),
            parse_hand("1112223334445z").unwrap(),
        ];
        for hand in &hands {
            assert_eq!(hand.len(), 13);
        }
        let event = Event::Deal {
            hands: hands.clone(),
            dora_indicator: parse_tile("7z").unwrap(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn round_start_carries_a_hex_seed_commitment() {
        let event = Event::RoundStart {
            round: Round {
                wind: Wind::East,
                number: 1,
            },
            dealer: Seat::new(0),
            honba: 0,
            riichi_sticks: 0,
            scores: [25_000; 4],
            seed_commit: "00".repeat(32),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(&"00".repeat(32)), "json={json}");
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn match_start_carries_the_ruleset() {
        let event = Event::MatchStart {
            players: [PlayerId(1), PlayerId(2), PlayerId(3), PlayerId(4)],
            rules: Ruleset::kin_no_ma(MatchLength::Hanchan),
        };
        let back: Event = serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
        assert_eq!(event, back);
    }

    /// 点棒は卓の中を移動するだけなので、増減の合計は必ず0になる。
    #[test]
    fn settlement_must_balance() {
        let balanced = Settlement {
            delta: [8000, -8000, 0, 0],
            entries: vec![SettlementEntry {
                seat: Seat::new(0),
                base: 8000,
                honba: 0,
                riichi_sticks: 0,
                liability: 0,
            }],
        };
        assert!(balanced.is_balanced());

        let broken = Settlement {
            delta: [8000, -7000, 0, 0],
            entries: vec![],
        };
        assert!(!broken.is_balanced());
    }

    /// 加槓は宣言と成立が別イベントであり、その間に槍槓を受け付けられる。
    #[test]
    fn kan_declaration_is_separate_from_completion() {
        let declared = Event::KanDeclared {
            seat: Seat::new(2),
            kind: MeldKind::Kakan,
            tile: parse_tile("5s").unwrap(),
        };
        let completed = Event::Call {
            seat: Seat::new(2),
            from: Seat::new(2),
            kind: MeldKind::Kakan,
            tiles: parse_hand("5555s").unwrap(),
        };
        assert_ne!(declared, completed);

        for event in [declared, completed] {
            let back: Event =
                serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
            assert_eq!(event, back);
        }
    }

    #[test]
    fn agari_carries_liability_and_optional_ura() {
        let result = AgariResult {
            seat: Seat::new(0),
            from: Some(Seat::new(1)),
            hand: parse_hand("11122233344455z").unwrap(),
            melds: vec![],
            win_tile: parse_tile("5z").unwrap(),
            yaku: vec![(YakuId::Daisangen, 13)],
            fu: 0,
            han: 13,
            score: 32_000,
            liability: Some(Liability {
                seat: Seat::new(2),
                yaku: YakuId::Daisangen,
                mode: LiabilityMode::Split,
            }),
            ura_indicators: None,
        };
        let event = Event::Agari {
            results: vec![result],
            settlement: Settlement {
                delta: [32_000, -16_000, -16_000, 0],
                entries: vec![],
            },
        };
        let back: Event = serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
        assert_eq!(event, back);
    }

    /// 裏ドラは Option であり、リーチ和了がないときは None で表す。
    /// 空配列との使い分けに頼らない。
    #[test]
    fn ura_indicators_absence_is_distinct_from_empty() {
        let json_none = serde_json::to_string(&Option::<Vec<Tile>>::None).unwrap();
        let json_empty = serde_json::to_string(&Some(Vec::<Tile>::new())).unwrap();
        assert_ne!(json_none, json_empty);
        assert_eq!(json_none, "null");
        assert_eq!(json_empty, "[]");
    }
}
