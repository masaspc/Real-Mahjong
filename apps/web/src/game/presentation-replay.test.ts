import { describe, expect, it } from "vitest";

import { Presentation } from "./presentation";
import { apply, emptyState } from "./state";
import type { GameState } from "./state";
import { ManualClock } from "../timeline/clock";
import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import seed1 from "./__fixtures__/match-seed1.jsonl?raw";

function envelopes(raw: string): ClientEventEnvelope[] {
  return raw
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => JSON.parse(line) as ClientEventEnvelope);
}

/**
 * 演出を挟まずに畳んだ、答え合わせ用の状態。
 *
 * **受信時刻を揃えて渡す。**`apply` は `request_action` の締切を
 * `nowMs + deadline_ms` で作るので、片方だけ 0 で畳むと `deadlineAt` が
 * 食い違う。牌譜の最後は `match_end` が `pending` を null にするため
 * 最終状態だけ見ると偶然一致してしまい、**検査になっていない。**
 */
function directly(all: ClientEventEnvelope[], times: number[]): GameState {
  let state = emptyState(0);
  all.forEach((envelope, index) => {
    state = apply(state, envelope, times[index] ?? 0);
  });
  return state;
}

describe("演出は盤面を変えない", () => {
  it("半荘を流し切ると、演出なしで畳んだ盤面と一致する", () => {
    const all = envelopes(seed1);
    expect(all.length).toBeGreaterThan(1_000);

    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    const times: number[] = [];
    for (const envelope of all) {
      times.push(clock.now());
      p.receive(envelope);
      // 1件ずつ、最長の演出（リーチ宣言 1,800ms）より長く進める。
      clock.advance(2_000);
      p.update();
    }

    expect(p.state).toEqual(directly(all, times));
    expect(p.pendingMs).toBe(0);
    expect(p.receivedSeq).toBe(all[all.length - 1]?.seq);
  });

  it("1件ごとに、締切まで含めて一致する", () => {
    const all = envelopes(seed1);
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    // **答え合わせ側も1件ずつ進める。**毎回 slice して畳み直すと
    // 1,304件の二乗になり、試験が終わらない。
    let expected = emptyState(0);

    for (const envelope of all) {
      const receivedAt = clock.now();
      p.receive(envelope);
      expected = apply(expected, envelope, receivedAt);
      clock.advance(2_000);
      p.update();
      // 最終状態だけを見ると、`match_end` が `pending` を null にするので
      // 締切の食い違いが消えてしまう。**途中を見ないと検査にならない。**
      expect(p.state).toEqual(expected);
    }
  });

  it("時計を進めずに全部積んで早送りしても、同じ盤面になる", () => {
    const all = envelopes(seed1);
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    const times = all.map(() => 0);
    for (const envelope of all) {
      p.receive(envelope);
    }
    p.skip();
    p.update();

    // **早送りは見せ方であって、結果ではない。**
    expect(p.state).toEqual(directly(all, times));
  });
});
