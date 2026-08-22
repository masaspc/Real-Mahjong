import { tileFaceSvg, tileBackSvg } from "./ui/tile-face";
import { tileLabel } from "./game/tiles";
import { applyTileTheme } from "./ui/board";
import { resultPanel } from "./ui/result";
import { emptyState } from "./game/state";
import type { GameState } from "./game/state";
import type { Tile } from "./protocol/Tile";
import type { Seat } from "./protocol/Seat";
import "./ui/board.css";

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

// **牌の地色は `board.ts` が CSS 変数へ流し込む。**この頁は `renderBoard`
// を通らないので、自分で入れないと 2D の牌が「黒地に黒い線」になる。
applyTileTheme(document.documentElement);

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


/**
 * 局の結果の見本。
 *
 * **対局で和了を待って確かめるのは運任せである。**役の並び、裏ドラ、
 * 点棒の増減が読める形に収まっているかを、ここで決め打ちで見る。
 */
function sampleBoard(): GameState {
  const state = emptyState(0 as Seat);
  state.round = { wind: "East", number: 1 };
  state.phase = "playing";
  return state;
}

const make = {
  node<K extends keyof HTMLElementTagNameMap>(
    tag: K,
    className?: string,
    text?: string,
  ): HTMLElementTagNameMap[K] {
    const element = document.createElement(tag);
    if (className) element.className = className;
    if (text !== undefined) element.textContent = text;
    return element;
  },
  tileNode(tile: Tile): HTMLElement {
    const span = document.createElement("span");
    span.className = "tile";
    span.innerHTML = tileFaceSvg(tile);
    return span;
  },
};

const stage = document.createElement("div");
stage.className = "stage";
const heading = document.createElement("p");
heading.textContent = "局の結果（見本）";
sheet.append(heading, stage);

stage.append(
  resultPanel(
    {
      kind: "agari",
      results: [
        {
          seat: 0 as Seat,
          from: 2 as Seat,
          hand: [0, 1, 2, 9, 10, 11, 18, 19, 20, 27, 27] as Tile[],
          melds: [],
          win_tile: 20 as Tile,
          yaku: [
            ["riichi", 1],
            ["ippatsu", 1],
            ["sanshoku_doujun", 2],
            ["dora", 2],
            ["ura_dora", 1],
          ],
          fu: 40,
          han: 7,
          score: 12000,
          liability: null,
          ura_indicators: [5, 30] as Tile[],
        },
      ],
      tenpai: [],
      ryuukyoku: null,
      delta: [12300, -300, -12000, 0],
      at: 0,
    },
    sampleBoard(),
    make,
    () => {},
  ),
);
