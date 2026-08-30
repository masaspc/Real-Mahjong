// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";

import { emptyState, type GameState } from "../game/state";
import { renderBoard } from "./board";

let root: HTMLElement;

function stateWith(patch: Partial<GameState>): GameState {
  return { ...emptyState(0), ...patch };
}

beforeEach(() => {
  document.body.replaceChildren();
  root = document.createElement("div");
  document.body.append(root);
});

describe("盤面の呼び名", () => {
  it("点棒の行に名前が出る", () => {
    renderBoard(
      root,
      stateWith({ players: ["まさ", "たろう", "CPU1", "CPU2"] }),
      () => {},
    );
    const scores = [...root.querySelectorAll(".score")].map((n) => n.textContent);
    expect(scores[0]).toContain("まさ");
    expect(scores.join(" ")).toContain("たろう");
    expect(scores.join(" ")).not.toContain("席0");
  });

  it("名前が届く前は席番号のまま出る", () => {
    // **空欄にしない。**MatchStart より前に描く瞬間がある。
    renderBoard(root, stateWith({}), () => {});
    expect(root.querySelector(".score")?.textContent).toContain("席0");
  });

  it("手番の一行も名前で言う", () => {
    renderBoard(
      root,
      stateWith({ players: ["まさ", "たろう", "CPU1", "CPU2"], turn: 2 }),
      () => {},
    );
    expect(root.querySelector(".turn")?.textContent).toBe("CPU1 を待っています");
  });

  /**
   * **名前は他人が入力した文字列である。**待合と同じく、盤面でも印には
   * ならないことを見張る。
   */
  it("名前は文字として入る。印にはならない", () => {
    const attack = "<img src=x onerror=alert(1)>";
    renderBoard(root, stateWith({ players: [attack, "b", "c", "d"] }), () => {});
    expect(root.querySelectorAll("img").length).toBe(0);
    expect(root.querySelector(".score")?.textContent).toContain(attack);
  });
});
