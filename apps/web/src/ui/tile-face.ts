/// <reference types="vite/client" />
import type { Tile } from "../protocol/Tile";
import { isRed, kindOf } from "../game/tiles";

/**
 * 34種の牌図。`?raw` で文字列として読み込む。
 *
 * **`*?raw` の型は `vite/client` が既に宣言している。**`tsconfig.json` の
 * `types` を空にしているためグローバルには入らないが、上の参照でこの
 * ファイルだけに効かせられる。`vite-env.d.ts` に新しい宣言を足すと、この
 * タスクの `Files:`（`tile-face.ts` と試験のみ）を超えてしまう。
 */
import m1 from "../assets/tiles/m1.svg?raw";
import m2 from "../assets/tiles/m2.svg?raw";
import m3 from "../assets/tiles/m3.svg?raw";
import m4 from "../assets/tiles/m4.svg?raw";
import m5 from "../assets/tiles/m5.svg?raw";
import m6 from "../assets/tiles/m6.svg?raw";
import m7 from "../assets/tiles/m7.svg?raw";
import m8 from "../assets/tiles/m8.svg?raw";
import m9 from "../assets/tiles/m9.svg?raw";
import p1 from "../assets/tiles/p1.svg?raw";
import p2 from "../assets/tiles/p2.svg?raw";
import p3 from "../assets/tiles/p3.svg?raw";
import p4 from "../assets/tiles/p4.svg?raw";
import p5 from "../assets/tiles/p5.svg?raw";
import p6 from "../assets/tiles/p6.svg?raw";
import p7 from "../assets/tiles/p7.svg?raw";
import p8 from "../assets/tiles/p8.svg?raw";
import p9 from "../assets/tiles/p9.svg?raw";
import s1 from "../assets/tiles/s1.svg?raw";
import s2 from "../assets/tiles/s2.svg?raw";
import s3 from "../assets/tiles/s3.svg?raw";
import s4 from "../assets/tiles/s4.svg?raw";
import s5 from "../assets/tiles/s5.svg?raw";
import s6 from "../assets/tiles/s6.svg?raw";
import s7 from "../assets/tiles/s7.svg?raw";
import s8 from "../assets/tiles/s8.svg?raw";
import s9 from "../assets/tiles/s9.svg?raw";
import east from "../assets/tiles/east.svg?raw";
import south from "../assets/tiles/south.svg?raw";
import west from "../assets/tiles/west.svg?raw";
import north from "../assets/tiles/north.svg?raw";
import white from "../assets/tiles/white.svg?raw";
import green from "../assets/tiles/green.svg?raw";
import red from "../assets/tiles/red.svg?raw";

/**
 * 牌の面。取り込んだ牌図をそのまま返す。
 *
 * **牌の面は唯一この定義から取る。**2D の盤面（`ui/board.ts`）も 3D の
 * 牌面アトラス（`scene/face-atlas.ts`）もここを通す。片方だけ差し替えると
 * 鳴きのボタンと卓の牌で見た目が食い違う。
 */

/** 符号 0..=33 の順に並べる。0-8=萬 9-17=筒 18-26=索 27-33=字（東南西北白發中）。 */
const SOURCES: readonly string[] = [
  m1, m2, m3, m4, m5, m6, m7, m8, m9,
  p1, p2, p3, p4, p5, p6, p7, p8, p9,
  s1, s2, s3, s4, s5, s6, s7, s8, s9,
  east, south, west, north, white, green, red,
];

const SVG_NS = "http://www.w3.org/2000/svg";

/**
 * 1枚ぶんの SVG。
 *
 * **入力は 0..=36 の数値だけ。**利用者の文字列は一切混ざらないので、
 * 取り込んだ SVG をそのまま `innerHTML` に入れてよい。
 */
export function tileFaceSvg(tile: Tile): string {
  const kind = kindOf(tile);
  const source = SOURCES[kind];
  if (source === undefined) {
    throw new Error(`牌の符号が範囲外: ${tile}`);
  }
  return isRed(tile) ? reddened(source) : source;
}

/** 赤ドラの絵。**Task 7 で素材の中身を読んでから実装する。** */
function reddened(source: string): string {
  return source;
}

/** 裏向きの牌。他家の手牌に使う。 */
export function tileBackSvg(): string {
  return (
    `<svg xmlns="${SVG_NS}" viewBox="0 0 40 54" class="tile-face" aria-hidden="true">` +
    `<rect x="0.7" y="0.7" width="38.6" height="52.6" rx="4.5" ` +
    `fill="#2f7a52" stroke="#1c4b33" stroke-width="1.2"/>` +
    `<rect x="6" y="7" width="28" height="40" rx="3" fill="none" ` +
    `stroke="#7fc39b" stroke-width="1.4"/>` +
    `</svg>`
  );
}
