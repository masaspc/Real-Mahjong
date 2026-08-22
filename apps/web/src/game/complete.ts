import type { Tile } from "../protocol/Tile";
import { kindOf } from "./tiles";

/**
 * 手牌の形が和了形かどうか。**表示のためだけに使う。**
 *
 * **ここは判定の権威ではない。**和了できるかどうかを決めるのはサーバであり、
 * 画面はサーバが `request_action` に載せた `tsumo` の有無に従う。この関数は
 * 「形は揃っているのにツモが出ない」という状況を見つけて、**なぜ和了できない
 * のかを一言添えるため**だけにある。誤っても打牌や和了の可否は変わらない。
 *
 * なぜ要るか。役無しの和了は認められないので、四面子一雀頭が揃っていても
 * 役が1つも無ければ和了できない。鳴いた手でよく起きる。画面は何も言わない
 * ので、遊ぶ側には「揃っているのに上がれない」としか見えない。
 *
 * 役の判定はしない。**それは core の仕事であり、ここへ写すと二重定義になる。**
 * 形が揃っているかだけを見る。
 */

/** 34種の枚数。赤ドラは同じ種類として数える。 */
function countKinds(tiles: Tile[]): number[] {
  const counts = new Array<number>(34).fill(0);
  for (const tile of tiles) {
    const kind = kindOf(tile);
    counts[kind] = (counts[kind] ?? 0) + 1;
  }
  return counts;
}

/** 数牌の並び（0-8 萬 / 9-17 筒 / 18-26 索）で、順子が種類をまたがない境目。 */
function isSuit(kind: number): boolean {
  return kind < 27;
}

/** 残りが面子だけに分けきれるか。先頭から貪欲に、刻子か順子を外していく。 */
function melds(counts: number[], need: number): boolean {
  if (need === 0) {
    return counts.every((n) => n === 0);
  }
  const head = counts.findIndex((n) => n > 0);
  if (head < 0) {
    return false;
  }

  // 刻子として外す。
  if ((counts[head] ?? 0) >= 3) {
    counts[head] = (counts[head] ?? 0) - 3;
    const ok = melds(counts, need - 1);
    counts[head] = (counts[head] ?? 0) + 3;
    if (ok) {
      return true;
    }
  }

  // 順子として外す。**種類をまたがせない。**9萬・1筒・2筒 を順子にしない。
  const suit = Math.floor(head / 9);
  const withinSuit = isSuit(head) && head % 9 <= 6 && Math.floor((head + 2) / 9) === suit;
  if (withinSuit && (counts[head + 1] ?? 0) > 0 && (counts[head + 2] ?? 0) > 0) {
    counts[head] = (counts[head] ?? 0) - 1;
    counts[head + 1] = (counts[head + 1] ?? 0) - 1;
    counts[head + 2] = (counts[head + 2] ?? 0) - 1;
    const ok = melds(counts, need - 1);
    counts[head] = (counts[head] ?? 0) + 1;
    counts[head + 1] = (counts[head + 1] ?? 0) + 1;
    counts[head + 2] = (counts[head + 2] ?? 0) + 1;
    if (ok) {
      return true;
    }
  }

  return false;
}

/** 国士無双の対象。老頭牌と字牌の13種。 */
const ORPHANS = [0, 8, 9, 17, 18, 26, 27, 28, 29, 30, 31, 32, 33];

/**
 * 形が和了形か。
 *
 * @param tiles 手の中にある牌。**ツモ牌を含める。**副露の牌は含めない。
 * @param meldCount 副露の数。四面子のうち何組が既に卓上にあるか。
 */
export function isWinningShape(tiles: Tile[], meldCount: number): boolean {
  const need = 4 - meldCount;
  if (need < 0 || tiles.length !== need * 3 + 2) {
    return false;
  }
  const counts = countKinds(tiles);

  // 七対子と国士は門前でしか成立しない。
  if (meldCount === 0) {
    if (counts.filter((n) => n === 2).length === 7) {
      return true;
    }
    const orphansOk =
      ORPHANS.every((kind) => (counts[kind] ?? 0) >= 1) &&
      ORPHANS.reduce((sum, kind) => sum + (counts[kind] ?? 0), 0) === 14;
    if (orphansOk) {
      return true;
    }
  }

  // 標準形。雀頭の候補をすべて試す。**貪欲に決め打つと取りこぼす。**
  for (let kind = 0; kind < 34; kind += 1) {
    if ((counts[kind] ?? 0) < 2) {
      continue;
    }
    counts[kind] = (counts[kind] ?? 0) - 2;
    const ok = melds(counts, need);
    counts[kind] = (counts[kind] ?? 0) + 2;
    if (ok) {
      return true;
    }
  }
  return false;
}
