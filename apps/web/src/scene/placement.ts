import type { GameState, MeldView } from "../game/state";
import type { Seat } from "../protocol/Seat";
import type { Tile } from "../protocol/Tile";
import {
  DISCARDS_PER_ROW,
  HAND_Z,
  TILE,
  discardPosition,
  relativeSeat,
  seatRotation,
  wallPosition,
  type Vec3,
} from "./layout";

/** 卓のどこに置かれる牌か。 */
export type PlacementKind = "hand" | "drawn" | "river" | "meld" | "wall";

export type Placement = {
  /** 牌1枚を指す鍵。**重複すると牌が消えたり重なったりする。** */
  key: string;
  kind: PlacementKind;
  /** 絶対席。見せる向きは `position` に織り込み済み。 */
  seat: Seat;
  /** 伏せ牌は 0 を入れる。`faceUp` が偽なら中身は使わない。 */
  encoded: Tile;
  position: Vec3;
  /**
   * 立てるか寝かせるか。
   *
   * **牌面は作られた時点で +z を向いている。**手牌はそのまま立て、
   * 河・山は寝かせる。これが無いと河の牌が卓から生えて立つ。
   */
  rotationX: number;
  rotationY: number;
  faceUp: boolean;
  /**
   * 押せる牌か。**自分の手牌とツモ牌だけが真。**
   *
   * 伏せ牌は中身に 0（一萬）が入っているので、他家の牌を押せると
   * 一萬を切ったことになってしまう。
   */
  pickable: boolean;
};

/**
 * 牌の回転を合成する順序。
 *
 * **既定の `"XYZ"` では、自分以外の席の牌が全部あらぬ方を向く。**three.js の
 * `"XYZ"` は行列を `Rx * Ry` の順に掛けるので、席の向き（Y）を回してから
 * 卓へ寝かせる（X）ぶんが**世界の軸**に対してかかる。牌面の法線 +z は
 *
 * - 席0（Y=0）: 上を向く。正しい
 * - 席2（Y=π）: **下を向く。**捨て牌が裏返って置かれる
 * - 席1/3（Y=±π/2）: **横を向く。**牌が立ったまま真横を向き、河が板の列に見える
 *
 * となる。実際、対面の河は伏せ牌に、左右の河と山は無地の細長い塊に見えていた。
 * 利用者の「相手の捨て牌が表示されず」はこれである。
 *
 * `"YXZ"` は `Ry * Rx` の順に掛ける。先に牌自身の枠で寝かせ、そのあと席の
 * 向きへ回すので、どの席でも牌面は上を向いたまま向きだけが変わる。
 */
export const ROTATION_ORDER = "YXZ";

/** 河と鳴いた牌を横に倒す角度。 */
const SIDEWAYS = Math.PI / 2;

/** 卓に寝かせる角度。牌面を上へ向ける。 */
const LYING = -Math.PI / 2;

/**
 * 自分の手牌を手前へ倒す角度。
 *
 * **真っ直ぐ立てると読めない。**カメラは (0, 31, 22) から俯瞰しているので、
 * 立てた牌の面はほぼ真横から見ることになる。手前へ倒して面をカメラへ
 * 向けると、自分の手牌だけが正面から読める。
 *
 * 寝かせきる（`LYING`）と他家の伏せ牌や河と見分けがつかなくなるため、
 * 立っている姿は保つ。
 */
const TILTED = -Math.PI * 0.28;

/** 横に倒した牌が余分に取る幅。 */
const RIICHI_EXTRA = TILE.height - TILE.width;

/** 一辺は17トン＝34枚。 */
const SLOTS_PER_SIDE = 34;

/** 山は四辺で136枚。 */
const WALL_SLOTS = SLOTS_PER_SIDE * 4;

/**
 * 手牌と副露を並べる列の、片側の幅の上限。
 *
 * **隣の席の列と卓の角でぶつかってはいけない。**各席の列は z=10 に
 * あり、隣席では回転して x=10 になる。立てた牌は厚み 0.6 を占めるので
 * 隣席の列は 9.7..10.3。こちらの牌の端が 9.2 を超えると角で重なる。
 */
const ROW_HALF_WIDTH = 9.2;

/** 手牌と副露を並べる奥行き。 */
const ROW_Z = HAND_Z;

/** 手牌と副露のあいだの空き。 */
const HAND_MELD_GAP = 0.8;

/** 副露どうしのあいだの空き。 */
const MELD_GAP = 0.3;

function rotateY(point: Vec3, radians: number): Vec3 {
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  return {
    x: point.x * cos + point.z * sin,
    y: point.y,
    z: -point.x * sin + point.z * cos,
  };
}

/**
 * 手牌と副露を1つの列として置く。
 *
 * **多すぎるときは詰める。**四副露では手牌と副露で18枚ぶんになり、
 * そのまま並べると隣席の列と卓の角でぶつかる。席の領域からはみ出す
 * くらいなら、少し重ねてでも自分の側に収める。
 */
