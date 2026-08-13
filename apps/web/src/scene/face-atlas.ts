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

/** 牌の側面と底。象牙色の無地。 */
function bodySvg(): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 54"><rect width="40" height="54" fill="#f0e6c8"/></svg>`;
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
  images.forEach((image, cell) => {
    const column = cell % ATLAS.columns;
    const row = Math.floor(cell / ATLAS.columns);
    context.drawImage(image, column * cellW, row * cellH, cellW, cellH);
  });
}

