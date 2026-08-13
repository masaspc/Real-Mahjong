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

  /**
   * **画像として読ませる以上、名前空間の宣言が要る。**
   *
   * `innerHTML` へ入れるだけなら省いても描けるので、2D の盤面では気付け
   * ない。`data:` URL の SVG は独立した文書として解析されるため、宣言が
   * 無いと SVG と見なされず読み込みに失敗する。`paintAtlas` は 39 枚を
   * `Promise.all` で待つので、**1枚欠けるだけで牌が全部、仮の文字ラベル
   * のまま出る。**
   */
  it("どのセルも SVG の名前空間を宣言している", () => {
    for (let cell = 0; cell < ATLAS.cells; cell += 1) {
      expect(decodeURIComponent(faceDataUrl(cell))).toContain(
        'xmlns="http://www.w3.org/2000/svg"',
      );
    }
  });

  it("範囲外は拒む", () => {
    expect(() => faceDataUrl(ATLAS.cells)).toThrow();
    expect(() => faceDataUrl(-1)).toThrow();
  });
});

