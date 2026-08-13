import type { Tile } from "../protocol/Tile";

/** 赤ドラのエンコード。34=赤5m, 35=赤5p, 36=赤5s。 */
const RED = { 34: 4, 35: 13, 36: 22 } as const;

const HONORS = ["東", "南", "西", "北", "白", "發", "中"] as const;

/** 赤ドラかどうか。 */
export function isRed(tile: Tile): boolean {
  return tile === 34 || tile === 35 || tile === 36;
}

/**
 * 赤ドラを同じ種類の5へ均した 0..=33 の値。
 * **並べ替えと「同じ牌か」の判定はこちらで行う。**
 */
export function kindOf(tile: Tile): number {
  if (!Number.isInteger(tile) || tile < 0 || tile > 36) {
    throw new Error(`牌のエンコードが範囲外: ${tile}`);
  }
  return isRed(tile) ? RED[tile as 34 | 35 | 36] : tile;
}

/** 人が読む表記。赤ドラは 0m / 0p / 0s。 */
export function tileLabel(tile: Tile): string {
  const kind = kindOf(tile);
  // **`noUncheckedIndexedAccess` が効いているので、添字は undefined を含む。**
  // `kindOf` が範囲を検めているため実際には外れないが、型は素直に扱う。
  if (kind >= 27) {
    return HONORS[kind - 27] ?? "?";
  }
  const suit = "mps"[Math.floor(kind / 9)] ?? "?";
  const digit = isRed(tile) ? 0 : (kind % 9) + 1;
  return `${digit}${suit}`;
}

/**
 * 萬子・筒子・索子・字牌の順に並べる。
 * **赤ドラは同じ5の位置に来る。**元の配列は壊さない。
 */
export function sortTiles(tiles: Tile[]): Tile[] {
  return [...tiles].sort((a, b) => kindOf(a) - kindOf(b) || Number(isRed(b)) - Number(isRed(a)));
}
