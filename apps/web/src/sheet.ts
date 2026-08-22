import { tileFaceSvg, tileBackSvg } from "./ui/tile-face";
import { tileLabel } from "./game/tiles";
import type { Tile } from "./protocol/Tile";

/**
 * 37種と裏面を並べるだけの頁。
 *
 * **牌の絵は 3D を経由せずに確かめられる。**アトラスへ焼く前の段階で
 * 色や字形が正しいかを見るのに、対局も WebGL も要らない。色を変えたら
 * まずここを撮る。
 */

const groups: [string, Tile[]][] = [
  ["萬子", range(0, 9)],
  ["筒子", range(9, 18)],
  ["索子", range(18, 27)],
  ["字牌", range(27, 34)],
  ["赤ドラ", range(34, 37)],
];

function range(from: number, to: number): Tile[] {
  return Array.from({ length: to - from }, (_, i) => (from + i) as Tile);
}

function cell(svg: string, label: string): HTMLElement {
  const box = document.createElement("div");
  box.className = "cell";
  const face = document.createElement("div");
  face.className = "face";
  face.innerHTML = svg;
  const caption = document.createElement("span");
  caption.textContent = label;
  box.append(face, caption);
  return box;
}

const sheet = document.querySelector("#sheet");
if (sheet === null) {
  throw new Error("#sheet が無い");
}

for (const [name, tiles] of groups) {
  const heading = document.createElement("p");
  heading.textContent = name;
  const row = document.createElement("div");
  row.className = "row";
  for (const tile of tiles) {
    row.append(cell(tileFaceSvg(tile), tileLabel(tile)));
  }
  sheet.append(heading, row);
}

const back = document.createElement("div");
back.className = "row";
back.append(cell(tileBackSvg(), "裏"));
sheet.append(back);
