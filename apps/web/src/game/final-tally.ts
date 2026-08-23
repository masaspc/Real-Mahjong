import type { Ruleset } from "../protocol/Ruleset";

/**
 * 終局の順位点。
 *
 * **表示のための計算である。**素点も順位もサーバが決めたものをそのまま使い、
 * ここでは並べ替えも同点の裁定もしない。やるのは「素点を返し点との差に直し、
 * ウマとオカを足す」だけである。
 *
 * **返し点もウマも卓のルールから取る。**画面が 30,000 や 15 を定数で持つと、
 * 部屋の設定を変えた瞬間に嘘の数字を出す。
 *
 * 端数は丸めない。**丸め方は流儀が分かれる**（五捨六入・四捨五入・切り捨て）
 * ので、決めずに 0.1 単位のまま出す。素点 32,300 なら +2.3 と書く。
 */

export type Tally = {
  seat: number;
  /** 1 が1位。サーバの決めた順位。 */
  rank: number;
  /** 素点。 */
  raw: number;
  /** 返し点との差を 1,000 点単位にしたもの。 */
  base: number;
  /** 順位点。 */
  uma: number;
  /** オカ。1位だけが受け取る。 */
  oka: number;
  /** 合計。4人ぶんの和は必ず 0 になる。 */
  total: number;
};

export function finalTally(
  rules: Ruleset,
  finalScores: number[],
  placements: number[],
): Tally[] {
  // オカは「配給原点と返し点の差」を4人ぶん集めたもの。1位が総取りする。
  const oka = ((rules.return_score - rules.start_score) * 4) / 1_000;

  const rows = finalScores.map((raw, seat) => {
    const rank = placements[seat] ?? seat + 1;
    const base = (raw - rules.return_score) / 1_000;
    const uma = rules.uma[rank - 1] ?? 0;
    const share = rank === 1 ? oka : 0;
    return {
      seat,
      rank,
      raw,
      base,
      uma,
      oka: share,
      total: base + uma + share,
    };
  });

  return rows.sort((a, b) => a.rank - b.rank);
}
