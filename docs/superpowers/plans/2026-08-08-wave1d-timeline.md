# Wave 1d: 演出タイムライン骨格 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** イベント列を演出として再生する骨格を作る。スキップ・早送り・再接続時の追いつきが同じ機構で成立することを、描画なしで検証できる状態にする。

**Architecture:** 演出の記述を**時刻 `t` を与えれば状態が決まる宣言的な形**にする。`async/await` で書くとスキップも追いつきも実装できなくなるため、これは設計上の必須要件である。論理状態（`GameState`）と表示状態（`PresentedState`）を分け、演出は論理から遅れて追いつく。

**Tech Stack:** TypeScript 7 / Vite 8 / 依存ライブラリを追加しない（描画は Wave 1c、ここでは純粋なロジックのみ）

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md` の第6章・第7章
**作業規約:** `AGENTS.md`

## Global Constraints

- **編集してよいのは `apps/web/src/timeline/` 配下のみ**
- `apps/web/src/scene/`（Wave 1c の所有）と `apps/web/src/main.ts` を編集しない。結線は Wave 1 完了後にコーディネータが行う
- `apps/web/src/protocol/` は Rust からの生成物である。**手で編集しない**
- `crates/` を一切編集しない
- 描画ライブラリを使わない。このタスクは Three.js に依存しない純粋なロジックである
- 演出時間の値は `protocol` の演出カタログと一致させる。Draw=250 / Discard=350 / Pon=Chi=700 / Kan=1100 / RiichiDeclare=1800 / DoraReveal=800（ms）
- 完了条件は `pnpm --filter @real-mahjong/web typecheck` が通り、`pnpm --filter @real-mahjong/web test` が全通過すること

---

### Task 1: テスト環境と時刻の抽象

演出は時間に依存するため、テストから時刻を制御できないと検証できない。実時間に依存しない形を先に用意する。

**Files:**
- Create: `apps/web/src/timeline/clock.ts`
- Create: `apps/web/src/timeline/clock.test.ts`
- Modify: `apps/web/package.json`
- Create: `apps/web/vitest.config.ts`

**Interfaces:**
- Produces:
  - `export interface Clock { now(): number }`
  - `export class ManualClock implements Clock` — `advance(ms: number): void`, `set(ms: number): void`
  - `export const systemClock: Clock`

- [ ] **Step 1: テストランナーを入れる**

```bash
pnpm --filter @real-mahjong/web add -D vitest
```

`apps/web/package.json` の `scripts` に追記する。

```json
"test": "vitest run",
"test:watch": "vitest"
```

`apps/web/vitest.config.ts`:

```ts
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
  },
});
```

- [ ] **Step 2: 失敗するテストを書く**

`apps/web/src/timeline/clock.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { ManualClock } from "./clock";

describe("ManualClock", () => {
  it("starts at zero and advances by the given amount", () => {
    const clock = new ManualClock();
    expect(clock.now()).toBe(0);
    clock.advance(250);
    expect(clock.now()).toBe(250);
    clock.advance(100);
    expect(clock.now()).toBe(350);
  });

  it("can jump to an absolute time", () => {
    const clock = new ManualClock();
    clock.set(1800);
    expect(clock.now()).toBe(1800);
  });

  it("refuses to go backwards", () => {
    const clock = new ManualClock();
    clock.set(500);
    expect(() => clock.set(100)).toThrow();
    expect(() => clock.advance(-1)).toThrow();
  });
});
```

- [ ] **Step 3: テストが失敗することを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: `./clock` が見つからず失敗

- [ ] **Step 4: 実装を書く**

`apps/web/src/timeline/clock.ts`:

```ts
/**
 * 演出の進行を測る時計。
 *
 * テストから時刻を制御できないと演出は検証できないため、実時間を直接
 * 参照せず必ずこの抽象を通す。
 */
export interface Clock {
  now(): number;
}

/** テスト用。時刻を手で進める。 */
export class ManualClock implements Clock {
  #current = 0;

  now(): number {
    return this.#current;
  }

  advance(ms: number): void {
    if (ms < 0) {
      throw new Error(`時計は戻せない（advance(${ms})）`);
    }
    this.#current += ms;
  }

