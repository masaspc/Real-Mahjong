# Wave 3g 牌の移動 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 牌の瞬間移動をなくす。ツモは山から手元へ、打牌は手牌から河へ動き、残った手牌は新しい並びへ滑る。

**Architecture:** 牌の同一性は**イベントから決める**。配置の差分を見た目で突き合わせるのではなく、`discard` の `seat`/`tile`/`manner`、`draw` の `seat` から「どこからどこへ動いたか」を一意に定める。動きは `seek(t)` で決まる純粋な関数として持ち、毎フレーム `from + ease(t)×(to-from)` を計算し直す。**位置を累積しない。**

**Tech Stack:** TypeScript / Vitest / Three.js

## Global Constraints

- **牌の中間位置を状態として持たない。**イベント・前後の配置・経過時刻だけから毎回計算する。累積すると早送り4倍・6秒超の切り捨て・再接続で**牌が空中に取り残される**
- **`timeline/` は読み取りの窓口を足すだけ。**`EffectPlayer` の再生の挙動（待ち時間・加速・切り捨て）は1行も変えない。Wave 3f では「変更しない」としていたが、**いま何をどこまで再生中かを外から問えないと動きを描けない**ため、`current` の取得のみ足す。既存の試験がそのまま通ることで挙動不変を担保する
- **演出を畳む経路を1つにする。**`?effects=off`・手で叩いた早送り・6秒超の自動切り捨ての3つは、すべて同じ終了処理を通す。**別経路を作ると `effects=off` だけ挙動が違う事故になる。**再接続はここに含めない。Wave 3f で、取り直した backlog は通常の加速と6秒判定に委ねる設計にした
- **動いている牌は掴めない。**移動中はレイキャストの対象から外す。終端で最終配置へ焼き込んでから押せるようにする
- 実時間を直接読まない。時刻は `Clock` のみ。試験は `ManualClock` で駆動する
- 既存の 183 件の試験の期待値を緩めない

## このウェーブの範囲

**ツモ・打牌・手牌の整列だけ**を動かす。鳴き・リーチ棒・ドラめくり（Wave 3h）、和了と流局の結果（Wave 3i）、音とキャラ（Wave 3j）は入れない。**動きの土台を1つ作り、他のイベントは今までどおり即座に反映する。**

---

### Task 1: 再生位置を外から問えるようにする

`EffectPlayer` に「いま再生中のイベントと、その経過時刻」を返す窓口を足す。**足すのは読み取りだけで、再生の挙動は変えない。**

**Files:**
- Modify: `apps/web/src/timeline/player.ts`
- Modify: `apps/web/src/timeline/player.test.ts`

**Interfaces:**
- Produces: `EffectPlayer.current: { event: ClientEvent; durationMs: number; elapsedMs: number } | null`

- [ ] **Step 1: 失敗する試験を書く**

`apps/web/src/timeline/player.test.ts` の末尾（最後の `});` の直前）へ足す。

```ts
  it("再生中のイベントと経過時刻を問える", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock);
    player.push(discardEvent());

    clock.advance(100);
    player.update();

    const current = player.current;
    expect(current?.event).toEqual(discardEvent());
    expect(current?.durationMs).toBe(350);
    expect(current?.elapsedMs).toBe(100);
  });

  it("再生し終えたら現在のイベントは無い", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock);
    player.push(discardEvent());

    clock.advance(350);
    player.update();

    expect(player.current).toBeNull();
  });

  it("経過時刻は演出の長さで頭打ちになる", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock);
    player.push(discardEvent());
    player.push(discardEvent());

    // 1件目を終え、2件目へ 100ms 入ったところ。
    clock.advance(450);
    player.update();

    // **超過分を返してはならない。**`seek` に渡すと終端を越える。
    expect(player.current?.elapsedMs).toBe(100);
  });

  it("早送りすると現在のイベントは無くなる", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock);
    player.push(discardEvent());
    player.skip();

    expect(player.current).toBeNull();
  });
```

試験の先頭に、この節で使う打牌イベントを作る関数を足す。既にある同等のものを使ってよい。

```ts
function discardEvent(): ClientEvent {
  return { type: "discard", seat: 0, tile: 5, manner: "tedashi" };
}
```

- [ ] **Step 2: 試験が落ちることを確かめる**

Run: `pnpm --dir apps/web test src/timeline/player.test.ts`
Expected: FAIL（`current` が無い）

- [ ] **Step 3: 実装する**

`apps/web/src/timeline/player.ts` の `playbackRate` の下へ足す。**他の場所は触らない。**

```ts
  /**
   * いま再生中の演出と、その経過時刻。
   *
   * **牌の動きはこれを唯一の進捗の根拠にする。**描画側で経過を数え直すと、
   * 早送り（最大4倍）や切り捨てとずれ、牌が空中に取り残される。
   *
   * 経過は演出の長さで頭打ちにする。`Timeline.seek` は範囲外を丸めるが、
   * 越えた値を渡すと弧の頂点を過ぎた牌が戻って見える実装を誘発する。
   */
  get current(): {
    event: ClientEvent;
    durationMs: number;
    elapsedMs: number;
  } | null {
    const head = this.#queue[0];
    if (head === undefined || head.durationMs === 0) {
      return null;
    }
    if (this.#startedAt === null) {
      return null;
    }
    const elapsed = (this.#clock.now() - this.#startedAt) * this.#rate;
    return {
      event: head.event,
      durationMs: head.durationMs,
      elapsedMs: Math.max(0, Math.min(elapsed, head.durationMs)),
    };
  }
```

