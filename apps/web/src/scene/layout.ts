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

/** 牌の寸法比。実際の麻雀牌に近い比率。 */
export const TILE = {
  width: 1,
  height: 1.35,
  depth: 0.6,
} as const;

/** 河は6枚で1段。 */
export const DISCARDS_PER_ROW = 6;

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
    z: 7.5,
  };
  return rotateY(local, seatRotation(seat));
}

/**
 * 河の1段目を置く奥行き。
 *
 * **4段目が卓の中央へ届いてはいけない。**寝かせた牌は z 方向に
 * 1.35 を占めるので、4段目が z=0.15 に来ると対面の河と重なる。
 * 一方で1段目を奥へ出しすぎると山（z=6.2）へ食い込む。
 * 4.8 なら4段目が 0.075..1.425、1段目が 4.125..5.475 に収まる。
 */
const RIVER_FRONT_Z = 4.8;

/** 河。6枚ごとに1段、段が進むほど卓中央へ寄る。 */
export function discardPosition(seat: number, index: number): Vec3 {
  const row = Math.floor(index / DISCARDS_PER_ROW);
  const column = index % DISCARDS_PER_ROW;
  const local: Vec3 = {
    x: (column - (DISCARDS_PER_ROW - 1) / 2) * TILE.width,
    y: TILE.depth / 2,
    z: RIVER_FRONT_Z - row * TILE.height,
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
    z: 6.2,
  };
  return rotateY(local, seatRotation(seat));
}