  set(ms: number): void {
    if (ms < this.#current) {
      throw new Error(`時計は戻せない（${this.#current} → ${ms}）`);
    }
    this.#current = ms;
  }
}

export const systemClock: Clock = {
  now: () => performance.now(),
};
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: 3テスト PASS

- [ ] **Step 6: コミット**

```bash
git add apps/web
git commit -m "feat(web): 演出の時刻を制御できる時計を追加"
```

---

### Task 2: 宣言的タイムライン

**この設計が Wave 1d の中核である。** 演出を「時刻 `t` を与えれば状態が決まる関数」として表す。`async/await` で書くと、スキップも早送りも再接続時の追いつきも実装できなくなる。

**Files:**
- Create: `apps/web/src/timeline/timeline.ts`
- Create: `apps/web/src/timeline/timeline.test.ts`

**Interfaces:**
- Produces:
  - `export interface Timeline<S> { readonly durationMs: number; seek(t: number): S }`
  - `export function constant<S>(value: S, durationMs: number): Timeline<S>`
  - `export function tween(from: number, to: number, durationMs: number, ease?: Easing): Timeline<number>`
  - `export function sequence<S>(parts: Timeline<S>[]): Timeline<S>`
  - `export function parallel<S extends object>(parts: { [K in keyof S]: Timeline<S[K]> }): Timeline<S>`
  - `export function delay(durationMs: number): Timeline<null>`
  - `export type Easing = (t: number) => number`
  - `export const linear: Easing`, `export const easeOutCubic: Easing`

- [ ] **Step 1: 失敗するテストを書く**

```ts
import { describe, expect, it } from "vitest";
import {
  constant,
  delay,
  easeOutCubic,
  linear,
  parallel,
  sequence,
  tween,
} from "./timeline";

describe("tween", () => {
  it("interpolates between the endpoints", () => {
    const t = tween(0, 100, 400);
    expect(t.seek(0)).toBe(0);
    expect(t.seek(200)).toBe(50);
    expect(t.seek(400)).toBe(100);
  });

  it("clamps outside its duration", () => {
    const t = tween(0, 100, 400);
    expect(t.seek(-50)).toBe(0);
    expect(t.seek(9999)).toBe(100);
  });

  it("applies the easing function", () => {
    const t = tween(0, 100, 100, easeOutCubic);
    // 減速するので中間時点では線形より進んでいる
    expect(t.seek(50)).toBeGreaterThan(50);
    expect(t.seek(0)).toBe(0);
    expect(t.seek(100)).toBe(100);
  });
});

describe("sequence", () => {
  it("plays its parts one after another", () => {
    const t = sequence([tween(0, 10, 100), tween(10, 20, 100)]);
    expect(t.durationMs).toBe(200);
    expect(t.seek(0)).toBe(0);
    expect(t.seek(100)).toBe(10);
    expect(t.seek(150)).toBe(15);
    expect(t.seek(200)).toBe(20);
  });

  it("is seekable to any point regardless of order", () => {
    const t = sequence([tween(0, 10, 100), tween(10, 20, 100)]);
    // 後ろから前へ飛んでも同じ値になる。これがスキップと追いつきを可能にする。
    expect(t.seek(180)).toBe(18);
    expect(t.seek(50)).toBe(5);
    expect(t.seek(180)).toBe(18);
  });

  it("an empty sequence has zero duration", () => {
    expect(sequence([]).durationMs).toBe(0);
  });
});

describe("parallel", () => {
  it("advances every part on the same clock", () => {
    const t = parallel({
      x: tween(0, 100, 200),
      y: tween(0, 50, 100),
    });
    expect(t.durationMs).toBe(200);
    expect(t.seek(100)).toEqual({ x: 50, y: 50 });
    // 短い方は終わった値で止まる
    expect(t.seek(200)).toEqual({ x: 100, y: 50 });
  });
});

describe("constant and delay", () => {
  it("hold a single value for their duration", () => {
    const t = constant("held", 300);
    expect(t.durationMs).toBe(300);
    expect(t.seek(0)).toBe("held");
    expect(t.seek(300)).toBe("held");
  });

  it("delay carries no value", () => {
    expect(delay(350).durationMs).toBe(350);
    expect(delay(350).seek(100)).toBeNull();
  });
});

describe("easing", () => {
  it("is a unit interval mapping", () => {
    for (const ease of [linear, easeOutCubic]) {
      expect(ease(0)).toBeCloseTo(0);
      expect(ease(1)).toBeCloseTo(1);
    }
  });
});
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: `./timeline` が見つからず失敗

- [ ] **Step 3: 実装を書く**

```ts
/**
 * 宣言的な演出タイムライン。
 *
 * 時刻 t を与えれば状態が決まる形にしてある。async/await で書くと
 * スキップ・早送り・再接続時の追いつきが実装できなくなるため、
 * この形は設計上の必須要件である（仕様 7.3）。
 */