- [ ] **Step 4: 試験が通り、既存の挙動が変わっていないことを確かめる**

Run: `pnpm --dir apps/web test src/timeline/player.test.ts`
Expected: 4 passed 以上（既存の試験がすべて通ったうえで新しい4件が通る）

Run: `pnpm --dir apps/web test`
Expected: 187 passed

- [ ] **Step 5: コミット**

```bash
git add apps/web/src/timeline/player.ts apps/web/src/timeline/player.test.ts
git commit -m "feat(web): 再生中の演出と経過時刻を問える窓口を足す

牌の動きは、経過を数え直さずこれを唯一の根拠にする。**描画側で数えると
早送り（最大4倍）や6秒超の切り捨てとずれ、牌が空中に取り残される。**

再生の挙動は1行も変えていない。既存の試験がそのまま通ることで担保する。"
```

---

### Task 2: 再生中のイベントと、その前後の盤面を出す

`Presentation` に「いま再生中のイベント・経過時刻・それを適用した後の盤面」を足す。
**描画側が前後の配置を作れるようにするためで、状態の持ち方は変えない。**

**Files:**
- Modify: `apps/web/src/game/presentation.ts`
- Modify: `apps/web/src/game/presentation.test.ts`

**Interfaces:**
- Consumes: `EffectPlayer.current`（Task 1）
- Produces: `Presentation.active: { event: ClientEvent; elapsedMs: number; durationMs: number; nextState: GameState } | null`

- [ ] **Step 1: 失敗する試験を書く**

`apps/web/src/game/presentation.test.ts` の `describe` の中へ足す。

```ts
  it("再生中のイベントと、適用後の盤面を出す", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));

    clock.advance(100);
    p.update();

    const active = p.active;
    // 表示中の盤面にはまだ無い。
    expect(p.state.seats[1].river).toHaveLength(0);
    // 適用後の盤面には有る。**この差が動きの起点と着地になる。**
    expect(active?.nextState.seats[1].river).toHaveLength(1);
    expect(active?.elapsedMs).toBe(100);
    expect(active?.durationMs).toBe(350);
  });

  it("再生し終えたら再生中のものは無い", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));

    clock.advance(350);
    p.update();

    expect(p.active).toBeNull();
    expect(p.state.seats[1].river).toHaveLength(1);
  });

  it("再生中の盤面を何度読んでも表示は進まない", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));
    p.update();

    // **毎フレーム読むので、読むたびに表示が進んでは困る。**
    p.active;
    p.active;
    expect(p.state.seats[1].river).toHaveLength(0);
  });
```

- [ ] **Step 2: 試験が落ちることを確かめる**

Run: `pnpm --dir apps/web test src/game/presentation.test.ts`
Expected: FAIL（`active` が無い）

- [ ] **Step 3: 実装する**

`presentation.ts` に控えを1つ足し、`active` を実装する。

```ts
  /** `active` の作り直しを避けるための控え。畳んだ件数が変われば作り直す。 */
  #activeAt: number | null = null;
  #activeNext: GameState | null = null;
```

```ts
  /**
   * いま再生中のイベントと、それを適用した後の盤面。
   *
   * **`state` が動きの起点、`nextState` が着地点である。**再生中は
   * `state` にまだ入っていないので、両方を配置へ直せば差が動きになる。
   */
  get active(): {
    event: ClientEvent;
    elapsedMs: number;
    durationMs: number;
    nextState: GameState;
  } | null {
    const current = this.#player.current;
    if (current === null) {
      return null;
    }
    // 再生中のものは「次に畳むもの」である。
    const item = this.#envelopes[this.#folded];
    if (item === undefined) {
      return null;
    }
    // **毎フレーム畳み直さない。**apply は状態を深く複製するので、
    // 同じイベントのあいだは作った結果を使い回す。
    if (this.#activeAt !== this.#folded || this.#activeNext === null) {
      this.#activeAt = this.#folded;
      this.#activeNext = apply(this.#state, item.envelope, item.receivedAt);
    }
    return {
      event: current.event,
      elapsedMs: current.elapsedMs,
      durationMs: current.durationMs,
      nextState: this.#activeNext,
    };
  }
```

`#fold` の中で、畳んだら控えを捨てる。

```ts
      this.#state = apply(this.#state, item.envelope, item.receivedAt);
      this.#activeNext = null;
```

- [ ] **Step 4: 試験が通ることを確かめる**

このタスクで足した3件だけを見る。名前に「再生」を含むのはこの3件だけである。

Run: `pnpm --dir apps/web test src/game/presentation.test.ts -t 再生`
Expected: 3 passed

