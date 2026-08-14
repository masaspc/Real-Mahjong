# Wave 3f 演出タイムラインの結線 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 受信したイベントをその場で盤面へ反映するのをやめ、`timeline/` の再生器を通して演出の時間ぶん遅らせて見せる。

**Architecture:** 状態を2つに分ける。**論理状態**は受信した時点で確定し、再接続に使う `seq` を持つ。**表示状態**は再生器が出し終えたイベントだけを畳んだもので、画面はこちらだけを描く。`EffectPlayer` は既に完成しているので、書くのは「畳み込み係」と結線だけである。

**Tech Stack:** TypeScript / Vitest / Three.js（描画は Wave 3e-2 のものをそのまま使う）

## Global Constraints

- **`timeline/` の5ファイルは変更しない。**`catalog.ts` `clock.ts` `player.ts` `plugin.ts` `timeline.ts` は試験付きで完成している。足りない機能があれば、それを包む側に書く
- **演出は論理状態を変えない。**遅れて見せるだけで、最終的な盤面は演出が無い場合と1ビットも変わらない。これは Task 3 で機械的に確かめる
- **締切は動かさない。**サーバが `deadline_for(rules, now, bank, lead_in)` で演出ぶんを既に加算している（`crates/mahjong-engine/src/timing.rs:22`）。クライアント側で締切に触れてはならない
- 実時間を直接読まない。時刻は必ず `Clock` を通す。試験は `ManualClock` で駆動する
- 既存の 171 件の試験の期待値を緩めない

---

### Task 1: 演出を挟んだ表示状態を作る

受信したイベントを `EffectPlayer` に積み、**出し終えたぶんだけ**を `apply` で畳んで表示状態にする係を作る。ここは純粋な部品で、DOM も WebSocket も触らない。

**Files:**
- Create: `apps/web/src/game/presentation.ts`
- Create: `apps/web/src/game/presentation.test.ts`

**Interfaces:**
- Consumes: `apply(previous, envelope, nowMs)` と `emptyState(you)`（`game/state.ts`）、`EffectPlayer` `defaultCatchUp`（`timeline/player.ts`）、`Clock` `ManualClock`（`timeline/clock.ts`）
- Produces: `class Presentation` — `receive(envelope)` / `update(nowMs)` / `state` / `receivedSeq` / `skip()` / `jumpToLatest()` / `pendingMs`

- [ ] **Step 1: 失敗する試験を書く**

`apps/web/src/game/presentation.test.ts` に次を書く。

```ts
import { describe, expect, it } from "vitest";

import { Presentation } from "./presentation";
import { ManualClock } from "../timeline/clock";
import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";

/** 打牌のイベント1件。演出は 350ms。 */
function discard(seq: number, seat: 0 | 1 | 2 | 3, tile: number): ClientEventEnvelope {
  return {
    seq,
    event: { type: "discard", seat, tile, manner: "tegiri", riichi: false },
  } as unknown as ClientEventEnvelope;
}

/** ツモのイベント1件。演出は 250ms。 */
function draw(seq: number, seat: 0 | 1 | 2 | 3): ClientEventEnvelope {
  return { seq, event: { type: "draw", seat } } as unknown as ClientEventEnvelope;
}

describe("Presentation", () => {
  it("演出が終わるまで盤面へ出さない", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));

    p.update();
    expect(p.state.seats[1].river).toHaveLength(0);

    clock.advance(349);
    p.update();
    expect(p.state.seats[1].river).toHaveLength(0);

    clock.advance(1);
    p.update();
    expect(p.state.seats[1].river).toHaveLength(1);
  });

  it("受信した seq は演出を待たずに進む", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(7, 1, 5));

    // **再接続はここを見る。**表示に合わせて遅らせると、演出中に切れた
    // ときに同じイベントを取り直して二重に積む。
    expect(p.receivedSeq).toBe(7);
    expect(p.state.lastSeq).toBeNull();
  });

  it("溜まった演出を早送りできる", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));
    p.receive(discard(2, 2, 6));

    p.skip();
    p.update();
    expect(p.state.seats[1].river).toHaveLength(1);
    expect(p.state.seats[2].river).toHaveLength(1);
    expect(p.pendingMs).toBe(0);
  });

  it("復帰は演出を捨てて最新へ飛ぶ", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(draw(1, 0));
    p.receive(discard(2, 0, 5));

    p.jumpToLatest();
    p.update();
    expect(p.state.lastSeq).toBe(2);
    expect(p.pendingMs).toBe(0);
  });

  it("大きく遅れたら演出を捨てて追いつく", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    // 350ms × 20 = 7,000ms ぶん積む。既定の skipAfterMs は 6,000。
    for (let i = 1; i <= 20; i += 1) {
      p.receive(discard(i, 1, 5));
    }
    p.update();
    expect(p.state.seats[1].river).toHaveLength(20);
  });

  it("同じイベントを二度畳まない", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));
    clock.advance(350);

    p.update();
    p.update();
    p.update();
    expect(p.state.seats[1].river).toHaveLength(1);
  });

  it("演出を持たないイベントは待たせない", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    // match_start は effectOf が null を返すので 0ms。
    p.receive({
      seq: 0,
      event: { type: "match_start", you: 0 },
    } as unknown as ClientEventEnvelope);

    p.update();
    expect(p.state.lastSeq).toBe(0);
  });

  it("先頭が待っている間は後ろも出さない", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));
    p.receive(draw(2, 2));

    clock.advance(300);
    p.update();
    // **順序が入れ替わってはいけない。**ツモは 250ms だが、前の打牌が
    // 終わっていないので出せない。
    expect(p.state.lastSeq).toBeNull();
  });
});
```

