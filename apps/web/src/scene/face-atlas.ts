import { tileBackSvg, tileFaceSvg } from "../ui/tile-face";
import { ATLAS, BACK_INDEX, BODY_INDEX } from "./atlas";

/**
 * アトラスの1セルを SVG のデータ URL にする。
 *
 * **牌の面は `ui/tile-face.ts` が唯一の定義である。**ここで描き直すと
 * 2D と 3D で見た目がずれる。
 */
export function faceDataUrl(cell: number): string {
  if (!Number.isInteger(cell) || cell < 0 || cell >= ATLAS.cells) {
    throw new Error(`アトラスのセルが範囲外: ${cell}`);
  }
  const svg =
    cell === BACK_INDEX
      ? tileBackSvg()
      : cell === BODY_INDEX
        ? bodySvg()
        : tileFaceSvg(cell);
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** 牌体（側面・底）の地の色。象牙色。試験で `bodySvg()` と同じ値であることを確かめる。 */
export const BODY_FILL_COLOR = "#f0e6c8";

/** 牌の側面と底。象牙色の無地。 */
function bodySvg(): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 54"><rect width="40" height="54" fill="${BODY_FILL_COLOR}"/></svg>`;
}

/** 画像を読む手立て。試験では差し替える。 */
export type LoadImage = (url: string) => Promise<CanvasImageSource>;

/** ブラウザで画像を読む。 */
export const loadViaImage: LoadImage = (url) =>
  new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("牌面を読めない"));
    image.src = url;
  });

/**
 * `paintAtlasOn` が絵を置く先の最小限の窓口。
 *
 * 実際に使うのは `CanvasRenderingContext2D` のうちこの3つだけである。
 * **試験では canvas の代わりに、呼び出しを記録するだけの偽物を渡す。**
 */
export type Surface = {
  fillStyle: string | CanvasGradient | CanvasPattern;
  fillRect(x: number, y: number, w: number, h: number): void;
  drawImage(
    image: CanvasImageSource,
    x: number,
    y: number,
    w: number,
    h: number,
  ): void;
};

/**
 * 窓口（`Surface`）へ、あらかじめ読み込み済みの画像を39セル分並べて描く。
 *
 * **セルごとに、まず牌体の地の色で塗りつぶしてから画像を重ねる。**取り込んだ
 * 牌図は背景が透明な SVG なので、先に塗らないと下地（`atlas.ts` の仮描画）の
 * 文字ラベルが透けて二重写しになる。マテリアルが透過を有効にしていないため、
 * 透明化（`clearRect`）ではなく色を塗る。
 */
export function paintAtlasOn(
  surface: Surface,
  cellW: number,
  cellH: number,
  images: readonly CanvasImageSource[],
): void {
  images.forEach((image, cell) => {
    const column = cell % ATLAS.columns;
    const row = Math.floor(cell / ATLAS.columns);
    const x = column * cellW;
    const y = row * cellH;
    surface.fillStyle = BODY_FILL_COLOR;
    surface.fillRect(x, y, cellW, cellH);
    surface.drawImage(image, x, y, cellW, cellH);
  });
}

/**
 * アトラスへ39セルを描く。
 *
 * **非同期である。**できるまでは `atlas.ts` の仮のもので描き、
 * できたらテクスチャの `needsUpdate` を立てて差し替える。
 */
export async function paintAtlas(
  canvas: HTMLCanvasElement,
  load: LoadImage = loadViaImage,
): Promise<void> {
  const context = canvas.getContext("2d");
  if (context === null) {
    throw new Error("2D コンテキストを取得できない");
  }
  const cellW = canvas.width / ATLAS.columns;
  const cellH = canvas.height / ATLAS.rows;

  const images = await Promise.all(
    Array.from({ length: ATLAS.cells }, (_, cell) => load(faceDataUrl(cell))),
  );
  paintAtlasOn(context, cellW, cellH, images);
}

