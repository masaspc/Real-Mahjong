import { describe, expect, it } from "vitest";

import { isRed, kindOf, sortTiles, tileLabel } from "./tiles";

describe("牌の表記", () => {
  it("数牌を数字と種類で表す", () => {
    expect(tileLabel(0)).toBe("1m");
    expect(tileLabel(8)).toBe("9m");
    expect(tileLabel(9)).toBe("1p");
    expect(tileLabel(17)).toBe("9p");
    expect(tileLabel(18)).toBe("1s");
    expect(tileLabel(26)).toBe("9s");
  });

  it("字牌を漢字で表す", () => {
    expect(tileLabel(27)).toBe("東");
    expect(tileLabel(28)).toBe("南");
    expect(tileLabel(29)).toBe("西");
    expect(tileLabel(30)).toBe("北");
    expect(tileLabel(31)).toBe("白");
    expect(tileLabel(32)).toBe("發");
    expect(tileLabel(33)).toBe("中");
  });

  it("赤ドラを 0 で表す", () => {
    expect(tileLabel(34)).toBe("0m");
    expect(tileLabel(35)).toBe("0p");
    expect(tileLabel(36)).toBe("0s");
  });

  it("赤ドラを見分ける", () => {
    expect(isRed(34)).toBe(true);
    expect(isRed(35)).toBe(true);
    expect(isRed(36)).toBe(true);
    expect(isRed(4)).toBe(false);
  });

  it("赤ドラは同じ種類の5として扱う", () => {
    expect(kindOf(34)).toBe(kindOf(4));
    expect(kindOf(35)).toBe(kindOf(13));
    expect(kindOf(36)).toBe(kindOf(22));
  });

  it("範囲外を拒む", () => {
    expect(() => tileLabel(37)).toThrow();
    expect(() => tileLabel(-1)).toThrow();
  });

  it("萬子・筒子・索子・字牌の順に並べ、赤ドラは同じ5の位置へ置く", () => {
    // 9m 1m 赤5p 5p 東 1s
    const sorted = sortTiles([8, 0, 35, 13, 27, 18]);
    expect(sorted.map(tileLabel)).toEqual(["1m", "9m", "0p", "5p", "1s", "東"]);
  });

  it("並べ替えは元の配列を壊さない", () => {
    const original = [8, 0];
    sortTiles(original);
    expect(original).toEqual([8, 0]);
  });
});
