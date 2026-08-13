# Wave 3e-1 遊べる 2D クライアント実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** **このウェーブの終わりに、ブラウザを開けば CPU 3人と半荘が打てる。**

**Architecture:** サーバから届く `ClientEvent` を畳んで盤面の状態にし、その状態を描き、クリックを `Command` にして返す。**描画と状態を分ける。**状態の組み立てはブラウザに依存しない純粋な関数にして試験する。3D への差し替え（Wave 3e-2）で捨てるのは描画だけになる。

**Tech Stack:** TypeScript / Vite / Vitest。**新しい依存は入れない。**

---

## Global Constraints

- **`apps/web/src/protocol/` を手で編集しない。**`cargo test --package protocol` が生成する。
- **`apps/web/src/scene/` と `apps/web/src/timeline/` を変更しない。**Wave 1c・1d で完成しており、3D へ差し替えるときに使う。
- Rust 側（`crates/`）を一切変更しない。
- 日本語のコメントとテスト名を使う。
- `pnpm --dir apps/web typecheck` と `pnpm --dir apps/web test` を通す。
- テストは仕様である。**期待値を実装に合わせて書き換えてはならない。**
- 見た目を作り込まない。**遊べることが目的であり、美しさは Wave 3e-2 以降。**

---

## 前提（確認済みの事実）

- 牌は `0..=36` の数値。`0..=8` が 1m..9m、`9..=17` が 1p..9p、`18..=26` が 1s..9s、`27..=33` が 1z..7z（東南西北白發中）、`34/35/36` が赤5m/赤5p/赤5s。
- サーバは `ws://127.0.0.1:8080/ws?table=<id>&last_seq=<n>` で待つ。下りは `ClientEventEnvelope` の JSON、上りは `Command` の JSON。
- `RequestAction { window_id, options, deadline_ms }` の `deadline_ms` は**受け取った時点からの残りミリ秒**。再接続で送り直されるときはサーバが引き直してくれる。
- `ActionOption` は `discard{allowed, riichi_allowed}` / `chi{candidates}` / `pon{candidates}` / `kan{candidates}` / `ron` / `tsumo` / `kyuushu` / `pass`。

---

## File Structure

| ファイル | 責務 |
|---|---|
| `apps/web/src/game/tiles.ts` | 牌の表記と並べ替え |
| `apps/web/src/game/state.ts` | `ClientEvent` を畳んで盤面の状態にする |
| `apps/web/src/net/connection.ts` | WebSocket の接続・再接続・送信 |
| `apps/web/src/ui/board.ts` | 状態を DOM に描き、クリックを `Command` にする |
| `apps/web/src/ui/board.css` | 最低限の見た目 |
| `apps/web/src/main.ts` | 結線 |

---

### Task 1: 牌の表記

**Files:**
- Create: `apps/web/src/game/tiles.ts`
- Test: `apps/web/src/game/tiles.test.ts`

**Interfaces:**
- Produces: `tileLabel(tile: Tile): string`、`isRed(tile: Tile): boolean`、`sortTiles(tiles: Tile[]): Tile[]`、`kindOf(tile: Tile): number`

- [ ] **Step 1: 失敗するテストを書く**

`apps/web/src/game/tiles.test.ts`:

