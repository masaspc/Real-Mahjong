import { apply, emptyState } from "./game/state";
import type { GameState } from "./game/state";
import { connect } from "./net/connection";
import { placementsFor } from "./scene/placement";
import { TableScene } from "./scene/table";
import { discardChoices } from "./ui/actions";
import {
  clearRiichiReady,
  isRiichiReady,
  renderBoard,
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
let state: GameState = emptyState(0);

const draw = (): void => {
  scene.sync(placementsFor(state));
  renderBoard(uiRoot, state, (command) => connection.send(command));
};

const connection = connect({
  base: `ws://${location.host}/ws`,
  table: tableId(),
  lastSeq: () => state.lastSeq,
  onEvent(envelope) {
    state = apply(state, envelope, performance.now());
    draw();
  },
  onStatus(text) {
    document.title = `麻雀 — ${text}`;
  },
});

canvas.addEventListener("click", (event) => {
  const rect = canvas.getBoundingClientRect();
  const tile = scene.pickHandTile(event.clientX - rect.left, event.clientY - rect.top);
  if (tile === null) return;
  const command = discardChoices(state, isRiichiReady()).get(tile);
  if (command === undefined) return;
  connection.send(command);
  clearRiichiReady();
  draw();
});

const resize = (): void => {
  scene.resize(tableRoot.clientWidth, tableRoot.clientHeight);
};
addEventListener("resize", resize);
resize();

const renderFrame = (): void => {
  scene.render();
  requestAnimationFrame(renderFrame);
};
requestAnimationFrame(renderFrame);

draw();
setInterval(draw, 100);

(globalThis as unknown as { newTable: () => void }).newTable = () => {
  localStorage.removeItem("real-mahjong.table");
  location.reload();
};
