import { describe, expect, it } from "vitest";
import { BufferGeometry } from "three";
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
