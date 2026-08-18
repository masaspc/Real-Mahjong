import { TableScene } from "./scene/table";
import { placementsFor } from "./scene/placement";
import { motionsFor } from "./scene/motion";
import { apply } from "./game/state";
import type { ClientEvent } from "./protocol/ClientEvent";
import type { GameState, SeatView } from "./game/state";
import type { Seat } from "./protocol/Seat";
import type { Tile } from "./protocol/Tile";

/**
 * 卓の見た目を確かめるための、動かない盤面。
 *
 * **対局しながら目で確かめるのは運任せである。**加槓や暗槓は出るまで
 * 打たねばならず、出ない半荘もある。ここでは副露5種・リーチ宣言牌・
 * 長い河・山を最初から並べておき、1画面で全部を見えるようにする。
 *
 * 判定は入らない。`placementsFor` に食わせる形が整っていればよく、
 * 実際の局で作れる手牌である必要もない。**確かめたいのは置き場所だけ。**
 */

/** 牌の符号は 0..=36。0-8=萬 9-17=筒 18-26=索 27-33=字 34/35/36=赤 */
const M = (n: number): Tile => (n - 1) as Tile;
const P = (n: number): Tile => (n + 8) as Tile;
const S = (n: number): Tile => (n + 17) as Tile;
const EAST = 27 as Tile;
const WHITE = 31 as Tile;
const GREEN = 32 as Tile;
const RED_5P = 35 as Tile;

function river(tiles: Tile[], riichiAt = -1): SeatView["river"] {
  return tiles.map((tile, index) => ({ tile, riichi: index === riichiAt }));
}

/** 河を長くする。19枚目から4段目に入り、卓の中央へ伸びていく。 */
function longRiver(from: number, count: number): Tile[] {
  return Array.from({ length: count }, (_, i) => ((from + i) % 34) as Tile);
}

const seats: [SeatView, SeatView, SeatView, SeatView] = [
  {
    // 自分。チーとポンを出しておく。手牌は残り7枚。
    handSize: 7,
    river: river([M(1), P(9), S(3), EAST, M(7), P(2), WHITE, S(8), M(4)]),
    melds: [
      { kind: "chi", tiles: [S(3), S(4), S(5)], from: 3 },
      { kind: "pon", tiles: [P(7), P(7), P(7)], from: 1 },
    ],
    riichi: false,
    declaring: false,
  },
  {
    // 下家。大明槓。鳴いた牌が横に倒れる。
    handSize: 10,
    river: river([P(1), M(6), S(2), GREEN, P(4), M(9), S(6)]),
    melds: [{ kind: "minkan", tiles: [M(3), M(3), M(3), M(3)], from: 0 }],
    riichi: false,
    declaring: false,
  },
  {
    // 対面。暗槓。両端が伏せ、中2枚が表になる。リーチ済み。
    handSize: 10,
    river: river(longRiver(4, 21), 5),
    melds: [{ kind: "ankan", tiles: [S(9), S(9), S(9), S(9)], from: 2 }],
    riichi: true,
    declaring: false,
  },
  {
    // 上家。加槓。4枚目がポンした牌の上に積まれる。
    handSize: 10,
    river: river([WHITE, P(3), M(2), S(7), P(6)]),
    melds: [{ kind: "kakan", tiles: [P(5), P(5), RED_5P, P(5)], from: 2 }],
    riichi: false,
    declaring: false,
  },
];

const state: GameState = {
  you: 0,
  seats,
  hand: [M(1), M(2), M(3), P(4), P(5), S(7), EAST],
  drawn: RED_5P,
  round: { wind: "East", number: 1 },
  dealer: 0,
  honba: 1,
  sticks: 1,
  scores: [25000, 24000, 26000, 25000],
  doraIndicators: [P(3), S(1)],
  wallRemaining: 42,
  pending: null,
  // 上家が切った 1索 を鳴ける、という場面にしておく。
  lastDiscard: { seat: 3, tile: 18 },
  lastSeq: 1,
  phase: "playing",
  turn: 0,
  notice: null,
  finalScores: null,
};

const canvas = document.querySelector<HTMLCanvasElement>("#table");
if (!canvas) {
  throw new Error("#table が無い");
}

const scene = new TableScene(canvas);

/**
 * `?motion=0.5` で、打牌の動きを途中で止めた絵を出す。
 *
 * **対局を待たずに動きを見られるようにする。**進捗を指定して止められないと、
 * 動いているかどうかを目で追うしかない。
 */
const motionAt = (() => {
  const raw = new URLSearchParams(location.search).get("motion");
  if (raw === null) return null;
  const value = Number(raw);
  return Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : null;
})();

