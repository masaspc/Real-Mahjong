/**
 * 牌譜の一覧と、再生の操作盤。
 *
 * **盤面そのものは描かない。**3D の卓と点棒の行は対局中と同じものを
 * 使い、ここは「どの牌譜を開くか」と「どう再生するか」だけを持つ。
 */

import type { GameState } from "../game/state";
import { nameOf } from "../game/state";
import type { RecordCard } from "./api";

/** 選べる速さ。 */
export const RATES = [1, 2, 4] as const;

function node<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  if (className) element.className = className;
  // **名前は他人が入力した文字列である。**innerHTML に混ぜない。
  if (text !== undefined) element.textContent = text;
  return element;
}

/** いつ打ったか。**年は省く**——並ぶのは直近のものばかりである。 */
export function whenLabel(ms: number, now: number): string {
  const date = new Date(ms);
  const sameYear = new Date(now).getFullYear() === date.getFullYear();
  const month = date.getMonth() + 1;
  const day = date.getDate();
  const hour = String(date.getHours()).padStart(2, "0");
  const minute = String(date.getMinutes()).padStart(2, "0");
  return sameYear
    ? `${month}/${day} ${hour}:${minute}`
    : `${date.getFullYear()}/${month}/${day} ${hour}:${minute}`;
}

/** その対局がどう終わったか。 */
export function outcomeLabel(card: RecordCard): string {
  if (card.ended_ms === null) {
    return "途中まで";
  }
  return "終局";
}

export type ListHandlers = {
  open(id: string): void;
  back(): void;
};

/** 牌譜の一覧。 */
export function renderRecordList(
  root: HTMLElement,
  cards: RecordCard[],
  handlers: ListHandlers,
  now: number,
  notice?: string,
): void {
  const panel = node("div", "records");
  panel.append(node("h1", "records-title", "牌譜"));

  if (cards.length === 0) {
    // **空を黙って出さない。**まだ打っていないのか、鍵が変わったのかを言う。
    panel.append(
      node(
        "p",
        "records-empty",
        "この端末で打った半荘はまだありません。対局すると、ここに残ります。",
      ),
    );
  }

  const list = node("ul", "record-list");
  for (const card of cards) {
    const item = node("li", "record-item");
    const button = node("button", "record-open");
    button.append(node("span", "record-when", whenLabel(card.started_ms, now)));
    button.append(node("span", "record-players", card.players.join("・")));
    button.append(node("span", "record-outcome", outcomeLabel(card)));
    button.addEventListener("click", () => handlers.open(card.id));
    item.append(button);
    list.append(item);
  }
  panel.append(list);

  const back = node("button", "records-button quiet", "ロビーへ戻る");
  back.addEventListener("click", () => handlers.back());
  panel.append(back);

  if (notice) {
    panel.append(node("p", "records-notice", notice));
  }
  root.replaceChildren(panel);
}

export type ReplayHandlers = {
  setRate(rate: number): void;
  seek(index: number): void;
  back(): void;
};

export type ReplayView = {
  /** 局の頭の位置。目次に使う。 */
  roundStarts: number[];
  rate: number;
  fed: number;
  total: number;
  state: GameState;
};

/** 再生の操作盤。盤面の上に重ねる。 */
export function renderReplayBar(
  root: HTMLElement,
  view: ReplayView,
  handlers: ReplayHandlers,
): void {
  const bar = node("div", "replay-bar");

  const back = node("button", "replay-button", "牌譜一覧");
  back.addEventListener("click", () => handlers.back());
  bar.append(back);

  // 局の目次。**東1局から順に並ぶ。**
  const rounds = node("div", "replay-rounds");
  view.roundStarts.forEach((index, order) => {
    const button = node("button", "replay-round", `${order + 1}局目`);
    if (view.fed >= index) button.classList.add("passed");
    button.addEventListener("click", () => handlers.seek(index));
    rounds.append(button);
  });
  bar.append(rounds);

  const speeds = node("div", "replay-rates");
  for (const rate of RATES) {
    const button = node(
      "button",
      `replay-rate${view.rate === rate ? " active" : ""}`,
      `${rate}倍`,
    );
    button.addEventListener("click", () => handlers.setRate(rate));
    speeds.append(button);
  }
  bar.append(speeds);

  // どこまで来たか。
  const progress = node("div", "replay-progress");
  const filled = node("div", "replay-progress-fill");
  const ratio = view.total === 0 ? 0 : view.fed / view.total;
  filled.style.width = `${Math.round(ratio * 100)}%`;
  progress.append(filled);
  bar.append(progress);

  bar.append(node("span", "replay-who", `${nameOf(view.state, view.state.you)} の視点`));

  root.replaceChildren(bar);
}

/** 牌譜の画面を抜けた先。 */
export type Chosen = { kind: "replay"; id: string } | { kind: "lobby" };

/**
 * 牌譜の一覧を動かし、開く対局が決まるまで待つ。
 *
 * **一覧が引けなくても行き止まりにしない。**鍵が変わっていれば0件だし、
 * サーバが古ければ断られる。どちらもロビーへ戻れるようにする。
 */
export function mountRecords(
  root: HTMLElement,
  load: () => Promise<RecordCard[]>,
  now: () => number,
): Promise<Chosen> {
  return new Promise((resolve) => {
    const handlers: ListHandlers = {
      open: (id) => resolve({ kind: "replay", id }),
      back: () => resolve({ kind: "lobby" }),
    };
    renderRecordList(root, [], handlers, now(), "読み込んでいます……");
    load().then(
      (cards) => renderRecordList(root, cards, handlers, now()),
      () =>
        renderRecordList(
          root,
          [],
          handlers,
          now(),
          "牌譜を読み込めませんでした。",
        ),
    );
  });
}
