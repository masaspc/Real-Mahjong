import type { Command } from "../protocol/Command";
import type { Tile } from "../protocol/Tile";
import type { GameState } from "../game/state";
import { tileLabel } from "../game/tiles";
import { actionsFor, canDeclareRiichi, discardChoices } from "./actions";
import { tileFaceSvg } from "./tile-face";

let riichiReady = false;
let pendingWindow: number | null = null;

function node<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  if (className) element.className = className;
  if (text !== undefined) element.textContent = text;
  return element;
}

function tileNode(tile: Tile): HTMLElement {
  const element = node("span", "tile");
  element.innerHTML = tileFaceSvg(tile);
  element.setAttribute("aria-label", tileLabel(tile));
  element.title = tileLabel(tile);
  return element;
}

function actionButton(
  label: string,
  command: Command,
  send: (command: Command) => void,
): HTMLButtonElement {
  const button = node("button", "action", label);
  button.addEventListener("click", () => {
    clearRiichiReady();
    send(command);
  });
  return button;
}

function plainButton(label: string, onClick: () => void): HTMLButtonElement {
  const button = node("button", "action", label);
  button.addEventListener("click", onClick);
  return button;
}

function renderActions(
  state: GameState,
  send: (command: Command) => void,
): HTMLElement {
  const actions = node("div", "actions");
  if (!state.pending) return actions;

  if (canDeclareRiichi(state)) {
    const button = node(
      "button",
      `action${riichiReady ? " selected" : ""}`,
      riichiReady ? "リーチ待機中" : "リーチ",
    );
    button.addEventListener("click", () => {
      riichiReady = !riichiReady;
    });
    actions.append(button);
  }
  for (const choice of actionsFor(state)) {
    actions.append(actionButton(choice.label, choice.command, send));
  }
  return actions;
}

function windLabel(wind: string): string {
  return { East: "東", South: "南", West: "西", North: "北" }[wind] ?? wind;
}

export function isRiichiReady(): boolean {
  return riichiReady;
}

export function clearRiichiReady(): void {
  riichiReady = false;
}

/** 3D 卓へ重ねる局情報と操作 UI を描く。牌の配置は描かない。 */
export function renderBoard(
  root: HTMLElement,
  state: GameState,
  send: (command: Command) => void,
): void {
  if (pendingWindow !== state.pending?.windowId) {
    pendingWindow = state.pending?.windowId ?? null;
    clearRiichiReady();
  }
  if (discardChoices(state, false).size === 0) clearRiichiReady();

  const board = node("main", "board");
  const round = state.round
    ? `${windLabel(state.round.wind)}${state.round.number}局`
    : "開始待ち";
  const header = node("header", "summary");
  header.append(
    node("strong", undefined, round),
    node("span", undefined, `${state.honba}本場`),
    node("span", undefined, `供託 ${state.sticks}`),
    node("span", undefined, `残り ${state.wallRemaining}枚`),
  );
  const dora = node("span", "dora", "ドラ表示 ");
  for (const tile of state.doraIndicators) dora.append(tileNode(tile));
  header.append(
    dora,
    plainButton("新しい卓", () =>
      (globalThis as unknown as { newTable: () => void }).newTable(),
    ),
  );

  const scores = node("section", "scores");
  for (let offset = 0; offset < 4; offset += 1) {
    const seat = (state.you + offset) % 4;
    const view = state.seats[seat];
    if (!view) throw new Error(`席が範囲外: ${seat}`);
    const marks = [
      seat === state.you ? "自分" : "",
      seat === state.dealer ? "親" : "",
      view.riichi ? "立直" : "",
    ]
      .filter(Boolean)
      .join("・");
    scores.append(
      node(
        "span",
        `score seat-${offset}`,
        `席${seat}${marks ? ` ${marks}` : ""} ${(state.scores[seat] ?? 0).toLocaleString()}点`,
      ),
    );
  }
  board.append(header, scores);

  const controls = node("section", "controls");
  controls.append(renderActions(state, send));
  if (state.pending) {
    const remaining = Math.max(0, state.pending.deadlineAt - performance.now());
    const meter = node("div", "timer");
    const fill = node("span", "timer-fill");
    fill.style.width = `${Math.min(100, remaining / 200)}%`;
    meter.append(fill);
    controls.append(meter);
  }
  board.append(controls);

  if (state.notice) board.append(node("div", "notice", state.notice));
  if (state.phase === "matchOver" && state.finalScores) {
    const ranking = state.finalScores
      .map((score, seat) => ({ score, seat }))
      .sort((a, b) => b.score - a.score);
    board.append(
      node(
        "div",
        "ranking",
        ranking
          .map(
            (entry, index) =>
              `${index + 1}位 席${entry.seat} ${entry.score.toLocaleString()}点`,
          )
          .join(" / "),
      ),
    );
  }
  root.replaceChildren(board);
}