ファイル全体と、全体の退行も見る。

Run: `pnpm --dir apps/web test src/game/presentation.test.ts`
Expected: 12 passed

Run: `pnpm --dir apps/web test`
Expected: 190 passed

- [ ] **Step 5: コミット**

```bash
git add apps/web/src/game/presentation.ts apps/web/src/game/presentation.test.ts
git commit -m "feat(web): 再生中のイベントと適用後の盤面を出す

表示中の盤面が動きの起点、適用後の盤面が着地点になる。両方を配置へ直せば
差が動きになる。**適用後の盤面は毎フレーム作り直さず控える。**apply は
状態を深く複製するため。"
```

---

### Task 3: どの牌がどこへ動くかをイベントから決める

前後の配置とイベントから、動く牌の一覧を作る純粋な関数を書く。
**Three.js には触らない。**

**Files:**
- Create: `apps/web/src/scene/motion.ts`
- Create: `apps/web/src/scene/motion.test.ts`

**Interfaces:**
- Consumes: `Placement`（`scene/placement.ts`）、`Vec3`（`scene/layout.ts`）
- Produces: `motionsFor(before: Placement[], after: Placement[], event: ClientEvent, viewer: Seat): Motion[]`
- Produces: `poseAt(motion: Motion, progress: number): Pose`

**補間そのものも純粋関数にする。**`TableScene` の中で計算すると、WebGL 抜きでは
試せないので**目で見るしかなくなる。**弧の高さも回転の向きも、ここで機械的に
確かめられる形にしておく。

**同一性はイベントから決める。配置の見た目で突き合わせない。**
他家の手牌は伏せ牌で `encoded` が 0 なので、見た目で突き合わせると同じ点数の解が
複数でき、**フレームごとに違う牌が選ばれて手牌がちらつく。**席とイベントから
一意に決まる規則にする。

規則は3段で、**上から順に当てて、当たったものは下の段では扱わない。**

**第1段。領域をまたぐ牌をイベントから決める。**

| イベント | 起点 | 着地 |
|---|---|---|
| ツモ（自分） | 消えた山の牌 | `drawn-${席}` |
| ツモ（他家） | 消えた山の牌 | その席の手牌の**添字が最大**の牌 |
| 打牌（自分・ツモ切り） | `drawn-${自分}` | 新しくできた河の牌 |
| 打牌（自分・手出し） | `hand-${自分}-${i}`（`encoded` が一致する最初のもの） | 新しくできた河の牌 |
| 打牌（自分・手出し）の**ツモ牌** | `drawn-${自分}` | ツモ牌が入った `hand-${自分}-${j}` |
| 打牌（他家） | その席の手牌の添字が最大の牌 | 新しくできた河の牌 |

**手出しでは牌が2枚動く。**手出しをすると、それまで手牌から離れて置かれていた
ツモ牌が手牌へ吸収される（`state.ts:203` の `state.hand.push(state.drawn)`）。
`drawn-0` は消え、その牌は `hand-0-${j}` として現れる。**鍵が変わるので、
第2段（`hand-*` どうしの対応）も第3段（同じ鍵の対応）も拾えない。**
ここを第1段に入れないと、切った牌が河へ飛ぶ横でツモ牌が手牌へ瞬間移動する。

着地の添字 `j` は、第2段の対応を取ったあとに**余った `after` の手牌の位置**である。
ツモ牌と同じ牌が手牌にもある場合は、余りが一意に決まるのでそれを使う。

**消えた山の牌は番号が最も小さいものである。**`placement.ts:304` の
`slot = WALL_SLOTS - wallRemaining + index` は、残りが減るほど**開始の番号が
上がる**。したがって `wall-135` は常に残り、消えるのは下端である。
**「最も大きいもの」を選ぶと、両方に存在する牌を掴んで動きが1つも出ない。**

**第2段。自分の手牌の中の並び替え。**
`before` と `after` の `hand-${自分}-*` を、**同じ `encoded` の n 個目どうし**で
対応させる。第1段で起点になった牌は除く。自分の手牌はツモのたびに
`sortTiles` で並べ替わる（`state.ts:207`）ので、鍵で対応させると**中身が
入れ替わった牌が滑って見える。**

**第3段。それ以外で、鍵が前後の両方にあり姿勢が変わったもの。**
そのまま滑らせる。`hand-${自分}-*` は第2段が扱うので除く。

**この第3段が無いと Goal を達成できない。**手牌も河も列を中央揃えするため、
枚数が1枚変わると**残りの牌がすべて半スロットずれる。**他家がツモると
その席の13枚が、リーチ宣言牌が河へ入るとその段の牌が、それぞれ動く。
1枚だけを動かして残りを瞬間移動させたのでは、瞬間移動は無くならない。

- [ ] **Step 1: 型と失敗する試験を書く**

`apps/web/src/scene/motion.test.ts`：