function rowPosition(relative: number, slot: number, total: number, lift?: number): Vec3 {
  const spacing = Math.min(TILE.width, (ROW_HALF_WIDTH * 2) / Math.max(total, 1));
  // 手牌は立てるので高さの半分、副露は寝かせるので厚みの半分。
  const y = lift === undefined ? TILE.height / 2 : TILE.depth / 2 + lift;
  return rotateY(
    { x: (slot - (total - 1) / 2) * spacing, y, z: ROW_Z },
    seatRotation(relative),
  );
}

type MeldPiece = {
  tile: Tile;
  faceUp: boolean;
  sideways: boolean;
  /** 同じ場所へ積む段。加槓の4枚目だけが 1 になる。 */
  stack: number;
};

/**
 * 副露1組を並べ直す。
 *
 * **鳴いた牌は横に倒し、取得元が分かる位置へ置く。**上家からなら左端、
 * 対面なら左から2枚目、下家なら右端。倒さないと誰から鳴いたか
 * 分からず、盤面として成立しない。
 *
 * **暗槓は両端2枚だけ伏せる。**4枚とも伏せると何を槓したか分からない。
 *
 * **加槓の末尾は鳴いた牌ではなく、後から足した4枚目である。**
 * エンジンは元のポンの牌列へ4枚目を押し込む。末尾を鳴いた牌として
 * 倒すと、加槓だけ別の牌が倒れる。
 */
export function layMeld(meld: MeldView, owner: Seat): MeldPiece[] {
  const tiles = [...meld.tiles];

  if (meld.kind === "ankan") {
    return tiles.map((tile, index) => ({
      tile,
      faceUp: index !== 0 && index !== tiles.length - 1,
      sideways: false,
      stack: 0,
    }));
  }

  const source = relativeSeat(meld.from, owner);
  const slotOf = (size: number) => (source === 3 ? 0 : source === 2 ? 1 : size - 1);

  if (meld.kind === "kakan") {
    // **4枚目は鳴いた牌の上へ積む。**横一列に並べると普通の副露と
    // 見分けがつかない。エンジンは末尾へ4枚目を押し込むので、
    // 末尾を取り出して鳴いた牌と同じ場所の2段目へ置く。
    const fourth = tiles.pop();
    const slot = slotOf(3);
    const pieces: MeldPiece[] = tiles.map((tile, index) => ({
      tile,
      faceUp: true,
      sideways: index === slot,
      stack: 0,
    }));
    if (fourth !== undefined) {
      pieces.push({ tile: fourth, faceUp: true, sideways: true, stack: 1 });
    }
    return pieces;
  }

  // チー・ポン・大明槓。エンジンは鳴いた牌を末尾へ入れる。
  const called = tiles.pop();
  const pieces: MeldPiece[] = tiles.map((tile) => ({
    tile,
    faceUp: true,
    sideways: false,
    stack: 0,
  }));
  if (called !== undefined) {
    pieces.splice(slotOf(pieces.length + 1), 0, {
      tile: called,
      faceUp: true,
      sideways: true,
      stack: 0,
    });
  }
  return pieces;
}

/**
 * 盤面から牌の置き場所を出す。
 *
 * **Three.js に触らない。**WebGL は自動試験にかけられないので、
 * 「どの牌をどこへ置くか」だけをここで決めて試験する。
 */
