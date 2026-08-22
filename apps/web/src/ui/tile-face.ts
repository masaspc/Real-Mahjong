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
 * `class="tile-face"` を持つ文字列に対しては何もしない。冪等にしておくことで、
 * `tileFaceSvg` と `tileBackSvg` の両方からこの1つの関数を素直に呼べる。
 */
export function normalizeTileSvg(source: string): string {
  return source.replace(SVG_OPEN_TAG, (whole, attrs: string) => {
    const width = /\bwidth="([^"]+)"/.exec(attrs)?.[1];
    const height = /\bheight="([^"]+)"/.exec(attrs)?.[1];
    if (width === undefined || height === undefined) {
      throw new Error("牌のSVGに width/height が無く viewBox を作れない");
    }
    const withViewBox = /\bviewBox="/.test(attrs)
      ? attrs
      : `${attrs} viewBox="0 0 ${width} ${height}"`;
    // **`class` は足すのではなく混ぜる。**既に別の `class` を持つ SVG に
    // 素朴に付け足すと `class="a" class="tile-face"` になり、ブラウザは
    // 後ろを捨てるので寸法の指定が効かなくなる。属性が2つある不正な XML を
    // 作っておいて、片方が黙って無視されるという最も気付きにくい壊れ方を
    // するので、既存の値へ追記する形にする。
    const existing = /\bclass="([^"]*)"/.exec(withViewBox);
    const names = existing?.[1] ?? "";
    let withClass: string;
    if (existing === null) {
      withClass = `${withViewBox} class="tile-face"`;
    } else if (names.split(/\s+/).includes("tile-face")) {
      withClass = withViewBox;
    } else {
      const merged = names === "" ? "tile-face" : `${names} tile-face`;
      withClass = withViewBox.replace(existing[0], `class="${merged}"`);
    }
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
  // **赤ドラは字形ごと赤にする。**種類ごとの色より優先する。
  const svg = isRed(tile) ? paint(source, RED) : inked(source, kind);
  return normalizeTileSvg(svg);
}

/**
 * 牌の色。
 *
 * **取り込んだ牌図は1本のパスでできた字形である。**筒の丸ごと・索の節ごとに
 * 色を変えるような塗り分けは、この素材ではできない（`fill` は字形全体に
 * 1つしか無い）。できるのは「1枚を1色で塗る」ことと、上下で色を分けることの
 * 2つだけなので、その範囲で本物に寄せる。
 *
 * - 萬子: 数字は黒、「萬」は赤。上下で分ける
 * - 筒子: 青
 * - 索子: 緑
 * - 風牌: 黒
 * - 發: 緑 / 中: 赤 / 白: 青い枠
 * - 赤ドラ: 字形ごと赤
 */
const BLACK = "#1c1c1c";
const RED = "#b3261e";
const GREEN = "#1a7a3c";
const BLUE = "#1f4e9c";

/**
 * 字形の色を差し替える。
 *
 * **素材は色の書き方が3通りある。**1つの書き方だけ直すと、残りは黒いまま
 * 出る。実際、`fill:#000000` だけを見ていたときは九筒だけが黒く残った。
 *
 * | 書き方 | 件数 | 例 |
 * |---|---|---|
 * | `<g style="…fill:#000000…">` の字形 | 31 | 萬子・索子・1〜8筒・風牌・發 |
 * | `stroke="#000"` の線画 | 2 | 九筒・白 |
 * | 何も書かず既定の黒に頼る | 1 | 中 |
 *
 * 3通り全部に効かせる。根の `fill` は最後の1件のためで、`<g>` の指定が
 * ある素材では上書きされるので害は無い。
 */
function paint(source: string, color: string): string {
  return source
    .replace("fill:#000000", `fill:${color}`)
    .replace(/stroke="#000(000)?"/g, `stroke="${color}"`)
    .replace(SVG_OPEN_TAG, (_whole, attrs: string) => `<svg${attrs} fill="${color}">`);
}

/**
 * 萬子だけ、上下で色を分ける。
 *
 * 数字が上、「萬」が下に来る字形なので、**字形の外接矩形の高さ 52% で
 * 色を切り替える**と数字が黒・萬が赤になる。境目を持つ勾配を1つ置き、
 * 同じ位置に2つの停止点を重ねて段差にする。
 *
 * パスを分割しないので、字形の中身に依存しない。
 */
function twoTone(source: string, kind: number): string {
  const id = `mj-man-${kind}`;
  const defs =
    `<defs><linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1">` +
    `<stop offset="0.52" stop-color="${BLACK}"/>` +
    `<stop offset="0.52" stop-color="${RED}"/>` +
    `</linearGradient></defs>`;
  return source
    .replace("fill:#000000", `fill:url(#${id})`)
    .replace(
      SVG_OPEN_TAG,
      (_whole, attrs: string) => `<svg${attrs} fill="url(#${id})">${defs}`,
    );
}

/**
 * 白。
 *
 * **真っ白にする。**素材は入れ子の長方形2本を描いているが、日本の牌の白は
 * 何も書かれていない無地である。牌そのものに縁があるので、枠を描くと
 * 二重になり、枠ではなく汚れのように見える。
 *
 * 中身を全部落として、`<svg>` の殻だけ残す。`normalizeTileSvg` が
 * `width`/`height` から `viewBox` を作るので、殻だけでも寸法は保たれる。
 */
function whiteDragon(source: string): string {
  return source.replace(/<g[\s\S]*<\/g>/, "");
}

/** 種類ごとの塗り。0-8=萬 9-17=筒 18-26=索 27-30=東南西北 31=白 32=發 33=中 */
function inked(source: string, kind: number): string {
  if (kind < 9) return twoTone(source, kind);
  if (kind < 18) return paint(source, BLUE);
  if (kind < 27) return paint(source, GREEN);
  if (kind < 31) return paint(source, BLACK);
  if (kind === 31) return whiteDragon(source);
  if (kind === 32) return paint(source, GREEN);
  return paint(source, RED);
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