```typescript
import { describe, expect, it } from "vitest";

import { isRed, kindOf, sortTiles, tileLabel } from "./tiles";

describe("牌の表記", () => {
  it("数牌を数字と種類で表す", () => {
    expect(tileLabel(0)).toBe("1m");
    expect(tileLabel(8)).toBe("9m");
    expect(tileLabel(9)).toBe("1p");
    expect(tileLabel(17)).toBe("9p");
    expect(tileLabel(18)).toBe("1s");
    expect(tileLabel(26)).toBe("9s");
  });

  it("字牌を漢字で表す", () => {
    expect(tileLabel(27)).toBe("東");
    expect(tileLabel(28)).toBe("南");
    expect(tileLabel(29)).toBe("西");
    expect(tileLabel(30)).toBe("北");
    expect(tileLabel(31)).toBe("白");
    expect(tileLabel(32)).toBe("發");
    expect(tileLabel(33)).toBe("中");
  });

  it("赤ドラを 0 で表す", () => {
    expect(tileLabel(34)).toBe("0m");
    expect(tileLabel(35)).toBe("0p");
    expect(tileLabel(36)).toBe("0s");
  });

  it("赤ドラを見分ける", () => {
    expect(isRed(34)).toBe(true);
    expect(isRed(35)).toBe(true);
    expect(isRed(36)).toBe(true);
    expect(isRed(4)).toBe(false);
  });

  it("赤ドラは同じ種類の5として扱う", () => {
    expect(kindOf(34)).toBe(kindOf(4));
    expect(kindOf(35)).toBe(kindOf(13));
    expect(kindOf(36)).toBe(kindOf(22));
  });

  it("範囲外を拒む", () => {
    expect(() => tileLabel(37)).toThrow();
    expect(() => tileLabel(-1)).toThrow();
  });

  it("萬子・筒子・索子・字牌の順に並べ、赤ドラは同じ5の位置へ置く", () => {
    // 9m 1m 赤5p 5p 東 1s
    const sorted = sortTiles([8, 0, 35, 13, 27, 18]);
    expect(sorted.map(tileLabel)).toEqual(["1m", "9m", "0p", "5p", "1s", "東"]);
  });

  it("並べ替えは元の配列を壊さない", () => {
    const original = [8, 0];
    sortTiles(original);
    expect(original).toEqual([8, 0]);
  });
});
```

- [ ] **Step 2: 落ちることを確かめる**

Run: `pnpm --dir apps/web test tiles`
Expected: `./tiles` が見つからず失敗

- [ ] **Step 3: 実装する**

`apps/web/src/game/tiles.ts`:

```typescript
import type { Tile } from "../protocol/Tile";

/** 赤ドラのエンコード。34=赤5m, 35=赤5p, 36=赤5s。 */
const RED = { 34: 4, 35: 13, 36: 22 } as const;

const HONORS = ["東", "南", "西", "北", "白", "發", "中"] as const;

/** 赤ドラかどうか。 */
export function isRed(tile: Tile): boolean {
  return tile === 34 || tile === 35 || tile === 36;
}

/**
 * 赤ドラを同じ種類の5へ均した 0..=33 の値。
 * **並べ替えと「同じ牌か」の判定はこちらで行う。**
 */
export function kindOf(tile: Tile): number {
  if (!Number.isInteger(tile) || tile < 0 || tile > 36) {
    throw new Error(`牌のエンコードが範囲外: ${tile}`);
  }
  return isRed(tile) ? RED[tile as 34 | 35 | 36] : tile;
}

/** 人が読む表記。赤ドラは 0m / 0p / 0s。 */
export function tileLabel(tile: Tile): string {
  const kind = kindOf(tile);
  if (kind >= 27) {
    return HONORS[kind - 27];
  }
  const suit = "mps"[Math.floor(kind / 9)];
  const digit = isRed(tile) ? 0 : (kind % 9) + 1;
  return `${digit}${suit}`;
}

/**
 * 萬子・筒子・索子・字牌の順に並べる。
 * **赤ドラは同じ5の位置に来る。**元の配列は壊さない。
 */
export function sortTiles(tiles: Tile[]): Tile[] {
  return [...tiles].sort((a, b) => kindOf(a) - kindOf(b) || Number(isRed(b)) - Number(isRed(a)));
}
```

- [ ] **Step 4: 通ることを確かめる**

Run: `pnpm --dir apps/web test tiles`
Expected: 8 passed

- [ ] **Step 5: コミット**

```bash
pnpm --dir apps/web typecheck
git add apps/web/src/game/tiles.ts apps/web/src/game/tiles.test.ts
git commit -m "feat(web): 牌の表記と並べ替え

牌は 0..=36 の数値なので、人が読める形に直す層が要る。赤ドラは
同じ種類の5へ均してから並べ、表記のときだけ 0m として区別する。"
```

---

### Task 2: 盤面の組み立て

**Files:**
- Create: `apps/web/src/game/state.ts`
- Test: `apps/web/src/game/state.test.ts`

**Interfaces:**
- Consumes: Task 1 の `sortTiles`。`../protocol/*` の生成型。
- Produces:
  - `export type SeatView`, `export type GameState`, `export type Pending`
  - `emptyState(you: Seat): GameState`
  - `apply(state: GameState, envelope: ClientEventEnvelope, nowMs: number): GameState`

- [ ] **Step 1: 失敗するテストを書く**

`apps/web/src/game/state.test.ts`:

