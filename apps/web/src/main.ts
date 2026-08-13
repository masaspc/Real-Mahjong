import { apply, emptyState } from "./game/state";
import type { GameState } from "./game/state";
import { connect } from "./net/connection";
import { renderBoard } from "./ui/board";
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

let state: GameState = emptyState(0);
const connection = connect({
  base: `ws://${location.host}/ws`,
  table: tableId(),
  lastSeq: () => state.lastSeq,
  onEvent(envelope) {
    state = apply(state, envelope, performance.now());
    renderBoard(root, state, (command) => connection.send(command));
  },
  onStatus(text) {
    document.title = `麻雀 — ${text}`;
  },
});

setInterval(() => renderBoard(root, state, (command) => connection.send(command)), 100);

(globalThis as unknown as { newTable: () => void }).newTable = () => {
  localStorage.removeItem("real-mahjong.table");
  location.reload();
};
