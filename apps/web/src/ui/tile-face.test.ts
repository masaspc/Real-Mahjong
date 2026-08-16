import { describe, expect, it } from "vitest";

import { tileFaceSvg } from "./tile-face";

describe("牌の面を描く", () => {
  it("34種すべてに素材がある", () => {
    for (let kind = 0; kind < 34; kind += 1) {
      expect(tileFaceSvg(kind).length).toBeGreaterThan(0);
    }
  });

  it("種類ごとに違う絵である", () => {
    const seen = new Set<string>();
    for (let kind = 0; kind < 34; kind += 1) {
      seen.add(tileFaceSvg(kind));
    }
    // **34種が34通りでなければ、どれかが取り違えられている。**
    expect(seen.size).toBe(34);
  });

  it("範囲外は拒む", () => {
    expect(() => tileFaceSvg(99)).toThrow();
  });

  it("赤ドラは通常の五と違う絵になる", () => {
    expect(tileFaceSvg(34)).not.toEqual(tileFaceSvg(4));
    expect(tileFaceSvg(35)).not.toEqual(tileFaceSvg(13));
    expect(tileFaceSvg(36)).not.toEqual(tileFaceSvg(22));
  });

  it("赤ドラでも牌の形は保たれる", () => {
    // **背景まで赤く塗ると牌に見えない。**元と同じ長さの範囲に収まることで、
    // 丸ごと置き換えていないことを見る。
    for (const [red, plain] of [[34, 4], [35, 13], [36, 22]] as const) {
      const a = tileFaceSvg(red).length;
      const b = tileFaceSvg(plain).length;
      expect(Math.abs(a - b)).toBeLessThan(b * 0.2);
    }
  });
});
