use serde::{Deserialize, Serialize};

use crate::tile::{Tile, TileKind};

/// クライアントがサーバへ送れる操作の全て。これ以外は受け付けない。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Discard {
        tile: Tile,
        riichi: bool,
    },
    /// window_id は RequestAction が示したものをそのまま返す。
    /// これがないと、遅延した応答や再送を別のウィンドウへ適用してしまう。
    CallResponse {
        window_id: u32,
        response: CallResponse,
    },
    Ankan {
        kind: TileKind,
    },
    Kakan {
        tile: Tile,
    },
    Tsumo,
    Kyuushu,
}

/// 反応ウィンドウへの応答。使う手牌が曖昧になりうる鳴きは牌を明示する。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CallResponse {
    Pass,
    Chi { tiles: [Tile; 2] },
    Pon { tiles: [Tile; 2] },
    Kan,
    Ron,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KanCandidate {
    Ankan { kind: TileKind },
    Kakan { tile: Tile },
    Minkan,
}

/// サーバが提示する、その席がいま取れる操作。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionOption {
    Discard {
        allowed: Vec<Tile>,
        riichi_allowed: Vec<Tile>,
    },
    Chi {
        candidates: Vec<[Tile; 2]>,
    },
    Pon {
        candidates: Vec<[Tile; 2]>,
    },
    Kan {
        candidates: Vec<KanCandidate>,
    },
    Ron,
    Tsumo,
    Kyuushu,
    Pass,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation::{parse_hand, parse_tile};

    #[test]
    fn command_round_trips_through_json() {
        let command = Command::Discard {
            tile: parse_tile("0p").unwrap(),
            riichi: true,
        };
        let json = serde_json::to_string(&command).unwrap();
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(command, back);
    }

    #[test]
    fn chi_names_the_two_tiles_from_hand() {
        let tiles = parse_hand("34p").unwrap();
        let response = CallResponse::Chi {
            tiles: [tiles[0], tiles[1]],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("chi"), "json={json}");
        let back: CallResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, back);
    }

    #[test]
    fn pass_is_representable() {
        let json = serde_json::to_string(&CallResponse::Pass).unwrap();
        let back: CallResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, CallResponse::Pass);
    }

    /// 応答は必ずウィンドウを名指しする。遅延した応答の取り違えを防ぐ。
    #[test]
    fn call_response_carries_the_window_id() {
        let command = Command::CallResponse {
            window_id: 12,
            response: CallResponse::Ron,
        };
        let json = serde_json::to_string(&command).unwrap();
        assert!(json.contains("\"window_id\":12"), "json={json}");
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(command, back);
    }

    #[test]
    fn action_option_round_trips_through_json() {
        let option = ActionOption::Discard {
            allowed: parse_hand("123m").unwrap(),
            riichi_allowed: parse_hand("1m").unwrap(),
        };
        let back: ActionOption =
            serde_json::from_str(&serde_json::to_string(&option).unwrap()).unwrap();
        assert_eq!(option, back);
    }
}
