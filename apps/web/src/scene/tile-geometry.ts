import {
  BoxGeometry,
  BufferGeometry,
  Material,
  MeshStandardMaterial,
  Texture,
} from "three";

import { BACK_INDEX, BODY_INDEX, atlasIndexOf, uvOffsetOf } from "./atlas";
import { TILE } from "./layout";

/** 牌面が +z を向いた箱。頂点ごとに UV を持つ。 */
export function createTileGeometry(): BufferGeometry {
  const geometry = new BoxGeometry(TILE.width, TILE.height, TILE.depth);
  fillFaceUv(geometry, 0, 24, BODY_INDEX);
  return geometry;
}

function fillFaceUv(
  geometry: BufferGeometry,
  start: number,
  count: number,
  cell: number,
): void {
  const uv = geometry.getAttribute("uv");
  if (uv === undefined) {
    throw new Error("uv 属性を持たないジオメトリには適用できない");
  }
  const { u, v, du, dv } = uvOffsetOf(cell);
  const corners: [number, number][] = [
    [u, v + dv],
    [u + du, v + dv],
    [u, v],
    [u + du, v],
  ];
  for (let i = 0; i < count; i += 1) {
    const corner = corners[i % 4]!;
    uv.setXY(start + i, corner[0], corner[1]);
  }
  uv.needsUpdate = true;
}

/** 牌面（+z）の4頂点だけを指定されたアトラスセルへ移す。 */
export function applyFaceUv(
  geometry: BufferGeometry,
  encoded: number,
  faceUp: boolean,
): void {
  const cell = faceUp ? atlasIndexOf(encoded) : BACK_INDEX;
  fillFaceUv(geometry, 16, 4, cell);
}

/** 動的シャドウを使わない牌用マテリアル。 */
export function createTileMaterial(atlas: Texture): Material {
  return new MeshStandardMaterial({
    map: atlas,
    roughness: 0.55,
    metalness: 0,
  });
}