```typescript
import { describe, expect, it } from "vitest";

import type { ClientEvent } from "../protocol/ClientEvent";
import { apply, emptyState } from "./state";

let seq = 0;
function fold(events: ClientEvent[], nowMs = 0) {
  let state = emptyState(0);
  for (const event of events) {
    state = apply(state, { seq: (seq += 1), event }, nowMs);
  }
  return state;
}

const roundStart: ClientEvent = {
  type: "round_start",
  round: { wind: "east", number: 1 },
  dealer: 0,
  honba: 0,
  riichi_sticks: 0,
  scores: [25000, 25000, 25000, 25000],
  seed_commit: "abc",
};

const deal: ClientEvent = {
  type: "deal",
  your_hand: [0, 1, 2, 9, 10, 11, 18, 19, 20, 27, 27, 30, 30],
  hand_sizes: [13, 13, 13, 13],
  dora_indicator: 5,
};

describe("盤面の組み立て", () => {
  it("配牌で手牌が入る", () => {
    const state = fold([roundStart, deal]);
    expect(state.hand).toHaveLength(13);
    expect(state.doraIndicators).toEqual([5]);
    expect(state.scores).toEqual([25000, 25000, 25000, 25000]);
  });

  it("自分のツモは手牌と分けて持つ", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
    ]);
    expect(state.hand).toHaveLength(13);
    expect(state.drawn).toBe(33);
    expect(state.wallRemaining).toBe(69);
  });

  it("他家のツモは枚数だけ動かす", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
    ]);
    expect(state.drawn).toBeNull();
    expect(state.seats[1].handSize).toBe(14);
  });

  it("自分の打牌でツモ牌が消え、河へ積まれる", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 0, tile: 33, manner: "tsumogiri" },
    ]);
    expect(state.drawn).toBeNull();
    expect(state.hand).toHaveLength(13);
    expect(state.seats[0].river.map((d) => d.tile)).toEqual([33]);
  });

  it("手牌から切ると、ツモ牌が手牌へ入る", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 0, tile: 0, manner: "hand" },
    ]);
    expect(state.drawn).toBeNull();
    expect(state.hand).toHaveLength(13);
    expect(state.hand).toContain(33);
    expect(state.hand).not.toContain(0);
  });

  it("鳴かれた牌は河から取り除かれる", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 1, tile: 5, manner: "hand" },
      { type: "call", seat: 2, from: 1, kind: "pon", tiles: [5, 5] },
    ]);
    expect(state.seats[1].river).toHaveLength(0);
    expect(state.seats[2].melds).toHaveLength(1);
    expect(state.seats[2].melds[0].tiles).toEqual([5, 5]);
  });

  it("リーチ宣言牌に印が付く", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "riichi", seat: 2, step: "declare" },
      { type: "draw", seat: 2, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 2, tile: 9, manner: "hand" },
    ]);
    expect(state.seats[2].river[0].riichi).toBe(true);
  });

  it("リーチが成立すると席に印が付く", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "riichi", seat: 2, step: "declare" },
      { type: "riichi", seat: 2, step: "accepted" },
    ]);
    expect(state.seats[2].riichi).toBe(true);
  });

  it("要求は締切の絶対時刻を持つ", () => {
    const state = fold(
      [
        roundStart,
        deal,
        {
          type: "request_action",
          window_id: 3,
          options: [{ type: "discard", allowed: [0, 1], riichi_allowed: [] }],
          deadline_ms: 5000,
        },
      ],
      1_000,
    );
    expect(state.pending?.windowId).toBe(3);
    expect(state.pending?.deadlineAt).toBe(6_000);
  });

  it("打牌すると要求が消える", () => {
    const state = fold([
      roundStart,
      deal,
      {
        type: "request_action",
        window_id: 3,
        options: [{ type: "discard", allowed: [0], riichi_allowed: [] }],
        deadline_ms: 5000,
      },
      { type: "discard", seat: 0, tile: 0, manner: "hand" },
    ]);
    expect(state.pending).toBeNull();
  });

  it("新ドラが増える", () => {
    const state = fold([roundStart, deal, { type: "dora_reveal", indicator: 7 }]);
    expect(state.doraIndicators).toEqual([5, 7]);
  });

  it("局が変わると河と手牌が消える", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 0, tile: 33, manner: "tsumogiri" },
      roundStart,
    ]);
    expect(state.seats[0].river).toHaveLength(0);
    expect(state.hand).toHaveLength(0);
    expect(state.doraIndicators).toEqual([]);
  });

  it("終局で順位が入る", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "match_end", final_scores: [30000, 25000, 24000, 21000], placements: [1, 2, 3, 4] },
    ]);
    expect(state.phase).toBe("matchOver");
    expect(state.finalScores).toEqual([30000, 25000, 24000, 21000]);
  });

  it("連番を覚える", () => {
    const state = fold([roundStart, deal]);
    expect(state.lastSeq).not.toBeNull();
  });
});
```

