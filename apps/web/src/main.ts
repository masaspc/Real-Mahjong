import { soundOf } from "./audio/catalog";
import { Sfx } from "./audio/sfx";
import { Presentation } from "./game/presentation";
import type { GameState } from "./game/state";
import { connect, socketBase } from "./net/connection";
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
import { listRecords, readRecord, recordEvents } from "./records/api";
import { Replay } from "./records/replay";
import { mountRecords, renderReplayBar } from "./records/screen";
import "./records/records.css";

/**
 * 演出を切って遊ぶ口。
 *
 * **間の長さが妥当かは、入れた場合と切った場合を並べないと決められない。**
 * `?effects=off` で受信した端から見せる。
 */
const effectsOff = new URLSearchParams(location.search).get("effects") === "off";

const app = document.querySelector<HTMLElement>("#app");
if (!app) throw new Error("#app が無い");
const root: HTMLElement = app;

/**
 * 音。**利用者が画面を触るまで `AudioContext` は作らない。**
 *
 * ブラウザは操作なしに音を出すことを許さない。読み込み時に作ると
 * `suspended` のまま生まれ、あとから再開しても鳴らない状態を持ち歩く。
 */
const sfx = new Sfx();
// 盤面のボタンから触れるようにする。`board.ts` は音を知らなくてよい。
(globalThis as unknown as { sfx: Sfx }).sfx = sfx;

// **操作のたびに呼ぶ。**最初の1回で作り、以後は止まっていれば再開する。
// 音の有無に関わらず、操作の文脈でしか `AudioContext` は開始できない。
for (const kind of ["pointerdown", "keydown"] as const) {
  addEventListener(kind, () => sfx.unlock(), { passive: true });
}

(globalThis as unknown as { newTable: () => void }).newTable = () => {
  clearSeat();
  location.reload();
};

/** 卓の器。**対局でも再生でも同じものを使う。** */
type Stage = {
  tableRoot: HTMLElement;
  uiRoot: HTMLElement;
  canvas: HTMLCanvasElement;
  scene: TableScene;
};

function buildStage(): Stage {
  const tableRoot = document.createElement("div");
  tableRoot.id = "table";
  const uiRoot = document.createElement("div");
  uiRoot.id = "ui";
  const canvas = document.createElement("canvas");
  tableRoot.append(canvas);
  root.replaceChildren(tableRoot, uiRoot);

  const scene = new TableScene(canvas);
  const resize = (): void => {
    scene.resize(tableRoot.clientWidth, tableRoot.clientHeight);
  };
  addEventListener("resize", resize);
  resize();
  return { tableRoot, uiRoot, canvas, scene };
}

/**
 * 盤面を描く。
 *
 * **毎フレーム HTML を作り直してはならない。**`renderBoard` は
 * `replaceChildren` で全部を差し替えるため、押下と離上の間にボタンが消え、
 * クリックが一度も成立しない。状態が変わったときだけ作り直す。
 */
function painter(
  stage: Stage,
  send: (command: import("./protocol/Command").Command) => void,
) {
  let renderedState: unknown = null;
  let renderedRiichi = false;

  const drawBoard = (presentation: Presentation, force = false): void => {
    const state = presentation.state;
    const riichi = isRiichiReady();
    if (!force && renderedState === state && renderedRiichi === riichi) {
      return;
    }
    renderedState = state;
    renderedRiichi = riichi;
    renderBoard(stage.uiRoot, state, send);
  };

  const draw = (presentation: Presentation): void => {
    const state = presentation.state;
    const active = presentation.active;
    if (active === null) {
      stage.scene.sync(placementsFor(state));
    } else {
      const before = placementsFor(state);
      const after = placementsFor(active.nextState);
      const motions = motionsFor(before, after, active.event, state.you);
      // **進捗は再生器の時計から作る。**ここで数え直すと早送りとずれ、
      // 待ち時間だけ先に終わって牌が空中に残る。
      const progress =
        active.durationMs === 0 ? 1 : active.elapsedMs / active.durationMs;
      stage.scene.syncWithMotion(after, motions, progress);
    }
    drawBoard(presentation);
    // バーだけは毎フレーム動かす。**作り直さずに幅を書き換える。**
    updateTimer(stage.uiRoot, state, performance.now());
  };

  return { draw, drawBoard };
}

