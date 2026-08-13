import type { Command } from "../protocol/Command";
import type { Tile } from "../protocol/Tile";
import type { GameState, MeldView, SeatView } from "../game/state";
import { sortTiles, tileLabel } from "../game/tiles";
import { actionsFor, canDeclareRiichi, discardChoices } from "./actions";
import { tileBackSvg, tileFaceSvg } from "./tile-face";

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

/**
 * 牌1枚。**面は SVG で描く。**中身は自分で組み立てた静的な文字列で、
 * 利用者の入力は混ざらない。読み上げ用にラベルを持たせる。
 */
function tileNode<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  tile: Tile,
): HTMLElementTagNameMap[K] {
  const element = node(tag, className);
  element.innerHTML = tileFaceSvg(tile);
  element.setAttribute("aria-label", tileLabel(tile));
  element.title = tileLabel(tile);
  return element;
}

function tiles(values: Tile[], className = "tiles"): HTMLElement {
  const row = node("span", className);
  for (const tile of values) row.append(tileNode("span", "tile", tile));
  return row;
}

/** 伏せた牌を並べる。他家の手牌に使う。 */
function backs(count: number): HTMLElement {
  const row = node("span", "tiles");
  for (let i = 0; i < count; i += 1) {
    const back = node("span", "tile back");
    back.innerHTML = tileBackSvg();
    row.append(back);
  }
  return row;
}

function meld(meldView: MeldView): HTMLElement {
  const view = node("span", "meld");
  view.append(tiles(meldView.tiles), node("small", undefined, meldView.kind));
  return view;
}

function seatPanel(seatNumber: number, seat: SeatView, state: GameState): HTMLElement {
  const panel = node("section", "seat");
  const marks = [seatNumber === state.dealer ? "親" : "", seat.riichi ? "立直" : ""]
    .filter(Boolean)
    .join("・");
  panel.append(node("h2", undefined, `席${seatNumber} ${marks} ${(state.scores[seatNumber] ?? 0).toLocaleString()}点`));
  if (seatNumber !== state.you) {
    const concealed = node("div", "concealed");
    concealed.append(backs(seat.handSize));
    panel.append(concealed);
  }
  const melds = node("div", "melds");
  for (const value of seat.melds) melds.append(meld(value));
  panel.append(melds);
  const river = node("div", "river");
  for (const discarded of seat.river) {
    river.append(
      tileNode("span", `tile${discarded.riichi ? " riichi-discard" : ""}`, discarded.tile),
    );
  }
  panel.append(river);
  return panel;
}

function actionButton(label: string, command: Command, send: (command: Command) => void): HTMLButtonElement {
  const button = node("button", "action", label);
  button.addEventListener("click", () => {
    riichiReady = false;
    send(command);
  });
  return button;
}

/**
 * サーバへ送らないボタン。
 *
 * **`actionButton` を流用してダミーのコマンドを渡さない。**送られない
 * ことがコールバック側の都合に依存してしまい、`actionButton` を変えた
 * ときに本当に送られるようになる。
 */
function plainButton(label: string, onClick: () => void): HTMLButtonElement {
  const button = node("button", "action", label);
  button.addEventListener("click", onClick);
  return button;
}

function renderActions(state: GameState, send: (command: Command) => void): HTMLElement {
  const actions = node("div", "actions");
  const pending = state.pending;
  if (!pending) return actions;

  if (canDeclareRiichi(state)) {
    const button = node("button", `action${riichiReady ? " selected" : ""}`, riichiReady ? "リーチ待機中" : "リーチ");
    button.addEventListener("click", () => { riichiReady = !riichiReady; });
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

/** 現在の状態を、操作できる最小限の2D卓として描く。 */
export function renderBoard(root: HTMLElement, state: GameState, send: (command: Command) => void): void {
  if (pendingWindow !== state.pending?.windowId) {
    pendingWindow = state.pending?.windowId ?? null;
    riichiReady = false;
  }
  if (discardChoices(state, false).size === 0) riichiReady = false;

  root.replaceChildren();
  const board = node("main", "board");
  const round = state.round ? `${windLabel(state.round.wind)}${state.round.number}局` : "開始待ち";
  const header = node("header", "summary");
  header.append(node("strong", undefined, round), node("span", undefined, `${state.honba}本場`), node("span", undefined, `供託 ${state.sticks}`), node("span", undefined, `残り ${state.wallRemaining}枚`));
  const dora = node("span", "dora", "ドラ表示 ");
  dora.append(tiles(state.doraIndicators));
  header.append(
    dora,
    plainButton("新しい卓", () => (globalThis as unknown as { newTable: () => void }).newTable()),
  );
  board.append(header);

  const opponents = node("div", "opponents");
  for (let offset = 1; offset < 4; offset += 1) {
    const seatNumber = (state.you + offset) % 4;
    const seat = state.seats[seatNumber];
    if (seat) opponents.append(seatPanel(seatNumber, seat, state));
  }
  const mine = state.seats[state.you];
  if (!mine) throw new Error(`席が範囲外: ${state.you}`);
  board.append(opponents, seatPanel(state.you, mine, state));

  const hand = node("section", "my-hand");
  hand.append(node("h2", undefined, `自分（席${state.you}）の手牌`));
  const discards = discardChoices(state, riichiReady);
  const handRow = node("div", "hand-row");
  for (const tile of sortTiles(state.hand)) {
    const button = tileNode("button", "tile hand-tile", tile);
    const command = discards.get(tile);
    button.disabled = command === undefined;
    button.addEventListener("click", () => {
      if (command) send(command);
    });
    handRow.append(button);
  }
  if (state.drawn !== null) {
    const button = tileNode("button", "tile hand-tile drawn", state.drawn);
    const command = discards.get(state.drawn);
    button.disabled = command === undefined;
    button.addEventListener("click", () => {
      if (command) send(command);
    });
    handRow.append(button);
  }
  hand.append(handRow, renderActions(state, send));
  board.append(hand);

  if (state.pending) {
    const remaining = Math.max(0, state.pending.deadlineAt - performance.now());
    const meter = node("div", "timer");
    const fill = node("span", "timer-fill");
    fill.style.width = `${Math.min(100, remaining / 200)}%`;
    meter.append(fill);
    board.append(meter);
  }
  if (state.notice) board.append(node("div", "notice", state.notice));
  if (state.phase === "matchOver" && state.finalScores) {
    const ranking = state.finalScores.map((score, seat) => ({ score, seat })).sort((a, b) => b.score - a.score);
    board.append(node("div", "ranking", ranking.map((entry, index) => `${index + 1}位 席${entry.seat} ${entry.score.toLocaleString()}点`).join(" / ")));
  }
  root.append(board);
}
