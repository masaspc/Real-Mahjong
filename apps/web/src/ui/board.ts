import type { Command } from "../protocol/Command";
import type { Tile } from "../protocol/Tile";
import type { GameState } from "../game/state";
import { tileLabel } from "../game/tiles";
import { BODY_FILL_COLOR } from "../scene/face-atlas";
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

/**
 * 牌の地色を CSS へ流し込む。
 *
 * **同じ色を CSS にも書くと、いつか片方だけ変わる。**3D のアトラスが
 * `BODY_FILL_COLOR` で塗っているのと同じ色を、2D の `.tile-face` も敷く。
 * 定義を1箇所に保つため、色そのものは CSS に書かず変数で渡す。
 */
function applyTileTheme(root: HTMLElement): void {
  root.style.setProperty("--tile-body", BODY_FILL_COLOR);
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
  tiles: Tile[] = [],
): HTMLButtonElement {
  const button = node("button", "action");
  // **文字だけでは何を鳴くのか分からない。**牌そのものを並べる。
  button.append(node("span", "action-label", label));
  if (tiles.length > 0) {
    const row = node("span", "action-tiles");
    for (const tile of tiles) row.append(tileNode(tile));
    button.append(row);
  }
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
    actions.append(actionButton(choice.label, choice.command, send, choice.tiles));
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
/**
 * 持ち時間バーの残りだけを更新する。
 *
 * **バーのために盤面を作り直してはならない。**`renderBoard` は
 * `replaceChildren` で全部を差し替えるので、毎フレーム呼ぶとボタンが
 * 押下と離上の間に消え、**クリックが一度も成立しない。**
 * 動かす必要があるのはこの幅だけである。
 */
export function updateTimer(root: HTMLElement, state: GameState, nowMs: number): void {
  const fill = root.querySelector<HTMLElement>(".timer-fill");
  if (!fill) {
    return;
  }
  const remaining = state.pending
    ? Math.max(0, state.pending.deadlineAt - nowMs)
    : 0;
  fill.style.width = `${Math.min(100, remaining / 200)}%`;

  // **棒だけでは「あと何秒か」が読めない。**残り3秒と残り15秒の区別が
  // つかず、急ぐべきかどうかを判断できない。数字を併記する。
  const text = root.querySelector<HTMLElement>(".timer-text");
  if (text) {
    text.textContent = state.pending
      ? `残り ${Math.ceil(remaining / 1000)}秒`
      : "";
  }
}

/**
 * いま何を待っているのかの一行。
 *
 * **止まっているのか自分が見落としているのかが分からないのが一番困る。**
 * 自分の番なら「あなたの番」、他家の番なら誰を待っているのかを出す。
 */
function turnLabel(state: GameState): string {
  if (state.notice) {
    return "";
  }
  if (state.pending) {
    return state.lastDiscard ? "鳴くか決めてください" : "あなたの番";
  }
  if (state.turn === null) {
    return "";
  }
  return state.turn === state.you ? "あなたの番" : `席${state.turn} を待っています`;
}

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
  // **帯とボタンを縦に積むと自分の手牌を覆う。**手牌は画面の下端に描かれて
  // おり、鳴くかどうかを決める瞬間にこそ手が読めなくなる。1行に並べる。
  const row = node("div", "control-row");
  if (state.pending && state.lastDiscard) {
    // **鳴けるときは、何を鳴くのかを真っ先に見せる。**
    const target = node("div", "target");
    target.append(node("span", undefined, `席${state.lastDiscard.seat} が捨てた`));
    target.append(tileNode(state.lastDiscard.tile));
    row.append(target);
  }
  row.append(renderActions(state, send));
  controls.append(row);
  const status = node("div", "status");
  status.append(node("span", "turn", turnLabel(state)));
  if (state.pending) {
    const meter = node("div", "timer");
    const fill = node("span", "timer-fill");
    meter.append(fill);
    status.append(meter, node("span", "timer-text"));
  }
  controls.append(status);
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
  applyTileTheme(root);
  root.replaceChildren(board);
}