- [ ] **Step 2: 試験が落ちることを確かめる**

Run: `pnpm --dir apps/web test src/game/presentation.test.ts`
Expected: FAIL（`presentation.ts` が無い）

- [ ] **Step 3: 実装する**

`apps/web/src/game/presentation.ts` を作る。

```ts
import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import type { Seat } from "../protocol/Seat";
import { EffectPlayer, defaultCatchUp } from "../timeline/player";
import type { CatchUpPolicy } from "../timeline/player";
import type { Clock } from "../timeline/clock";
import { apply, emptyState } from "./state";
import type { GameState } from "./state";

/**
 * 受信したイベントを、演出の時間ぶん遅らせて盤面に見せる。
 *
 * **論理状態と表示状態を分ける。**イベントは届いた時点で確定しているが、
 * 見せるのは演出が終わってからである。この分離が無いと、再接続やタブ復帰で
 * 「まだ演出を見ていない」ことを理由に盤面が巻き戻る。
 *
 * 締切には触れない。サーバが `deadline_for` で演出ぶん（lead_in）を既に
 * 足しているので、**遅らせても思考時間は減らない。**
 */
export class Presentation {
  readonly #player: EffectPlayer;
  #state: GameState;
  /** 受信済みの最大 seq。**再接続はこれを送る。**表示より先を行く。 */
  #receivedSeq: number | null = null;
  /** 受信順の控え。`EffectPlayer` は seq を持たないのでこちらで持つ。 */
  #envelopes: ClientEventEnvelope[] = [];
  /** すでに畳んだ件数。二重に畳まないための印。 */
  #folded = 0;

  constructor(you: Seat, clock: Clock, policy: CatchUpPolicy = defaultCatchUp) {
    this.#player = new EffectPlayer(clock, policy);
    this.#state = emptyState(you);
  }

  get state(): GameState {
    return this.#state;
  }

  get receivedSeq(): number | null {
    return this.#receivedSeq;
  }

  get pendingMs(): number {
    return this.#player.pendingMs;
  }

  get playbackRate(): number {
    return this.#player.playbackRate;
  }

  receive(envelope: ClientEventEnvelope): void {
    this.#envelopes.push(envelope);
    this.#receivedSeq = envelope.seq;
    this.#player.push(envelope.event);
  }

  /** 出し終えたぶんを盤面へ畳む。毎フレーム呼ぶ。 */
  update(): void {
    this.#player.update();
    this.#fold();
  }

  /** 溜まっている演出を捨てて、いますぐ全部見せる。 */
  skip(): void {
    this.#player.skip();
    this.#fold();
  }

  /** 再接続・タブ復帰。演出を捨てて最新状態へ飛ぶ。 */
  jumpToLatest(): void {
    this.#player.jumpToLatest();
    this.#fold();
  }

  /**
   * 再生器が出し終えた件数まで畳む。
   *
   * **件数で進める。**`presented` は push した順にそのまま並ぶので、
   * 控えておいた封筒の同じ位置が対応する。イベントの中身で突き合わせると
   * 同型のイベント（同じ席の同じ牌の打牌）で取り違える。
   */
  #fold(): void {
    const done = this.#player.presented.length;
    for (; this.#folded < done; this.#folded += 1) {
      const envelope = this.#envelopes[this.#folded];
      if (envelope === undefined) {
        return;
      }
      this.#state = apply(this.#state, envelope, performance.now());
    }
  }
}
```

- [ ] **Step 4: 試験が通ることを確かめる**

Run: `pnpm --dir apps/web test src/game/presentation.test.ts`
Expected: 8 passed

全体でも退行が無いことを見る。

Run: `pnpm --dir apps/web test`
Expected: 179 passed

- [ ] **Step 5: 型検査**

