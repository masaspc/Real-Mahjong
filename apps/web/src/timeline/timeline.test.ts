import { describe, expect, it } from "vitest";
import {
  constant,
  delay,
  easeOutCubic,
  linear,
  parallel,
  sequence,
  tween,
} from "./timeline";

describe("tween", () => {
  it("interpolates between the endpoints", () => {
    const t = tween(0, 100, 400);
    expect(t.seek(0)).toBe(0);
    expect(t.seek(200)).toBe(50);
    expect(t.seek(400)).toBe(100);
  });

  it("clamps outside its duration", () => {
    const t = tween(0, 100, 400);
    expect(t.seek(-50)).toBe(0);
    expect(t.seek(9999)).toBe(100);
  });

  it("applies the easing function", () => {
    const t = tween(0, 100, 100, easeOutCubic);
    // 減速するので中間時点では線形より進んでいる
    expect(t.seek(50)).toBeGreaterThan(50);
    expect(t.seek(0)).toBe(0);
    expect(t.seek(100)).toBe(100);
  });
});

describe("sequence", () => {
  it("plays its parts one after another", () => {
    const t = sequence([tween(0, 10, 100), tween(10, 20, 100)]);
    expect(t.durationMs).toBe(200);
    expect(t.seek(0)).toBe(0);
    expect(t.seek(100)).toBe(10);
    expect(t.seek(150)).toBe(15);
    expect(t.seek(200)).toBe(20);
  });

  it("is seekable to any point regardless of order", () => {
    const t = sequence([tween(0, 10, 100), tween(10, 20, 100)]);
    // 後ろから前へ飛んでも同じ値になる。これがスキップと追いつきを可能にする。
    expect(t.seek(180)).toBe(18);
    expect(t.seek(50)).toBe(5);
    expect(t.seek(180)).toBe(18);
  });

  it("an empty sequence has zero duration", () => {
    expect(sequence([]).durationMs).toBe(0);
  });
});

describe("parallel", () => {
  it("advances every part on the same clock", () => {
    const t = parallel({
      x: tween(0, 100, 200),
      y: tween(0, 50, 100),
    });
    expect(t.durationMs).toBe(200);
    expect(t.seek(100)).toEqual({ x: 50, y: 50 });
    // 短い方は終わった値で止まる
    expect(t.seek(200)).toEqual({ x: 100, y: 50 });
  });
});

describe("constant and delay", () => {
  it("hold a single value for their duration", () => {
    const t = constant("held", 300);
    expect(t.durationMs).toBe(300);
    expect(t.seek(0)).toBe("held");
    expect(t.seek(300)).toBe("held");
  });

  it("delay carries no value", () => {
    expect(delay(350).durationMs).toBe(350);
    expect(delay(350).seek(100)).toBeNull();
  });
});

describe("easing", () => {
  it("is a unit interval mapping", () => {
    for (const ease of [linear, easeOutCubic]) {
      expect(ease(0)).toBeCloseTo(0);
      expect(ease(1)).toBeCloseTo(1);
    }
  });
});
