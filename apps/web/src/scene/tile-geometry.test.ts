import { describe, expect, it } from "vitest";
import { BufferGeometry } from "three";
import { BACK_INDEX, BODY_INDEX, uvOffsetOf } from "./atlas";
import { TILE } from "./layout";
import { applyFaceUv, createTileGeometry } from "./tile-geometry";

describe("createTileGeometry", () => {
  it("has the declared proportions", () => {
    const geometry = createTileGeometry();
    geometry.computeBoundingBox();
    const box = geometry.boundingBox;
    expect(box).not.toBeNull();
    const size = box!.max.clone().sub(box!.min);
    expect(size.x).toBeCloseTo(TILE.width, 5);
    expect(size.y).toBeCloseTo(TILE.height, 5);
    expect(size.z).toBeCloseTo(TILE.depth, 5);
  });

  it("carries uv attributes for every vertex", () => {
    const geometry = createTileGeometry();
    const position = geometry.getAttribute("position");
    const uv = geometry.getAttribute("uv");
    expect(uv).toBeDefined();
    expect(uv.count).toBe(position.count);
  });

  it("面と側面は牌体、背は裏面の色になる", () => {
    // **実物は白い面板と色のついた背が貼り合わさっている。**全部を同じ色に
    // すると、どちらから見ても同じ板に見える。
    const geometry = createTileGeometry();
    const uv = geometry.getAttribute("uv");
    const position = geometry.getAttribute("position");
    const body = uvOffsetOf(BODY_INDEX);
    const back = uvOffsetOf(BACK_INDEX);
    // **UV は Float32 で持たれる。**セルの境目にぴったり乗る値は倍精度の
    // ままでは表せない。丸め1つぶんだけ緩める。
    const slack = 1e-6;
    const inside = (i: number, cell: { u: number; v: number; du: number; dv: number }) =>
      uv.getX(i) >= cell.u - slack &&
      uv.getX(i) <= cell.u + cell.du + slack &&
      uv.getY(i) >= cell.v - slack &&
      uv.getY(i) <= cell.v + cell.dv + slack;

    let backCount = 0;
    for (let i = 0; i < uv.count; i += 1) {
      if (position.getZ(i) < -TILE.depth / 2 + 0.07) {
        expect(inside(i, back), `背の頂点 ${i} が裏面のセルに無い`).toBe(true);
        backCount += 1;
      } else {
        expect(inside(i, body), `面か側面の頂点 ${i} が牌体のセルに無い`).toBe(true);
      }
    }
    expect(backCount).toBeGreaterThan(0);
  });

  it("角が丸い", () => {
    // **直方体では麻雀牌に見えない。**角の頂点が、四隅の直角の位置よりも
    // 内側に無ければ丸まっていない。
    const geometry = createTileGeometry();
    const position = geometry.getAttribute("position");
    let corners = 0;
    for (let i = 0; i < position.count; i += 1) {
      const x = Math.abs(position.getX(i));
      const y = Math.abs(position.getY(i));
      if (x > TILE.width / 2 - 1e-6 && y > TILE.height / 2 - 1e-6) {
        corners += 1;
      }
    }
    expect(corners).toBe(0);
  });
});

describe("applyFaceUv", () => {
  it("moves the face uv into the requested atlas cell", () => {
    const geometry = createTileGeometry();
    applyFaceUv(geometry, 0, true);
    const first = Array.from(
      (geometry.getAttribute("uv") as { array: ArrayLike<number> }).array,
    );

    applyFaceUv(geometry, 20, true);
    const second = Array.from(
      (geometry.getAttribute("uv") as { array: ArrayLike<number> }).array,
    );
    expect(second).not.toEqual(first);
  });

  it("uses the back cell when the tile is face down", () => {
    const geometry = createTileGeometry();
    applyFaceUv(geometry, 5, true);
    const up = Array.from(
      (geometry.getAttribute("uv") as { array: ArrayLike<number> }).array,
    );

    applyFaceUv(geometry, 5, false);
    const down = Array.from(
      (geometry.getAttribute("uv") as { array: ArrayLike<number> }).array,
    );
    expect(down).not.toEqual(up);
  });

  it("rejects a geometry without uv", () => {
    const bare = new BufferGeometry();
    expect(() => applyFaceUv(bare, 0, true)).toThrow();
  });
});
