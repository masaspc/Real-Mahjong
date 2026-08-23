import { describe, expect, it } from "vitest";

import type { ClientEvent } from "../protocol/ClientEvent";
import { soundOf } from "./catalog";

describe("イベントと音の対応", () => {
  it("打牌の音は牌が着いてから鳴る", () => {
    // **演出の頭で鳴らすと、まだ手元にある牌から音がする。**打牌の演出は
    // 350ms なので、着地に合わせて遅らせる。
    const cue = soundOf({
      type: "discard",
      seat: 0,
      tile: 0,
      manner: "tedashi",
    });
    expect(cue?.name).toBe("clack");
    expect(cue?.delayMs).toBeGreaterThan(0);
    expect(cue?.delayMs).toBeLessThan(350);
  });

  it("宣言の音は演出の頭で鳴る", () => {
    // 鳴きも立直も、宣言そのものが音である。遅らせると牌を倒し終えてから
    // 声が出る。
    expect(
      soundOf({ type: "call", seat: 1, from: 0, kind: "pon", tiles: [0, 0, 0] })
        ?.delayMs,
    ).toBe(0);
    expect(
      soundOf({ type: "riichi", seat: 1, step: "declare" })?.delayMs,
    ).toBe(0);
  });

  it("立直の成立では鳴らさない", () => {
    // **宣言と成立の2件で1つの出来事である。**両方で鳴らすと二度鳴る。
    expect(soundOf({ type: "riichi", seat: 1, step: "accepted" })).toBeNull();
  });

  it("槓は宣言でだけ鳴らす", () => {
    // `kan_declared` が宣言で、`call` は帳簿上の記録。両方で鳴らすと二度鳴る。
    expect(
      soundOf({ type: "kan_declared", seat: 0, kind: "ankan", tile: 0 })?.name,
    ).toBe("call");
    for (const kind of ["ankan", "minkan", "kakan"] as const) {
      expect(
        soundOf({ type: "call", seat: 0, from: 0, kind, tiles: [0, 0, 0, 0] }),
        `${kind} で二度鳴る`,
      ).toBeNull();
    }
  });

  it("局の終わりは和了と流局で音が違う", () => {
    expect(
      soundOf({
        type: "agari",
        results: [],
        settlement: { delta: [0, 0, 0, 0], entries: [] },
      })?.name,
    ).toBe("agari");
    expect(
      soundOf({
        type: "ryuukyoku",
        kind: "exhaustive",
        initiator: null,
        tenpai: [false, false, false, false],
        revealed_hands: [],
        nagashi_winners: [],
        settlement: { delta: [0, 0, 0, 0], entries: [] },
      })?.name,
    ).toBe("ryuukyoku");
  });

  it("音を持たないイベントは null", () => {
    expect(
      soundOf({
        type: "deal",
        your_hand: [],
        hand_sizes: [13, 13, 13, 13],
        dora_indicator: 0,
      }),
    ).toBeNull();
  });
});
