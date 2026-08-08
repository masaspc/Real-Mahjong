# Wave 1c: 3D卓と牌の描画 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 卓と牌を Three.js で描く土台を作る。牌の配置計算とアトラスの UV 計算を、描画から切り離してテストできる形にする。

**Architecture:** 仕様 7.1 の方針に従う。**山はバッチした1メッシュ、動く牌のみ個別メッシュ**とし、牌面は34種＋裏＋赤ドラ3種をアトラス1枚にまとめて UV オフセットで切り替える。動的シャドウは使わず、接地影で代用する。

**Tech Stack:** TypeScript 7 / Vite 8 / Three.js

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md` の第7章
**作業規約:** `AGENTS.md`

## Global Constraints

- **編集してよいのは `apps/web/src/scene/` 配下のみ**
- `apps/web/src/timeline/`（Wave 1d の所有）と `apps/web/src/main.ts` を編集しない。結線は Wave 1 完了後にコーディネータが行う
- `apps/web/src/protocol/` は Rust からの生成物である。**手で編集しない**
- `crates/` を一切編集しない
- **座標系**: 卓の中心を原点、卓面を y=0、自席から見て奥が -z、右が +x。単位は「牌の幅 = 1」
- **牌の寸法比**: 幅 1、高さ 1.35、厚み 0.6（実際の麻雀牌の比率に近い値）
- 動的シャドウマップを使わない。接地影で代用する
- テキストを 3D 空間に焼かない。文字は 2D レイヤー（Wave 1 完了後の担当）
- 完了条件は `pnpm --filter @real-mahjong/web typecheck` が通り、`pnpm --filter @real-mahjong/web test` が全通過すること

## 描画とロジックの分離

**Three.js に触る部分をできるだけ薄くする。** 配置計算と UV 計算は純粋関数として切り出し、
ヘッドレスでテストする。Three.js を使うのはそれらの結果をメッシュへ適用する層だけにする。

| ファイル | 責務 | テスト |
|---|---|---|
| `layout.ts` | 席・河・手牌・山の座標計算 | 純粋関数として検証 |
| `atlas.ts` | 牌の種類 → アトラス上の UV | 純粋関数として検証 |
| `tile-geometry.ts` | 牌のジオメトリとマテリアル生成 | 生成物の形だけ検証 |
| `table.ts` | 卓・シーン・カメラ・ライティング | 手動確認 |
| `tile-pool.ts` | 動く牌の個別メッシュ管理 | 貸出・返却の挙動を検証 |

---

### Task 1: Three.js の導入と座標系の定義

**Files:**
- Create: `apps/web/src/scene/layout.ts`
- Create: `apps/web/src/scene/layout.test.ts`

**Interfaces:**
- Produces:
  - `export type Vec3 = { x: number; y: number; z: number }`
  - `export const TILE = { width: 1, height: 1.35, depth: 0.6 }`
  - `export function seatRotation(seat: number): number` — 自席を 0 とした相対席のラジアン
  - `export function relativeSeat(absolute: number, viewer: number): number`
  - `export function handPosition(seat: number, index: number, handSize: number): Vec3`
  - `export function discardPosition(seat: number, index: number): Vec3`
  - `export function wallPosition(seat: number, index: number): Vec3`

河は**6枚×3段**、山は各家17トン×2段とする。

`three` / `@types/three` / `vitest` は**既に導入済み**である（Wave 1d と共有する
`package.json` の衝突を避けるため、コーディネータが先に入れた）。
**`package.json` と `vitest.config.ts` を編集しないこと。**

- [ ] **Step 1: 失敗するテストを書く**

```ts
import { describe, expect, it } from "vitest";
import {
  TILE,
  discardPosition,
  handPosition,
  relativeSeat,
  seatRotation,
  wallPosition,
} from "./layout";

describe("relativeSeat", () => {
  it("puts the viewer at zero", () => {
    expect(relativeSeat(2, 2)).toBe(0);
    expect(relativeSeat(3, 2)).toBe(1);
    expect(relativeSeat(0, 2)).toBe(2);
    expect(relativeSeat(1, 2)).toBe(3);
  });
});

describe("seatRotation", () => {
  it("turns a quarter circle per seat", () => {
    expect(seatRotation(0)).toBeCloseTo(0);
    expect(seatRotation(1)).toBeCloseTo(Math.PI / 2);
    expect(seatRotation(2)).toBeCloseTo(Math.PI);
    expect(seatRotation(3)).toBeCloseTo((3 * Math.PI) / 2);
  });
});