```ts
import { describe, expect, it } from "vitest";

import { motionsFor, poseAt } from "./motion";
import type { Motion } from "./motion";
import { placementsFor } from "./placement";
import { apply, emptyState } from "../game/state";
import type { GameState } from "../game/state";
import type { ClientEvent } from "../protocol/ClientEvent";
import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";

function envelope(seq: number, event: ClientEvent): ClientEventEnvelope {
  return { seq, event };
}

/**
 * 東1局の頭。自分に13枚、他家も13枚、山は70枚。
 *
 * **手牌は `round_start` では配られない。**局の情報を運ぶのは `round_start`、
 * 手牌を配るのは `deal` である。`round_start` に `hand` を書くと型が通らず、
 * 通したとしても全員の手牌が0枚のまま試験が無意味になる。
 */
function started(): GameState {
  let state = emptyState(0);
  state = apply(
    state,
    envelope(0, {
      type: "round_start",
      round: { wind: "East", number: 1 },
      dealer: 0,
      honba: 0,
      riichi_sticks: 0,
      scores: [25000, 25000, 25000, 25000],
      seed_commit: "x",
    }),
    0,
  );
  state = apply(
    state,
    envelope(1, {
      type: "deal",
      your_hand: [0, 1, 2, 9, 10, 11, 18, 19, 20, 27, 27, 33, 33],
      hand_sizes: [13, 13, 13, 13],
      dora_indicator: 5,
    }),
    0,
  );
  return state;
}

/**
 * 自分がツモった直後。手牌13枚 + ツモ牌1枚。
 *
 * **打牌の試験はここから始める。**13枚から切ると12枚になり、実際の局には
 * 現れない形になる。ツモ→打牌の 14→13 が本来の経路である。
 */
function drawnMine(): GameState {
  return apply(
    started(),
    envelope(2, { type: "draw", seat: 0, tile: 4, source: "wall", wall_remaining: 69 }),
    0,
  );
}

/** 他家がツモった直後。その席だけ14枚。 */
function drawnOther(): GameState {
  return apply(
    started(),
    envelope(2, { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 }),
    0,
  );
}

function step(state: GameState, event: ClientEvent): {
  before: GameState;
  after: GameState;
} {
  return { before: state, after: apply(state, envelope(1, event), 0) };
}

describe("motionsFor", () => {
  it("自分のツモは山から手元へ動く", () => {
    const event: ClientEvent = {
      type: "draw",
      seat: 0,
      tile: 4,
      source: "wall",
      wall_remaining: 69,
    };
    const { before, after } = step(started(), event);
    const motions = motionsFor(
      placementsFor(before, 0),
      placementsFor(after, 0),
      event,
      0,
    );

    const drawn = motions.find((m) => m.toKey === "drawn-0");
    expect(drawn).toBeDefined();
    expect(drawn?.fromKey.startsWith("wall-")).toBe(true);
    // 山から持ち上げて置く。**真横に滑らせると起点が分からない。**
    expect(drawn?.lift).toBeGreaterThan(0);
  });

  it("他家のツモは山からその席の手牌の端へ動く", () => {
    const event: ClientEvent = {
      type: "draw",
      seat: 1,
      tile: null,
      source: "wall",
      wall_remaining: 69,
    };
    const { before, after } = step(started(), event);
    const motions = motionsFor(
      placementsFor(before, 0),
      placementsFor(after, 0),
      event,
      0,
    );

    const drawn = motions.find((m) => m.toKey.startsWith("hand-1-"));
    expect(drawn?.fromKey.startsWith("wall-")).toBe(true);
    // **1件だけを期待してはいけない。**手牌は列を中央揃えするので、
    // 13枚が14枚になると残りの13枚も動く。それも動きとして出る。
    expect(motions.length).toBeGreaterThan(1);
  });

  it("そのまま居座る牌以外は、すべて動きで説明できる", () => {
    // **これがこのウェーブの Goal そのものである。**
    //
    // 「位置が変わったものに動きがあるか」では足りない。前の盤面に鍵が
    // 無い牌（ツモ牌が手牌へ吸収されるなど）を飛ばしてしまい、**瞬間移動が
    // 起きるまさにその場所を検査しない。**
    //
    // 正しい言い方はこうである。**後の盤面のどの牌も、前と寸分違わず
    // 同じ場所に同じ姿勢で在ったか、さもなくば動いて来たかのどちらかである。**
    const scenarios: { name: string; from: GameState; event: ClientEvent }[] = [
      {
        name: "自分のツモ",
        from: started(),
        event: { type: "draw", seat: 0, tile: 4, source: "wall", wall_remaining: 69 },
      },
      {
        name: "他家のツモ",
        from: started(),
        event: { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
      },
      {
        name: "自分のツモ切り",
        from: drawnMine(),
        event: { type: "discard", seat: 0, tile: 4, manner: "tsumogiri" },
      },
      {
        name: "自分の手出し",
        from: drawnMine(),
        event: { type: "discard", seat: 0, tile: 0, manner: "tedashi" },
      },
      {
        name: "他家の打牌",
        from: drawnOther(),
        event: { type: "discard", seat: 1, tile: 7, manner: "tsumogiri" },
      },
    ];

    for (const scenario of scenarios) {
      const { before, after } = step(scenario.from, scenario.event);
      const beforeAll = placementsFor(before, 0);
      const afterAll = placementsFor(after, 0);
      const motions = motionsFor(beforeAll, afterAll, scenario.event, 0);

      const wasAt = new Map(beforeAll.map((p) => [p.key, p]));
      const covered = new Set(motions.map((m) => m.toKey));

      for (const p of afterAll) {
        const old = wasAt.get(p.key);
        // 位置だけでなく姿勢も見る。**手牌は立ち河は寝るので、回転を見ないと
        // 「同じ場所で向きだけ変わった」牌を見逃す。**
        const stayed =
          old !== undefined &&
          old.position.x === p.position.x &&
          old.position.y === p.position.y &&
          old.position.z === p.position.z &&
          old.rotationX === p.rotationX &&
          old.rotationY === p.rotationY;
        if (!stayed && !covered.has(p.key)) {
          throw new Error(`${scenario.name}: ${p.key} が動きなしで現れた`);
        }
      }
    }
  });

  it("弧の頂点は両端より高い", () => {
    const motion: Motion = {
      id: "x",
      fromKey: "hand-0-0",
      toKey: "river-0-0",
      encoded: 4,
      faceUp: true,
      from: { position: { x: 0, y: 0.6, z: 10 }, rotationX: 0, rotationY: 0 },
      to: { position: { x: 0, y: 0.3, z: 7 }, rotationX: -Math.PI / 2, rotationY: 0 },
      lift: 1.2,
    };

    // **両端では弧が 0 になる。**ここが 0 でないと、置いた瞬間に牌が沈む。
    expect(poseAt(motion, 0).position.y).toBeCloseTo(0.6);
    expect(poseAt(motion, 1).position.y).toBeCloseTo(0.3);
    // 途中は両端のどちらより高い。
    expect(poseAt(motion, 0.5).position.y).toBeGreaterThan(0.6);
  });

  it("回転は最短の向きへ回る", () => {
    const motion: Motion = {
      id: "y",
      fromKey: "a",
      toKey: "b",
      encoded: 0,
      faceUp: false,
      from: { position: { x: 0, y: 0, z: 0 }, rotationX: 0, rotationY: Math.PI * 0.9 },
      to: { position: { x: 0, y: 0, z: 0 }, rotationX: 0, rotationY: -Math.PI * 0.9 },
      lift: 0,
    };

    // 0.9π から -0.9π へは、0 を通る 1.8π ではなく π を跨ぐ 0.2π が近い。
    // **素直に線形補間すると牌が一周する。**
    const mid = poseAt(motion, 0.5).rotationY;
    expect(Math.abs(mid)).toBeGreaterThan(Math.PI * 0.9);
  });

  it("他家の打牌は手牌の端から河へ動く", () => {
    const event: ClientEvent = {
      type: "discard",
      seat: 1,
      tile: 7,
      manner: "tsumogiri",
    };
    // **13枚から切らない。**実際の局では必ずツモを挟んだ 14 枚から切る。
    const { before, after } = step(drawnOther(), event);
    const motions = motionsFor(
      placementsFor(before, 0),
      placementsFor(after, 0),
      event,
      0,
    );

    const toRiver = motions.find((m) => m.toKey === "river-1-0");
    expect(toRiver).toBeDefined();
    expect(toRiver?.fromKey.startsWith("hand-1-")).toBe(true);
  });

  it("同じ盤面なら何も動かない", () => {
    const state = started();
    const event: ClientEvent = { type: "dora_reveal", indicator: 8 };
    const motions = motionsFor(
      placementsFor(state, 0),
      placementsFor(state, 0),
      event,
      0,
    );
    expect(motions).toHaveLength(0);
  });

  it("動きは起点と着地の両方の姿勢を持つ", () => {
    const event: ClientEvent = {
      type: "discard",
      seat: 1,
      tile: 7,
      manner: "tsumogiri",
    };
    const { before, after } = step(started(), event);
    const motions = motionsFor(
      placementsFor(before, 0),
      placementsFor(after, 0),
      event,
      0,
    );

    const m = motions[0];
    expect(m).toBeDefined();
    // 手牌は立ち、河は寝る。**姿勢を補間しないと途中で牌が刺さって見える。**
    expect(m?.from.rotationX).not.toBe(m?.to.rotationX);
  });
});
```