/** 対局。 */
function playLive(token: string): void {
  const stage = buildStage();
  const presentation = new Presentation(0, systemClock);
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

  const connection = connect({
    base: socketBase(location),
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

  const { draw, drawBoard } = painter(stage, (command) =>
    connection.send(command),
  );

  stage.canvas.addEventListener("click", (event) => {
    const rect = stage.canvas.getBoundingClientRect();
    const tile = stage.scene.pickHandTile(
      event.clientX - rect.left,
      event.clientY - rect.top,
    );
    const command =
      tile === null
        ? undefined
        : discardChoices(presentation.state, isRiichiReady()).get(tile);
    if (command === undefined) {
      // **牌を選んでいない叩きは早送りにする。**締切は動かないので、
      // 速く見終わるだけで有利にも不利にもならない。
      presentation.skip();
      draw(presentation);
      return;
    }
    connection.send(command);
    clearRiichiReady();
    drawBoard(presentation, true);
    draw(presentation);
  });

  const frame = (): void => {
    presentation.update();
    draw(presentation);
    stage.scene.render();
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);

  document.addEventListener("visibilitychange", () => {
    // 裏に回っている間 requestAnimationFrame は止まり、演出が溜まる。
    // **ここでも飛ばすと決めつけない。**溜まりが少なければ普通に見せる。
    if (!document.hidden) {
      presentation.update();
      draw(presentation);
    }
  });

  draw(presentation);
}

/**
 * 牌譜の再生。
 *
 * **操作はできない。**保存した列を流すだけなので、盤面の手番も鳴きの
 * 選択肢も出さない。出すと押せてしまい、押しても何も起きない。
 */
function playRecord(id: string): void {
  const stage = buildStage();
  // **盤面を操作盤のぶん下げる。**重ねたままだと局名とドラ表示が隠れる。
  document.body.classList.add("replaying");
  const bar = document.createElement("div");
  root.append(bar);

  void (async () => {
    const head = await readRecord(id);
    const events = await recordEvents(id);
    const replay = new Replay(events, head.you, systemClock);
    const { draw } = painter(stage, () => {
      // 再生では何も送らない。
    });

    const drawBar = (): void => {
      renderReplayBar(
        bar,
        {
          roundStarts: replay.roundStarts,
          rate: replay.rate,
          fed: replay.fed,
          total: replay.total,
          state: replay.presentation.state,
        },
        {
          setRate: (rate) => {
            replay.setRate(rate);
            drawBar();
          },
          seek: (index) => {
            replay.seek(index, head.you);
            drawBar();
          },
          back: () => location.reload(),
        },
      );
    };
    drawBar();

    let lastFed = -1;
    const frame = (): void => {
      replay.update();
      draw(withoutChoices(replay.presentation));
      stage.scene.render();
      // 目次の進みは、流し込んだ件数が変わったときだけ描き直す。
      if (replay.fed !== lastFed) {
        lastFed = replay.fed;
        drawBar();
      }
      requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  })();
}

/**
 * 再生用に、選べる手を落とした見え方を作る。
 *
 * **押せる形で出さない。**牌譜に `RequestAction` が残っているので、
 * そのまま描くと鳴きのボタンが並び、押しても何も起きない。
 */
function withoutChoices(presentation: Presentation): Presentation {
  const state: GameState = presentation.state;
  if (state.pending !== null) {
    state.pending = null;
  }
  return presentation;
}

for (;;) {
  const entry = await mountLobby(root);
  if (entry.kind === "play") {
    playLive(entry.token);
    break;
  }
  const chosen = await mountRecords(root, listRecords, () => Date.now());
  if (chosen.kind === "replay") {
    playRecord(chosen.id);
    break;
  }
}
