import { describe, expect, it } from "vitest";

import type { Tile } from "../protocol/Tile";
import { isWinningShape } from "./complete";

/** 記法から牌を作る。`m1` `p5` `s9` `z1`(東) `z5`(白) */
function t(text: string): Tile[] {
  const bases: Record<string, number> = { m: 0, p: 9, s: 18, z: 27 };
  const out: Tile[] = [];
  for (const token of text.split(/\s+/).filter(Boolean)) {
    const suit = token[0];
    const base = suit === undefined ? undefined : bases[suit];
    if (base === undefined) {
      throw new Error(`読めない記法: ${token}`);
    }
    for (const digit of token.slice(1)) {
      out.push((base + Number(digit) - 1) as Tile);
    }
  }
  return out;
}

describe("和了形かどうか", () => {
  it("四面子一雀頭は和了形", () => {
    expect(isWinningShape(t("m123 m456 m789 p22 s111"), 0)).toBe(true);
  });

  it("一枚足りない形は和了形でない", () => {
    expect(isWinningShape(t("m123 m456 m789 p2 s111"), 0)).toBe(false);
  });

  it("雀頭を貪欲に決めると取りこぼす形も拾う", () => {
    // **先頭から雀頭を決め打つと落ちる。**m1 を雀頭にすると残りが
    // 分けきれないが、m4 を雀頭にすれば揃う。
    expect(isWinningShape(t("m111 m234 m44 p567 s789"), 0)).toBe(true);
  });

  it("順子が種類をまたがない", () => {
    // 9萬・1筒・2筒 を順子と数えてはいけない。
    expect(isWinningShape(t("m99 m123 m456 p123 p9 s12"), 0)).toBe(false);
  });

  it("七対子は門前でだけ和了形", () => {
    const hand = t("m11 m22 m33 p44 p55 s66 z11");
    expect(isWinningShape(hand, 0)).toBe(true);
  });

  it("同じ牌4枚は七対子にならない", () => {
    // 2組と数えると4枚使いを許してしまう。
    expect(isWinningShape(t("m1111 m22 m33 p44 p55 s66"), 0)).toBe(false);
  });

  it("国士無双は13種すべてと重なり1枚", () => {
    expect(isWinningShape(t("m19 p19 s19 z1234567 z1"), 0)).toBe(true);
    expect(isWinningShape(t("m19 p19 s19 z1234567 m2"), 0)).toBe(false);
  });

  it("副露している数だけ手の中の面子が減る", () => {
    // 2副露なら手の中は8枚で、二面子と雀頭。
    expect(isWinningShape(t("m123 m456 p77"), 2)).toBe(true);
    expect(isWinningShape(t("m123 m456 p77"), 0)).toBe(false);
  });

  it("副露していると七対子と国士は成立しない", () => {
    expect(isWinningShape(t("m11 m22 m33 p44 p55 s66 z11"), 1)).toBe(false);
  });

  it("赤ドラは同じ種類として数える", () => {
    // 34=赤5萬。5萬（符号4）と同じ扱いにならないと、赤を持った瞬間に
    // 和了形でなくなる。
    const withRed = [...t("m345 m678 p111 s99"), 34 as Tile, 4 as Tile, 4 as Tile];
    expect(withRed).toHaveLength(14);
    expect(isWinningShape(withRed, 0)).toBe(true);
  });
});
