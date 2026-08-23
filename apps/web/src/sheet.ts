import { tileFaceSvg, tileBackSvg } from "./ui/tile-face";
import { tileLabel } from "./game/tiles";
import { applyTileTheme, finalPanel } from "./ui/board";
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


/**
 * 終局の板の見本。
 *
 * **半荘を打ち切るのを待って確かめるのは現実的でない。**CPU4人でも4分強
 * かかるうえ、終わった卓は掃除されるので撮り直しがきかない。ここで決め打つ。
 */
const over = sampleBoard();
over.phase = "matchOver";
over.finalScores = [-2_300, 41_000, 17_500, 43_800];
over.placements = [4, 2, 3, 1];
over.rules = {
  length: "Hanchan",
  start_score: 25_000,
  return_score: 30_000,
  uma: [15, 5, -5, -15],
  red_dora_count: 3,
  kuitan: true,
  double_ron: true,
  formal_tenpai: true,
  noten_penalty: 3_000,
  nagashi_mangan: true,
  liability: true,
  round_up_mangan: false,
  busted_ends_match: true,
  base_think_ms: 5_000,
  think_bank_ms: 20_000,
  network_grace_ms: 500,
  min_reaction_window_ms: 350,
};

const overHeading = document.createElement("p");
overHeading.textContent = "終局（見本）";
const overStage = document.createElement("div");
overStage.className = "stage";
sheet.append(overHeading, overStage);
overStage.append(finalPanel(over, make.node, () => {}));