describe("handPosition", () => {
  it("lays the viewer's hand along +x, centred on the origin", () => {
    const size = 13;
    const first = handPosition(0, 0, size);
    const last = handPosition(0, size - 1, size);
    expect(first.z).toBeCloseTo(last.z);
    expect(last.x - first.x).toBeCloseTo(TILE.width * (size - 1));
    // 中央に寄っている
    expect(first.x + last.x).toBeCloseTo(0);
  });

  it("keeps tiles resting on the table", () => {
    expect(handPosition(0, 0, 13).y).toBeGreaterThan(0);
  });

  it("rotates the whole hand for other seats", () => {
    const mine = handPosition(0, 0, 13);
    const across = handPosition(2, 0, 13);
    // 対面は原点をはさんで反対側
    expect(across.x).toBeCloseTo(-mine.x);
    expect(across.z).toBeCloseTo(-mine.z);
  });
});

describe("discardPosition", () => {
  it("fills six per row before starting the next", () => {
    const a = discardPosition(0, 0);
    const f = discardPosition(0, 5);
    const g = discardPosition(0, 6);
    expect(a.z).toBeCloseTo(f.z);
    expect(g.z).not.toBeCloseTo(a.z);
    expect(g.x).toBeCloseTo(a.x);
  });

  it("grows away from the player row by row", () => {
    const row0 = discardPosition(0, 0);
    const row1 = discardPosition(0, 6);
    const row2 = discardPosition(0, 12);
    // 自席の河は手前から奥（-z 方向）へは伸びず、卓中央へ向かう
    expect(Math.abs(row1.z)).toBeLessThan(Math.abs(row0.z));
    expect(Math.abs(row2.z)).toBeLessThan(Math.abs(row1.z));
  });

  /** 4段目以降も破綻せず並ぶ（実戦では稀だが落ちてはいけない）。 */
  it("does not break past the third row", () => {
    const far = discardPosition(0, 23);
    expect(Number.isFinite(far.x)).toBe(true);
    expect(Number.isFinite(far.z)).toBe(true);
  });
});

describe("wallPosition", () => {
  it("stacks two tiles per position", () => {
    const lower = wallPosition(0, 0);
    const upper = wallPosition(0, 1);
    expect(upper.x).toBeCloseTo(lower.x);
    expect(upper.z).toBeCloseTo(lower.z);
    expect(upper.y).toBeGreaterThan(lower.y);
  });

  it("advances along the wall every two tiles", () => {
    const first = wallPosition(0, 0);
    const second = wallPosition(0, 2);
    expect(second.x).not.toBeCloseTo(first.x);
  });
});
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: `./layout` が見つからず失敗

- [ ] **Step 3: 実装を書く**

