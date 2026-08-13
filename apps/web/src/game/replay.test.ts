import { describe, expect, it } from "vitest";

// **`?raw` で読む。**`node:fs` を使うと `@types/node` が要る。
import seed1 from "./__fixtures__/match-seed1.jsonl?raw";
import seed3 from "./__fixtures__/match-seed3.jsonl?raw";
import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import { apply, emptyState } from "./state";

const FIXTURES: Record<string, string> = {
  "match-seed1.jsonl": seed1,
  "match-seed3.jsonl": seed3,
};

function load(name: string): ClientEventEnvelope[] {
  const text = FIXTURES[name];
  if (text === undefined) {
    throw new Error(`牌譜が無い: ${name}`);
  }
  return text
    .trim()
    .split("\n")
    .map((line: string) => JSON.parse(line) as ClientEventEnvelope);
}

/** 副露1つにつき手の中の3枚ぶんが外へ出る。 */
function concealedTarget(melds: number): number {
  return 13 - melds * 3;
}

describe.each(["match-seed1.jsonl", "match-seed3.jsonl"])("実際の半荘 %s を畳む", (file) => {
  it("自分の持ち牌が常に整合する", () => {
    const events = load(file);
    let state = emptyState(0);
    const problems: string[] = [];

    for (const envelope of events) {
      state = apply(state, envelope, 0);
      if (state.hand.length === 0) continue;
      const concealed = state.hand.length + (state.drawn === null ? 0 : 1);
      const target = concealedTarget(state.seats[0]?.melds.length ?? 0);
      // ツモってから切るまでは1枚多い。
      if (concealed !== target && concealed !== target + 1) {
        problems.push(`seq=${envelope.seq} ${envelope.event.type}: 持ち牌${concealed} 期待${target}か${target + 1}`);
      }
    }
    expect(problems.slice(0, 5)).toEqual([]);
  });

  it("他家の枚数が常に整合する", () => {
    const events = load(file);
    let state = emptyState(0);
    const problems: string[] = [];

    for (const envelope of events) {
      state = apply(state, envelope, 0);
      for (let i = 1; i < 4; i += 1) {
        const seat = state.seats[i];
        if (!seat || seat.handSize === 0) continue;
        const target = concealedTarget(seat.melds.length);
        if (seat.handSize !== target && seat.handSize !== target + 1) {
          problems.push(`seq=${envelope.seq} ${envelope.event.type}: 席${i} ${seat.handSize}枚 期待${target}か${target + 1}`);
        }
      }
    }
    expect(problems.slice(0, 5)).toEqual([]);
  });
});
