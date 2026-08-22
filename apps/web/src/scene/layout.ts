/**
 * 卓上の座標計算。
 *
 * 卓の中心が原点、卓面が y=0。自席（相対席0）から見て奥が -z、右が +x。
 * 単位は牌の幅を 1 とする。
 *
 * Three.js に依存しない純粋関数にしてある。配置の正しさを描画なしで
 * 検証できるようにするためである。
 */

export type Vec3 = { x: number; y: number; z: number };

/**
 * 牌の寸法比。
 *
 * リーチ麻雀の牌はおよそ 20 x 26 x 16mm である。**厚みを薄くすると板に
 * 見える。**幅の 0.6 倍にしていたが、実物は 0.8 倍ある。
 */
export const TILE = {
  width: 1,
  height: 1.3,
  depth: 0.8,
} as const;

/** 河は6枚で1段。 */
export const DISCARDS_PER_ROW = 6;

/**
 * 卓の広さ（一辺）。
 *
 * **山は一辺17枚ぶんある。**それを中心から 6.2 の位置に置くと、
 * 四辺が卓の角で交差する。17枚を並べる辺どうしがぶつからないためには、
 * 辺の位置が半分の長さ（8.5）より外になければならない。
 */
export const TABLE_SIZE = 26;

/** 山を置く距離。**一辺の半分（8.5）より外。** */
const WALL_Z = 11.5;

/** 手牌と副露を並べる距離。山より内側、河より外側。 */
export const HAND_Z = 10;

/**
 * 河の1段目を置く奥行き。
 *
 * **4段目が中心へ寄りすぎると、隣席の河と重なる。**河は6枚幅なので
 * 中心から 3 より内へ入ってはいけない。4段（5.4）ぶんを 3.05 から
 * 積むと、1段目の外端は 8.45 になり、山（10.825 から）にも
 * 手牌（9.7 から）にも当たらない。
 */
const RIVER_FRONT_Z = 7.925;

/** 河は4段まで。1人が捨てる枚数は多くても21枚ほどで、6枚 x 4段に収まる。 */
export const RIVER_ROWS = 4;

/**
 * 河の1段目を置く奥行き。**中心側の端である。**
 *
 * 段は中心側から始まり、埋まるたびに手前（自分の側）へ降りてくる。
 * 4段目がちょうど `RIVER_FRONT_Z` に来るよう逆算する。
 */
const RIVER_BACK_Z = RIVER_FRONT_Z - (RIVER_ROWS - 1) * TILE.height;

/** 自席から見た相対席。自分が 0、下家が 1、対面が 2、上家が 3。 */
export function relativeSeat(absolute: number, viewer: number): number {
  return (absolute - viewer + 4) % 4;
}

/** 相対席の回転角（ラジアン）。 */
export function seatRotation(seat: number): number {
  return (seat * Math.PI) / 2;
}

/** 原点まわりに y 軸で回す。 */
function rotateY(point: Vec3, radians: number): Vec3 {
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  return {
    x: point.x * cos + point.z * sin,
    y: point.y,
    z: -point.x * sin + point.z * cos,
  };
}

/** 手牌。自席の手前に一列。 */
export function handPosition(seat: number, index: number, handSize: number): Vec3 {
  const spread = TILE.width * (handSize - 1);
  const local: Vec3 = {
    x: index * TILE.width - spread / 2,
    y: TILE.height / 2,
    z: HAND_Z,
  };
  return rotateY(local, seatRotation(seat));
}

/**
 * 河。6枚ごとに1段、段が進むほど卓中央へ寄る。
 *
 * `xShift` は、その段にリーチ宣言牌があるときのずれ。
 * **横に倒した牌は幅が 1.35 になる。**間隔 1 のまま並べると
 * 隣の牌に 0.175 食い込むので、呼ぶ側がずらす量を渡す。
 */
export function discardPosition(seat: number, index: number, xShift = 0): Vec3 {
  const row = Math.floor(index / DISCARDS_PER_ROW);
  const column = index % DISCARDS_PER_ROW;
  const local: Vec3 = {
    x: (column - (DISCARDS_PER_ROW - 1) / 2) * TILE.width + xShift,
    y: TILE.depth / 2,
    // **段は手前へ降りる。**1段目を卓の中心側に置き、埋まるたびに自分の
    // 側へ降りてくる。逆にすると、切るたびに新しい牌が遠ざかっていき、
    // いちばん見たい直前の数枚が卓の真ん中で他家の河と混ざる。
    z: RIVER_BACK_Z + row * TILE.height,
  };
  return rotateY(local, seatRotation(seat));
}

/** 山。2枚で1トン、17トンで一辺。 */
export function wallPosition(seat: number, index: number): Vec3 {
  const stack = Math.floor(index / 2);
  const upper = index % 2 === 1;
  const local: Vec3 = {
    x: (stack - 8) * TILE.width,
    y: TILE.depth / 2 + (upper ? TILE.depth : 0),
    z: WALL_Z,
  };
  return rotateY(local, seatRotation(seat));
}
