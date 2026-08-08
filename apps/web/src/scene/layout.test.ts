import { describe, expect, it } from "vitest";
import {
  TILE,
  discardPosition,
  handPosition,
  relativeSeat,
  seatRotation,
  wallPosition,
} from "./layout";

describe("relativeSeat", () => {
  it("puts the viewer at zero", () => {
    expect(relativeSeat(2, 2)).toBe(0);
    expect(relativeSeat(3, 2)).toBe(1);
    expect(relativeSeat(0, 2)).toBe(2);
    expect(relativeSeat(1, 2)).toBe(3);
  });
});

describe("seatRotation", () => {
  it("turns a quarter circle per seat", () => {
    expect(seatRotation(0)).toBeCloseTo(0);
    expect(seatRotation(1)).toBeCloseTo(Math.PI / 2);
    expect(seatRotation(2)).toBeCloseTo(Math.PI);
    expect(seatRotation(3)).toBeCloseTo((3 * Math.PI) / 2);
  });
});

describe("handPosition", () => {
  it("lays the viewer's hand along +x, centred on the origin", () => {
    const size = 13;
    const first = handPosition(0, 0, size);
    const last = handPosition(0, size - 1, size);
    expect(first.z).toBeCloseTo(last.z);
    expect(last.x - first.x).toBeCloseTo(TILE.width * (size - 1));
    expect(first.x + last.x).toBeCloseTo(0);
  });

  it("keeps tiles resting on the table", () => {
    expect(handPosition(0, 0, 13).y).toBeGreaterThan(0);
  });

  it("rotates the whole hand for other seats", () => {
    const mine = handPosition(0, 0, 13);
    const across = handPosition(2, 0, 13);
    expect(across.x).toBeCloseTo(-mine.x);
    expect(across.z).toBeCloseTo(-mine.z);
  });
});

describe("discardPosition", () => {
  it("fills six per row before starting the next", () => {
    const a = discardPosition(0, 0);
    const f = discardPosition(0, 5);
    const g = discardPosition(0, 6);
    expect(a.z).toBeCloseTo(f.z);
    expect(g.z).not.toBeCloseTo(a.z);
    expect(g.x).toBeCloseTo(a.x);
  });

  it("grows away from the player row by row", () => {
    const row0 = discardPosition(0, 0);
    const row1 = discardPosition(0, 6);
    const row2 = discardPosition(0, 12);
    expect(Math.abs(row1.z)).toBeLessThan(Math.abs(row0.z));
    expect(Math.abs(row2.z)).toBeLessThan(Math.abs(row1.z));
  });

  it("does not break past the third row", () => {
    const far = discardPosition(0, 23);
    expect(Number.isFinite(far.x)).toBe(true);
    expect(Number.isFinite(far.z)).toBe(true);
  });
});

describe("wallPosition", () => {
  it("stacks two tiles per position", () => {
    const lower = wallPosition(0, 0);
    const upper = wallPosition(0, 1);
    expect(upper.x).toBeCloseTo(lower.x);
    expect(upper.z).toBeCloseTo(lower.z);
    expect(upper.y).toBeGreaterThan(lower.y);
  });

  it("advances along the wall every two tiles", () => {
    const first = wallPosition(0, 0);
    const second = wallPosition(0, 2);
    expect(second.x).not.toBeCloseTo(first.x);
  });
});