/** 自分が 1m を手出しする。手牌から河へ動く。 */
const MOTION_EVENT: ClientEvent = {
  type: "discard",
  seat: 0,
  tile: 0,
  manner: "tedashi",
};

/** どの席から見るかを変えられる。自席が手前に来ることを席ごとに確かめる。 */
function show(viewer: Seat): void {
  if (motionAt === null) {
    scene.sync(placementsFor(state, viewer));
    return;
  }
  const after = apply(state, { seq: 999, event: MOTION_EVENT }, 0);
  const beforeAll = placementsFor(state, viewer);
  const afterAll = placementsFor(after, viewer);
  scene.syncWithMotion(
    afterAll,
    motionsFor(beforeAll, afterAll, MOTION_EVENT, viewer),
    motionAt,
  );
}

/** `?viewer=2` で見る席を選べる。**押さずに席ごとの絵を撮れる。** */
function seatFromQuery(): Seat {
  const raw = Number(new URLSearchParams(location.search).get("viewer"));
  return (Number.isInteger(raw) && raw >= 0 && raw <= 3 ? raw : 0) as Seat;
}

let viewer: Seat = seatFromQuery();
const initialLabel = document.querySelector("#viewer-label");
if (initialLabel) {
  initialLabel.textContent = `見ている席: ${viewer}`;
}
show(viewer);

document.querySelector("#viewer")?.addEventListener("click", () => {
  viewer = ((viewer + 1) % 4) as Seat;
  const label = document.querySelector("#viewer-label");
  if (label) {
    label.textContent = `見ている席: ${viewer}`;
  }
  show(viewer);
});

function resize(): void {
  scene.resize(canvas!.clientWidth, canvas!.clientHeight);
}
resize();
addEventListener("resize", resize);

/**
 * `?still=1` なら数コマだけ描いて止める。
 *
 * **撮影のために回し続けない。**仮想時間で撮ると、止まらないループは
 * 千コマ単位で描き直され、ソフトウェア描画では終わらない。
 *
 * ただし1コマで即止めるだけでは中身が空の png になる。理由は2つある。
 *
 * 1. 牌面アトラス（`table.ts` の `paintAtlas`）は非同期に焼ける。焼き上がり前に
 *    止めると、テクスチャが差し替わる前の絵になる。**猶予コマ数の勘に頼らず、**
 *    `scene.ready` を実際に待つ。`paintAtlas` が失敗しても `scene.ready` は
 *    解決するので、待ちっぱなしにはならない。
 * 2. ヘッドレス Chrome の `--virtual-time-budget` 下では、WebGL の描画バッファに
 *    正しく描けていても（`gl.readPixels` で確認済み）、ページの合成側がその
 *    コマを拾わないまま `--screenshot` が走ることがある。コマ数を増やしても
 *    この空白は直らなかった。**WebGL の canvas をそのまま撮らせるのをやめ、**
 *    最後に描いた1枚を普通の 2D canvas へ焼き写し、WebGL の canvas をそれに
 *    差し替えてから止める。普通の 2D canvas は合成のタイミングに左右されず、
 *    確実に撮影に写る。
 */
const still = new URLSearchParams(location.search).get("still") === "1";

/**
 * アトラスの焼き上がり後、合成に確実に乗せるための猶予コマ数。
 *
 * 焼き上がり自体は `scene.ready` で確かめ済みなので、ここでの猶予は
 * 上記の理由2（合成側の取りこぼし）だけを埋める。
 */
const STILL_SETTLE_FRAMES = 15;

function frame(): void {
  scene.render();
  requestAnimationFrame(frame);
}

/** WebGL の canvas を、直前に描いた絵のままの 2D canvas へ差し替えて止める。 */
function freezeCanvas(): void {
  const snapshot = document.createElement("canvas");
  snapshot.id = canvas!.id;
  snapshot.width = canvas!.width;
  snapshot.height = canvas!.height;
  snapshot.style.cssText = "display:block;width:100%;height:100%;";
  snapshot.getContext("2d")?.drawImage(canvas!, 0, 0);
  canvas!.replaceWith(snapshot);
}

if (still) {
  // アトラスの焼き上がりを実際に待ってから、猶予コマを描いて止める。
  void scene.ready.then(() => {
    let settleFrames = 0;
    function settle(): void {
      scene.render();
      settleFrames += 1;
      if (settleFrames < STILL_SETTLE_FRAMES) {
        requestAnimationFrame(settle);
        return;
      }
      freezeCanvas();
    }
    requestAnimationFrame(settle);
  });
} else {
  requestAnimationFrame(frame);
}
