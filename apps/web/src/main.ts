import { soundOf } from "./audio/catalog";
import { Sfx } from "./audio/sfx";
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
import { clearSeat } from "./lobby/api";
import { mountLobby } from "./lobby/screen";
import "./lobby/lobby.css";

/**
 * 演出を切って遊ぶ口。
 *
 * **間の長さが妥当かは、入れた場合と切った場合を並べないと決められない。**
 * `?effects=off` で受信した端から見せる。
 */
const effectsOff = new URLSearchParams(location.search).get("effects") === "off";

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("#app が無い");

// **卓が立つまでは盤面を組み立てない。**3D の卓と牌の用意は重く、
// 待合を見ている間に走らせる意味が無い。
const token = await mountLobby(root);

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
 * 音。**利用者が画面を触るまで `AudioContext` は作らない。**
 *
 * ブラウザは操作なしに音を出すことを許さない。読み込み時に作ると
 * `suspended` のまま生まれ、あとから再開しても鳴らない状態を持ち歩く。
 */
const sfx = new Sfx();
// 盤面のボタンから触れるようにする。`board.ts` は音を知らなくてよい。
(globalThis as unknown as { sfx: Sfx }).sfx = sfx;
// 追いつきの様子を外から測るための口。**画面の動きには関与しない。**
(globalThis as unknown as { presentation: Presentation }).presentation =
  presentation;
presentation.onStart((event, skipped) => {
  // **まとめて出たぶんは鳴らさない。**再接続やタブ復帰では数十件が一度に
  // 出るので、そのまま鳴らすと打牌音が束になって弾ける。
  if (skipped) {
    return;
  }
  const cue = soundOf(event);
  if (cue !== null) {
    sfx.play(cue.name, cue.delayMs);
  }
});

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
  token,
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

// **操作のたびに呼ぶ。**最初の1回で作り、以後は止まっていれば再開する。
// 音の有無に関わらず、操作の文脈でしか `AudioContext` は開始できない。
for (const kind of ["pointerdown", "keydown"] as const) {
  addEventListener(kind, () => sfx.unlock(), { passive: true });
}

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
  clearSeat();
  location.reload();
};