export interface Timeline<S> {
  readonly durationMs: number;
  seek(t: number): S;
}

export type Easing = (t: number) => number;

export const linear: Easing = (t) => t;

export const easeOutCubic: Easing = (t) => 1 - (1 - t) ** 3;

function clamp01(value: number): number {
  if (value < 0) return 0;
  if (value > 1) return 1;
  return value;
}

export function constant<S>(value: S, durationMs: number): Timeline<S> {
  return {
    durationMs,
    seek: () => value,
  };
}

export function delay(durationMs: number): Timeline<null> {
  return constant(null, durationMs);
}

export function tween(
  from: number,
  to: number,
  durationMs: number,
  ease: Easing = linear,
): Timeline<number> {
  return {
    durationMs,
    seek(t: number): number {
      if (durationMs <= 0) return to;
      const progress = ease(clamp01(t / durationMs));
      return from + (to - from) * progress;
    },
  };
}

export function sequence<S>(parts: Timeline<S>[]): Timeline<S> {
  const total = parts.reduce((sum, part) => sum + part.durationMs, 0);
  return {
    durationMs: total,
    seek(t: number): S {
      if (parts.length === 0) {
        throw new Error("空の sequence は seek できない");
      }
      let remaining = t;
      for (const part of parts) {
        if (remaining <= part.durationMs) {
          return part.seek(remaining);
        }
        remaining -= part.durationMs;
      }
      const last = parts[parts.length - 1]!;
      return last.seek(last.durationMs);
    },
  };
}

export function parallel<S extends object>(parts: {
  [K in keyof S]: Timeline<S[K]>;
}): Timeline<S> {
  const entries = Object.entries(parts) as [keyof S, Timeline<S[keyof S]>][];
  const total = entries.reduce(
    (max, [, part]) => Math.max(max, part.durationMs),
    0,
  );
  return {
    durationMs: total,
    seek(t: number): S {
      const out = {} as S;
      for (const [key, part] of entries) {
        out[key] = part.seek(t);
      }
      return out;
    },
  };
}
```

`sequence` が空のときに `seek` が投げるのは、`durationMs` が 0 の
タイムラインを再生しようとした呼び出し側のバグを早く見つけるためである。

- [ ] **Step 4: テストが通ることを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: 全テスト PASS

- [ ] **Step 5: コミット**

```bash
git add apps/web
git commit -m "feat(web): seek 可能な宣言的タイムラインを追加"
```

---

### Task 3: 演出カタログの共有

サーバとクライアントが同じ表を見ることが要件である。Rust 側の値と一致していることを検査で固定する。

**Files:**
- Create: `apps/web/src/timeline/catalog.ts`
- Create: `apps/web/src/timeline/catalog.test.ts`

**Interfaces:**
- Consumes: `apps/web/src/protocol/ClientEvent`
- Produces:
  - `export type EffectKind = "draw" | "discard" | "pon" | "chi" | "kan" | "riichi_declare" | "dora_reveal"`
  - `export function effectDurationMs(kind: EffectKind): number`
  - `export function effectOf(event: ClientEvent): EffectKind | null`
  - `export function leadInMs(events: ClientEvent[]): number`

- [ ] **Step 1: 失敗するテストを書く**

```ts
import { describe, expect, it } from "vitest";
import type { ClientEvent } from "../protocol/ClientEvent";
import { effectDurationMs, effectOf, leadInMs } from "./catalog";

const discard: ClientEvent = {
  type: "discard",
  seat: 1,
  tile: 3,
  manner: "tedashi",
};

