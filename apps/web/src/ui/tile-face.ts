import type { Tile } from "../protocol/Tile";
import { isRed, kindOf } from "../game/tiles";

/**
 * 牌の面を SVG で描く。
 *
 * **文字ラベルでは麻雀牌に見えない。**萬子は漢数字と萬、筒子は円、
 * 索子は竹、字牌は東南西北白發中を描く。赤ドラは赤で描き分ける。
 *
 * 画像を持たないのは、差し替え可能なマニフェスト管理（assets/）へ
 * 移すまでの繋ぎだからである。3D の牌面アトラスは `scene/atlas.ts` にある。
 */

const FACE_W = 40;
const FACE_H = 54;

const MAN_DIGITS = ["一", "二", "三", "四", "五", "六", "七", "八", "九"] as const;
const HONORS = ["東", "南", "西", "北", "白", "發", "中"] as const;

/**
 * SVG の名前空間。
 *
 * **`innerHTML` に入れるだけなら要らないが、外すと 3D の牌面が死ぬ。**
 * HTML に直に書いた `<svg>` は名前空間が補われるのに対し、`data:` URL で
 * 画像として読ませたものは独立した文書として解析されるので、宣言が無いと
 * SVG と見なされず読み込みに失敗する。`scene/face-atlas.ts` はこれを画像
 * として読み、しかも 39 枚を `Promise.all` で待つ。**1枚欠けると全部が
 * 焼けず、牌が仮の文字ラベルのまま出る。**
 */
const SVG_NS = "http://www.w3.org/2000/svg";

const INK = "#1a1a2e";
const RED = "#c0392b";
const GREEN = "#1f7a44";
const BLUE = "#1b5fa8";

/** 筒子の円の並び。左上を原点とした比率で持つ。 */
const PIN_LAYOUT: readonly (readonly (readonly [number, number])[])[] = [
  [[0.5, 0.5]],
  [[0.5, 0.3], [0.5, 0.7]],
  [[0.28, 0.24], [0.5, 0.5], [0.72, 0.76]],
  [[0.32, 0.3], [0.68, 0.3], [0.32, 0.7], [0.68, 0.7]],
  [[0.3, 0.26], [0.7, 0.26], [0.5, 0.5], [0.3, 0.74], [0.7, 0.74]],
  [[0.32, 0.22], [0.68, 0.22], [0.32, 0.5], [0.68, 0.5], [0.32, 0.78], [0.68, 0.78]],
  [[0.28, 0.18], [0.5, 0.28], [0.72, 0.38], [0.32, 0.62], [0.68, 0.62], [0.32, 0.84], [0.68, 0.84]],
  [[0.32, 0.16], [0.68, 0.16], [0.32, 0.39], [0.68, 0.39], [0.32, 0.62], [0.68, 0.62], [0.32, 0.85], [0.68, 0.85]],
  [[0.25, 0.2], [0.5, 0.2], [0.75, 0.2], [0.25, 0.5], [0.5, 0.5], [0.75, 0.5], [0.25, 0.8], [0.5, 0.8], [0.75, 0.8]],
];

/**
 * 索子の竹の並び。
 *
 * **筒子と同じ並びにしてはいけない。**索子は縦の竹なので、斜めに置くと
 * 牌に見えない。1索は本来は鳥なので別に描く。
 */
const SOU_LAYOUT: readonly (readonly (readonly [number, number])[])[] = [
  [[0.5, 0.5]],
  [[0.5, 0.27], [0.5, 0.73]],
  [[0.5, 0.2], [0.3, 0.72], [0.7, 0.72]],
  [[0.3, 0.28], [0.7, 0.28], [0.3, 0.72], [0.7, 0.72]],
  [[0.3, 0.22], [0.7, 0.22], [0.5, 0.5], [0.3, 0.78], [0.7, 0.78]],
  [[0.28, 0.24], [0.5, 0.24], [0.72, 0.24], [0.28, 0.76], [0.5, 0.76], [0.72, 0.76]],
  [[0.5, 0.17], [0.28, 0.55], [0.5, 0.55], [0.72, 0.55], [0.28, 0.85], [0.5, 0.85], [0.72, 0.85]],
  [[0.28, 0.19], [0.5, 0.19], [0.72, 0.19], [0.28, 0.5], [0.72, 0.5], [0.28, 0.81], [0.5, 0.81], [0.72, 0.81]],
  [[0.28, 0.18], [0.5, 0.18], [0.72, 0.18], [0.28, 0.5], [0.5, 0.5], [0.72, 0.5], [0.28, 0.82], [0.5, 0.82], [0.72, 0.82]],
];

function pinCircle(cx: number, cy: number, red: boolean): string {
  const outer = red ? RED : BLUE;
  const inner = red ? "#e8b0a8" : "#d94f4f";
  return (
    `<circle cx="${cx}" cy="${cy}" r="5.1" fill="#fdfdf7" stroke="${outer}" stroke-width="1.7"/>` +
    `<circle cx="${cx}" cy="${cy}" r="1.9" fill="${inner}"/>`
  );
}

/**
 * 竹1本。真ん中に節を入れる。
 *
 * **丈は段数で変える。**3段に並べるときに 2段ぶんの丈で描くと、
 * 上下の竹と重なって潰れる。
 */