- [ ] **Step 2: 試験が落ちることを確かめる**

Run: `pnpm --dir apps/web test src/scene/motion.test.ts`
Expected: FAIL（`motion.ts` が無い）

- [ ] **Step 3: 実装する**

`apps/web/src/scene/motion.ts` を作る。要点は次のとおり。

- `Pose = { position: Vec3; rotationX: number; rotationY: number }`
- `Motion = { id: string; fromKey: string; toKey: string; encoded: Tile; faceUp: boolean; from: Pose; to: Pose; lift: number }`
- `before` と `after` を `Map<string, Placement>` にする
- **第1段。**イベントの型で分岐し、上の表のとおり起点と着地の鍵を決める
  - 「消えた山の牌」は `before` にあって `after` に無い `wall-` の鍵。
    **複数あるときは番号が最も小さいものを選ぶ。**`slot` は
    `WALL_SLOTS - wallRemaining + index` なので、残りが減ると開始番号が上がる。
    **番号が大きいほうを選ぶと、両方に在る牌を掴んで動きが1つも出ない**
  - 「新しくできた河の牌」は `after` にあって `before` に無い `river-${席}-` の鍵
  - 他家の手牌の端は、`hand-${席}-` のうち**添字が最大**のもの
- **第2段。**自分の席の `hand-${viewer}-*` を、`encoded` ごとに出現順で対応させる。
  第1段で起点に使った鍵は除く。位置が変わったものだけ動きにする
