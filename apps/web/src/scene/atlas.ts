/** 34種＋赤ドラ3種＋裏面を収める牌面アトラス。 */
export const ATLAS = {
  columns: 8,
  rows: 5,
  cells: 38,
} as const;

/** 裏面のセル。牌のエンコード（0..=36）とは重ならない。 */
export const BACK_INDEX = 37;

/** 牌のエンコード（0..=36）をアトラスのセル番号へ変換する。 */
export function atlasIndexOf(encoded: number): number {
  if (!Number.isInteger(encoded) || encoded < 0 || encoded > 36) {
    throw new Error(`牌のエンコードが範囲外: ${encoded}`);
  }
  return encoded;
}

/** アトラス内のセルに対応する UV 範囲を返す。 */
export function uvOffsetOf(cellIndex: number): {
  u: number;
  v: number;
  du: number;
  dv: number;
} {
  if (!Number.isInteger(cellIndex) || cellIndex < 0 || cellIndex >= ATLAS.cells) {
    throw new Error(`アトラスのセル番号が範囲外: ${cellIndex}`);
  }
  const du = 1 / ATLAS.columns;
  const dv = 1 / ATLAS.rows;
  const column = cellIndex % ATLAS.columns;
  const row = Math.floor(cellIndex / ATLAS.columns);
  return { u: column * du, v: row * dv, du, dv };
}

const SUIT_MARK = ["m", "p", "s"] as const;
const HONOR_MARK = ["東", "南", "西", "北", "白", "發", "中"] as const;

function labelOf(cellIndex: number): { text: string; red: boolean } {
  if (cellIndex === BACK_INDEX) {
    return { text: "", red: false };
  }
  if (cellIndex >= 34) {
    const suit = SUIT_MARK[cellIndex - 34] ?? "?";
    return { text: `5${suit}`, red: true };
  }
  if (cellIndex >= 27) {
    return { text: HONOR_MARK[cellIndex - 27] ?? "?", red: false };
  }
  const suit = SUIT_MARK[Math.floor(cellIndex / 9)] ?? "?";
  return { text: `${(cellIndex % 9) + 1}${suit}`, red: false };
}

/** 素材が無い段階で使う、牌種を判別可能なアトラスを生成する。 */
export function drawPlaceholderAtlas(size = 1024): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (ctx === null) {
    throw new Error("2D コンテキストを取得できない");
  }

  const cellW = size / ATLAS.columns;
  const cellH = size / ATLAS.rows;
  ctx.fillStyle = "#1b1b1b";
  ctx.fillRect(0, 0, size, size);

  for (let cell = 0; cell < ATLAS.cells; cell += 1) {
    const column = cell % ATLAS.columns;
    const row = Math.floor(cell / ATLAS.columns);
    const x = column * cellW;
    const y = row * cellH;
    const label = labelOf(cell);

    ctx.fillStyle = cell === BACK_INDEX ? "#c9a227" : "#f6f1e3";
    ctx.fillRect(x + 2, y + 2, cellW - 4, cellH - 4);

    if (label.text !== "") {
      ctx.fillStyle = label.red ? "#c0392b" : "#222222";
      ctx.font = `${Math.floor(cellH * 0.5)}px sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(label.text, x + cellW / 2, y + cellH / 2);
    }
  }

  return canvas;
}
