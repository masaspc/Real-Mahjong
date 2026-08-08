use serde::{Deserialize, Serialize};

/// 成立しうる役とドラの識別子。表示順・翻数は採点側が決める。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum YakuId {
    // 1翻
    MenzenTsumo,
    Riichi,
    Ippatsu,
    Chankan,
    RinshanKaihou,
    HaiteiRaoyue,
    HouteiRaoyui,
    Pinfu,
    Tanyao,
    Iipeiko,
    YakuhaiHaku,
    YakuhaiHatsu,
    YakuhaiChun,
    YakuhaiRoundWind,
    YakuhaiSeatWind,
    // 2翻
    DoubleRiichi,
    Chiitoitsu,
    Toitoi,
    Sanankou,
    SanshokuDoukou,
    Sankantsu,
    Shousangen,
    Honroutou,
    SanshokuDoujun,
    Ittsu,
    Chanta,
    // 3翻
    Ryanpeikou,
    Junchan,
    Honitsu,
    // 6翻
    Chinitsu,
    // 役満
    Tenhou,
    Chiihou,
    KokushiMusou,
    // 既定の変換では kokushi_musou13 になる。契約として読みやすい形を明示する。
    #[serde(rename = "kokushi_musou_13")]
    KokushiMusou13,
    Suuankou,
    SuuankouTanki,
    Daisangen,
    Shousuushii,
    Daisuushii,
    Tsuuiisou,
    Ryuuiisou,
    Chinroutou,
    ChuurenPoutou,
    #[serde(rename = "chuuren_poutou_9")]
    ChuurenPoutou9,
    Suukantsu,
    // ドラ（役ではないが同じ枠で表示される）
    Dora,
    AkaDora,
    UraDora,
}

impl YakuId {
    pub fn is_yakuman(self) -> bool {
        use YakuId::*;
        matches!(
            self,
            Tenhou
                | Chiihou
                | KokushiMusou
                | KokushiMusou13
                | Suuankou
                | SuuankouTanki
                | Daisangen
                | Shousuushii
                | Daisuushii
                | Tsuuiisou
                | Ryuuiisou
                | Chinroutou
                | ChuurenPoutou
                | ChuurenPoutou9
                | Suukantsu
        )
    }

    /// ドラ枠（役の有無判定には数えない）かどうか。
    pub fn is_dora(self) -> bool {
        matches!(self, YakuId::Dora | YakuId::AkaDora | YakuId::UraDora)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yakuman_classification_is_correct() {
        assert!(YakuId::KokushiMusou.is_yakuman());
        assert!(YakuId::Suuankou.is_yakuman());
        assert!(YakuId::ChuurenPoutou.is_yakuman());
        assert!(!YakuId::Riichi.is_yakuman());
        assert!(!YakuId::Chinitsu.is_yakuman());
        assert!(!YakuId::Dora.is_yakuman());
    }

    #[test]
    fn dora_kinds_are_not_true_yaku() {
        assert!(YakuId::Dora.is_dora());
        assert!(YakuId::AkaDora.is_dora());
        assert!(YakuId::UraDora.is_dora());
        assert!(!YakuId::Tanyao.is_dora());
    }

    #[test]
    fn serializes_as_a_stable_snake_case_string() {
        let json = serde_json::to_string(&YakuId::MenzenTsumo).unwrap();
        assert_eq!(json, "\"menzen_tsumo\"");
        let back: YakuId = serde_json::from_str("\"kokushi_musou_13\"").unwrap();
        assert_eq!(back, YakuId::KokushiMusou13);

        // 数字で終わる役は既定の変換だと区切りが消える。明示指定を検査で固定する。
        assert_eq!(
            serde_json::to_string(&YakuId::KokushiMusou13).unwrap(),
            "\"kokushi_musou_13\""
        );
        assert_eq!(
            serde_json::to_string(&YakuId::ChuurenPoutou9).unwrap(),
            "\"chuuren_poutou_9\""
        );
    }

    /// 全variantが往復すること。契約の取りこぼしを防ぐ。
    #[test]
    fn every_yaku_round_trips_through_json() {
        use YakuId::*;
        let all = [
            MenzenTsumo,
            Riichi,
            Ippatsu,
            Chankan,
            RinshanKaihou,
            HaiteiRaoyue,
            HouteiRaoyui,
            Pinfu,
            Tanyao,
            Iipeiko,
            YakuhaiHaku,
            YakuhaiHatsu,
            YakuhaiChun,
            YakuhaiRoundWind,
            YakuhaiSeatWind,
            DoubleRiichi,
            Chiitoitsu,
            Toitoi,
            Sanankou,
            SanshokuDoukou,
            Sankantsu,
            Shousangen,
            Honroutou,
            SanshokuDoujun,
            Ittsu,
            Chanta,
            Ryanpeikou,
            Junchan,
            Honitsu,
            Chinitsu,
            Tenhou,
            Chiihou,
            KokushiMusou,
            KokushiMusou13,
            Suuankou,
            SuuankouTanki,
            Daisangen,
            Shousuushii,
            Daisuushii,
            Tsuuiisou,
            Ryuuiisou,
            Chinroutou,
            ChuurenPoutou,
            ChuurenPoutou9,
            Suukantsu,
            Dora,
            AkaDora,
            UraDora,
        ];
        assert_eq!(all.len(), 48, "役の一覧に追加したらこの数も更新する");

        for yaku in all {
            let json = serde_json::to_string(&yaku).unwrap();
            let back: YakuId = serde_json::from_str(&json).unwrap();
            assert_eq!(back, yaku, "{json} が往復しない");
        }
    }
}