- [ ] **Step 2: 落ちることを確かめる**

Run: `pnpm --dir apps/web test state`
Expected: `./state` が見つからず失敗

- [ ] **Step 3: 実装する**

`apps/web/src/game/state.ts`:

```typescript
import type { ActionOption } from "../protocol/ActionOption";
import type { ClientEvent } from "../protocol/ClientEvent";
import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import type { MeldKind } from "../protocol/MeldKind";
import type { Round } from "../protocol/Round";
import type { Seat } from "../protocol/Seat";
import type { Tile } from "../protocol/Tile";
import { sortTiles } from "./tiles";

/** 河に積まれた1枚。 */
export type Discarded = {
  tile: Tile;
  /** リーチ宣言牌。横向きに描く。 */
  riichi: boolean;
};

export type MeldView = {
  kind: MeldKind;
  tiles: Tile[];
  from: Seat;
};

export type SeatView = {
  handSize: number;
  river: Discarded[];
  melds: MeldView[];
  riichi: boolean;
  /** 次の打牌がリーチ宣言牌になる席。 */
  declaring: boolean;
};

/** いま自分が選べること。 */
export type Pending = {
  windowId: number;
  options: ActionOption[];
  /** 締切の絶対時刻（`performance.now()` と同じ尺度）。 */
  deadlineAt: number;
};

export type GameState = {
  you: Seat;
  seats: [SeatView, SeatView, SeatView, SeatView];
  /** 自分の手牌。ツモ牌は含まない。 */
  hand: Tile[];
  /** 自分のツモ牌。**手牌と分けて持つと、ツモ切りが描き分けられる。** */
  drawn: Tile | null;
  round: Round | null;
  dealer: Seat;
  honba: number;
  sticks: number;
  scores: number[];
  doraIndicators: Tile[];
  wallRemaining: number;
  pending: Pending | null;
  lastSeq: number | null;
  phase: "waiting" | "playing" | "matchOver";
  /** 和了や流局の要約。画面の帯に出す。 */
  notice: string | null;
  finalScores: number[] | null;
};

function emptySeat(): SeatView {
  return { handSize: 0, river: [], melds: [], riichi: false, declaring: false };
}

export function emptyState(you: Seat): GameState {
  return {
    you,
    seats: [emptySeat(), emptySeat(), emptySeat(), emptySeat()],
    hand: [],
    drawn: null,
    round: null,
    dealer: 0,
    honba: 0,
    sticks: 0,
    scores: [25000, 25000, 25000, 25000],
    doraIndicators: [],
    wallRemaining: 70,
    pending: null,
    lastSeq: null,
    phase: "waiting",
    notice: null,
    finalScores: null,
  };
}

function clone(state: GameState): GameState {
  return {
    ...state,
    seats: state.seats.map((s) => ({
      ...s,
      river: [...s.river],
      melds: [...s.melds],
    })) as GameState["seats"],
    hand: [...state.hand],
    doraIndicators: [...state.doraIndicators],
    scores: [...state.scores],
  };
}

/** 1件のイベントを畳む。**状態を書き換えず、新しい状態を返す。** */
export function apply(
  previous: GameState,
  envelope: ClientEventEnvelope,
  nowMs: number,
): GameState {
  const state = clone(previous);
  state.lastSeq = envelope.seq;
  const event: ClientEvent = envelope.event;

  switch (event.type) {
    case "match_start":
      state.you = event.you;
      state.phase = "playing";
      break;

    case "round_start": {
      const you = state.you;
      const carried = { ...emptyState(you), lastSeq: state.lastSeq, phase: "playing" as const };
      Object.assign(state, carried);
      state.round = event.round;
      state.dealer = event.dealer;
      state.honba = event.honba;
      state.sticks = event.riichi_sticks;
      state.scores = [...event.scores];
      state.notice = null;
      break;
    }

    case "deal":
      state.hand = sortTiles(event.your_hand);
      state.doraIndicators = [event.dora_indicator];
      for (let i = 0; i < 4; i += 1) {
        state.seats[i].handSize = event.hand_sizes[i];
      }
      break;

    case "draw":
      state.wallRemaining = event.wall_remaining;
      if (event.seat === state.you && event.tile !== null) {
        state.drawn = event.tile;
      } else {
        state.seats[event.seat].handSize += 1;
      }
      break;

    case "discard": {
      const seat = state.seats[event.seat];
      seat.river.push({ tile: event.tile, riichi: seat.declaring });
      seat.declaring = false;
      if (event.seat === state.you) {
        // ツモ牌を切ったなら手牌はそのまま。手牌から切ったならツモ牌が入る。
        if (state.drawn !== null && state.drawn === event.tile) {
          state.drawn = null;
        } else {
          const at = state.hand.indexOf(event.tile);
          if (at >= 0) {
            state.hand.splice(at, 1);
          }
          if (state.drawn !== null) {
            state.hand.push(state.drawn);
            state.drawn = null;
          }
          state.hand = sortTiles(state.hand);
        }
        state.pending = null;
      } else {
        seat.handSize -= 1;
      }
      break;
    }

    case "riichi":
      if (event.step === "declare") {
        state.seats[event.seat].declaring = true;
      } else {
        state.seats[event.seat].riichi = true;
      }
      break;

    case "call": {
      // 鳴かれた牌は打った席の河から消える。
      const source = state.seats[event.from];
      source.river.pop();
      const caller = state.seats[event.seat];
      caller.melds.push({ kind: event.kind, tiles: event.tiles, from: event.from });
      if (event.seat === state.you) {
        for (const tile of event.tiles) {
          const at = state.hand.indexOf(tile);
          if (at >= 0) {
            state.hand.splice(at, 1);
          }
        }
        state.pending = null;
      } else {
        caller.handSize -= event.tiles.length;
      }
      break;
    }

    case "dora_reveal":
      state.doraIndicators.push(event.indicator);
      break;

    case "request_action":
      state.pending = {
        windowId: event.window_id,
        options: event.options,
        deadlineAt: nowMs + event.deadline_ms,
      };
      break;

    case "action_passed":
      if (event.seat === state.you) {
        state.pending = null;
      }
      break;

    case "agari": {
      const winners = event.results.map((r) => `席${r.seat}`).join("・");
      state.notice = `${winners} 和了`;
      state.pending = null;
      break;
    }

    case "ryuukyoku":
      state.notice = "流局";
      state.pending = null;
      break;

    case "round_end":
      state.scores = [...event.scores];
      state.pending = null;
      break;

    case "match_end":
      state.phase = "matchOver";
      state.finalScores = [...event.final_scores];
      state.notice = "終局";
      state.pending = null;
      break;

    default:
      break;
  }

  return state;
}
```