function bamboo(cx: number, cy: number, red: boolean, half: number): string {
  const color = red ? RED : GREEN;
  const dark = red ? "#8c2a20" : "#14562f";
  const w = half > 6 ? 5.4 : 4.6;
  return (
    `<rect x="${cx - w / 2}" y="${cy - half}" width="${w}" height="${half * 2}" rx="${w / 2}" fill="${color}"/>` +
    // 節は幹を分断しない。白で抜くと豆が2つ並んで見える。
    `<rect x="${cx - w / 2}" y="${cy - 0.6}" width="${w}" height="1.2" fill="${dark}"/>`
  );
}

/** 1索は鳥。竹1本だと2索の半分に見えて紛らわしい。 */
function birdOfBamboo(red: boolean): string {
  const body = red ? RED : GREEN;
  const cx = FACE_W / 2;
  const cy = FACE_H / 2;
  return (
    `<ellipse cx="${cx}" cy="${cy + 2}" rx="7.5" ry="10" fill="${body}"/>` +
    `<circle cx="${cx}" cy="${cy - 9}" r="4.6" fill="${body}"/>` +
    `<circle cx="${cx + 1.6}" cy="${cy - 10}" r="1.1" fill="#fffdf2"/>` +
    `<path d="M ${cx + 4} ${cy - 8.5} l 5 1.6 l -5 1.8 z" fill="${RED}"/>` +
    `<path d="M ${cx - 7} ${cy} q 6 6 3 13" stroke="#fffdf2" stroke-width="1.5" fill="none"/>` +
    `<path d="M ${cx - 2} ${cy + 12} l -5 6 M ${cx + 2} ${cy + 12} l 5 6" stroke="${body}" stroke-width="1.6"/>`
  );
}

function suitBody(kind: number, red: boolean): string {
  // 数牌の描画域。牌の縁を避ける。
  const left = 6;
  const top = 7;
  const width = FACE_W - 12;
  const height = FACE_H - 14;
  const count = (kind % 9) + 1;

  if (kind < 9) {
    // 萬子。漢数字を上、萬を下に置く。
    const digit = MAN_DIGITS[count - 1] ?? "?";
    const digitColor = red ? RED : INK;
    return (
      `<text x="${FACE_W / 2}" y="24" text-anchor="middle" font-size="19" fill="${digitColor}" font-family="serif">${digit}</text>` +
      `<text x="${FACE_W / 2}" y="45" text-anchor="middle" font-size="17" fill="${RED}" font-family="serif">萬</text>`
    );
  }

  if (kind >= 18 && count === 1) {
    return birdOfBamboo(red);
  }

  const layout = kind < 18 ? PIN_LAYOUT[count - 1] : SOU_LAYOUT[count - 1];
  if (!layout) {
    return "";
  }
  if (kind < 18) {
    return layout
      .map(([rx, ry]) => pinCircle(left + rx * width, top + ry * height, red))
      .join("");
  }
  // 段数を数えて竹の丈を決める。
  const rows = new Set(layout.map(([, ry]) => ry)).size;
  const half = rows >= 3 ? 5.6 : 7.5;
  return layout
    .map(([rx, ry]) => bamboo(left + rx * width, top + ry * height, red, half))
    .join("");
}

function honorBody(kind: number): string {
  const index = kind - 27;
  const glyph = HONORS[index] ?? "?";
  if (index === 4) {
    // 白は字を書かず、枠だけを描く。
    return `<rect x="9" y="10" width="22" height="34" fill="none" stroke="${BLUE}" stroke-width="2.2" rx="2"/>`;
  }
  const color = index === 5 ? GREEN : index === 6 ? RED : INK;
  return `<text x="${FACE_W / 2}" y="38" text-anchor="middle" font-size="26" fill="${color}" font-family="serif">${glyph}</text>`;
}

/**
 * 1枚ぶんの SVG。
 *
 * **入力は 0..=36 の数値だけ。**利用者の文字列は一切混ざらないので、
 * 生成した文字列をそのまま `innerHTML` に入れてよい。
 */
export function tileFaceSvg(tile: Tile): string {
  const kind = kindOf(tile);
  const red = isRed(tile);
  const body = kind >= 27 ? honorBody(kind) : suitBody(kind, red);
  return (
    `<svg xmlns="${SVG_NS}" viewBox="0 0 ${FACE_W} ${FACE_H}" class="tile-face" aria-hidden="true">` +
    `<rect x="0.7" y="0.7" width="${FACE_W - 1.4}" height="${FACE_H - 1.4}" rx="4.5" ` +
    `fill="#fffdf2" stroke="#b9a97c" stroke-width="1.2"/>` +
    body +
    `</svg>`
  );
}

/** 裏向きの牌。他家の手牌に使う。 */
export function tileBackSvg(): string {
  return (
    `<svg xmlns="${SVG_NS}" viewBox="0 0 ${FACE_W} ${FACE_H}" class="tile-face" aria-hidden="true">` +
    `<rect x="0.7" y="0.7" width="${FACE_W - 1.4}" height="${FACE_H - 1.4}" rx="4.5" ` +
    `fill="#2f7a52" stroke="#1c4b33" stroke-width="1.2"/>` +
    `<rect x="6" y="7" width="${FACE_W - 12}" height="${FACE_H - 14}" rx="3" fill="none" ` +
    `stroke="#7fc39b" stroke-width="1.4"/>` +
    `</svg>`
  );
}