- **第3段。**残りのうち、鍵が前後の両方にあって姿勢が変わったものを滑らせる。
  `hand-${viewer}-*` は第2段が扱ったので除く
- 起点も着地も見つからないときは**動きを作らない。**当てずっぽうで動かすより、
  今までどおり即座に置いたほうがよい
- `lift` は第1段のみ正の値（ツモ 0.8、打牌 1.2）、第2段と第3段は 0
- `id` は `${fromKey}->${toKey}` とする。代理メッシュの鍵に使う

- [ ] **Step 4: 試験が通ることを確かめる**

Run: `pnpm --dir apps/web test src/scene/motion.test.ts`
Expected: 8 passed

Run: `pnpm --dir apps/web test`
Expected: 198 passed

- [ ] **Step 5: コミット**

```bash
git add apps/web/src/scene/motion.ts apps/web/src/scene/motion.test.ts
git commit -m "feat(web): どの牌がどこへ動くかをイベントから決める

**配置の見た目で突き合わせない。**他家の手牌は伏せ牌で encoded が 0 なので、
見た目で対応を取ると同じ点数の解が複数でき、フレームごとに違う牌が選ばれて
手牌がちらつく。席とイベントから一意に決まる規則にした。"
```

---

### Task 4: 動かして描く

`TableScene` を「確定した配置」と「移動中の代理」に分け、動きを毎フレーム計算し直して描く。
**移動中の牌は掴めないようにする。**

**Files:**
- Modify: `apps/web/src/scene/table.ts`
- Modify: `apps/web/src/main.ts`
- Modify: `apps/web/preview.html`
- Modify: `apps/web/src/preview.ts`

**Interfaces:**
- Consumes: `motionsFor`（Task 3）、`Presentation.active`（Task 2）、`tween` `easeOutCubic`（`timeline/timeline.ts`）
- Produces: `TableScene.syncWithMotion(placements: Placement[], motions: Motion[], progress: number): void`

- [ ] **Step 1: 描き分けを実装する**

`table.ts` に次を足す。`sync` はそのまま残す（動きが無いときの経路）。

- `syncWithMotion(placements, motions, progress)` は
  1. 動きの**着地の鍵**を集める
  2. その鍵を除いた `placements` を `sync` に渡して確定ぶんを置く
  3. 動きごとに代理のメッシュを1つ持ち、`from` と `to` を `progress` で補間して置く
  4. 補間は `from + easeOutCubic(t)×(to-from)`。**位置を足し込まない**
  5. 弧は `y` に `lift × 4t(1-t)` を足す（t=0 と t=1 で 0 になる）
  6. 代理のメッシュは `pickHandTile` の対象から外す
- 動きが空なら `sync` と同じ結果になること

**確定ぶんを置く処理を、公開の `sync` から切り出す。**

`sync` の中身をそのまま `#place(placements)` という私有のものへ移し、
公開の `sync` と `syncWithMotion` の両方がそれを呼ぶ形にする。

```ts
  /** 動きが無いときの入口。**代理をすべて捨ててから置く。** */
  sync(placements: Placement[]): void {
    this.#dropMoving();
    this.#place(placements);
  }

  /** 動きがあるときの入口。**今回の動きに無い代理だけを捨てる。** */
  syncWithMotion(placements: Placement[], motions: Motion[], progress: number): void {
    const alive = new Set(motions.map((m) => m.id));
    this.#dropMoving((id) => !alive.has(id));
    const landing = new Set(motions.map((m) => m.toKey));
    this.#place(placements.filter((p) => !landing.has(p.key)));
    // ...代理を progress で置く
  }
```

**`syncWithMotion` から公開の `sync` を呼んではならない。**呼ぶと毎フレーム
自分の代理を捨てて作り直すことになり、動きが1フレームも続かない。

**代理のメッシュは別の入れ物で持つ。**`#meshes` へ入れてはならない。
`#place` は「`placements` に無い鍵のメッシュを消す」（`table.ts:138`）ので、
**次のフレームで代理が消えてしまう。**

```ts
  /** 移動中の代理。`#meshes` とは別に持つ。鍵は Motion の id。 */
  readonly #moving = new Map<string, TileMesh>();
```

回収の規則を決めておく。**決めないと、イベントが切り替わった瞬間に前の代理が
卓上へ residue として残る。**

- `#dropMoving(pred?)` は、述語が無ければ全部、あれば真になった id だけを捨てる
- `sync` は全部捨てる。**動きが無い＝空中に牌があってはならない**
- `syncWithMotion` は今回の動きに無い id だけを捨てる
- `dispose` で代理もすべて捨てる