- [ ] **Step 4: 通ることを確かめる**

Run: `pnpm --dir apps/web test state`
Expected: 14 passed

- [ ] **Step 5: コミット**

```bash
pnpm --dir apps/web typecheck
git add apps/web/src/game/state.ts apps/web/src/game/state.test.ts
git commit -m "feat(web): ClientEvent から盤面を組み立てる

描画と状態を分ける。**状態の組み立てはブラウザに依存しない純粋な
関数**にしたので、3D へ差し替えるときに捨てるのは描画だけになる。

自分のツモ牌を手牌と分けて持つのは、ツモ切りと手出しを描き分ける
ため。鳴かれた牌は打った席の河から取り除く。"
```

---

### Task 3: 接続

**Files:**
- Create: `apps/web/src/net/connection.ts`
- Test: `apps/web/src/net/connection.test.ts`

**Interfaces:**
- Produces: `export type Sink`、`export function connect(options): Connection`、`Connection.send(command)`、`Connection.close()`

- [ ] **Step 1: 失敗するテストを書く**

`apps/web/src/net/connection.test.ts`:

```typescript
import { describe, expect, it, vi } from "vitest";

import { buildUrl } from "./connection";

describe("接続先の組み立て", () => {
  it("卓の id を載せる", () => {
    expect(buildUrl("ws://h/ws", "abc", null)).toBe("ws://h/ws?table=abc");
  });

  it("連番があれば載せる", () => {
    expect(buildUrl("ws://h/ws", "abc", 42)).toBe("ws://h/ws?table=abc&last_seq=42");
  });

  it("連番が 0 でも載せる", () => {
    // **0 を「無い」と取り違えると、対局の頭から送り直される。**
    expect(buildUrl("ws://h/ws", "abc", 0)).toBe("ws://h/ws?table=abc&last_seq=0");
  });

  it("卓の id を URL に安全な形へ直す", () => {
    expect(buildUrl("ws://h/ws", "a b&c", null)).toBe("ws://h/ws?table=a+b%26c");
  });
});
```