```ts
/**
 * 卓上の座標計算。
 *
 * 卓の中心が原点、卓面が y=0。自席（相対席0）から見て奥が -z、右が +x。
 * 単位は牌の幅を 1 とする。
 *
 * Three.js に依存しない純粋関数にしてある。配置の正しさを描画なしで
 * 検証できるようにするためである。
 */

export type Vec3 = { x: number; y: number; z: number };

/** 牌の寸法比。実際の麻雀牌に近い比率。 */
export const TILE = {
  width: 1,
  height: 1.35,
  depth: 0.6,
} as const;

/** 河は6枚で1段。 */
export const DISCARDS_PER_ROW = 6;

/** 自席から見た相対席。自分が 0、下家が 1、対面が 2、上家が 3。 */
export function relativeSeat(absolute: number, viewer: number): number {
  return (absolute - viewer + 4) % 4;
}

/** 相対席の回転角（ラジアン）。 */
export function seatRotation(seat: number): number {
  return (seat * Math.PI) / 2;
}

/** 原点まわりに y 軸で回す。 */
function rotateY(point: Vec3, radians: number): Vec3 {
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  return {
    x: point.x * cos + point.z * sin,
    y: point.y,
    z: -point.x * sin + point.z * cos,
  };
}

/** 手牌。自席の手前に一列。 */
export function handPosition(seat: number, index: number, handSize: number): Vec3 {
  const spread = TILE.width * (handSize - 1);
  const local: Vec3 = {
    x: index * TILE.width - spread / 2,
    y: TILE.height / 2,
    z: 7.5,
  };
  return rotateY(local, seatRotation(seat));
}

/** 河。6枚ごとに1段、段が進むほど卓中央へ寄る。 */
export function discardPosition(seat: number, index: number): Vec3 {
  const row = Math.floor(index / DISCARDS_PER_ROW);
  const column = index % DISCARDS_PER_ROW;
  const local: Vec3 = {
    x: (column - (DISCARDS_PER_ROW - 1) / 2) * TILE.width,
    y: TILE.depth / 2,
    z: 4.2 - row * TILE.height,
  };
  return rotateY(local, seatRotation(seat));
}

/** 山。2枚で1トン、17トンで一辺。 */
export function wallPosition(seat: number, index: number): Vec3 {
  const stack = Math.floor(index / 2);
  const upper = index % 2 === 1;
  const local: Vec3 = {
    x: (stack - 8) * TILE.width,
    y: TILE.depth / 2 + (upper ? TILE.depth : 0),
    z: 6.2,
  };
  return rotateY(local, seatRotation(seat));
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: 全テスト PASS

- [ ] **Step 5: コミット**

```bash
git add apps/web
git commit -m "feat(web): 卓上の座標計算を純粋関数として追加"
```

---

### Task 2: 牌面のテクスチャアトラス

34種＋裏＋赤ドラ3種を1枚にまとめ、UV オフセットで切り替える。**素材が無い段階でも動くよう、手続きで生成したアトラスを既定にする。**

**Files:**
- Create: `apps/web/src/scene/atlas.ts`
- Create: `apps/web/src/scene/atlas.test.ts`

**Interfaces:**
- Produces:
  - `export const ATLAS = { columns: 8, rows: 5, cells: 38 }`
  - `export function atlasIndexOf(encoded: number): number` — 牌のエンコード（0..=36）→ アトラスのセル番号。37 は裏面
  - `export const BACK_INDEX: number`
  - `export function uvOffsetOf(cellIndex: number): { u: number; v: number; du: number; dv: number }`
  - `export function drawPlaceholderAtlas(size?: number): HTMLCanvasElement`

- [ ] **Step 1: 失敗するテストを書く**

```ts
import { describe, expect, it } from "vitest";
import { ATLAS, BACK_INDEX, atlasIndexOf, uvOffsetOf } from "./atlas";

describe("atlasIndexOf", () => {
  it("maps every encoded tile to its own cell", () => {
    const seen = new Set<number>();
    for (let encoded = 0; encoded <= 36; encoded += 1) {
      const cell = atlasIndexOf(encoded);
      expect(cell).toBeGreaterThanOrEqual(0);
      expect(cell).toBeLessThan(ATLAS.cells);
      expect(seen.has(cell)).toBe(false);
      seen.add(cell);
    }
    expect(seen.size).toBe(37);
  });

  it("gives the back face its own cell", () => {
    expect(BACK_INDEX).toBeGreaterThanOrEqual(0);
    expect(BACK_INDEX).toBeLessThan(ATLAS.cells);
    for (let encoded = 0; encoded <= 36; encoded += 1) {
      expect(atlasIndexOf(encoded)).not.toBe(BACK_INDEX);
    }
  });

  it("rejects tiles outside the encoding", () => {
    expect(() => atlasIndexOf(37)).toThrow();
    expect(() => atlasIndexOf(-1)).toThrow();
  });
});

describe("uvOffsetOf", () => {
  it("returns a cell inside the unit square", () => {
    for (let cell = 0; cell < ATLAS.cells; cell += 1) {
      const uv = uvOffsetOf(cell);
      expect(uv.u).toBeGreaterThanOrEqual(0);
      expect(uv.v).toBeGreaterThanOrEqual(0);
      expect(uv.u + uv.du).toBeLessThanOrEqual(1 + 1e-9);
      expect(uv.v + uv.dv).toBeLessThanOrEqual(1 + 1e-9);
    }
  });

  it("gives every cell the same size", () => {
    const a = uvOffsetOf(0);
    const b = uvOffsetOf(ATLAS.cells - 1);
    expect(a.du).toBeCloseTo(b.du);
    expect(a.dv).toBeCloseTo(b.dv);
    expect(a.du).toBeCloseTo(1 / ATLAS.columns);
    expect(a.dv).toBeCloseTo(1 / ATLAS.rows);
  });

  it("moves right then down", () => {
    const first = uvOffsetOf(0);
    const next = uvOffsetOf(1);
    const wrapped = uvOffsetOf(ATLAS.columns);
    expect(next.u).toBeGreaterThan(first.u);
    expect(next.v).toBeCloseTo(first.v);
    expect(wrapped.u).toBeCloseTo(first.u);
    expect(wrapped.v).not.toBeCloseTo(first.v);
  });
});
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: `./atlas` が見つからず失敗