const riichiDeclare: ClientEvent = {
  type: "riichi",
  seat: 1,
  step: "declare",
};

const riichiAccepted: ClientEvent = {
  type: "riichi",
  seat: 1,
  step: "accepted",
};

describe("effect catalog", () => {
  /** Rust 側 protocol::effect と同じ値でなければならない。 */
  it("matches the values frozen in protocol", () => {
    expect(effectDurationMs("draw")).toBe(250);
    expect(effectDurationMs("discard")).toBe(350);
    expect(effectDurationMs("pon")).toBe(700);
    expect(effectDurationMs("chi")).toBe(700);
    expect(effectDurationMs("kan")).toBe(1100);
    expect(effectDurationMs("riichi_declare")).toBe(1800);
    expect(effectDurationMs("dora_reveal")).toBe(800);
  });

  it("maps events to their effect", () => {
    expect(effectOf(discard)).toBe("discard");
    expect(effectOf(riichiDeclare)).toBe("riichi_declare");
    // 成立側は点棒の移動のみで進行を止めない
    expect(effectOf(riichiAccepted)).toBeNull();
  });

  it("bookkeeping events carry no effect time", () => {
    const passed: ClientEvent = {
      type: "action_passed",
      seat: 2,
      window_id: 1,
    };
    expect(effectOf(passed)).toBeNull();
    expect(leadInMs([passed])).toBe(0);
  });

  it("sums only the events that have effects", () => {
    expect(leadInMs([riichiDeclare, discard, riichiAccepted])).toBe(1800 + 350);
  });
});
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: `./catalog` が見つからず失敗

- [ ] **Step 3: 実装を書く**

生成された `ClientEvent` の形に合わせる。`type` の値は Rust の
`#[serde(tag = "type", rename_all = "snake_case")]` に対応する。

```ts
import type { ClientEvent } from "../protocol/ClientEvent";

/**
 * 局の進行を止める演出の種類。
 *
 * **値は Rust の protocol::effect と一致していなければならない。**
 * サーバはこの表で思考時間の締切を計算し、クライアントは同じ表で再生する。
 * ずれると、演出を見ている間に持ち時間が削られるという理不尽が生まれる。
 */
export type EffectKind =
  | "draw"
  | "discard"
  | "pon"
  | "chi"
  | "kan"
  | "riichi_declare"
  | "dora_reveal";

const DURATIONS: Record<EffectKind, number> = {
  draw: 250,
  discard: 350,
  pon: 700,
  chi: 700,
  kan: 1100,
  riichi_declare: 1800,
  dora_reveal: 800,
};

export function effectDurationMs(kind: EffectKind): number {
  return DURATIONS[kind];
}

/** そのイベントが進行を止める演出を伴うか。伴わなければ null。 */
export function effectOf(event: ClientEvent): EffectKind | null {
  switch (event.type) {
    case "draw":
      return "draw";
    case "discard":
      return "discard";
    case "dora_reveal":
      return "dora_reveal";
    case "riichi":
      return event.step === "declare" ? "riichi_declare" : null;
    case "call":
    case "kan_declared":
      switch (event.kind) {
        case "chi":
          return "chi";
        case "pon":
          return "pon";
        default:
          return "kan";
      }
    default:
      return null;
  }
}

/** 直前に届いた一連のイベントの演出時間の合計。 */
export function leadInMs(events: ClientEvent[]): number {
  let total = 0;
  for (const event of events) {
    const kind = effectOf(event);
    if (kind !== null) {
      total += effectDurationMs(kind);
    }
  }
  return total;
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: 全テスト PASS

- [ ] **Step 5: コミット**

```bash
git add apps/web
git commit -m "feat(web): 演出カタログを protocol と同じ値で共有"
```

---

### Task 4: 論理状態と表示状態の分離、そして追いつき

**再接続とタブ復帰が破綻しないための要である。** 受信イベントは即座に論理状態へ反映し、表示は演出タイムラインが消費した分まで遅れて追いつく。

**Files:**
- Create: `apps/web/src/timeline/player.ts`
- Create: `apps/web/src/timeline/player.test.ts`

**Interfaces:**
- Consumes: Task 1 `Clock`、Task 3 `effectOf` / `effectDurationMs`
- Produces:
  - `export type CatchUpPolicy = { speedUpAfterMs: number; skipAfterMs: number }`
  - `export const defaultCatchUp: CatchUpPolicy`
  - `export class EffectPlayer` — `push(event: ClientEvent): void`, `update(): void`, `presented: ClientEvent[]`, `pendingMs: number`, `skip(): void`, `jumpToLatest(): void`, `playbackRate: number`

`presented` は「演出として再生し終えたイベント」を順に積んだ配列である。
描画層（Wave 1c）はこれを見て盤面を作る。

- [ ] **Step 1: 失敗するテストを書く**

```ts
import { describe, expect, it } from "vitest";
import type { ClientEvent } from "../protocol/ClientEvent";
import { ManualClock } from "./clock";
import { EffectPlayer, defaultCatchUp } from "./player";

