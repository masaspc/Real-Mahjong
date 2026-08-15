import {
  BufferGeometry,
  ExtrudeGeometry,
  Material,
  MeshStandardMaterial,
  Shape,
  Texture,
} from "three";

import { BACK_INDEX, BODY_INDEX, atlasIndexOf, uvOffsetOf } from "./atlas";
import { TILE } from "./layout";

/**
 * 牌の形。**直方体では麻雀牌に見えない。**
 *
 * 実物は角が丸く、面の縁が落ちていて、白い面板と色のついた背が
 * 貼り合わさっている。角を尖らせたまま絵だけ貼ると、板に絵を貼った物に
 * 見える。
 */

/** 角の丸み。幅に対する比。実物は 20mm の牌で 2mm ほど。 */
const CORNER = 0.1;
/** 面の縁の落とし。ここが無いと絵が板の縁で断ち切られて見える。 */
const BEVEL = 0.06;

/** 角の丸い長方形。 */
function roundedRect(width: number, height: number, radius: number): Shape {
  const shape = new Shape();
  const w = width / 2;
  const h = height / 2;
  shape.moveTo(-w + radius, -h);
  shape.lineTo(w - radius, -h);
  shape.quadraticCurveTo(w, -h, w, -h + radius);
  shape.lineTo(w, h - radius);
  shape.quadraticCurveTo(w, h, w - radius, h);
  shape.lineTo(-w + radius, h);
  shape.quadraticCurveTo(-w, h, -w, h - radius);
  shape.lineTo(-w, -h + radius);
  shape.quadraticCurveTo(-w, -h, -w + radius, -h);
  return shape;
}

/** 牌面が +z を向いた、角の丸い牌。頂点ごとに UV を持つ。 */
export function createTileGeometry(): BufferGeometry {
  // 面取りのぶんを引いて押し出す。合計が `TILE.depth` になる。
  const shape = roundedRect(
    TILE.width - BEVEL * 2,
    TILE.height - BEVEL * 2,
    CORNER,
  );
  const geometry = new ExtrudeGeometry(shape, {
    depth: TILE.depth - BEVEL * 2,
    bevelEnabled: true,
    bevelThickness: BEVEL,
    bevelSize: BEVEL,
    // **分割を増やすと頂点が跳ね上がる。**角の丸みは小さいので、
    // 1段と2分割で十分に丸く見える。4分割だと1枚 708 頂点になり、
    // 136枚の山を置いた時点で描画が持たない。
    bevelSegments: 1,
    curveSegments: 2,
  });
  geometry.center();

  // **前面・背面・側面を頂点の z で仕分ける。**押し出しは面ごとの
  // グループを持たないので、位置から自分で決める。
  //
  // **面取りは1段・2分割なので、頂点は z 方向に4段しか無い。**両端の
  // 面（z=±half）と、そこから面取りぶん内側へ入った側面の境目
  // （z=±(half-BEVEL)）の4段だけで、途中を埋める頂点は無い。しきい値に
  // `BEVEL` を混ぜると面取りの境目まで前面・背面に含んでしまい、後で
  // `applyFaceUv` がその頂点を牌の縁（x/y がセルの端）のまま牌面用の
  // セルへ塗ってしまう。**しきい値は本当の面（z=±half）だけに絞る。**
  const position = geometry.getAttribute("position");
  const half = TILE.depth / 2;
  const front: number[] = [];
  const back: number[] = [];
  const side: number[] = [];
  for (let i = 0; i < position.count; i += 1) {
    const z = position.getZ(i);
    if (z > half - 1e-4) {
      front.push(i);
    } else if (z < -half + 1e-4) {
      back.push(i);
    } else {
      side.push(i);
    }
  }
  geometry.userData["front"] = front;

  // 側面と面取りは牌体（象牙色）。実物は縁も面と同じ象牙色で、背だけ
  // 色が違う。**位置で塗ると縁の頂点がセルの端をつかむので、1点で塗る。**
  paintFlat(geometry, side, BODY_INDEX);
  paint(geometry, back, BACK_INDEX);
  paint(geometry, front, BODY_INDEX);
  return geometry;
}

/**
 * 指定した頂点を、アトラスのセルへ写す。
 *
 * 牌の面は `x`/`y` の位置をそのままセルの中の位置に対応させる。
 * **押し出しの UV は形の座標そのままなので、正規化しないと絵が飛ぶ。**
 */
function paint(
  geometry: BufferGeometry,
  indices: number[],
  cell: number,
): void {
  const uv = geometry.getAttribute("uv");
  if (uv === undefined) {
    throw new Error("uv 属性を持たないジオメトリには適用できない");
  }
  const position = geometry.getAttribute("position");
  const { u, v, du, dv } = uvOffsetOf(cell);
  for (const i of indices) {
    // 牌の中心を原点とした比率へ直す。縁へ寄った頂点は端に張り付く。
    const fx = clamp01(position.getX(i) / TILE.width + 0.5);
    const fy = clamp01(position.getY(i) / TILE.height + 0.5);
    uv.setXY(i, u + fx * du, v + fy * dv);
  }
  uv.needsUpdate = true;
}

function clamp01(value: number): number {
  return value < 0 ? 0 : value > 1 ? 1 : value;
}

/**
 * 指定した頂点を、セルの中心1点へ寄せる。
 *
 * **無地の面を引き伸ばさない。**牌体のセルは一様なので、位置に応じて
 * 広げるとセルの縁の画素を拾い、厚み方向へ筋が出る。
 */
function paintFlat(
  geometry: BufferGeometry,
  indices: number[],
  cell: number,
): void {
  const uv = geometry.getAttribute("uv");
  if (uv === undefined) {
    throw new Error("uv 属性を持たないジオメトリには適用できない");
  }
  const { u, v, du, dv } = uvOffsetOf(cell);
  for (const i of indices) {
    uv.setXY(i, u + du / 2, v + dv / 2);
  }
  uv.needsUpdate = true;
}

/** 牌面（+z）の頂点だけを指定されたアトラスセルへ移す。 */
export function applyFaceUv(
  geometry: BufferGeometry,
  encoded: number,
  faceUp: boolean,
): void {
  const front = geometry.userData["front"];
  if (!Array.isArray(front)) {
    throw new Error("牌のジオメトリではない");
  }
  paint(geometry, front as number[], faceUp ? atlasIndexOf(encoded) : BACK_INDEX);
}

/** 動的シャドウを使わない牌用マテリアル。 */
export function createTileMaterial(atlas: Texture): Material {
  return new MeshStandardMaterial({
    map: atlas,
    // 象牙は少しつやがある。真っ平らだと紙に見える。
    roughness: 0.42,
    metalness: 0,
  });
}