export function placementsFor(state: GameState, viewer: Seat = state.you): Placement[] {
  const out: Placement[] = [];
  if (state.round === null) {
    return out;
  }

  for (let absolute = 0; absolute < 4; absolute += 1) {
    const seat = state.seats[absolute];
    if (!seat) {
      continue;
    }
    const relative = relativeSeat(absolute, viewer);
    const facing = seatRotation(relative);
    const mine = absolute === state.you;

    const hand = mine ? state.hand : [];
    const size = mine ? hand.length : seat.handSize;
    const handWidth = size + (mine && state.drawn !== null ? 1 : 0);

    // 手牌と副露を1つの列として数える。**別々に置くと隣席とぶつかる。**
    const laidMelds = seat.melds.map((meld) => layMeld(meld, absolute));
    const meldWidth = laidMelds.reduce(
      (width, pieces) => width + pieces.filter((piece) => piece.stack === 0).length,
      0,
    );
    const total =
      handWidth +
      (meldWidth > 0 ? HAND_MELD_GAP + meldWidth + MELD_GAP * (laidMelds.length - 1) : 0);

    for (let index = 0; index < size; index += 1) {
      out.push({
        key: `hand-${absolute}-${index}`,
        kind: "hand",
        seat: absolute,
        encoded: hand[index] ?? 0,
        position: rowPosition(relative, index, total),
        // 自分の手牌だけ手前へ倒す。他家は伏せているので立てたままでよい。
        rotationX: mine ? TILTED : 0,
        rotationY: facing,
        faceUp: mine,
        pickable: mine,
      });
    }

    if (mine && state.drawn !== null) {
      out.push({
        key: `drawn-${absolute}`,
        kind: "drawn",
        seat: absolute,
        encoded: state.drawn,
        position: rowPosition(relative, size + 0.6, total),
        rotationX: TILTED,
        rotationY: facing,
        faceUp: true,
        pickable: true,
      });
    }

    seat.river.forEach((discarded, index) => {
      // **横に倒した宣言牌は幅が 1.35 になる。**後ろの牌をずらさないと
      // 隣に食い込む。段の幅が広がるぶん、段ごと中央へ寄せ直す。
      const row = Math.floor(index / DISCARDS_PER_ROW);
      const rowStart = row * DISCARDS_PER_ROW;
      let laidBefore = 0;
      for (let k = rowStart; k < index; k += 1) {
        if (seat.river[k]?.riichi) laidBefore += 1;
      }
      let laidInRow = 0;
      for (let k = rowStart; k < rowStart + DISCARDS_PER_ROW; k += 1) {
        if (seat.river[k]?.riichi) laidInRow += 1;
      }
      const shift =
        laidBefore * RIICHI_EXTRA +
        (discarded.riichi ? RIICHI_EXTRA / 2 : 0) -
        (laidInRow * RIICHI_EXTRA) / 2;
      out.push({
        key: `river-${absolute}-${index}`,
        kind: "river",
        seat: absolute,
        encoded: discarded.tile,
        position: discardPosition(relative, index, shift),
        rotationX: LYING,
        rotationY: facing + (discarded.riichi ? SIDEWAYS : 0),
        faceUp: true,
        pickable: false,
      });
    });

    let meldColumn = handWidth + HAND_MELD_GAP;
    laidMelds.forEach((pieces, meldIndex) => {
      let sidewaysColumn = meldColumn;
      pieces.forEach((piece, tileIndex) => {
        // **倒した牌は幅 1.35 を取る。**前後に半分ずつ空けないと食い込む。
        if (piece.stack === 0 && piece.sideways) {
          meldColumn += RIICHI_EXTRA / 2;
          sidewaysColumn = meldColumn;
        }
        const column = piece.stack > 0 ? sidewaysColumn : meldColumn;
        out.push({
          key: `meld-${absolute}-${meldIndex}-${tileIndex}`,
          kind: "meld",
          seat: absolute,
          encoded: piece.faceUp ? piece.tile : 0,
          position: rowPosition(relative, column, total, piece.stack * TILE.depth),
          rotationX: LYING,
          rotationY: facing + (piece.sideways ? SIDEWAYS : 0),
          faceUp: piece.faceUp,
          pickable: false,
        });
        if (piece.stack === 0) {
          meldColumn += piece.sideways ? 1 + RIICHI_EXTRA / 2 : 1;
        }
      });
      meldColumn += MELD_GAP;
    });
  }

  // 山。**中身はサーバから来ない。**残り枚数ぶんの伏せ牌を並べる。
  //
  // **一続きに置き、片端から減らす。**4辺へ1枚ずつ配ると、1枚ツモる
  // たびに4辺が順番に欠けていき、山に見えない。
  for (let index = 0; index < state.wallRemaining; index += 1) {
    const slot = WALL_SLOTS - state.wallRemaining + index;
    const side = Math.floor(slot / SLOTS_PER_SIDE) as Seat;
    // **山も見ている席から見た向きへ直す。**ここだけ絶対席のままだと、
    // 席を変えて見たときに山の欠けだけが動かず、卓の一部が取り残される。
    const relative = relativeSeat(side, viewer);
    out.push({
      key: `wall-${slot}`,
      kind: "wall",
      seat: side,
      encoded: 0,
      position: wallPosition(relative, slot % SLOTS_PER_SIDE),
      rotationX: LYING,
      rotationY: seatRotation(relative),
      faceUp: false,
      pickable: false,
    });
  }

  return out;
}

/** レイキャストが当たった牌。手前のものから並んでいる。 */
export type Hit = { key: string; distance: number };

/**
 * 当たった牌から、押してよい1枚を選ぶ。
 *
 * **手前に押せない牌があっても、その奥の押せる牌を拾ってはいけない。**
 * 河や他家の牌の陰にある手牌を押せると、狙っていない牌が飛ぶ。
 * いちばん手前が押せる牌のときだけ返す。
 */
export function pickFrom(hits: Hit[], placements: Placement[]): Tile | null {
  // 返すのは牌の種類であって、どの1枚を押したかではない。手牌に同じ牌が
  // 2枚あればどちらか区別しないが、エンジンも牌の種類だけで打牌を決める
  // ので困らない。**「押した1枚を指す API ではない」ことに注意。**
  if (hits.length === 0) {
    return null;
  }
  const nearest = [...hits].sort((a, b) => a.distance - b.distance)[0];
  if (!nearest) {
    return null;
  }
  const found = placements.find((placement) => placement.key === nearest.key);
  if (!found || !found.pickable) {
    return null;
  }
  return found.encoded;
}