- [ ] **Step 3: 実装を書く**

```ts
/**
 * 牌面のテクスチャアトラス。
 *
 * 34種＋赤ドラ3種＋裏面の38セルを1枚に敷き、UV オフセットで切り替える。
 * ドローコールを増やさずに全種類を描くための基本手段である。
 *
 * 素材が揃っていない段階でも動くよう、手続きで生成したアトラスを既定にする。
 * 差し替えはマニフェスト経由で行う（仕様 7.4）。
 */

export const ATLAS = {
  columns: 8,
  rows: 5,
  /** 34種 + 赤ドラ3 + 裏面1 */
  cells: 38,
} as const;

/** 裏面のセル。牌のエンコード（0..=36）とは重ならない位置に置く。 */
export const BACK_INDEX = 37;

/** 牌のエンコード（0..=36）→ アトラスのセル番号。 */
export function atlasIndexOf(encoded: number): number {
  if (!Number.isInteger(encoded) || encoded < 0 || encoded > 36) {
    throw new Error(`牌のエンコードが範囲外: ${encoded}`);
  }
  return encoded;
}

export function uvOffsetOf(cellIndex: number): {
  u: number;
  v: number;
  du: number;
  dv: number;
} {
  if (!Number.isInteger(cellIndex) || cellIndex < 0 || cellIndex >= ATLAS.cells) {
    throw new Error(`アトラスのセル番号が範囲外: ${cellIndex}`);
  }
  const du = 1 / ATLAS.columns;
  const dv = 1 / ATLAS.rows;
  const column = cellIndex % ATLAS.columns;
  const row = Math.floor(cellIndex / ATLAS.columns);
  return {
    u: column * du,
    v: row * dv,
    du,
    dv,
  };
}

const SUIT_MARK = ["m", "p", "s"] as const;
const HONOR_MARK = ["東", "南", "西", "北", "白", "發", "中"] as const;

/** そのセルに描く文字。プレースホルダ用。 */
function labelOf(cellIndex: number): { text: string; red: boolean } {
  if (cellIndex === BACK_INDEX) {
    return { text: "", red: false };
  }
  if (cellIndex >= 34) {
    const suit = SUIT_MARK[cellIndex - 34] ?? "?";
    return { text: `5${suit}`, red: true };
  }
  if (cellIndex >= 27) {
    return { text: HONOR_MARK[cellIndex - 27] ?? "?", red: false };
  }
  const suit = SUIT_MARK[Math.floor(cellIndex / 9)] ?? "?";
  return { text: `${(cellIndex % 9) + 1}${suit}`, red: false };
}

/**
 * 素材が無い段階で使うアトラスを手続きで描く。
 *
 * 見た目は仮でよいが、**牌の種類が一目で判別できること**が要件である。
 * これが無いと Wave 2 以降の動作確認ができない。
 */
export function drawPlaceholderAtlas(size = 1024): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (ctx === null) {
    throw new Error("2D コンテキストを取得できない");
  }

  const cellW = size / ATLAS.columns;
  const cellH = size / ATLAS.rows;

  ctx.fillStyle = "#1b1b1b";
  ctx.fillRect(0, 0, size, size);

  for (let cell = 0; cell < ATLAS.cells; cell += 1) {
    const column = cell % ATLAS.columns;
    const row = Math.floor(cell / ATLAS.columns);
    const x = column * cellW;
    const y = row * cellH;
    const label = labelOf(cell);

    ctx.fillStyle = cell === BACK_INDEX ? "#c9a227" : "#f6f1e3";
    ctx.fillRect(x + 2, y + 2, cellW - 4, cellH - 4);

    if (label.text !== "") {
      ctx.fillStyle = label.red ? "#c0392b" : "#222222";
      ctx.font = `${Math.floor(cellH * 0.5)}px sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(label.text, x + cellW / 2, y + cellH / 2);
    }
  }

  return canvas;
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: 全テスト PASS

