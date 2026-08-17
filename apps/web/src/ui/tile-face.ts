/// <reference types="vite/client" />
import type { Tile } from "../protocol/Tile";
import { isRed, kindOf } from "../game/tiles";

/**
 * 34種の牌図。`?raw` で文字列として読み込む。
 *
 * **`*?raw` の型は `vite/client` が既に宣言している。**`tsconfig.json` の
 * `types` を空にしているためグローバルには自動では入らないが、この
 * ファイル冒頭の参照ディレクティブでアンビエント宣言として読み込んでいる。
 * **アンビエント宣言はプログラム全体（他のファイルも含む）に効く。**この
 * ファイルに書いたのは単に「ここで最初に必要になったから」であり、
 * 効果範囲をこのファイルに閉じ込めているわけではない。`vite-env.d.ts` に
 * 移すのはこのタスクの `Files:`（`tile-face.ts` と試験のみ）を超えるため
 * 見送っている。
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

/** ルート `<svg ...>` の開きタグ全体（属性部分を捕捉する）。 */
const SVG_OPEN_TAG = /<svg\b([^>]*)>/;

/**
 * 取り込んだ SVG のルート `<svg>` タグに `class="tile-face"` と `viewBox`
 * を注入する。
 *
 * **取り込み素材34件はどれも `class="tile-face"` も `viewBox` も持たない**
 * （`grep -l 'class="tile-face"' apps/web/src/assets/tiles/*.svg` /
 * `grep -l viewBox ...` はいずれも0件）。`board.css` の `.tile-face` は
 * `class="tile-face"` を持つ要素にしか当たらないため、これを注入し
 * ないと 2D の盤面（`ui/board.ts`）では `width`/`height` の実寸
 * （74.7×95.1px 等）でそのまま描かれ、28px の牌の枠からはみ出す。3D の
 * 牌面アトラス（`scene/face-atlas.ts`）は描画先の寸法を別途指定するため
 * 影響を受けないが、`tileFaceSvg`/`tileBackSvg` は唯一の定義であり両方が
 * 通るので、ここで両方に注入する。
 *
 * `viewBox` は素材ごとの `width`/`height` から作る（値は素材間で微妙に
 * 異なる。例: `m8.svg` は `width="74.700005"`）。既に `viewBox` や
 * `class="tile-face"` を持つ文字列（`tileBackSvg` の手書き SVG）に対しては
 * 何もしない。冪等にしておくことで、`tileFaceSvg` と `tileBackSvg` の
 * 両方からこの1つの関数を素直に呼べる。
 */
function normalizeTileSvg(source: string): string {
  return source.replace(SVG_OPEN_TAG, (whole, attrs: string) => {
    const width = /\bwidth="([^"]+)"/.exec(attrs)?.[1];
    const height = /\bheight="([^"]+)"/.exec(attrs)?.[1];
    if (width === undefined || height === undefined) {
      throw new Error("牌のSVGに width/height が無く viewBox を作れない");
    }
    const withViewBox = /\bviewBox="/.test(attrs)
      ? attrs
      : `${attrs} viewBox="0 0 ${width} ${height}"`;
    const withClass = /\bclass="tile-face"/.test(withViewBox)
      ? withViewBox
      : `${withViewBox} class="tile-face"`;
    return `<svg${withClass}>`;
  });
}

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
  const svg = isRed(tile) ? reddened(source) : source;
  return normalizeTileSvg(svg);
}

/**
 * 赤ドラの絵。
 *
 * **一括の色置換はしていない。**`m5.svg`/`p5.svg`/`s5.svg` を読むと、3ファイル
 * とも構造は同じで、色は `fill=` 属性ではなく数字の `<path>` を包む `<g>` の
 * `style` 属性の中に `fill:#000000;fill-opacity:1;stroke:none;...` として
 * 1箇所だけ書かれている（`fill=` 属性は0件）。牌の背景色は `span.tile` 側の
 * CSS が持ち、この SVG 自体は数字とその縁取りの黒いパス1本だけを描いている。
 * よってこの `fill:#000000` だけを赤に変えれば、数字の絵だけが赤くなり、
 * 背景（＝この文字列には存在しない）が赤くなることはない。
 */
function reddened(source: string): string {
  return source.replace("fill:#000000", "fill:#c0392b");
}

/**
 * 裏向きの牌。他家の手牌に使う。
 *
 * 手書きの SVG。`width`/`height` だけを持たせ、`class="tile-face"` と
 * `viewBox` は他の34件と同じ `normalizeTileSvg` に作らせる。手で
 * `viewBox="0 0 40 54"` と書いても値は一致するが、経路を1本にしておく
 * ほうが `width`/`height` を変えたときにズレない。
 */
export function tileBackSvg(): string {
  const svg =
    `<svg xmlns="${SVG_NS}" width="40" height="54" aria-hidden="true">` +
    `<rect x="0.7" y="0.7" width="38.6" height="52.6" rx="4.5" ` +
    `fill="#2f7a52" stroke="#1c4b33" stroke-width="1.2"/>` +
    `<rect x="6" y="7" width="28" height="40" rx="3" fill="none" ` +
    `stroke="#7fc39b" stroke-width="1.4"/>` +
    `</svg>`;
  return normalizeTileSvg(svg);
}