- [ ] **Step 2: 実装する**

`apps/web/src/net/connection.ts`:

```typescript
import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import type { Command } from "../protocol/Command";

/** 接続先の URL を組み立てる。 */
export function buildUrl(base: string, table: string, lastSeq: number | null): string {
  const params = new URLSearchParams({ table });
  // **0 を「無い」と取り違えると、対局の頭から送り直される。**
  if (lastSeq !== null) {
    params.set("last_seq", String(lastSeq));
  }
  return `${base}?${params.toString()}`;
}

export type Connection = {
  send(command: Command): void;
  close(): void;
};

export type Options = {
  base: string;
  table: string;
  /** いま届いている連番。再接続のときに渡す。 */
  lastSeq(): number | null;
  onEvent(envelope: ClientEventEnvelope): void;
  onStatus(text: string): void;
};

/**
 * 繋ぎ、切れたら繋ぎ直す。
 *
 * **切れたら最後に受け取った連番から送り直してもらう。**対局の頭から
 * やり直さないので、再読み込みしても続きから遊べる。
 */
export function connect(options: Options): Connection {
  let socket: WebSocket | null = null;
  let closed = false;
  let retry = 0;

  const open = () => {
    if (closed) {
      return;
    }
    socket = new WebSocket(buildUrl(options.base, options.table, options.lastSeq()));
    socket.onopen = () => {
      retry = 0;
      options.onStatus("接続");
    };
    socket.onmessage = (message) => {
      options.onEvent(JSON.parse(message.data as string) as ClientEventEnvelope);
    };
    socket.onclose = () => {
      if (closed) {
        return;
      }
      retry += 1;
      const wait = Math.min(1000 * retry, 5000);
      options.onStatus(`切断。${wait / 1000}秒後に繋ぎ直します`);
      setTimeout(open, wait);
    };
  };

  open();

  return {
    send(command) {
      if (socket && socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify(command));
      }
    },
    close() {
      closed = true;
      socket?.close();
    },
  };
}
```

- [ ] **Step 3: 通ることを確かめる**

Run: `pnpm --dir apps/web test connection`
Expected: 4 passed

- [ ] **Step 4: コミット**

```bash
pnpm --dir apps/web typecheck
git add apps/web/src/net/connection.ts apps/web/src/net/connection.test.ts
git commit -m "feat(web): WebSocket の接続と再接続

切れたら最後に受け取った連番から送り直してもらう。**対局の頭から
やり直さないので、再読み込みしても続きから遊べる。**

連番 0 を「無い」と取り違えないよう、null との区別をテストで固定した。"
```

---

### Task 4: 画面と操作

**Files:**
- Create: `apps/web/src/ui/board.ts`
- Create: `apps/web/src/ui/board.css`
- Modify: `apps/web/src/main.ts`
- Modify: `apps/web/index.html`

**Interfaces:**
- Consumes: Task 1〜3 のすべて。
- Produces: `export function renderBoard(root: HTMLElement, state: GameState, send: (c: Command) => void): void`

**この Task に自動試験は置かない。**描画と入力は目で確かめる。状態の組み立ては Task 2 が試験している。

- [ ] **Step 1: 画面を書く**

`apps/web/src/ui/board.ts` に次を実装する。**見た目を作り込まない。**