`drawPlaceholderAtlas` は `document` を要るためテストしない。Task 5 の手動確認で見る。

- [ ] **Step 5: コミット**

```bash
git add apps/web
git commit -m "feat(web): 牌面アトラスの UV 計算とプレースホルダ生成を追加"
```

---

### Task 3: 動く牌のプール

**山はバッチ、動く牌のみ個別メッシュ**という方針を実装する。個別メッシュは使い回し、毎回作らない。

**Files:**
- Create: `apps/web/src/scene/tile-pool.ts`
- Create: `apps/web/src/scene/tile-pool.test.ts`

**Interfaces:**
- Produces:
  - `export interface PooledTile { readonly id: number; encoded: number; position: Vec3; rotationY: number; faceUp: boolean }`
  - `export class TilePool` — `acquire(encoded: number): PooledTile`, `release(tile: PooledTile): void`, `readonly active: PooledTile[]`, `readonly size: number`, `readonly inUse: number`

- [ ] **Step 1: 失敗するテストを書く**

```ts
import { describe, expect, it } from "vitest";
import { TilePool } from "./tile-pool";

describe("TilePool", () => {
  it("hands out a tile and tracks it as in use", () => {
    const pool = new TilePool();
    const tile = pool.acquire(5);
    expect(tile.encoded).toBe(5);
    expect(pool.inUse).toBe(1);
    expect(pool.active).toContain(tile);
  });

  it("reuses a released tile instead of growing", () => {
    const pool = new TilePool();
    const first = pool.acquire(1);
    const firstId = first.id;
    pool.release(first);
    expect(pool.inUse).toBe(0);

    const second = pool.acquire(2);
    expect(second.id).toBe(firstId);
    expect(second.encoded).toBe(2);
    expect(pool.size).toBe(1);
  });

  it("grows only when every tile is in use", () => {
    const pool = new TilePool();
    const a = pool.acquire(1);
    const b = pool.acquire(2);
    expect(pool.size).toBe(2);
    expect(a.id).not.toBe(b.id);
  });

  /** 同時に個別で存在する牌は50前後に収まる想定（仕様 7.1）。 */
  it("stays small for a realistic table", () => {
    const pool = new TilePool();
    const held = [];
    for (let i = 0; i < 50; i += 1) {
      held.push(pool.acquire(i % 34));
    }
    expect(pool.size).toBe(50);
    for (const tile of held) {
      pool.release(tile);
    }
    expect(pool.inUse).toBe(0);
    expect(pool.size).toBe(50);
  });

  it("refuses to release a tile twice", () => {
    const pool = new TilePool();
    const tile = pool.acquire(1);
    pool.release(tile);
    expect(() => pool.release(tile)).toThrow();
  });

  it("resets presentation state when acquired", () => {
    const pool = new TilePool();
    const first = pool.acquire(1);
    first.position = { x: 9, y: 9, z: 9 };
    first.rotationY = 1.5;
    first.faceUp = false;
    pool.release(first);

    const second = pool.acquire(2);
    expect(second.position).toEqual({ x: 0, y: 0, z: 0 });
    expect(second.rotationY).toBe(0);
    expect(second.faceUp).toBe(true);
  });
});
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: `./tile-pool` が見つからず失敗

- [ ] **Step 3: 実装を書く**

```ts
import type { Vec3 } from "./layout";

/**
 * 個別に動かせる牌ひとつ。
 *
 * 描画層はこの `id` でメッシュを引く。プールが使い回すため、
 * `id` は牌の種類ではなく「メッシュの枠」を指す。
 */
export interface PooledTile {
  readonly id: number;
  encoded: number;
  position: Vec3;
  rotationY: number;
  faceUp: boolean;
}

/**
 * 動く牌の個別メッシュを使い回す。
 *
 * 山は側面しか見えず個別に動かないためバッチした1メッシュで描く。
 * 個別メッシュが要るのは手牌・鳴き・河の直近だけで、同時に50前後に
 * 収まる（仕様 7.1）。毎回メッシュを作らずここで貸し借りする。
 */
export class TilePool {
  #tiles: PooledTile[] = [];
  #free: number[] = [];
  #inUse = new Set<number>();

  get size(): number {
    return this.#tiles.length;
  }

  get inUse(): number {
    return this.#inUse.size;
  }

  get active(): PooledTile[] {
    return this.#tiles.filter((tile) => this.#inUse.has(tile.id));
  }

