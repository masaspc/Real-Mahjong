// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";

import { emptyState } from "../game/state";
import type { RecordCard } from "./api";
import {
  outcomeLabel,
  RATES,
  renderRecordList,
  renderReplayBar,
  whenLabel,
} from "./screen";

let root: HTMLElement;

beforeEach(() => {
  document.body.replaceChildren();
  root = document.createElement("div");
  document.body.append(root);
});

function card(patch: Partial<RecordCard> = {}): RecordCard {
  return {
    id: "abc",
    players: ["まさ", "たろう", "CPU1", "CPU2"],
    started_ms: Date.UTC(2026, 7, 30, 3, 4),
    ended_ms: Date.UTC(2026, 7, 30, 3, 40),
    result: null,
    ...patch,
  };
}

describe("いつ打ったか", () => {
  it("同じ年なら年を省く", () => {
    const at = new Date(2026, 7, 30, 15, 4).getTime();
    const now = new Date(2026, 11, 1).getTime();
    expect(whenLabel(at, now)).toBe("8/30 15:04");
  });

  it("年をまたいだら年から出す", () => {
    // **省いたままだと、去年の対局が今年のものに見える。**
    const at = new Date(2025, 0, 2, 9, 5).getTime();
    const now = new Date(2026, 11, 1).getTime();
    expect(whenLabel(at, now)).toBe("2025/1/2 09:05");
  });
});

describe("どう終わったか", () => {
  it("終局と途中を言い分ける", () => {
    expect(outcomeLabel(card())).toBe("終局");
    expect(outcomeLabel(card({ ended_ms: null }))).toBe("途中まで");
  });
});

describe("牌譜の一覧", () => {
  const handlers = { open: () => {}, back: () => {} };
  const now = Date.UTC(2026, 7, 31);

  it("並んだぶんだけ行が出る", () => {
    renderRecordList(root, [card(), card({ id: "def" })], handlers, now);
    expect(root.querySelectorAll(".record-item").length).toBe(2);
  });

  /** **空を黙って出さない。**まだ打っていないのか、鍵が変わったのかを言う。 */
  it("1件も無ければ、その旨を言う", () => {
    renderRecordList(root, [], handlers, now);
    expect(root.querySelector(".records-empty")?.textContent).toContain("まだ");
    expect(root.querySelectorAll(".record-item").length).toBe(0);
  });

  it("押すとその対局を開く", () => {
    const opened: string[] = [];
    renderRecordList(root, [card({ id: "xyz" })], { ...handlers, open: (id) => opened.push(id) }, now);
    root.querySelector<HTMLElement>(".record-open")?.dispatchEvent(new Event("click"));
    expect(opened).toEqual(["xyz"]);
  });

  /** 名前は他人が入力した文字列である。印にはならない。 */
  it("名前は文字として入る", () => {
    const attack = "<img src=x onerror=alert(1)>";
    renderRecordList(root, [card({ players: [attack, "b", "c", "d"] })], handlers, now);
    expect(root.querySelectorAll("img").length).toBe(0);
    expect(root.querySelector(".record-players")?.textContent).toContain(attack);
  });
});

describe("再生の操作盤", () => {
  const handlers = { setRate: () => {}, seek: () => {}, back: () => {} };
  const view = {
    roundStarts: [1, 120, 260],
    rate: 1,
    fed: 130,
    total: 400,
    state: { ...emptyState(2), players: ["a", "b", "わたし", "d"] },
  };

  it("局の頭が目次になる", () => {
    renderReplayBar(root, view, handlers);
    const rounds = [...root.querySelectorAll(".replay-round")];
    expect(rounds.map((r) => r.textContent)).toEqual(["1局目", "2局目", "3局目"]);
  });

  it("通り過ぎた局に印が付く", () => {
    // **いまどこにいるかが分からないと、戻る先を選べない。**
    renderReplayBar(root, view, handlers);
    const rounds = [...root.querySelectorAll(".replay-round")];
    expect(rounds[0]?.classList.contains("passed")).toBe(true);
    expect(rounds[1]?.classList.contains("passed")).toBe(true);
    expect(rounds[2]?.classList.contains("passed")).toBe(false);
  });

  it("押した局の位置を渡す", () => {
    const sought: number[] = [];
    renderReplayBar(root, view, { ...handlers, seek: (index) => sought.push(index) });
    [...root.querySelectorAll<HTMLElement>(".replay-round")][2]?.dispatchEvent(
      new Event("click"),
    );
    expect(sought).toEqual([260]);
  });

  it("速さは選べて、いまのものに印が付く", () => {
    renderReplayBar(root, { ...view, rate: 2 }, handlers);
    const rates = [...root.querySelectorAll(".replay-rate")];
    expect(rates.map((r) => r.textContent)).toEqual(RATES.map((r) => `${r}倍`));
    expect(rates[1]?.classList.contains("active")).toBe(true);
    expect(rates[0]?.classList.contains("active")).toBe(false);
  });

  it("押した速さを渡す", () => {
    const chosen: number[] = [];
    renderReplayBar(root, view, { ...handlers, setRate: (rate) => chosen.push(rate) });
    [...root.querySelectorAll<HTMLElement>(".replay-rate")][2]?.dispatchEvent(
      new Event("click"),
    );
    expect(chosen).toEqual([4]);
  });

  it("どこまで来たかを幅で示す", () => {
    renderReplayBar(root, view, handlers);
    const fill = root.querySelector<HTMLElement>(".replay-progress-fill");
    expect(fill?.style.width).toBe("33%");
  });

  it("誰の視点かを言う", () => {
    // **牌譜は自分の席の視界でしか見られない。**誰の目で見ているかを出す。
    renderReplayBar(root, view, handlers);
    expect(root.querySelector(".replay-who")?.textContent).toBe("わたし の視点");
  });
});
