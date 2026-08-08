//! 期待値テーブルの読み込み。
//!
//! これは実装側ではなく**仕様側の資産**である。Wave 1 の各実装は、
//! ここに書かれた値を通すことが完了条件になる。
//! 期待値の変更にはコーディネータの承認を要する（`AGENTS.md` 参照）。

use std::path::PathBuf;

use protocol::yaku::YakuId;
use serde::Deserialize;

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WinType {
    Tsumo,
    Ron,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub struct MeldSpec {
    pub kind: String,
    pub tiles: String,
    pub from: u8,
    pub called_tile: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub struct Context {
    pub seat_wind: String,
    pub round_wind: String,
    pub riichi: bool,
    pub double_riichi: bool,
    pub ippatsu: bool,
    pub rinshan: bool,
    pub chankan: bool,
    pub haitei: bool,
    pub houtei: bool,
    pub dora_indicators: Vec<String>,
    pub ura_indicators: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub struct YakuExpect {
    pub id: YakuId,
    pub han: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub struct Expect {
    pub yaku: Vec<YakuExpect>,
    pub fu: u8,
    pub han: u8,
    pub payment: Payment,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub struct ScoringCase {
    pub id: String,
    pub note: String,
    /// 和了牌を含まない手牌。和了者は常に席0とする。
    pub concealed: String,
    pub melds: Vec<MeldSpec>,
    pub win_tile: String,
    pub win_type: WinType,
    pub context: Context,
    pub expect: Expect,
}

/// 向聴数の期待値。-1 が和了、0 がテンパイ。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub struct ShantenExpect {
    pub overall: i8,
    /// その形が論点になるケースでのみ指定する。
    pub chiitoitsu: Option<i8>,
    pub kokushi: Option<i8>,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub struct ShantenCase {
    pub id: String,
    pub note: String,
    pub concealed: String,
    /// 副露の数。手牌の枚数は 13 - melds*3（ツモ番なら +1）。
    pub melds: u8,
    pub expect: ShantenExpect,
}

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
}

fn load_dir<T: serde::de::DeserializeOwned>(subdir: &str) -> Vec<T> {
    let dir = fixtures_root().join(subdir);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} を読めない: {e}", dir.display()))
        .map(|entry| entry.expect("ディレクトリ項目").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();

    let mut out = Vec::new();
    for path in paths {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} を読めない: {e}", path.display()));
        let batch: Vec<T> = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} の解釈に失敗: {e}", path.display()));
        out.extend(batch);
    }
    out
}

pub fn load_scoring_cases() -> Vec<ScoringCase> {
    load_dir("scoring")
}

pub fn load_shanten_cases() -> Vec<ShantenCase> {
    load_dir("shanten")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashMap, HashSet};

    #[test]
    fn every_scoring_case_loads() {
        let cases = load_scoring_cases();
        assert!(cases.len() >= 10, "実際に読めたのは {} 件", cases.len());
    }

    #[test]
    fn scoring_case_ids_are_unique() {
        let cases = load_scoring_cases();
        let mut seen = HashSet::new();
        for case in &cases {
            assert!(
                seen.insert(case.id.clone()),
                "id が重複している: {}",
                case.id
            );
        }
    }

    #[test]
    fn declared_han_equals_the_sum_of_listed_yaku() {
        for case in load_scoring_cases() {
            let sum: u32 = case.expect.yaku.iter().map(|y| y.han as u32).sum();
            assert_eq!(
                sum, case.expect.han as u32,
                "{}: 役の翻の合計({sum})と han({}) が食い違う",
                case.id, case.expect.han
            );
        }
    }

    #[test]
    fn tsumo_cases_use_a_tsumo_payment() {
        for case in load_scoring_cases() {
            let matches = matches!(
                (&case.win_type, &case.expect.payment),
                (WinType::Ron, Payment::Ron { .. })
                    | (WinType::Tsumo, Payment::TsumoDealer { .. })
                    | (WinType::Tsumo, Payment::TsumoNonDealer { .. })
            );
            assert!(matches, "{}: 和了種別と支払い形式が食い違う", case.id);
        }
    }

    #[test]
    fn dealer_flag_agrees_with_the_payment_shape() {
        for case in load_scoring_cases() {
            let is_dealer = case.context.seat_wind == "east";
            if let Payment::TsumoDealer { .. } = case.expect.payment {
                assert!(is_dealer, "{}: 親ツモ支払いなのに自風が東でない", case.id);
            }
            if let Payment::TsumoNonDealer { .. } = case.expect.payment {
                assert!(!is_dealer, "{}: 子ツモ支払いなのに自風が東", case.id);
            }
        }
    }

    /// 手牌と和了牌が同じなら、手牌由来の役も一致していなければならない。
    /// 断幺九の取りこぼしはこの検査で機械的に見つかる。
    #[test]
    fn identical_hands_declare_identical_yaku_sets() {
        let cases = load_scoring_cases();
        let mut by_hand: HashMap<(String, String), Vec<&ScoringCase>> = HashMap::new();
        for case in &cases {
            if !case.melds.is_empty() {
                continue;
            }
            by_hand
                .entry((case.concealed.clone(), case.win_tile.clone()))
                .or_default()
                .push(case);
        }

        // 状況役（立直・ツモ・ドラ等）は文脈で変わるため、手牌のみで決まる役に絞る。
        let structural = |case: &ScoringCase| -> BTreeSet<YakuId> {
            case.expect
                .yaku
                .iter()
                .map(|y| y.id)
                .filter(|id| {
                    !id.is_dora()
                        && !matches!(
                            id,
                            YakuId::Riichi
                                | YakuId::DoubleRiichi
                                | YakuId::Ippatsu
                                | YakuId::MenzenTsumo
                                | YakuId::HaiteiRaoyue
                                | YakuId::HouteiRaoyui
                                | YakuId::RinshanKaihou
                                | YakuId::Chankan
                        )
                })
                .collect()
        };

        for ((hand, win), group) in by_hand {
            let Some(first) = group.first() else { continue };
            let expected = structural(first);
            for case in &group {
                assert_eq!(
                    structural(case),
                    expected,
                    "{hand}+{win}: {} と {} で手牌由来の役が食い違う",
                    first.id,
                    case.id
                );
            }
        }
    }

    #[test]
    fn every_shanten_case_loads() {
        let cases = load_shanten_cases();
        assert!(cases.len() >= 10, "実際に読めたのは {} 件", cases.len());
    }

    #[test]
    fn shanten_values_are_in_range() {
        for case in load_shanten_cases() {
            assert!(
                (-1..=8).contains(&case.expect.overall),
                "{}: overall が範囲外({})",
                case.id,
                case.expect.overall
            );
        }
    }

    #[test]
    fn overall_never_exceeds_a_declared_form() {
        for case in load_shanten_cases() {
            for form in [case.expect.chiitoitsu, case.expect.kokushi]
                .into_iter()
                .flatten()
            {
                assert!(
                    case.expect.overall <= form,
                    "{}: overall({}) が個別形({form}) を上回っている",
                    case.id,
                    case.expect.overall
                );
            }
        }
    }

    #[test]
    fn hand_size_matches_the_declared_meld_count() {
        for case in load_shanten_cases() {
            let tiles = protocol::notation::parse_hand(&case.concealed)
                .unwrap_or_else(|e| panic!("{}: 記法が不正 {e}", case.id));
            let expected = 13 - (case.melds as usize) * 3;
            assert!(
                tiles.len() == expected || tiles.len() == expected + 1,
                "{}: 手牌が{}枚。副露{}なら{}枚か{}枚のはず",
                case.id,
                tiles.len(),
                case.melds,
                expected,
                expected + 1
            );
        }
    }

    /// 採点ケースも副露込みで13枚になっていなければならない。
    #[test]
    fn scoring_hands_total_thirteen_tiles_before_the_winning_tile() {
        for case in load_scoring_cases() {
            let concealed = protocol::notation::parse_hand(&case.concealed)
                .unwrap_or_else(|e| panic!("{}: 手牌の記法が不正 {e}", case.id));
            let melded: usize = case
                .melds
                .iter()
                .map(|m| {
                    protocol::notation::parse_hand(&m.tiles)
                        .unwrap_or_else(|e| panic!("{}: 副露の記法が不正 {e}", case.id))
                        .len()
                })
                .sum();
            assert_eq!(
                concealed.len() + melded,
                13,
                "{}: 手牌{}枚＋副露{}枚",
                case.id,
                concealed.len(),
                melded
            );
            protocol::notation::parse_tile(&case.win_tile)
                .unwrap_or_else(|e| panic!("{}: 和了牌の記法が不正 {e}", case.id));
        }
    }
}