  acquire(encoded: number): PooledTile {
    const recycled = this.#free.pop();
    const tile =
      recycled === undefined
        ? this.#grow()
        : (this.#tiles[recycled] as PooledTile);

    tile.encoded = encoded;
    tile.position = { x: 0, y: 0, z: 0 };
    tile.rotationY = 0;
    tile.faceUp = true;
    this.#inUse.add(tile.id);
    return tile;
  }

  release(tile: PooledTile): void {
    if (!this.#inUse.delete(tile.id)) {
      throw new Error(`使用中でない牌を返却した（id=${tile.id}）`);
    }
    this.#free.push(tile.id);
  }

  #grow(): PooledTile {
    const tile: PooledTile = {
      id: this.#tiles.length,
      encoded: 0,
      position: { x: 0, y: 0, z: 0 },
      rotationY: 0,
      faceUp: true,
    };
    this.#tiles.push(tile);
    return tile;
  }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: 全テスト PASS

- [ ] **Step 5: コミット**

```bash
git add apps/web
git commit -m "feat(web): 動く牌の個別メッシュを使い回すプールを追加"
```

---

### Task 4: 牌のジオメトリとマテリアル

**Files:**
- Create: `apps/web/src/scene/tile-geometry.ts`
- Create: `apps/web/src/scene/tile-geometry.test.ts`

**Interfaces:**
- Consumes: Task 1 `TILE`、Task 2 `uvOffsetOf` / `atlasIndexOf`
- Produces:
  - `export function createTileGeometry(): THREE.BufferGeometry` — 牌面が1つの面グループになった角丸ボックス
  - `export function applyFaceUv(geometry: THREE.BufferGeometry, encoded: number, faceUp: boolean): void`
  - `export function createTileMaterial(atlas: THREE.Texture): THREE.Material`

牌面（白）と牌体（黄）を1マテリアルで持つため、アトラスに牌体用のセルも含める。

- [ ] **Step 1: 失敗するテストを書く**

Three.js はヘッドレスでもジオメトリを作れる。描画せずに属性だけ検証する。

```ts
import { describe, expect, it } from "vitest";
import { BufferGeometry } from "three";
import { TILE } from "./layout";
import { applyFaceUv, createTileGeometry } from "./tile-geometry";

describe("createTileGeometry", () => {
  it("has the declared proportions", () => {
    const geometry = createTileGeometry();
    geometry.computeBoundingBox();
    const box = geometry.boundingBox;
    expect(box).not.toBeNull();
    const size = box!.max.clone().sub(box!.min);
    expect(size.x).toBeCloseTo(TILE.width, 5);
    expect(size.y).toBeCloseTo(TILE.height, 5);
    expect(size.z).toBeCloseTo(TILE.depth, 5);
  });

  it("carries uv attributes for every vertex", () => {
    const geometry = createTileGeometry();
    const position = geometry.getAttribute("position");
    const uv = geometry.getAttribute("uv");
    expect(uv).toBeDefined();
    expect(uv.count).toBe(position.count);
  });
});

describe("applyFaceUv", () => {
  it("moves the face uv into the requested atlas cell", () => {
    const geometry = createTileGeometry();
    applyFaceUv(geometry, 0, true);
    const first = Array.from(
      (geometry.getAttribute("uv") as { array: ArrayLike<number> }).array,
    );

    applyFaceUv(geometry, 20, true);
    const second = Array.from(
      (geometry.getAttribute("uv") as { array: ArrayLike<number> }).array,
    );

    expect(second).not.toEqual(first);
  });

  it("uses the back cell when the tile is face down", () => {
    const geometry = createTileGeometry();
    applyFaceUv(geometry, 5, true);
    const up = Array.from(
      (geometry.getAttribute("uv") as { array: ArrayLike<number> }).array,
    );

    applyFaceUv(geometry, 5, false);
    const down = Array.from(
      (geometry.getAttribute("uv") as { array: ArrayLike<number> }).array,
    );

    expect(down).not.toEqual(up);
  });

  it("rejects a geometry without uv", () => {
    const bare = new BufferGeometry();
    expect(() => applyFaceUv(bare, 0, true)).toThrow();
  });
});
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: `./tile-geometry` が見つからず失敗

- [ ] **Step 3: 実装を書く**

`BoxGeometry` の面順は +x, -x, +y, -y, +z, -z である。牌面は +z に置く。

```ts
import {
  BoxGeometry,
  BufferGeometry,
  Material,
  MeshStandardMaterial,
  Texture,
} from "three";

import { TILE } from "./layout";
import { BACK_INDEX, atlasIndexOf, uvOffsetOf } from "./atlas";

/** 牌面が +z を向いた箱。頂点ごとに uv を持つ。 */
export function createTileGeometry(): BufferGeometry {
  const geometry = new BoxGeometry(TILE.width, TILE.height, TILE.depth);
  // BoxGeometry は面ごとに 4 頂点、6 面で 24 頂点を持つ。
  // 牌面（+z）は 5 番目の面、つまり頂点 16..19。
  return geometry;
}

/** 牌面（+z）の4頂点だけをアトラスの該当セルへ移す。 */
export function applyFaceUv(
  geometry: BufferGeometry,
  encoded: number,
  faceUp: boolean,
): void {
  const uv = geometry.getAttribute("uv");
  if (uv === undefined) {
    throw new Error("uv 属性を持たないジオメトリには適用できない");
  }

  const cell = faceUp ? atlasIndexOf(encoded) : BACK_INDEX;
  const { u, v, du, dv } = uvOffsetOf(cell);

  // +z 面の 4 頂点。左上・右上・左下・右下の順。
  const FACE_START = 16;
  const corners: [number, number][] = [
    [u, v + dv],
    [u + du, v + dv],
    [u, v],
    [u + du, v],
  ];
  for (let i = 0; i < corners.length; i += 1) {
    const corner = corners[i]!;
    uv.setXY(FACE_START + i, corner[0], corner[1]);
  }
  uv.needsUpdate = true;
}

/**
 * 牌のマテリアル。
 *
 * 動的シャドウは使わない（仕様 7.1）。影は接地影のデカールで代用するため、
 * ここでは受影も投影も有効にしない。
 */
export function createTileMaterial(atlas: Texture): Material {
  return new MeshStandardMaterial({
    map: atlas,
    roughness: 0.55,
    metalness: 0.0,
  });
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `pnpm --filter @real-mahjong/web test`
Expected: 全テスト PASS

Three.js が Node 上で動かない場合は `vitest.config.ts` の
`test.environment` を `"jsdom"` にし、`jsdom` を dev-dependency に足す。

- [ ] **Step 5: コミット**

```bash
git add apps/web
git commit -m "feat(web): 牌のジオメトリと面 UV の差し替えを追加"
```

---

### Task 5: 卓のシーンと確認用のデモ

実際に目で見られる状態にする。ここまでの純粋関数が正しく組み合わさっているかは、最終的に見て確かめるしかない。

**Files:**
- Create: `apps/web/src/scene/table.ts`
- Create: `apps/web/src/scene/demo.ts`

**Interfaces:**
- Consumes: Task 1〜4 のすべて
- Produces:
  - `export class TableScene` — `constructor(canvas: HTMLCanvasElement)`, `render(): void`, `resize(w, h): void`, `showDemoHand(): void`, `dispose(): void`

- [ ] **Step 1: 実装を書く**

```ts
import {
  AmbientLight,
  DirectionalLight,
  Mesh,
  MeshStandardMaterial,
  PerspectiveCamera,
  PlaneGeometry,
  Scene,
  Texture,
  WebGLRenderer,
} from "three";

import { drawPlaceholderAtlas } from "./atlas";
import { TILE, discardPosition, handPosition, wallPosition } from "./layout";
import { applyFaceUv, createTileGeometry, createTileMaterial } from "./tile-geometry";
import { TilePool } from "./tile-pool";

/**
 * 卓のシーン。
 *
 * カメラは自席からの固定俯瞰（仕様 7.1）。動的シャドウは使わず、
 * 環境光と平行光の2灯で立体感を出す。
 */
export class TableScene {
  readonly #renderer: WebGLRenderer;
  readonly #scene = new Scene();
  readonly #camera: PerspectiveCamera;
  readonly #pool = new TilePool();
  readonly #atlas: Texture;
  readonly #meshes = new Map<number, Mesh>();

  constructor(canvas: HTMLCanvasElement) {
    this.#renderer = new WebGLRenderer({ canvas, antialias: true });
    this.#renderer.setPixelRatio(Math.min(devicePixelRatio, 2));

    this.#camera = new PerspectiveCamera(38, 16 / 9, 0.1, 100);
    // 自席の後ろやや上から卓面を見下ろす。
    this.#camera.position.set(0, 14, 13);
    this.#camera.lookAt(0, 0, 1);

    this.#atlas = new Texture(drawPlaceholderAtlas());
    this.#atlas.needsUpdate = true;

    this.#scene.add(new AmbientLight(0xffffff, 0.7));
    const key = new DirectionalLight(0xffffff, 0.9);
    key.position.set(4, 12, 6);
    this.#scene.add(key);

    // 卓面。
    const felt = new Mesh(
      new PlaneGeometry(20, 20),
      new MeshStandardMaterial({ color: 0x14532d, roughness: 0.95 }),
    );
    felt.rotation.x = -Math.PI / 2;
    this.#scene.add(felt);
  }

  /** 4人分の手牌・河・山を仮に並べて、配置と見た目を確かめる。 */
  showDemoHand(): void {
    for (let seat = 0; seat < 4; seat += 1) {
      for (let i = 0; i < 13; i += 1) {
        // 自席だけ表向き、他家は伏せる。
        this.#place(seat === 0 ? i * 2 : 0, handPosition(seat, i, 13), seat === 0);
      }
      for (let i = 0; i < 8; i += 1) {
        this.#place((i * 3) % 34, discardPosition(seat, i), true);
      }
      for (let i = 0; i < 34; i += 1) {
        this.#place(0, wallPosition(seat, i), false);
      }
    }
  }

  #place(encoded: number, position: { x: number; y: number; z: number }, faceUp: boolean): void {
    const tile = this.#pool.acquire(encoded);
    tile.position = position;
    tile.faceUp = faceUp;

    const geometry = createTileGeometry();
    applyFaceUv(geometry, encoded, faceUp);
    const mesh = new Mesh(geometry, createTileMaterial(this.#atlas));
    mesh.position.set(position.x, position.y, position.z);
    // 手牌は立てて、河と山は寝かせる。
    if (position.y > TILE.depth) {
      mesh.rotation.x = -Math.PI / 12;
    } else {
      mesh.rotation.x = -Math.PI / 2;
    }
    this.#scene.add(mesh);
    this.#meshes.set(tile.id, mesh);
  }

  resize(width: number, height: number): void {
    this.#camera.aspect = width / height;
    this.#camera.updateProjectionMatrix();
    this.#renderer.setSize(width, height, false);
  }

  render(): void {
    this.#renderer.render(this.#scene, this.#camera);
  }

  dispose(): void {
    for (const mesh of this.#meshes.values()) {
      mesh.geometry.dispose();
    }
    this.#meshes.clear();
    this.#atlas.dispose();
    this.#renderer.dispose();
  }
}
```

`apps/web/src/scene/demo.ts`:

```ts
import { TableScene } from "./table";

/**
 * 卓の見た目を確かめるための入口。
 *
 * main.ts への結線は Wave 1 完了後にコーディネータが行うため、
 * ここでは呼ばれたときだけ動く形にしておく。
 */
export function mountDemo(container: HTMLElement): () => void {
  const canvas = document.createElement("canvas");
  container.appendChild(canvas);

  const scene = new TableScene(canvas);
  scene.showDemoHand();

  const resize = () => {
    scene.resize(container.clientWidth, container.clientHeight);
    scene.render();
  };
  resize();
  addEventListener("resize", resize);

  return () => {
    removeEventListener("resize", resize);
    scene.dispose();
    canvas.remove();
  };
}
```

- [ ] **Step 2: 型検査を通す**

Run: `pnpm --filter @real-mahjong/web typecheck`
Expected: エラーなし

- [ ] **Step 3: 全テストを通す**

Run: `pnpm --filter @real-mahjong/web test`
Expected: 全テスト PASS

- [ ] **Step 4: コミット**

```bash
git add apps/web
git commit -m "feat(web): 卓のシーンと確認用デモを追加"
```

---

## Wave 1c 完了の判定

- [ ] `pnpm --filter @real-mahjong/web typecheck` が通る
- [ ] `pnpm --filter @real-mahjong/web test` が全通過する
- [ ] 座標計算とアトラスの UV 計算が Three.js に依存せずテストされている
- [ ] `apps/web/src/timeline/` と `main.ts` を編集していない
- [ ] 動的シャドウマップを使っていない
