import type { Command } from "../protocol/Command";
import type { Tile } from "../protocol/Tile";
import type { GameState } from "../game/state";
import { tileLabel } from "../game/tiles";
import { BODY_FILL_COLOR } from "../scene/face-atlas";
import { isWinningShape } from "../game/complete";
import { actionsFor, canDeclareRiichi, discardChoices } from "./actions";
import { RESULT_SHOWN_MS, resultPanel } from "./result";
import { tileFaceSvg } from "./tile-face";

let riichiReady = false;
let pendingWindow: number | null = null;
/** 利用者が自分で閉じた結果。同じものを出し直さない。 */
let dismissedResultAt: number | null = null;

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
export function applyTileTheme(root: HTMLElement): void {
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

/** 手の中（ツモ牌を含む）が和了形か。副露の数だけ面子が減る。 */
function completeInHand(state: GameState): boolean {
  if (state.drawn === null) {
    return false;
  }
  const seat = state.seats[state.you];
  const meldCount = seat ? seat.melds.length : 0;
  return isWinningShape([...state.hand, state.drawn], meldCount);
}

/** 音の入切ボタンの文字。**いまの状態ではなく、押したらどうなるかを書く。** */
function soundLabel(): string {
  const audio = (globalThis as unknown as { sfx?: { muted: boolean } }).sfx;
  return audio?.muted === true ? "音を出す" : "音を消す";
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
  // **出しっぱなしにしない。**読み終わる頃には次の局が進んでいる。
  const panel = root.querySelector<HTMLElement>(".result");
  if (panel !== null && state.result !== null) {
    panel.hidden = nowMs - state.result.at > RESULT_SHOWN_MS;
  }

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
    if (state.lastDiscard) {
      return "鳴くか決めてください";
    }
    // **形が揃っているのにツモが出ないときは、理由を言う。**役が1つも
    // 無ければ和了できない。鳴いた手でよく起きるが、画面が何も言わないと
    // 「揃っているのに上がれない」としか見えない。
    // **判定の権威はサーバである。**ここは形だけを見て一言添える。
    const offersTsumo = state.pending.options.some((o) => o.type === "tsumo");
    if (!offersTsumo && completeInHand(state)) {
      return "形は揃っていますが、役が無いので和了できません";
    }
    return "あなたの番";
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
    // **音は切れないと困る。**人のいる場所で開くこともある。
    plainButton(soundLabel(), () => {
      const audio = (globalThis as unknown as { sfx?: { muted: boolean } }).sfx;
      if (audio !== undefined) {
        audio.muted = !audio.muted;
      }
      renderBoard(root, state, send);
    }),
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
    // **いま誰の番かを、卓の席そのものの近くで示す。**画面の下端の一行
    // だけでは、点数と手番を目で結び付けられない。
    const active = state.turn === seat && !state.notice;
    const cell = node(
      "span",
      `score seat-${offset}${active ? " active" : ""}`,
      `席${seat}${marks ? ` ${marks}` : ""} ${(state.scores[seat] ?? 0).toLocaleString()}点`,
    );
    // 直前の局の増減。**結果の板を閉じても、誰がいくら動いたかは残す。**
    const delta = state.result?.delta[seat] ?? 0;
    if (state.result !== null && delta !== 0) {
      cell.append(
        node(
          "span",
          `score-delta ${delta > 0 ? "plus" : "minus"}`,
          `${delta > 0 ? "+" : ""}${delta.toLocaleString()}`,
        ),
      );
    }
    scores.append(cell);
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

  // **結果は次の局の上に重ねる。**サーバは間を置かずに配り直すので、
  // 局の状態と一緒に消すと役も点数も読めない。
  const result = state.result;
  if (result !== null && result.at !== dismissedResultAt) {
    board.append(
      resultPanel(result, state, { node, tileNode }, () => {
        dismissedResultAt = result.at;
      }),
    );
  } else if (state.notice) {
    board.append(node("div", "notice", state.notice));
  }
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
