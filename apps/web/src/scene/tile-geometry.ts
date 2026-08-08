import {
  BoxGeometry,
  BufferGeometry,
  Material,
  MeshStandardMaterial,
  Texture,
} from "three";

import { BACK_INDEX, atlasIndexOf, uvOffsetOf } from "./atlas";
import { TILE } from "./layout";

/** 牌面が +z を向いた箱。頂点ごとに UV を持つ。 */
export function createTileGeometry(): BufferGeometry {
  return new BoxGeometry(TILE.width, TILE.height, TILE.depth);
}

/** 牌面（+z）の4頂点だけを指定されたアトラスセルへ移す。 */
export function applyFaceUv(
  geometry: BufferGeometry,
  encoded: number,
  faceUp: boolean,
): void {
  const uv = geometry.getAttribute("uv");
  if (uv === undefined) {
    throw new Error("uv 属性を持たないジオメトリには適用できない");
  }

  const cell = faceUp ? atlasIndexOf(encoded) : BACK_INDEX;
  const { u, v, du, dv } = uvOffsetOf(cell);
  const faceStart = 16;
  const corners: [number, number][] = [
    [u, v + dv],
    [u + du, v + dv],
    [u, v],
    [u + du, v],
  ];
  for (let i = 0; i < corners.length; i += 1) {
    const corner = corners[i]!;
    uv.setXY(faceStart + i, corner[0], corner[1]);
  }
  uv.needsUpdate = true;
}

/** 動的シャドウを使わない牌用マテリアル。 */
export function createTileMaterial(atlas: Texture): Material {
  return new MeshStandardMaterial({
    map: atlas,
    roughness: 0.55,
    metalness: 0,
  });
}
