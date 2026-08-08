import { describe, expect, it } from "vitest";
import type { ClientEvent } from "../protocol/ClientEvent";
import { effectDurationMs, effectOf, leadInMs } from "./catalog";

const discard: ClientEvent = {
  type: "discard",
  seat: 1,
  tile: 3,
  manner: "tedashi",
};

const riichiDeclare: ClientEvent = {
  type: "riichi",
  seat: 1,
  step: "declare",
};

const riichiAccepted: ClientEvent = {
  type: "riichi",
  seat: 1,
  step: "accepted",
};

describe("effect catalog", () => {
  /** Rust 側 protocol::effect と同じ値でなければならない。 */
  it("matches the values frozen in protocol", () => {
    expect(effectDurationMs("draw")).toBe(250);
    expect(effectDurationMs("discard")).toBe(350);
    expect(effectDurationMs("pon")).toBe(700);
    expect(effectDurationMs("chi")).toBe(700);
    expect(effectDurationMs("kan")).toBe(1100);
    expect(effectDurationMs("riichi_declare")).toBe(1800);
    expect(effectDurationMs("dora_reveal")).toBe(800);
  });

  it("maps events to their effect", () => {
    expect(effectOf(discard)).toBe("discard");
    expect(effectOf(riichiDeclare)).toBe("riichi_declare");
    // 成立側は点棒の移動のみで進行を止めない
    expect(effectOf(riichiAccepted)).toBeNull();
  });

  it("distinguishes chi, pon and kan", () => {
    const call = (kind: "chi" | "pon" | "ankan"): ClientEvent => ({
      type: "call",
      seat: 1,
      from: 0,
      kind,
      tiles: [1, 2, 3],
    });
    expect(effectOf(call("chi"))).toBe("chi");
    expect(effectOf(call("pon"))).toBe("pon");
    expect(effectOf(call("ankan"))).toBe("kan");
  });

  /** 加槓の宣言も槓の演出時間を持つ。槍槓の受付はこの間に入る。 */
  it("gives the kan declaration its own effect time", () => {
    const declared: ClientEvent = {
      type: "kan_declared",
      seat: 2,
      kind: "kakan",
      tile: 13,
    };
    expect(effectOf(declared)).toBe("kan");
  });

  it("bookkeeping events carry no effect time", () => {
    const passed: ClientEvent = {
      type: "action_passed",
      seat: 2,
      window_id: 1,
    };
    expect(effectOf(passed)).toBeNull();
    expect(leadInMs([passed])).toBe(0);
  });

  it("sums only the events that have effects", () => {
    expect(leadInMs([riichiDeclare, discard, riichiAccepted])).toBe(1800 + 350);
  });

  it("an empty list has no lead in", () => {
    expect(leadInMs([])).toBe(0);
  });
});
