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

  it("distinguishes chi and pon", () => {
    const call = (kind: "chi" | "pon" | "ankan"): ClientEvent => ({
      type: "call",
      seat: 1,
      from: 0,
      kind,
      tiles: [1, 2, 3],
    });
    expect(effectOf(call("chi"))).toBe("chi");
    expect(effectOf(call("pon"))).toBe("pon");
    // 槓の演出は宣言側が持つ。成立側は帳簿上の記録なので null。
    expect(effectOf(call("ankan"))).toBeNull();
  });

  /** 槓の演出は宣言が持つ。槍槓の受付はこの間に入る。 */
  it("gives the kan declaration its own effect time", () => {
    const declared: ClientEvent = {
      type: "kan_declared",
      seat: 2,
      kind: "kakan",
      tile: 13,
    };
    expect(effectOf(declared)).toBe("kan");
  });

  /** 宣言と成立の両方に演出を割り当てると二重計上になる。 */
  it("counts a kan animation once across declaration and completion", () => {
    const declared: ClientEvent = {
      type: "kan_declared",
      seat: 0,
      kind: "kakan",
      tile: 22,
    };
    const completed: ClientEvent = {
      type: "call",
      seat: 0,
      from: 0,
      kind: "kakan",
      tiles: [22],
    };
    const dora: ClientEvent = { type: "dora_reveal", indicator: 27 };
    expect(leadInMs([declared, completed, dora])).toBe(1100 + 800);
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
