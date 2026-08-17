import { describe, expect, it } from "vitest";

import { tileBackSvg, tileFaceSvg } from "./tile-face";

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

  it("34種すべてに class=\"tile-face\" が付く", () => {
    // **取り込み素材はどれも class を持たない。**`board.css` の
    // `.tile-face { width: 100%; height: auto; ... }` はこのクラスが
    // 付いた要素にしか当たらない。付かないと 2D の盤面で実寸のまま
    // 描かれ、28px の牌の枠からはみ出す。
    for (let kind = 0; kind < 34; kind += 1) {
      expect(tileFaceSvg(kind)).toContain('class="tile-face"');
    }
  });

  it("34種すべてに viewBox が付く", () => {
    // **取り込み素材はどれも viewBox を持たない。**無いと width/height の
    // 実寸でそのまま描かれ、CSS の width:100%/height:auto が効かない。
    for (let kind = 0; kind < 34; kind += 1) {
      expect(tileFaceSvg(kind)).toMatch(/<svg[^>]*\bviewBox="/);
    }
  });

  it("裏向きの牌にも class と viewBox が付く", () => {
    const back = tileBackSvg();
    expect(back).toContain('class="tile-face"');
    expect(back).toMatch(/<svg[^>]*\bviewBox="/);
  });
});
