import { describe, expect, it } from "vitest";

import type { Ruleset } from "../protocol/Ruleset";
import { finalTally } from "./final-tally";

/** 金の間の既定。返し 30,000、ウマ 15/5/-5/-15。 */
const rules: Ruleset = {
  length: "Hanchan",
  start_score: 25_000,
  return_score: 30_000,
  uma: [15, 5, -5, -15],
  red_dora_count: 3,
  kuitan: true,
  double_ron: true,
  formal_tenpai: true,
  noten_penalty: 3_000,
  nagashi_mangan: true,
  liability: true,
  round_up_mangan: false,
  busted_ends_match: true,
  base_think_ms: 5_000,
  think_bank_ms: 20_000,
  network_grace_ms: 500,
  min_reaction_window_ms: 350,
};

describe("終局の順位点", () => {
  it("4人ぶんの合計は必ず 0 になる", () => {
    // **これが崩れたら、どこかで点が湧いているか消えている。**オカを
    // 1位に渡し忘れる、ウマの符号を取り違える、返し点を配給原点と
    // 取り違える——どれもこの1本で捕まる。
    const tally = finalTally(rules, [45_300, 28_700, 21_000, 5_000], [1, 2, 3, 4]);
    const sum = tally.reduce((acc, row) => acc + row.total, 0);
    expect(sum).toBeCloseTo(0, 6);
  });

  it("素点が動かない引き分けでも合計は 0", () => {
    const tally = finalTally(rules, [25_000, 25_000, 25_000, 25_000], [1, 2, 3, 4]);
    expect(tally.reduce((acc, row) => acc + row.total, 0)).toBeCloseTo(0, 6);
    // 1位は -5.0 + 15 + 20 = +30.0
    expect(tally[0]?.total).toBeCloseTo(30, 6);
    // 4位は -5.0 - 15 = -20.0
    expect(tally[3]?.total).toBeCloseTo(-20, 6);
  });

  it("オカは1位だけが受け取る", () => {
    const tally = finalTally(rules, [45_300, 28_700, 21_000, 5_000], [1, 2, 3, 4]);
    expect(tally[0]?.oka).toBe(20);
    for (const row of tally.slice(1)) {
      expect(row.oka).toBe(0);
    }
  });

  it("順位はサーバの決めたものに従う。素点で並べ替えない", () => {
    // **同点の裁定を画面が持ってはいけない。**起家に近い順という規則は
    // サーバの側にあり、こちらが素点で並べ直すと食い違う。
    const tally = finalTally(rules, [25_000, 25_000, 30_000, 20_000], [2, 1, 3, 4]);
    expect(tally.map((row) => row.seat)).toEqual([1, 0, 2, 3]);
    expect(tally[0]?.uma).toBe(15);
    expect(tally[1]?.uma).toBe(5);
  });

  it("返し点とウマは卓のルールから取る", () => {
    // 画面が 30,000 や 15 を定数で持っていたら、ここで落ちる。
    const flat: Ruleset = {
      ...rules,
      start_score: 30_000,
      return_score: 30_000,
      uma: [10, 0, 0, -10],
    };
    const tally = finalTally(flat, [40_000, 30_000, 30_000, 20_000], [1, 2, 3, 4]);
    expect(tally[0]?.oka).toBe(0);
    expect(tally[0]?.total).toBeCloseTo(20, 6);
    expect(tally.reduce((acc, row) => acc + row.total, 0)).toBeCloseTo(0, 6);
  });

  it("端数は丸めない", () => {
    // 丸め方は流儀が分かれるので決めない。素点 32,300 は +2.3 のまま。
    const tally = finalTally(rules, [32_300, 30_000, 20_000, 17_700], [1, 2, 3, 4]);
    expect(tally[0]?.base).toBeCloseTo(2.3, 6);
  });
});