function discard(seat: number): ClientEvent {
  return { type: "discard", seat, tile: 3, manner: "tedashi" };
}

function draw(seat: number): ClientEvent {
  return { type: "draw", seat, tile: null, source: "wall", wall_remaining: 60 };
}

describe("EffectPlayer", () => {
  it("holds an event until its effect time has elapsed", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock);

    player.push(discard(0));
    player.update();
    expect(player.presented).toHaveLength(0);

    clock.advance(349);
    player.update();
    expect(player.presented).toHaveLength(0);

    clock.advance(1);
    player.update();
    expect(player.presented).toHaveLength(1);
  });

  it("plays queued events in order", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock);

    player.push(discard(0));
    player.push(draw(1));
    clock.advance(350);
    player.update();
    expect(player.presented).toHaveLength(1);

    clock.advance(250);
    player.update();
    expect(player.presented).toHaveLength(2);
    expect(player.presented[1]?.type).toBe("draw");
  });

  /** 進行を止めないイベントは待たずに通す。 */
  it("passes bookkeeping events through immediately", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock);
    player.push({ type: "action_passed", seat: 1, window_id: 1 });
    player.update();
    expect(player.presented).toHaveLength(1);
  });

  it("reports how much effect time is still queued", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock);
    player.push(discard(0));
    player.push(discard(1));
    player.update();
    expect(player.pendingMs).toBe(700);

    clock.advance(350);
    player.update();
    expect(player.pendingMs).toBe(350);
  });

  /** スキップは再生速度を上げるのではなく、待ち時間を捨てる。 */
  it("skip presents everything queued at once", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock);
    player.push(discard(0));
    player.push(discard(1));
    player.skip();
    player.update();
    expect(player.presented).toHaveLength(2);
    expect(player.pendingMs).toBe(0);
  });

  it("speeds up when the queue grows past the threshold", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock, defaultCatchUp);
    expect(player.playbackRate).toBe(1);

    for (let i = 0; i < 10; i += 1) {
      player.push(discard(i % 4));
    }
    player.update();
    expect(player.playbackRate).toBeGreaterThan(1);
  });

  /** リロード復帰では演出を捨てて最新へ飛ぶ。 */
  it("jumpToLatest presents everything and resets the rate", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock, defaultCatchUp);
    for (let i = 0; i < 30; i += 1) {
      player.push(discard(i % 4));
    }
    player.jumpToLatest();
    player.update();
    expect(player.presented).toHaveLength(30);
    expect(player.pendingMs).toBe(0);
    expect(player.playbackRate).toBe(1);
  });

  /** 積み上がりが極端なら演出を飛ばして状態だけ適用する。 */
  it("skips effects entirely once the queue is far behind", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock, defaultCatchUp);
    for (let i = 0; i < 60; i += 1) {
      player.push(discard(i % 4));
    }
    player.update();
    // 一度の update で全部出る（skipAfterMs を超えているため）
    expect(player.presented).toHaveLength(60);
  });
});
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: `./player` が見つからず失敗

- [ ] **Step 3: 実装を書く**