**残した代理は、毎回 `encoded` `faceUp` と UV を上書きする。**id は
`${fromKey}->${toKey}` なので、局が変わって同じ鍵の組み合わせが再び現れると
**前のイベントの牌面と表裏が residue として残る。**確定ぶんが
`applyFaceUv(...)` を毎回呼んでいる（`table.ts:157`）のと同じ理由である。
`from` と `to` も毎回入れ直す。**使い回すのはメッシュだけで、中身ではない。**

**回転は最短の向きへ回す。**`rotationX` と `rotationY` をそのまま線形に
補間すると、たとえば `π` から `-π` へ回すときに一周して見える。差を
`[-π, π]` へ畳んでから補間する。

```ts
/** 角度の差を [-π, π] へ畳む。**畳まないと牌が大回りする。** */
function shortestDelta(from: number, to: number): number {
  let d = (to - from) % (Math.PI * 2);
  if (d > Math.PI) d -= Math.PI * 2;
  if (d < -Math.PI) d += Math.PI * 2;
  return d;
}
```

補間は `from + shortestDelta(from, to) * easeOutCubic(t)` とする。

`main.ts` の描画ループを次のようにする。

```ts
const renderFrame = (): void => {
  presentation.update();
  const active = presentation.active;
  if (active === null) {
    scene.sync(placementsFor(presentation.state));
  } else {
    const before = placementsFor(presentation.state);
    const after = placementsFor(active.nextState);
    const motions = motionsFor(before, after, active.event, presentation.state.you);
    // **進捗は再生器の時計から作る。**ここで数え直すと早送りとずれる。
    const progress = active.durationMs === 0 ? 1 : active.elapsedMs / active.durationMs;
    scene.syncWithMotion(after, motions, progress);
  }
  renderBoard(uiRoot, presentation.state, (command) => connection.send(command));
  scene.render();
  requestAnimationFrame(renderFrame);
};
```

- [ ] **Step 2: 型検査とビルドが通ることを確かめる**

Run: `pnpm --dir apps/web typecheck && pnpm --dir apps/web build`
Expected: エラー0件でビルド成功

- [ ] **Step 3: 既存の試験が壊れていないことを確かめる**

Run: `pnpm --dir apps/web test`
Expected: 198 passed

- [ ] **Step 4: 人が見るための口を足す（合否判定には使わない）**

`preview.html` に、動きの途中を止めて描く口を足す。`?motion=0.5` のように
進捗を与えると、打牌の動きをその進捗で止めた絵を出す。

**この絵の良し悪しはループの合否にしない。**「自然に見えるか」は主観であり、
規約どおり `【要確認】` で人間へ上げる。**ループが判定するのは
`poseAt` の試験（Task 3）までである。**弧の高さも回転の向きも、そこで
機械的に確かめてある。

撮り方だけ残しておく。人間が見るときに使う。

```bash
pnpm --dir apps/web build
cargo run -p server --bin serve &
sleep 5
for t in 0 0.5 1; do
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless=new --disable-gpu --use-angle=swiftshader --enable-unsafe-swiftshader \
  --window-size=1280,720 --virtual-time-budget=20000 \
  --screenshot=motion-$t.png "http://127.0.0.1:8080/preview.html?motion=$t"
done
```

Expected: 3枚の png が書き出される。**中身の良し悪しは判定しない。**
書き出しに失敗した場合のみ、この手順の失敗とする。

- [ ] **Step 5: コミット**

```bash
git add apps/web/src/scene/table.ts apps/web/src/main.ts apps/web/preview.html apps/web/src/preview.ts
git commit -m "feat(web): 牌を動かして描く

確定した配置と移動中の代理を分ける。進捗は再生器の時計から作り、毎フレーム
from + ease(t)×(to-from) を計算し直す。**位置を足し込まない。**足し込むと
早送り4倍や6秒超の切り捨てとずれ、牌が空中に取り残される。

移動中の牌はレイキャストの対象から外す。終端で最終配置へ焼き込んでから押せる。"
```

---

### Task 5: 演出を畳む経路が牌を空中に残さないことを固定する

演出を途中でやめる経路は**3つ**ある。`?effects=off`・手で叩いた早送り・
6秒超の自動切り捨て。いずれも `Presentation.skip()` か `EffectPlayer` 内部の
`skip()` を通り、キューを空にして畳む。**どの経路でも `active` が `null` に
なる**ことを試験で固定する。`active` が残ると、代理の牌が空中で止まる。

**再接続はこの3つに含めない。**Wave 3f で、再接続では `jumpToLatest()` を
呼ばず、取り直した backlog を通常の加速と6秒判定へ渡す設計にした
（`main.ts` の `onStatus` にその旨を書いてある）。**「4経路が同じ出口」と
書くのは実配線と食い違う。**`jumpToLatest()` は API として残っているので、
呼んだときに `active` が消えることだけを別に確かめる。

