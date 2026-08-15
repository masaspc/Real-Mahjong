import { Presentation } from "./game/presentation";
import { connect } from "./net/connection";
import { motionsFor } from "./scene/motion";
import { placementsFor } from "./scene/placement";
import { TableScene } from "./scene/table";
import { systemClock } from "./timeline/clock";
import { discardChoices } from "./ui/actions";
import {
  clearRiichiReady,
  isRiichiReady,
  renderBoard,
  updateTimer,
} from "./ui/board";
import "./ui/board.css";

function tableId(): string {
  const key = "real-mahjong.table";
  let id = localStorage.getItem(key);
  if (!id) {
    id = Math.random().toString(36).slice(2, 10);
    localStorage.setItem(key, id);
  }
  return id;
}

/**
 * 演出を切って遊ぶ口。
 *
 * **間の長さが妥当かは、入れた場合と切った場合を並べないと決められない。**
 * `?effects=off` で受信した端から見せる。
 */
const effectsOff = new URLSearchParams(location.search).get("effects") === "off";

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("#app が無い");

const tableRoot = document.createElement("div");
tableRoot.id = "table";
const uiRoot = document.createElement("div");
uiRoot.id = "ui";
const canvas = document.createElement("canvas");
tableRoot.append(canvas);
root.replaceChildren(tableRoot, uiRoot);

const scene = new TableScene(canvas);
const presentation = new Presentation(0, systemClock);

/**
 * 盤面の HTML を作り直した時点の目印。
 *
 * **毎フレーム作り直してはならない。**`renderBoard` は `replaceChildren` で
 * 全部を差し替えるため、押下と離上の間にボタンが消え、**クリックが一度も
 * 成立しない。**状態が変わったときだけ作り直す。
 */
let renderedState: unknown = null;
let renderedRiichi = false;

const drawBoard = (force = false): void => {
  const state = presentation.state;
  const riichi = isRiichiReady();
  if (!force && renderedState === state && renderedRiichi === riichi) {
    return;
  }
  renderedState = state;
  renderedRiichi = riichi;
  renderBoard(uiRoot, state, (command) => connection.send(command));
};

const draw = (): void => {
  const state = presentation.state;
  const active = presentation.active;
  if (active === null) {
    scene.sync(placementsFor(state));
  } else {
    const before = placementsFor(state);
    const after = placementsFor(active.nextState);
    const motions = motionsFor(before, after, active.event, state.you);
    // **進捗は再生器の時計から作る。**ここで数え直すと早送りとずれ、
    // 待ち時間だけ先に終わって牌が空中に残る。
    const progress =
      active.durationMs === 0 ? 1 : active.elapsedMs / active.durationMs;
    scene.syncWithMotion(after, motions, progress);
  }
  drawBoard();
  // バーだけは毎フレーム動かす。**作り直さずに幅を書き換える。**
  updateTimer(uiRoot, state, performance.now());
};

const connection = connect({
  base: `ws://${location.host}/ws`,
  table: tableId(),
  // **表示ではなく受信に従う。**演出待ちのぶんを取り直すと二重に積む。
  lastSeq: () => presentation.receivedSeq,
  onEvent(envelope) {
    presentation.receive(envelope);
    if (effectsOff) {
      presentation.skip();
    }
  },
  onStatus(text) {
    // **ここで jumpToLatest を呼んではならない。**取り直した backlog は
    // EffectPlayer の方針に任せる。溜まりが 1,500ms を超えれば勝手に速まり、
    // 6,000ms を超えれば勝手に飛ぶ。判断を二重に持たない。
    document.title = `麻雀 — ${text}`;
  },
});

canvas.addEventListener("click", (event) => {
  const rect = canvas.getBoundingClientRect();
  const tile = scene.pickHandTile(event.clientX - rect.left, event.clientY - rect.top);
  const command = tile === null
    ? undefined
    : discardChoices(presentation.state, isRiichiReady()).get(tile);
  if (command === undefined) {
    // **牌を選んでいない叩きは早送りにする。**締切は動かないので、
    // 速く見終わるだけで有利にも不利にもならない。
    presentation.skip();
    draw();
    return;
  }
  connection.send(command);
  clearRiichiReady();
  drawBoard(true);
  draw();
});

const resize = (): void => {
  scene.resize(tableRoot.clientWidth, tableRoot.clientHeight);
};
addEventListener("resize", resize);
resize();

const renderFrame = (): void => {
  presentation.update();
  draw();
  scene.render();
  requestAnimationFrame(renderFrame);
};
requestAnimationFrame(renderFrame);

document.addEventListener("visibilitychange", () => {
  // 裏に回っている間 requestAnimationFrame は止まり、演出が溜まる。
  // **ここでも飛ばすと決めつけない。**溜まりが少なければ普通に見せる。
  if (!document.hidden) {
    presentation.update();
    draw();
  }
});

draw();

(globalThis as unknown as { newTable: () => void }).newTable = () => {
  localStorage.removeItem("real-mahjong.table");
  location.reload();
};