```ts
import type { ClientEvent } from "../protocol/ClientEvent";
import { effectDurationMs, effectOf } from "./catalog";
import type { Clock } from "./clock";

/**
 * 演出が論理状態からどれだけ遅れたら、どう取り戻すか。
 *
 * 少しの遅れは再生を速めて吸収し、大きく遅れたら演出を捨てて状態だけ合わせる。
 */
export type CatchUpPolicy = {
  /** この時間ぶん溜まったら再生を速める */
  speedUpAfterMs: number;
  /** この時間ぶん溜まったら演出を飛ばす */
  skipAfterMs: number;
};

export const defaultCatchUp: CatchUpPolicy = {
  speedUpAfterMs: 1_500,
  skipAfterMs: 6_000,
};

type Queued = {
  event: ClientEvent;
  durationMs: number;
};

/**
 * 受信イベントを演出として再生する。
 *
 * 論理状態は受信した時点で確定しており、`presented` はそこから遅れて
 * 追いつく表示側の状態である（仕様 6.3）。
 */
export class EffectPlayer {
  readonly #clock: Clock;
  readonly #policy: CatchUpPolicy;
  #queue: Queued[] = [];
  #shown: ClientEvent[] = [];
  /** 現在再生中の演出が始まった時刻 */
  #startedAt: number | null = null;
  #rate = 1;

  constructor(clock: Clock, policy: CatchUpPolicy = defaultCatchUp) {
    this.#clock = clock;
    this.#policy = policy;
  }

  get presented(): readonly ClientEvent[] {
    return this.#shown;
  }

  /** まだ再生し終えていない演出の合計時間。 */
  get pendingMs(): number {
    return this.#queue.reduce((sum, item) => sum + item.durationMs, 0);
  }

  get playbackRate(): number {
    return this.#rate;
  }

  push(event: ClientEvent): void {
    const kind = effectOf(event);
    this.#queue.push({
      event,
      durationMs: kind === null ? 0 : effectDurationMs(kind),
    });
  }

  /** 待ち時間を捨てて、いま溜まっている分をすべて表示する。 */
  skip(): void {
    for (const item of this.#queue) {
      this.#shown.push(item.event);
    }
    this.#queue = [];
    this.#startedAt = null;
  }

  /** 再接続やリロードからの復帰。演出を捨てて最新状態へ飛ぶ。 */
  jumpToLatest(): void {
    this.skip();
    this.#rate = 1;
  }

  update(): void {
    this.#rate = this.#rateFor(this.pendingMs);

    if (this.pendingMs >= this.#policy.skipAfterMs) {
      // 大きく遅れている。演出を飛ばして状態だけ合わせる。
      this.skip();
      return;
    }

    // eslint-disable-next-line no-constant-condition
    while (true) {
      const head = this.#queue[0];
      if (head === undefined) {
        this.#startedAt = null;
        return;
      }

      if (head.durationMs === 0) {
        this.#queue.shift();
        this.#shown.push(head.event);
        continue;
      }

      if (this.#startedAt === null) {
        this.#startedAt = this.#clock.now();
      }

      const elapsed = (this.#clock.now() - this.#startedAt) * this.#rate;
      if (elapsed < head.durationMs) {
        return;
      }

      this.#queue.shift();
      this.#shown.push(head.event);
      // 余った時間を次の演出へ持ち越す。
      this.#startedAt += head.durationMs / this.#rate;
    }
  }

  #rateFor(pending: number): number {
    if (pending < this.#policy.speedUpAfterMs) {
      return 1;
    }
    // 遅れに比例して速める。上限は4倍。
    const excess = pending / this.#policy.speedUpAfterMs;
    return Math.min(4, excess);
  }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: 全テスト PASS

- [ ] **Step 5: コミット**

```bash
git add apps/web
git commit -m "feat(web): 論理状態と表示状態を分けた演出プレイヤーを追加"
```

---

### Task 5: 演出プラグインの登録機構

キャラ演出を後から差し込めるようにする。**キャラを一切ロードしなくても基本演出だけでゲームが成立する**ことが要件である。

**Files:**
- Create: `apps/web/src/timeline/plugin.ts`
- Create: `apps/web/src/timeline/plugin.test.ts`

**Interfaces:**
- Consumes: Task 2 `Timeline`
- Produces:
  - `export interface EffectContext { seatOf(viewer: number): number }`
  - `export interface EffectPlugin { id: string; match(event: ClientEvent, ctx: EffectContext): boolean; play(event: ClientEvent, ctx: EffectContext): Timeline<unknown> }`
  - `export class EffectRegistry` — `register(plugin)`, `pluginsFor(event, ctx): EffectPlugin[]`, `has(id): boolean`

- [ ] **Step 1: 失敗するテストを書く**

```ts
import { describe, expect, it } from "vitest";
import type { ClientEvent } from "../protocol/ClientEvent";
import { EffectRegistry, type EffectPlugin } from "./plugin";
import { constant } from "./timeline";

