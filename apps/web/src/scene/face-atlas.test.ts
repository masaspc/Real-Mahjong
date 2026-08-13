import { describe, expect, it } from "vitest";

import { ATLAS, BACK_INDEX, BODY_INDEX } from "./atlas";
import { faceDataUrl } from "./face-atlas";

describe("牌面をアトラスへ焼く", () => {
  it("39セルすべてに絵がある", () => {
    for (let cell = 0; cell < ATLAS.cells; cell += 1) {
      expect(faceDataUrl(cell).startsWith("data:image/svg+xml")).toBe(true);
    }
  });

  it("牌の面は種類ごとに違う", () => {
    expect(faceDataUrl(0)).not.toEqual(faceDataUrl(1));
    expect(faceDataUrl(27)).not.toEqual(faceDataUrl(28));
  });

  it("赤ドラは通常の5と違う", () => {
    expect(faceDataUrl(34)).not.toEqual(faceDataUrl(4));
  });

  it("裏面と牌体は面と違う", () => {
    expect(faceDataUrl(BACK_INDEX)).not.toEqual(faceDataUrl(0));
    expect(faceDataUrl(BODY_INDEX)).not.toEqual(faceDataUrl(BACK_INDEX));
  });

  it("範囲外は拒む", () => {
    expect(() => faceDataUrl(ATLAS.cells)).toThrow();
    expect(() => faceDataUrl(-1)).toThrow();
  });
});

