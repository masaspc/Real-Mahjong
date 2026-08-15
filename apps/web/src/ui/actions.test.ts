import { describe, expect, it } from "vitest";

import type { ActionOption } from "../protocol/ActionOption";
import { emptyState, type GameState } from "../game/state";
import { actionsFor, canDeclareRiichi, discardChoices } from "./actions";

function offering(options: ActionOption[]): GameState {
  return { ...emptyState(0), pending: { windowId: 7, options, deadlineAt: 1000 } };
}

describe("選べる操作からコマンドを組み立てる", () => {
  it("要求が無ければ何も出さない", () => {
    expect(actionsFor(emptyState(0))).toEqual([]);
  });

  it("チーは候補ごとにボタンを出し、call_response で送る", () => {
    const choices = actionsFor(offering([{ type: "chi", candidates: [[0, 1], [1, 3]] }]));
    // 候補2つ + 見送り。
    expect(choices).toHaveLength(3);
    expect(choices[0]?.command).toEqual({
      type: "call_response",
      window_id: 7,
      response: { type: "chi", tiles: [0, 1] },
    });
  });

  it("ポンも call_response で送る", () => {
    const choices = actionsFor(offering([{ type: "pon", candidates: [[4, 4]] }]));
    expect(choices[0]?.command).toEqual({
      type: "call_response",
      window_id: 7,
      response: { type: "pon", tiles: [4, 4] },
    });
  });

  it("大明槓は call_response の kan で送る", () => {
    const choices = actionsFor(offering([{ type: "kan", candidates: [{ type: "minkan" }] }]));
    expect(choices[0]?.command).toEqual({
      type: "call_response",
      window_id: 7,
      response: { type: "kan" },
    });
  });

  it("暗槓は call_response ではなく専用のコマンドで送る", () => {
    // **送り方が違う。**取り違えるとサーバが受け付けない。
    const choices = actionsFor(offering([{ type: "kan", candidates: [{ type: "ankan", kind: 4 }] }]));
    expect(choices[0]?.command).toEqual({ type: "ankan", kind: 4 });
    // **牌は label ではなく tiles で渡す。**文字だけのボタンでは、何を
    // 鳴くのかが画面から分からない。
    expect(choices[0]?.label).toBe("暗槓");
    expect(choices[0]?.tiles).toEqual([4]);
  });

  it("加槓も専用のコマンドで、牌そのものを送る", () => {
    const choices = actionsFor(offering([{ type: "kan", candidates: [{ type: "kakan", tile: 34 }] }]));
    expect(choices[0]?.command).toEqual({ type: "kakan", tile: 34 });
    expect(choices[0]?.label).toBe("加槓");
    expect(choices[0]?.tiles).toEqual([34]);
  });

  it("3種類のカンが同時に出せる", () => {
    const choices = actionsFor(
      offering([
        {
          type: "kan",
          candidates: [{ type: "minkan" }, { type: "ankan", kind: 4 }, { type: "kakan", tile: 9 }],
        },
      ]),
    );
    // 末尾の見送りは別のテストで見る。ここは送り方の違いだけを固定する。
    expect(choices.slice(0, 3).map((c) => c.command.type)).toEqual([
      "call_response",
      "ankan",
      "kakan",
    ]);
  });

  it("ロンは call_response、ツモは専用のコマンド", () => {
    expect(actionsFor(offering([{ type: "ron" }]))[0]?.command).toEqual({
      type: "call_response",
      window_id: 7,
      response: { type: "ron" },
    });
    expect(actionsFor(offering([{ type: "tsumo" }]))[0]?.command).toEqual({ type: "tsumo" });
  });

  it("九種九牌", () => {
    expect(actionsFor(offering([{ type: "kyuushu" }]))[0]?.command).toEqual({ type: "kyuushu" });
  });

  it("反応ウィンドウでは必ず見送れる", () => {
    // **エンジンは ActionOption::Pass を一度も出さない。**それでも
    // CallResponse::Pass は常に受理される。押せないと、鳴きたくない席が
    // 時間切れまで待つことになり、他の3人まで待たされる。
    const choices = actionsFor(offering([{ type: "pon", candidates: [[4, 4]] }]));
    const last = choices[choices.length - 1];
    expect(last?.label).toBe("見送り");
    expect(last?.command).toEqual({
      type: "call_response",
      window_id: 7,
      response: { type: "pass" },
    });
  });

  it("自分の番には見送りを出さない", () => {
    // 打牌を求められているのだから、必ず何かを切る。
    const choices = actionsFor(
      offering([
        { type: "discard", allowed: [0], riichi_allowed: [] },
        { type: "tsumo" },
      ]),
    );
    expect(choices.map((c) => c.label)).toEqual(["ツモ"]);
  });

  it("打てる牌は allowed のものだけ", () => {
    const state = offering([{ type: "discard", allowed: [0, 5, 9], riichi_allowed: [] }]);
    const map = discardChoices(state, false);
    expect([...map.keys()]).toEqual([0, 5, 9]);
    expect(map.get(5)).toEqual({ type: "discard", tile: 5, riichi: false });
  });

  it("リーチ待機なら riichi_allowed のものだけが押せる", () => {
    // **allowed と riichi_allowed は違う。**リーチ後に振れる牌は限られる。
    const state = offering([{ type: "discard", allowed: [0, 5, 9], riichi_allowed: [5] }]);
    const map = discardChoices(state, true);
    expect([...map.keys()]).toEqual([5]);
    expect(map.get(5)).toEqual({ type: "discard", tile: 5, riichi: true });
  });

  it("リーチできるかは riichi_allowed の有無で決まる", () => {
    expect(canDeclareRiichi(offering([{ type: "discard", allowed: [0], riichi_allowed: [] }]))).toBe(false);
    expect(canDeclareRiichi(offering([{ type: "discard", allowed: [0], riichi_allowed: [0] }]))).toBe(true);
  });
});
