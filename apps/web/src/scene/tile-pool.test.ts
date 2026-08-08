import { describe, expect, it } from "vitest";
import { TilePool } from "./tile-pool";

describe("TilePool", () => {
  it("hands out a tile and tracks it as in use", () => {
    const pool = new TilePool();
    const tile = pool.acquire(5);
    expect(tile.encoded).toBe(5);
    expect(pool.inUse).toBe(1);
    expect(pool.active).toContain(tile);
  });

  it("reuses a released tile instead of growing", () => {
    const pool = new TilePool();
    const first = pool.acquire(1);
    const firstId = first.id;
    pool.release(first);
    expect(pool.inUse).toBe(0);

    const second = pool.acquire(2);
    expect(second.id).toBe(firstId);
    expect(second.encoded).toBe(2);
    expect(pool.size).toBe(1);
  });

  it("grows only when every tile is in use", () => {
    const pool = new TilePool();
    const a = pool.acquire(1);
    const b = pool.acquire(2);
    expect(pool.size).toBe(2);
    expect(a.id).not.toBe(b.id);
  });

  it("stays small for a realistic table", () => {
    const pool = new TilePool();
    const held = [];
    for (let i = 0; i < 50; i += 1) {
      held.push(pool.acquire(i % 34));
    }
    expect(pool.size).toBe(50);
    for (const tile of held) {
      pool.release(tile);
    }
    expect(pool.inUse).toBe(0);
    expect(pool.size).toBe(50);
  });

  it("refuses to release a tile twice", () => {
    const pool = new TilePool();
    const tile = pool.acquire(1);
    pool.release(tile);
    expect(() => pool.release(tile)).toThrow();
  });

  it("resets presentation state when acquired", () => {
    const pool = new TilePool();
    const first = pool.acquire(1);
    first.position = { x: 9, y: 9, z: 9 };
    first.rotationY = 1.5;
    first.faceUp = false;
    pool.release(first);

    const second = pool.acquire(2);
    expect(second.position).toEqual({ x: 0, y: 0, z: 0 });
    expect(second.rotationY).toBe(0);
    expect(second.faceUp).toBe(true);
  });
});
