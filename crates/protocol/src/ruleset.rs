use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/")]
pub enum MatchLength {
    Tonpuu,
    Hanchan,
}

/// 対局のルール設定。値の変更がコード変更にならないよう、すべてデータとして持つ。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/")]
pub struct Ruleset {
    pub length: MatchLength,
    pub start_score: i32,
    pub return_score: i32,
    pub uma: [i32; 4],
    pub red_dora_count: u8,
    pub kuitan: bool,
    pub double_ron: bool,
    pub formal_tenpai: bool,
    pub noten_penalty: i32,
    pub nagashi_mangan: bool,
    pub liability: bool,
    pub round_up_mangan: bool,
    pub busted_ends_match: bool,
    pub base_think_ms: u32,
    pub think_bank_ms: u32,
    pub network_grace_ms: u32,
    /// 打牌から次のツモまでの最短時間。鳴ける者の有無で間の長さが変わると
    /// それ自体が情報になるため、常にこの時間だけ待つ。打牌演出と同じ値に揃える。
    pub min_reaction_window_ms: u32,
}

impl Ruleset {
    /// 雀魂「金の間」準拠の既定値。
    pub fn kin_no_ma(length: MatchLength) -> Self {
        Ruleset {
            length,
            start_score: 25_000,
            return_score: 30_000,
            uma: [15, 5, -5, -15],
            red_dora_count: 3,
            kuitan: true,
            double_ron: true,
            formal_tenpai: true,
            noten_penalty: 3_000,
            nagashi_mangan: true,
            liability: true,
            round_up_mangan: false,
            busted_ends_match: true,
            base_think_ms: 5_000,
            think_bank_ms: 20_000,
            network_grace_ms: 500,
            min_reaction_window_ms: 350,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kin_no_ma_matches_the_spec_defaults() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);
        assert_eq!(rules.length, MatchLength::Hanchan);
        assert_eq!(rules.start_score, 25_000);
        assert_eq!(rules.return_score, 30_000);
        assert_eq!(rules.uma, [15, 5, -5, -15]);
        assert_eq!(rules.red_dora_count, 3);
        assert!(rules.kuitan);
        assert!(rules.double_ron);
        assert!(rules.formal_tenpai);
        assert_eq!(rules.noten_penalty, 3_000);
        assert!(rules.nagashi_mangan);
        assert!(rules.liability);
        assert!(!rules.round_up_mangan);
        assert!(rules.busted_ends_match);
    }

    #[test]
    fn timing_constants_match_the_spec() {
        let rules = Ruleset::kin_no_ma(MatchLength::Tonpuu);
        assert_eq!(rules.base_think_ms, 5_000);
        assert_eq!(rules.think_bank_ms, 20_000);
        assert_eq!(rules.network_grace_ms, 500);
        assert_eq!(rules.min_reaction_window_ms, 350);
    }

    #[test]
    fn uma_sums_to_zero() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);
        assert_eq!(rules.uma.iter().sum::<i32>(), 0);
    }
}