const ctx = { seatOf: (viewer: number) => viewer };

function plugin(id: string, matches: (e: ClientEvent) => boolean): EffectPlugin {
  return {
    id,
    match: (event) => matches(event),
    play: () => constant(id, 100),
  };
}

const discard: ClientEvent = {
  type: "discard",
  seat: 0,
  tile: 1,
  manner: "tedashi",
};

describe("EffectRegistry", () => {
  it("returns every plugin that matches, in registration order", () => {
    const registry = new EffectRegistry();
    registry.register(plugin("base", (e) => e.type === "discard"));
    registry.register(plugin("character", (e) => e.type === "discard"));
    registry.register(plugin("never", () => false));

    const found = registry.pluginsFor(discard, ctx).map((p) => p.id);
    expect(found).toEqual(["base", "character"]);
  });

  it("works with no plugins registered at all", () => {
    const registry = new EffectRegistry();
    expect(registry.pluginsFor(discard, ctx)).toEqual([]);
  });

  it("rejects duplicate ids so a bundle cannot be loaded twice", () => {
    const registry = new EffectRegistry();
    registry.register(plugin("base", () => true));
    expect(() => registry.register(plugin("base", () => true))).toThrow();
  });

  it("reports whether a bundle is loaded", () => {
    const registry = new EffectRegistry();
    expect(registry.has("character")).toBe(false);
    registry.register(plugin("character", () => true));
    expect(registry.has("character")).toBe(true);
  });
});
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: `./plugin` が見つからず失敗

- [ ] **Step 3: 実装を書く**

```ts
import type { ClientEvent } from "../protocol/ClientEvent";
import type { Timeline } from "./timeline";

/** 演出が盤面を参照するための最小限の窓口。 */
export interface EffectContext {
  /** 視点となる席から見た相対位置。描画層が実装する。 */
  seatOf(viewer: number): number;
}

export interface EffectPlugin {
  readonly id: string;
  match(event: ClientEvent, ctx: EffectContext): boolean;
  play(event: ClientEvent, ctx: EffectContext): Timeline<unknown>;
}

/**
 * 演出プラグインの登録簿。
 *
 * キャラ演出は別バンドルとして後から register する。キャラを一切
 * ロードしなくても基本演出だけでゲームが成立する（仕様 7.3）。
 */
export class EffectRegistry {
  readonly #plugins: EffectPlugin[] = [];

  register(plugin: EffectPlugin): void {
    if (this.has(plugin.id)) {
      throw new Error(`演出プラグイン '${plugin.id}' が二重に登録された`);
    }
    this.#plugins.push(plugin);
  }

  has(id: string): boolean {
    return this.#plugins.some((p) => p.id === id);
  }

  /** そのイベントに反応するプラグインを登録順に返す。 */
  pluginsFor(event: ClientEvent, ctx: EffectContext): EffectPlugin[] {
    return this.#plugins.filter((p) => p.match(event, ctx));
  }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: 全テスト PASS

- [ ] **Step 5: 全体を検証してコミット**

```bash
pnpm --filter @real-mahjong/web typecheck
pnpm --filter @real-mahjong/web test
git add apps/web
git commit -m "feat(web): 演出プラグインの登録機構を追加"
```

---

## Wave 1d 完了の判定

- [ ] `pnpm --filter @real-mahjong/web typecheck` が通る
- [ ] `pnpm --filter @real-mahjong/web test` が全通過する
- [ ] 演出時間が Rust の `protocol::effect` と一致している（Task 3 の検査）
- [ ] `sequence` を任意の順に `seek` しても同じ値が返る（Task 2 の検査）
- [ ] `apps/web/src/scene/` と `main.ts` を編集していない
- [ ] 描画ライブラリに依存していない