**Files:**
- Create: `apps/web/src/game/settle.test.ts`
- Modify: `apps/web/src/game/presentation.ts`

**`main.ts` は触らない。**このタスクは既にある振る舞いを試験で固定するもので、
結線は Task 4 で済んでいる。試験が落ちた場合は `presentation.ts` を直す。
**それでも直らないときは、勝手に設計を変えずに worker_done へ書いて終える。**

**Interfaces:**
- Consumes: `Presentation`（Task 2）

- [ ] **Step 1: 試験を書く**

```ts
import { describe, expect, it } from "vitest";

import { Presentation } from "./presentation";
import { ManualClock } from "../timeline/clock";
import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import type { Seat } from "../protocol/Seat";
import type { Tile } from "../protocol/Tile";

function discard(seq: number, seat: Seat, tile: Tile): ClientEventEnvelope {
  return { seq, event: { type: "discard", seat, tile, manner: "tedashi" } };
}

describe("早送りの出口", () => {
  it("手で早送りすると再生中のものが無くなる", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));
    p.update();
    expect(p.active).not.toBeNull();

    p.skip();
    // **動きが残ってはいけない。**残ると牌が空中で止まる。
    expect(p.active).toBeNull();
    expect(p.state.seats[1].river).toHaveLength(1);
  });

  it("6秒を超えて溜まると自動で切り捨て、再生中のものが無くなる", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    for (let i = 1; i <= 20; i += 1) {
      p.receive(discard(i, 1, 5));
    }
    p.update();

    expect(p.active).toBeNull();
    expect(p.state.seats[1].river).toHaveLength(20);
  });

  it("最新へ飛ばすと再生中のものが無くなる", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));
    p.update();

    p.jumpToLatest();
    expect(p.active).toBeNull();
  });
});
```

- [ ] **Step 2: 試験が通ることを確かめる**

Run: `pnpm --dir apps/web test src/game/settle.test.ts`
Expected: 3 passed

落ちる場合は `Presentation` 側を直す。**試験の期待値を緩めない。**

- [ ] **Step 3: 全体を確かめる**

Run: `pnpm --dir apps/web test`
Expected: 201 passed

Run: `pnpm --dir apps/web typecheck && pnpm --dir apps/web build`
Expected: エラー0件でビルド成功

- [ ] **Step 4: コミット**

```bash
git add apps/web/src/game/settle.test.ts apps/web/src/game/presentation.ts
git commit -m "test(web): 演出を畳む経路が牌を空中に残さないことを固定する

?effects=off・手で叩いた早送り・6秒超の自動切り捨ての3つが、いずれも
active を null にすることを見る。**残ると代理の牌が空中で止まる。**

再接続はここに含めない。Wave 3f で、取り直した backlog は通常の加速と
6秒判定に委ねる設計にしてある。"
```

---

## Self-Review

**仕様の網羅:** 仕様 7.3（seek 可能な演出）と 6.3（表示と論理の分離、早送り）を満たす。
仕様 3.1 の「動く牌のみ個別メッシュ」のうち、動く牌の側を作る。

**このウェーブがやらないこと:** 鳴きの集合と2Dコール表示、リーチ棒、ドラめくり（Wave 3h）、
和了と流局の結果表示（Wave 3i）、音とキャラ（Wave 3j）、**山のバッチ化**。
山は現在 `wallRemaining` ぶんの Mesh を個別に作っており仕様 3.1 に反しているが、
**動きの土台と同時に触ると原因の切り分けができなくなる**ので Wave 3h で直す。

**認められた妥協:**

- 動きはツモ・打牌・手牌の整列だけ。鳴きや和了は今までどおり即座に置く
- 弧は `y` に放物線を足すだけで、物理的な投げ上げではない
- 代理のメッシュは動きごとに1つ作る。同時に動くのは最大でも手牌13枚ぶんで、
  仕様の目安（20枚）に収まる

**型の整合:** `Motion` は Task 3 で定義し、Task 4 が読む。`Placement` `GameState`
`ClientEvent` は既存のものをそのまま使う。

**確認済みの事実:** `EffectPlayer` が `#queue[0]` と `#startedAt` と `#rate` から経過を
出していること（`apps/web/src/timeline/player.ts:121`）、`Presentation` の
「次に畳むもの」が `#envelopes[#folded]` であること、`placementsFor` が Three.js に
触らない純粋な関数であること、手牌が打牌のたびに `sortTiles` で並べ替わること
（`apps/web/src/game/state.ts:207`）は、実コードから読み取った。

## 人間に上げること

このウェーブが終わったら `【要確認】` を付けて次を問う。**ループには乗せない。**

- 動きの速さと弧の高さが自然か（`preview.html?motion=0/0.5/1` の3枚を見る）
- ツモ・打牌以外がまだ瞬間移動であることが、次のウェーブまで許容できるか

**このウェーブでも瞬間移動が残る場面**（Codex が実コードから挙げたもの）:
鳴き（手牌→副露、河→副露）、加槓による既存副露の組み替え、ドラ表示の追加、
局の開始と終了。いずれも Wave 3h 以降の範囲である。