Run: `pnpm --dir apps/web typecheck`
Expected: エラー0件

- [ ] **Step 6: コミット**

```bash
git add apps/web/src/game/presentation.ts apps/web/src/game/presentation.test.ts
git commit -m "feat(web): 演出の時間ぶん遅らせて盤面を見せる係を作る

受信した時点で確定する論理状態と、演出が終わってから進む表示状態を分ける。
**再接続に使う seq は表示ではなく受信に従う。**表示に合わせると、演出中に
切れたときに同じイベントを取り直して二重に積む。"
```

---

### Task 2: 画面を表示状態から描く

`main.ts` の結線を差し替える。**受信は再生器へ、描画は表示状態から。**

**Files:**
- Modify: `apps/web/src/main.ts`

**Interfaces:**
- Consumes: `Presentation`（Task 1）、`systemClock`（`timeline/clock.ts`）
- Produces: なし（結線のみ）

- [ ] **Step 1: 差し替える**

`apps/web/src/main.ts` の該当箇所を次のようにする。`emptyState` と `apply` の直接呼び出しは無くなる。

```ts
import { connect } from "./net/connection";
import { Presentation } from "./game/presentation";
import { systemClock } from "./timeline/clock";
import { placementsFor } from "./scene/placement";
import { TableScene } from "./scene/table";
import { discardChoices } from "./ui/actions";
import { clearRiichiReady, isRiichiReady, renderBoard } from "./ui/board";
import "./ui/board.css";
```

状態の持ち方を変える。

```ts
const scene = new TableScene(canvas);
const presentation = new Presentation(0, systemClock);

const draw = (): void => {
  const state = presentation.state;
  scene.sync(placementsFor(state));
  renderBoard(uiRoot, state, (command) => connection.send(command));
};

const connection = connect({
  base: `ws://${location.host}/ws`,
  table: tableId(),
  // **表示ではなく受信に従う。**演出待ちのぶんを取り直すと二重に積む。
  lastSeq: () => presentation.receivedSeq,
  onEvent(envelope) {
    presentation.receive(envelope);
  },
  onStatus(text) {
    document.title = `麻雀 — ${text}`;
    if (text.includes("接続")) {
      // 再接続の取り直しは演出を挟まずに最新へ飛ばす。
      presentation.jumpToLatest();
    }
  },
});
```

クリックは、打牌に使えないときだけ早送りにする。

```ts
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
  draw();
});
```

毎フレーム再生器を進める。`setInterval(draw, 100)` は消し、描画ループに寄せる。

```ts
const renderFrame = (): void => {
  presentation.update();
  draw();
  scene.render();
  requestAnimationFrame(renderFrame);
};
requestAnimationFrame(renderFrame);

document.addEventListener("visibilitychange", () => {
  // 裏に回っている間 requestAnimationFrame は止まる。戻ったら追いつく。
  if (!document.hidden) {
    presentation.jumpToLatest();
    draw();
  }
});
```

- [ ] **Step 2: 型検査とビルドが通ることを確かめる**

Run: `pnpm --dir apps/web typecheck && pnpm --dir apps/web build`
Expected: エラー0件でビルド成功

- [ ] **Step 3: 既存の試験が壊れていないことを確かめる**

Run: `pnpm --dir apps/web test`
Expected: 179 passed

- [ ] **Step 4: 実際に描けることを確かめる**

**遊んで確かめてはいけない。**決め打ちで撮る。

```bash
cargo run -p server --bin serve &
sleep 5
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless=new --disable-gpu --use-angle=swiftshader --enable-unsafe-swiftshader \
  --window-size=1280,720 --virtual-time-budget=25000 \
  --screenshot=wired.png "http://127.0.0.1:8080/"
```

Expected: `wired.png` に卓と手牌13枚が出る。**真っ黒や卓だけなら失敗。**
表示状態が畳まれておらず、盤面が空のまま止まっている。

- [ ] **Step 5: コミット**

```bash
git add apps/web/src/main.ts
git commit -m "feat(web): 画面を演出ぶん遅れた表示状態から描く

受信は再生器へ積み、描画は出し終えたぶんだけを畳んだ状態から行う。
牌を選んでいないクリックは早送りにする。**締切は動かない**ので、
速く見終わるだけで有利にも不利にもならない。"
```

---

### Task 3: 演出が盤面を変えないことを牌譜で確かめる

**これがこのウェーブで最も大事な不変条件である。**演出は見せる時刻を変えるだけで、
最終的な盤面を1ビットも変えてはならない。半荘まるごとで突き合わせる。

**Files:**
- Create: `apps/web/src/game/presentation-replay.test.ts`

**Interfaces:**
- Consumes: `Presentation`（Task 1）、`apply` `emptyState`（`game/state.ts`）、`ManualClock`、`src/game/__fixtures__/match-seed1.jsonl`

- [ ] **Step 1: 試験を書く**

```ts
import { describe, expect, it } from "vitest";

