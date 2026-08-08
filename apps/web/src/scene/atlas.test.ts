import { describe, expect, it } from "vitest";
import {
  ATLAS,
  BACK_INDEX,
  BODY_INDEX,
  atlasIndexOf,
  uvOffsetOf,
} from "./atlas";

describe("atlasIndexOf", () => {
  it("maps every encoded tile to its own cell", () => {
    const seen = new Set<number>();
    for (let encoded = 0; encoded <= 36; encoded += 1) {
      const cell = atlasIndexOf(encoded);
      expect(cell).toBeGreaterThanOrEqual(0);
      expect(cell).toBeLessThan(ATLAS.cells);
      expect(seen.has(cell)).toBe(false);
      seen.add(cell);
    }
    expect(seen.size).toBe(37);
  });

  it("gives the back face and body their own cells", () => {
    expect(BACK_INDEX).not.toBe(BODY_INDEX);
    for (const cell of [BACK_INDEX, BODY_INDEX]) {
      expect(cell).toBeGreaterThanOrEqual(0);
      expect(cell).toBeLessThan(ATLAS.cells);
    }
    for (let encoded = 0; encoded <= 36; encoded += 1) {
      expect(atlasIndexOf(encoded)).not.toBe(BACK_INDEX);
      expect(atlasIndexOf(encoded)).not.toBe(BODY_INDEX);
    }
  });

  it("rejects tiles outside the encoding", () => {
    expect(() => atlasIndexOf(37)).toThrow();
    expect(() => atlasIndexOf(-1)).toThrow();
  });
});

describe("uvOffsetOf", () => {
  it("returns a cell inside the unit square", () => {
    for (let cell = 0; cell < ATLAS.cells; cell += 1) {
      const uv = uvOffsetOf(cell);
      expect(uv.u).toBeGreaterThanOrEqual(0);
      expect(uv.v).toBeGreaterThanOrEqual(0);
      expect(uv.u + uv.du).toBeLessThanOrEqual(1 + 1e-9);
      expect(uv.v + uv.dv).toBeLessThanOrEqual(1 + 1e-9);
    }
  });

  it("gives every cell the same size", () => {
    const a = uvOffsetOf(0);
    const b = uvOffsetOf(ATLAS.cells - 1);
    expect(a.du).toBeCloseTo(b.du);
    expect(a.dv).toBeCloseTo(b.dv);
    expect(a.du).toBeCloseTo(1 / ATLAS.columns);
    expect(a.dv).toBeCloseTo(1 / ATLAS.rows);
  });

  it("moves right then down", () => {
    const first = uvOffsetOf(0);
    const next = uvOffsetOf(1);
    const wrapped = uvOffsetOf(ATLAS.columns);
    expect(next.u).toBeGreaterThan(first.u);
    expect(next.v).toBeCloseTo(first.v);
    expect(wrapped.u).toBeCloseTo(first.u);
    expect(wrapped.v).not.toBeCloseTo(first.v);
  });
});