- 上段: 場・本場・供託・残り枚数・ドラ表示・4席の点数（親に印）
- 中段: 他家3人の河と副露（席番号と、リーチ中の印）
- 下段: 自分の河、自分の手牌（`sortTiles` 済み）とツモ牌を右に離して置く
- 手牌の牌は `<button>` にする。`pending` に `discard` があり、その牌が `allowed` に含まれるときだけ押せる
- 押したら `{ type: "discard", tile, riichi: リーチ待機中か }` を送る
- `riichi_allowed` が空でなければ「リーチ」ボタンを出す。押すと待機状態になり、次に押した牌がリーチ宣言牌になる
- `pending` に `chi` / `pon` / `kan` / `ron` / `tsumo` / `kyuushu` があればボタンを出し、押したら対応する `Command` を送る。候補が複数ある鳴きは候補ごとにボタンを出す
- `pass` があれば「見送り」ボタンを出す
- 締切までの残りをバーで出す。`deadlineAt - performance.now()` を 100ms ごとに描き直す
- `notice` があれば帯に出す
- `phase === "matchOver"` なら最終順位を出す

牌の面は `tileLabel` の文字で描く。**画像は使わない。**Wave 3e-2 で 3D に差し替える。

- [ ] **Step 2: 結線する**

`apps/web/src/main.ts` を次の形にする。既存の型検査用の関数は消してよい（`protocol` の型は画面が使うので、生成物の検査は typecheck が担う）。

```typescript
import { apply, emptyState } from "./game/state";
import type { GameState } from "./game/state";
import { connect } from "./net/connection";
import { renderBoard } from "./ui/board";
import "./ui/board.css";

/** 卓の id。**再読み込みしても同じ卓に戻るために覚えておく。** */
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
if (!root) {
  throw new Error("#app が無い");
}

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

// 締切のバーを動かすために、イベントが来なくても描き直す。
setInterval(() => {
  renderBoard(root, state, (command) => connection.send(command));
}, 100);

/** 新しい卓を立てる。**評価中に何度も打ち直せるように。** */
(globalThis as unknown as { newTable: () => void }).newTable = () => {
  localStorage.removeItem("real-mahjong.table");
  location.reload();
};
```

- [ ] **Step 3: 実際に遊んで確かめる**

```bash
pnpm --dir apps/web build
cargo run -p server --bin serve
```

ブラウザで `http://127.0.0.1:8080` を開く。**次がすべてできること。**

1. 配牌 13 枚と親のツモが見える
2. 自分の番に手牌の牌を押すと切れて、CPU 3人が打ち返してくる
3. 鳴ける場面でボタンが出て、押すと鳴ける
4. リーチが宣言でき、宣言牌が横向きに見える
5. 和了ると点数が動く
6. **ブラウザを再読み込みしても、同じ局の続きから始まる**
7. 半荘が終わると順位が出る

**どれか1つでもできなければ、このウェーブは目的を果たしていない。**止めて報告すること。

- [ ] **Step 4: コミット**

```bash
pnpm --dir apps/web typecheck
pnpm --dir apps/web test
git add apps/web/src/ui apps/web/src/main.ts apps/web/index.html
git commit -m "feat(web): 遊べる 2D の卓

**ブラウザを開けば CPU 3人と半荘が打てる。**見た目は作り込まず、
牌は文字で描く。3D への差し替えは Wave 3e-2 で行い、そのとき捨てるのは
描画だけで、状態の組み立てと操作の論理はそのまま使う。"
```

---

## Self-Review

**仕様の網羅:** これは評価のための最小の遊べる形であり、仕様の演出（`timeline/`）と 3D（`scene/`）はまだ使わない。仕様 6.2 の持ち時間はバーとして見える。8.1 の再接続は `last_seq` で効く。

**このウェーブがやらないこと:** 3D 描画、2D キャラ、音、演出のタイムライン、アニメーション、見た目の作り込み。**遊べることが先。**

**認められた妥協:**

- 牌を文字で描く。3D への差し替えで捨てる。
- 他家の手牌は枚数だけ。伏せ牌の絵は描かない。
- 和了の詳細（役と点数の内訳）は帯に一行出すだけ。
- 局と局の間で止まらない。次の局がすぐ始まる。

**型の整合:** `GameState` / `SeatView` / `Pending` は Task 2 で定義し、Task 4 はそれを読むだけ。`Command` と `ClientEventEnvelope` は生成された型をそのまま使う。

**確認済みの事実:** 牌のエンコード（0..=36）、`ActionOption` の形、`deadline_ms` が受信時点からの残りであること、`ws://.../ws?table=&last_seq=` で繋がることは、Wave 3d までに実際に動かして確かめた。
