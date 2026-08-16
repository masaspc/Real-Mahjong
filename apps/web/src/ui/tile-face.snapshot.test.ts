import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";

import { tileFaceSvg, tileBackSvg } from "./tile-face";

/**
 * 牌ごとの絵を短い印にする。
 *
 * **絵そのものを見本にすると差分が読めない。**印だけを見本にすれば、
 * 入れ替わりや取り違えは検出でき、履歴も膨らまない。
 */
function mark(svg: string): string {
  return createHash("sha256").update(svg).digest("hex").slice(0, 12);
}

describe("牌姿一覧", () => {
  it("37種と裏面の絵が、承認された対応から動いていない", () => {
    const sheet: Record<string, string> = {};
    for (let tile = 0; tile <= 36; tile += 1) {
      sheet[String(tile)] = mark(tileFaceSvg(tile));
    }
    sheet["back"] = mark(tileBackSvg());
    // **アトラスの行が反転したときも、牌を取り違えたときも、ここが動く。**
    expect(sheet).toMatchSnapshot();
  });
});
