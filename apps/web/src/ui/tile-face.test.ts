import { describe, expect, it } from "vitest";

import { tileBackSvg, tileFaceSvg } from "./tile-face";

/** 描いた円や竹の数を数える。 */
function count(svg: string, needle: string): number {
  return svg.split(needle).length - 1;
}

describe("牌の面を描く", () => {
  it("37種すべてが SVG になる", () => {
    for (let tile = 0; tile <= 36; tile += 1) {
      const svg = tileFaceSvg(tile);
      expect(svg.startsWith("<svg")).toBe(true);
      expect(svg.endsWith("</svg>")).toBe(true);
    }
  });

  it("萬子は漢数字と萬を描く", () => {
    expect(tileFaceSvg(0)).toContain(">一<");
    expect(tileFaceSvg(8)).toContain(">九<");
    expect(tileFaceSvg(4)).toContain(">萬<");
  });

  it("筒子は枚数ぶんの円を描く", () => {
    // 1p は円1つ、9p は円9つ。
    expect(count(tileFaceSvg(9), 'r="5.1"')).toBe(1);
    expect(count(tileFaceSvg(13), 'r="5.1"')).toBe(5);
    expect(count(tileFaceSvg(17), 'r="5.1"')).toBe(9);
  });

  it("索子は枚数ぶんの竹を描く", () => {
    // 竹の幹は rx="2.6" の角丸。1本につき1つ。
    // 竹1本につき節（白い横棒）が1つ。
    expect(count(tileFaceSvg(19), 'height="1.2"')).toBe(2);
    expect(count(tileFaceSvg(22), 'height="1.2"')).toBe(5);
    expect(count(tileFaceSvg(26), 'height="1.2"')).toBe(9);
  });

  it("1索は竹ではなく鳥を描く", () => {
    // **竹1本だと2索の半分に見えて紛らわしい。**
    const svg = tileFaceSvg(18);
    expect(svg).toContain("<ellipse");
    expect(count(svg, "<rect")).toBe(1);
  });

  it("字牌は東南西北發中を描き、白は枠だけにする", () => {
    expect(tileFaceSvg(27)).toContain(">東<");
    expect(tileFaceSvg(28)).toContain(">南<");
    expect(tileFaceSvg(29)).toContain(">西<");
    expect(tileFaceSvg(30)).toContain(">北<");
    // 白は字を書かない。
    expect(tileFaceSvg(31)).not.toContain("<text");
    expect(tileFaceSvg(32)).toContain(">發<");
    expect(tileFaceSvg(33)).toContain(">中<");
  });

  it("發は緑、中は赤で描く", () => {
    expect(tileFaceSvg(32)).toContain("#1f7a44");
    expect(tileFaceSvg(33)).toContain("#c0392b");
  });

  it("赤ドラは通常の5と描き分ける", () => {
    // **同じ5でも赤かどうかが見て分かること。**
    expect(tileFaceSvg(34)).not.toEqual(tileFaceSvg(4));
    expect(tileFaceSvg(35)).not.toEqual(tileFaceSvg(13));
    expect(tileFaceSvg(36)).not.toEqual(tileFaceSvg(22));
  });

  it("赤ドラも枚数は5のまま", () => {
    expect(count(tileFaceSvg(35), 'r="5.1"')).toBe(5);
    expect(count(tileFaceSvg(36), 'height="1.2"')).toBe(5);
    expect(tileFaceSvg(34)).toContain(">五<");
  });

  it("裏面は面と違う", () => {
    expect(tileBackSvg()).not.toEqual(tileFaceSvg(0));
    expect(tileBackSvg()).toContain("<svg");
  });
});
