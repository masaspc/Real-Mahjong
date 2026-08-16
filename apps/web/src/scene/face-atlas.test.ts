import { describe, expect, it } from "vitest";

import { ATLAS, BACK_INDEX, BODY_INDEX } from "./atlas";
import { BODY_FILL_COLOR, faceDataUrl, paintAtlasOn } from "./face-atlas";
import type { Surface } from "./face-atlas";

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

/**
 * **`paintAtlasOn` は、下地を消してから描くという約束を守らなければならない。**
 *
 * 取り込んだ牌図（`load` が返す画像）は背景が透明な SVG である。セルを塗り
 * つぶさずに `drawImage` だけを呼ぶと、そのセルに前から残っていた仮描画
 * （`drawPlaceholderAtlas` の文字ラベル）が透けて残り、牌図と文字ラベルが
 * 二重写しになる（実際に起きていた不良）。
 *
 * WebGL や実際の canvas 描画は要らない。`paintAtlasOn` が受け取る `Surface` は
 * `fillRect` / `drawImage` / `fillStyle` だけの窓口なので、呼び出しを記録する
 * 偽物を渡せば、**画素を持ち出さずに「順序」「色」「位置と大きさの一致」を
 * 確かめられる。**
 */
describe("paintAtlasOn: セルを塗ってから描く", () => {
  type RecordedCall =
    | { kind: "fillRect"; fillStyle: string; x: number; y: number; w: number; h: number }
    | { kind: "drawImage"; x: number; y: number; w: number; h: number };

  function createRecordingSurface(): { surface: Surface; calls: RecordedCall[] } {
    const calls: RecordedCall[] = [];
    const surface: Surface = {
      fillStyle: "",
      fillRect(x, y, w, h) {
        // `paintAtlasOn` は文字列の色しか渡さない。`Surface.fillStyle` は
        // `CanvasRenderingContext2D` に合わせた広い型なので、記録用にここで
        // 文字列へ寄せる。
        calls.push({
          kind: "fillRect",
          fillStyle: String(surface.fillStyle),
          x,
          y,
          w,
          h,
        });
      },
      drawImage(_image, x, y, w, h) {
        calls.push({ kind: "drawImage", x, y, w, h });
      },
    };
    return { surface, calls };
  }

  // `paintAtlasOn` は画像の中身を見ないので、位置合わせの検査には不透明な
  // マーカーで足りる。
  const DUMMY_IMAGE = {} as CanvasImageSource;
  const CELL_W = 37;
  const CELL_H = 41;

  it("39セルすべてで fillRect が drawImage より先に呼ばれる", () => {
    const { surface, calls } = createRecordingSurface();
    const images = Array.from({ length: ATLAS.cells }, () => DUMMY_IMAGE);

    paintAtlasOn(surface, CELL_W, CELL_H, images);

    expect(calls.length).toBe(ATLAS.cells * 2);
    for (let cell = 0; cell < ATLAS.cells; cell += 1) {
      const fill = calls[cell * 2];
      const draw = calls[cell * 2 + 1];
      expect(fill?.kind).toBe("fillRect");
      expect(draw?.kind).toBe("drawImage");
    }
  });

  it("fillRect の色は牌体の地の色（bodySvg と同じ値）である", () => {
    const { surface, calls } = createRecordingSurface();
    const images = Array.from({ length: ATLAS.cells }, () => DUMMY_IMAGE);

    paintAtlasOn(surface, CELL_W, CELL_H, images);

    const fillCalls = calls.filter(
      (call): call is Extract<RecordedCall, { kind: "fillRect" }> =>
        call.kind === "fillRect",
    );
    expect(fillCalls.length).toBe(ATLAS.cells);
    for (const call of fillCalls) {
      expect(call.fillStyle).toBe(BODY_FILL_COLOR);
    }
  });

  it("fillRect と drawImage は同じセルで同じ位置・同じ大きさになる", () => {
    const { surface, calls } = createRecordingSurface();
    const images = Array.from({ length: ATLAS.cells }, () => DUMMY_IMAGE);

    paintAtlasOn(surface, CELL_W, CELL_H, images);

    for (let cell = 0; cell < ATLAS.cells; cell += 1) {
      const fill = calls[cell * 2];
      const draw = calls[cell * 2 + 1];
      if (fill?.kind !== "fillRect" || draw?.kind !== "drawImage") {
        throw new Error("呼び出しの順序が想定と違う");
      }

      const column = cell % ATLAS.columns;
      const row = Math.floor(cell / ATLAS.columns);
      const expectedX = column * CELL_W;
      const expectedY = row * CELL_H;

      expect(fill.x).toBe(expectedX);
      expect(fill.y).toBe(expectedY);
      expect(fill.w).toBe(CELL_W);
      expect(fill.h).toBe(CELL_H);

      expect(draw.x).toBe(fill.x);
      expect(draw.y).toBe(fill.y);
      expect(draw.w).toBe(fill.w);
      expect(draw.h).toBe(fill.h);
    }
  });
});