import { Presentation } from "./presentation";
import { apply, emptyState } from "./state";
import type { GameState } from "./state";
import { ManualClock } from "../timeline/clock";
import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import seed1 from "./__fixtures__/match-seed1.jsonl?raw";

function envelopes(raw: string): ClientEventEnvelope[] {
  return raw
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => JSON.parse(line) as ClientEventEnvelope);
}

/** 演出を挟まずに畳んだ、答え合わせ用の状態。 */
function directly(all: ClientEventEnvelope[]): GameState {
  let state = emptyState(0);
  for (const envelope of all) {
    state = apply(state, envelope, 0);
  }
  return state;
}

describe("演出は盤面を変えない", () => {
  it("半荘を流し切ると、演出なしで畳んだ盤面と一致する", () => {
    const all = envelopes(seed1);
    expect(all.length).toBeGreaterThan(1_000);

    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    for (const envelope of all) {
      p.receive(envelope);
      // 1件ずつ、最長の演出（リーチ宣言 1,800ms）より長く進める。
      clock.advance(2_000);
      p.update();
    }

    expect(p.state).toEqual(directly(all));
    expect(p.pendingMs).toBe(0);
    expect(p.receivedSeq).toBe(all[all.length - 1]?.seq);
  });

  it("時計を進めずに全部積んで早送りしても、同じ盤面になる", () => {
    const all = envelopes(seed1);
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    for (const envelope of all) {
      p.receive(envelope);
    }
    p.skip();
    p.update();

    // **早送りは見せ方であって、結果ではない。**
    expect(p.state).toEqual(directly(all));
  });
});
```

- [ ] **Step 2: 試験が通ることを確かめる**

Run: `pnpm --dir apps/web test src/game/presentation-replay.test.ts`
Expected: 2 passed

全体でも退行が無いことを見る。

Run: `pnpm --dir apps/web test`
Expected: 181 passed

- [ ] **Step 3: 試験が本当に効くことを確かめる**

`presentation.ts` の `#fold` で `this.#folded` を毎回 0 に戻すよう一時的に壊し、
試験が落ちることを確かめる。**確かめたら必ず元へ戻す。**

壊した状態で Run: `pnpm --dir apps/web test src/game/presentation-replay.test.ts`
Expected: FAIL（同じイベントを何度も畳むので盤面が合わない）

戻してから Run: `pnpm --dir apps/web test src/game/presentation-replay.test.ts`
Expected: 2 passed

- [ ] **Step 4: コミット**

```bash
git add apps/web/src/game/presentation-replay.test.ts
git commit -m "test(web): 演出が盤面を変えないことを半荘まるごとで確かめる

1304件を流し切った盤面が、演出なしで畳んだものと一致することを見る。
早送りしても同じであることも見る。**演出は見せ方であって結果ではない。**"
```

---

## Self-Review

**仕様の網羅:** 仕様 6.3（表示状態と論理状態の分離、早送り、再接続時の追いつき）を満たす。

**このウェーブがやらないこと:** 牌が動く補間、2D キャラ、音、`EffectRegistry` へのプラグイン登録。
**まず「正しい時刻に正しい盤面が出る」ことだけを作る。**動きと絵はその上に足せる。

**認められた妥協:**

- 演出は「待つ」だけで、牌は依然として瞬間移動する。`timeline.ts` の `tween` は次のウェーブで使う
- `#fold` の中で `performance.now()` を呼んでいる。これは `apply` が持ち時間バーの
  基準時刻に使うもので、**演出の進行には使っていない**（進行は `Clock` のみ）。
  試験では `pending` を持つイベントを扱わないため揺れない

**型の整合:** `Presentation` は Task 1 で定義し、Task 2・3 はそれを読むだけ。
`GameState` `ClientEventEnvelope` は既存のものをそのまま使う。

**確認済みの事実:** `EffectPlayer` の `presented` が push 順に並ぶこと、`effectOf` が
`match_start` に `null` を返すこと、サーバが `deadline_for(rules, now, bank, lead_in)` で
演出ぶんを締切へ加算していること（`crates/mahjong-engine/src/timing.rs:22`）、
`apply` が `ClientEventEnvelope` を取り `state.lastSeq` に `envelope.seq` を入れることは、
実コードから読み取った。

## 人間に上げること

このウェーブが終わったら `【要確認】` を付けて次を問う。**ループには乗せない。**

- 演出の間（打牌 350ms、リーチ 1,800ms）が体感として長すぎないか短すぎないか
- 牌が瞬間移動することが、補間を入れる前に許容できるか
