import { describe, expect, it } from "vitest";

import type { ClientEvent } from "../protocol/ClientEvent";
import { apply, emptyState } from "../game/state";
import type { GameState } from "../game/state";
import { TILE } from "./layout";
import { pickFrom, placementsFor } from "./placement";

let seq = 0;
function fold(events: ClientEvent[], you = 0): GameState {
  let state = emptyState(you);
  for (const event of events) {
    state = apply(state, { seq: (seq += 1), event }, 0);
  }
  return state;
}

const roundStart: ClientEvent = {
  type: "round_start",
  round: { wind: "East", number: 1 },
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

/** 3x3 の行列の掛け算。 */
function mul(a: number[][], b: number[][]): number[][] {
  const out: number[][] = [
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
  ];
  for (let i = 0; i < 3; i += 1) {
    for (let j = 0; j < 3; j += 1) {
      let sum = 0;
      for (let k = 0; k < 3; k += 1) sum += (a[i]?.[k] ?? 0) * (b[k]?.[j] ?? 0);
      out[i]![j] = sum;
    }
  }
  return out;
}

const rotX = (t: number) => [
  [1, 0, 0],
  [0, Math.cos(t), -Math.sin(t)],
  [0, Math.sin(t), Math.cos(t)],
];
const rotY = (t: number) => [
  [Math.cos(t), 0, Math.sin(t)],
  [0, 1, 0],
  [-Math.sin(t), 0, Math.cos(t)],
];
const rotZ = (t: number) => [
  [Math.cos(t), -Math.sin(t), 0],
  [Math.sin(t), Math.cos(t), 0],
  [0, 0, 1],
];

/**
 * 牌の姿勢。
 *
 * `rotationZ` を使わないので、Three.js の既定（`"XYZ"`、合成は
 * `Rz · Ry · Rx`）でも `Ry · Rx` になり、「寝かせてから席の向きへ回す」
 * という意図どおりになる。
 */
function orientation(p: { rotationX: number; rotationY: number }): number[][] {
  // Three.js の既定（XYZ 順）は合成が Rz·Ry·Rx。rotationZ を使わないので
  // Ry·Rx になり、「寝かせてから席の向きへ回す」という意図どおりになる。
  return mul(rotY(p.rotationY), rotX(p.rotationX));
}

/** 牌の面が向いている先。 */
function faceNormal(p: { rotationX: number; rotationY: number }) {
  const r = orientation(p);
  return { x: r[0]?.[2] ?? 0, y: r[1]?.[2] ?? 0, z: r[2]?.[2] ?? 0 };
}

/**
 * 牌が卓の上で占める範囲。
 *
 * **回した結果から厳密に出す。**幅と奥行きを入れ替えるだけの近似では、
 * 席が回ったときに実際の占有と合わない。
 */
function footprint(p: {
  position: { x: number; y: number; z: number };
  rotationX: number;
  rotationY: number;
}) {
  // **寸法を書き写さない。**`TILE` を変えたときに試験だけが古い値のまま
  // 残り、ぶつかっていないものを「ぶつかった」と言い出す。
  const half = [TILE.width / 2, TILE.height / 2, TILE.depth / 2];
  const r = orientation(p);
  const extent = [0, 1, 2].map((i) =>
    [0, 1, 2].reduce((sum, j) => sum + Math.abs(r[i]?.[j] ?? 0) * (half[j] ?? 0), 0),
  );
  return {
    x0: p.position.x - (extent[0] ?? 0),
    x1: p.position.x + (extent[0] ?? 0),
    z0: p.position.z - (extent[2] ?? 0),
    z1: p.position.z + (extent[2] ?? 0),
    y: p.position.y,
  };
}

/** 卓（26×26）に収まっているか。 */
function within(p: Parameters<typeof footprint>[0]): boolean {
  const f = footprint(p);
  return f.x0 > -13 && f.x1 < 13 && f.z0 > -13 && f.z1 < 13;
}

/**
 * 重なっている組を返す。
 *
 * **一部の種類だけを見てはいけない。**河と山を外していたために、
 * 隣席の河どうしの重なりと、山の四辺が角で交差することを見落として
 * いた。席をまたぐ組はすべて見る。
 *
 * 同じ席の同じ種類だけは外す。手牌と副露は多すぎるとき詰めるので
 * 触れ合うし、山は隣どうしが接するためである。**ただし河は外さない。**
 * リーチ宣言牌が隣に食い込むのは誤りだからである。副露の中の食い込みは
 * 「副露の中で、倒した牌が隣の牌に食い込まない」で個別に見ている。
 */
function overlappingPairs(all: ReturnType<typeof placementsFor>): string[] {
  const pairs: string[] = [];
  const solid = all;
  for (let i = 0; i < solid.length; i += 1) {
    for (let j = i + 1; j < solid.length; j += 1) {
      const a = solid[i];
      const b = solid[j];
      if (!a || !b) continue;
      // **同じ席の同じ種類を丸ごと外してはいけない。**河のリーチ宣言牌は
      // 横に倒れて幅が 1.35 になるので、同じ河の隣の牌に食い込みうる。
      // 隣り合う牌が接するのは許すが、食い込みは許さない。
      if (a.seat === b.seat && a.kind === b.kind && a.kind !== "river") continue;
      // 加槓は意図して同じ場所へ積む。高さが違えば重なりではない。
      if (Math.abs(a.position.y - b.position.y) > 0.5) continue;
      const fa = footprint(a);
      const fb = footprint(b);
      const hit =
        fa.x0 < fb.x1 - 0.001 &&
        fb.x0 < fa.x1 - 0.001 &&
        fa.z0 < fb.z1 - 0.001 &&
        fb.z0 < fa.z1 - 0.001;
      if (hit) pairs.push(`${a.key} と ${b.key}`);
    }
  }
  return pairs.slice(0, 3);
}

/** その席の、倒していない牌の向き。 */
function river0Facing(seat: number): number {
  return ((seat - 0 + 4) % 4) * (Math.PI / 2);
}

describe("盤面から牌の置き場所を出す", () => {
  it("何も始まっていなければ何も置かない", () => {
    expect(placementsFor(emptyState(0))).toEqual([]);
  });

  it("自分の手牌は表向きで13枚", () => {
    const mine = placementsFor(fold([roundStart, deal])).filter((p) => p.kind === "hand" && p.seat === 0);
    expect(mine).toHaveLength(13);
    expect(mine.every((p) => p.faceUp)).toBe(true);
  });

  it("他家の手牌は伏せる", () => {
    // **表にしたら覗き見になる。**
    const others = placementsFor(fold([roundStart, deal])).filter((p) => p.kind === "hand" && p.seat !== 0);
    expect(others).toHaveLength(39);
    expect(others.every((p) => !p.faceUp)).toBe(true);
  });

  it("自分の手牌は手前に並ぶ", () => {
    // 自席から見て手前は +z。
    const mine = placementsFor(fold([roundStart, deal])).filter((p) => p.kind === "hand" && p.seat === 0);
    expect(mine.every((p) => p.position.z > 0)).toBe(true);
  });

  it("下家の手牌は右側に並ぶ", () => {
    // **絶対席を相対席へ直さないと、自分の手牌が対面に出る。**
    const right = placementsFor(fold([roundStart, deal])).filter((p) => p.kind === "hand" && p.seat === 1);
    expect(right.every((p) => p.position.x > 0)).toBe(true);
  });

  it("どの席から見ても、自分が手前・下家が右・対面が奥・上家が左", () => {
    // **席の変換を間違えると、自分の手牌が対面に並ぶ。**
    for (let viewer = 0; viewer < 4; viewer += 1) {
      const all = placementsFor(fold([roundStart, deal], viewer), viewer).filter(
        (p) => p.kind === "hand",
      );
      const at = (offset: number) => all.filter((p) => p.seat === (viewer + offset) % 4);
      expect(at(0).every((p) => p.position.z > 0), `viewer=${viewer} 自分が手前`).toBe(true);
      expect(at(1).every((p) => p.position.x > 0), `viewer=${viewer} 下家が右`).toBe(true);
      expect(at(2).every((p) => p.position.z < 0), `viewer=${viewer} 対面が奥`).toBe(true);
      expect(at(3).every((p) => p.position.x < 0), `viewer=${viewer} 上家が左`).toBe(true);
    }
  });

  it("ツモ牌は手牌から離して置く", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
    ]);
    const drawn = placementsFor(state).filter((p) => p.kind === "drawn");
    expect(drawn).toHaveLength(1);
    expect(drawn[0]?.encoded).toBe(33);
    expect(drawn[0]?.faceUp).toBe(true);
  });

  it("河は表向きで捨てた順に並ぶ", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
    ]);
    const river = placementsFor(state).filter((p) => p.kind === "river");
    expect(river).toHaveLength(1);
    expect(river[0]?.encoded).toBe(5);
    expect(river[0]?.faceUp).toBe(true);
  });

  it("リーチ宣言牌は横に倒す", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "riichi", seat: 2, step: "declare" },
      { type: "draw", seat: 2, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 2, tile: 9, manner: "tedashi" },
    ]);
    const river = placementsFor(state).filter((p) => p.kind === "river" && p.seat === 2);
    // **同じ席の普通の打牌と比べる。**別の席と比べると、席ごとの
    // 向きの差だけで通ってしまい、倒し忘れを捕まえられない。
    const plain = placementsFor(
      fold([
        roundStart,
        deal,
        { type: "draw", seat: 2, tile: null, source: "wall", wall_remaining: 69 },
        { type: "discard", seat: 2, tile: 9, manner: "tedashi" },
      ]),
    ).filter((p) => p.kind === "river" && p.seat === 2);
    expect((river[0]?.rotationY ?? 0) - (plain[0]?.rotationY ?? 0)).toBeCloseTo(Math.PI / 2);
  });

  it("副露は表向きで並ぶ", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
      { type: "call", seat: 2, from: 1, kind: "pon", tiles: [5, 5, 5] },
    ]);
    const melds = placementsFor(state).filter((p) => p.kind === "meld");
    expect(melds).toHaveLength(3);
    expect(melds.every((p) => p.faceUp)).toBe(true);
  });

  it("暗槓は伏せて並べる", () => {
    // **暗槓の中身は見せない。**
    const state = fold([
      roundStart,
      {
        type: "deal",
        your_hand: [5, 5, 5, 5, 0, 1, 2, 9, 10, 11, 18, 19, 20],
        hand_sizes: [13, 13, 13, 13],
        dora_indicator: 8,
      },
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
      { type: "call", seat: 0, from: 0, kind: "ankan", tiles: [5, 5, 5, 5] },
    ]);
    const melds = placementsFor(state).filter((p) => p.kind === "meld");
    expect(melds).toHaveLength(4);
    expect(melds.some((p) => !p.faceUp)).toBe(true);
  });

  it("暗槓は両端2枚だけ伏せ、中2枚は表にする", () => {
    // **4枚とも伏せると何を槓したか分からない。**
    const state = fold([
      roundStart,
      {
        type: "deal",
        your_hand: [5, 5, 5, 5, 0, 1, 2, 9, 10, 11, 18, 19, 20],
        hand_sizes: [13, 13, 13, 13],
        dora_indicator: 8,
      },
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
      { type: "call", seat: 0, from: 0, kind: "ankan", tiles: [5, 5, 5, 5] },
    ]);
    const melds = placementsFor(state).filter((p) => p.kind === "meld");
    expect(melds.map((p) => p.faceUp)).toEqual([false, true, true, false]);
    expect(melds[1]?.encoded).toBe(5);
  });

  it("鳴いた牌は横に倒し、取得元で置く場所が変わる", () => {
    // **倒さないと誰から鳴いたか分からない。**
    const ponFrom = (from: number, caller: number) =>
      placementsFor(
        fold([
          roundStart,
          deal,
          { type: "draw", seat: from, tile: null, source: "wall", wall_remaining: 69 },
          { type: "discard", seat: from, tile: 5, manner: "tedashi" },
          { type: "call", seat: caller, from, kind: "pon", tiles: [5, 5, 5] },
        ]),
      ).filter((p) => p.kind === "meld" && p.seat === caller);

    // 席1が席0（上家）から鳴いたら左端が倒れる。
    const fromLeft = ponFrom(0, 1);
    expect(fromLeft.map((p) => p.rotationY !== river0Facing(p.seat))).toEqual([true, false, false]);

    // 席1が席2（下家）から鳴いたら右端が倒れる。
    const fromRight = ponFrom(2, 1);
    expect(fromRight.map((p) => p.rotationY !== river0Facing(p.seat))).toEqual([false, false, true]);

    // 席1が席3（対面）から鳴いたら真ん中が倒れる。
    const fromAcross = ponFrom(3, 1);
    expect(fromAcross.map((p) => p.rotationY !== river0Facing(p.seat))).toEqual([false, true, false]);
  });

  it("大明槓は4枚並び、鳴いた牌が倒れる", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 3, tile: null, source: "wall", wall_remaining: 60 },
      { type: "discard", seat: 3, tile: 5, manner: "tedashi" },
      { type: "call", seat: 0, from: 3, kind: "minkan", tiles: [5, 5, 5, 5] },
    ]);
    const melds = placementsFor(state).filter((p) => p.kind === "meld");
    expect(melds).toHaveLength(4);
    expect(melds.filter((p) => p.rotationY !== river0Facing(p.seat))).toHaveLength(1);
    // 上家から鳴いたので左端。
    expect(melds[0]?.rotationY).not.toBe(0);
    expect(melds.every((p) => p.faceUp)).toBe(true);
  });

  it("副露の中で、倒した牌が隣の牌に食い込まない", () => {
    // **倒すと幅が 1.35 になる。**間隔 1 のまま並べると食い込む。
    // 手牌と副露は詰めることがあるので総当たりからは外してあり、
    // ここで個別に見る。
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 3, tile: null, source: "wall", wall_remaining: 60 },
      { type: "discard", seat: 3, tile: 5, manner: "tedashi" },
      { type: "call", seat: 0, from: 3, kind: "pon", tiles: [5, 5, 5] },
    ]);
    const melds = placementsFor(state).filter((p) => p.kind === "meld");
    expect(melds).toHaveLength(3);
    for (let i = 1; i < melds.length; i += 1) {
      const left = footprint(melds[i - 1]!);
      const right = footprint(melds[i]!);
      expect(right.x0).toBeGreaterThanOrEqual(left.x1 - 0.001);
    }
  });

  it("どの席でも牌の面が正しい向きを向く", () => {
    // **回転の合成順を間違えると、席によって牌の面が上や下を向く。**
    // 手牌は席の正面（水平）、河と山は上、副露の鳴いた牌も席の正面。
    for (let viewer = 0; viewer < 4; viewer += 1) {
      const state = fold(
        [
          roundStart,
          deal,
          { type: "draw", seat: 3, tile: null, source: "wall", wall_remaining: 60 },
          { type: "discard", seat: 3, tile: 5, manner: "tedashi" },
          { type: "call", seat: 0, from: 3, kind: "pon", tiles: [5, 5, 5] },
        ],
        viewer,
      );
      for (const p of placementsFor(state, viewer)) {
        const n = faceNormal(p);
        if (p.kind === "river" || p.kind === "wall" || p.kind === "meld") {
          // 寝かせた牌は面が真上。
          expect(Math.abs(n.y - 1), `${p.key} の面が上を向いていない`).toBeLessThan(0.01);
        } else {
          // **立てた牌は、その席から見て手前（+z を回した向き）を向く。**
          // 上下を向かないだけでは、左右が逆でも通ってしまう。
          const relative = (p.seat - viewer + 4) % 4;
          const want = {
            x: Math.sin((relative * Math.PI) / 2),
            z: Math.cos((relative * Math.PI) / 2),
          };
          // **自分の手牌は読ませるために手前へ倒してある。**倒すと面が
          // 斜め上を向くので、水平成分だけを見て左右と前後を確かめる。
          const mine = p.seat === state.you && (p.kind === "hand" || p.kind === "drawn");
          if (mine) {
            expect(n.y, `${p.key} が手前へ倒れていない`).toBeGreaterThan(0.1);
          } else {
            expect(Math.abs(n.y), `${p.key} の面が上下を向いている`).toBeLessThan(0.01);
          }
          const flat = Math.hypot(n.x, n.z);
          expect(flat, `${p.key} の面が真上を向いている`).toBeGreaterThan(0.1);
          expect(
            Math.abs(n.x / flat - want.x),
            `${p.key} の面の左右が違う`,
          ).toBeLessThan(0.01);
          expect(
            Math.abs(n.z / flat - want.z),
            `${p.key} の面の前後が違う`,
          ).toBeLessThan(0.01);
        }
      }
    }
  });

  it("山も見ている席に合わせて回る", () => {
    // **ここだけ絶対席のままだと、席を変えたとき山の欠けだけが動かない。**
    const events: ClientEvent[] = [
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 20 },
    ];
    const asEast = placementsFor(fold(events, 0), 0).filter((p) => p.kind === "wall");
    const asSouth = placementsFor(fold(events, 1), 1).filter((p) => p.kind === "wall");
    expect(asEast).toHaveLength(20);
    expect(asSouth).toHaveLength(20);
    // 見る席が変われば、同じ鍵の牌が別の場所に来る。
    const a = asEast[0];
    const b = asSouth.find((p) => p.key === a?.key);
    expect(b).toBeDefined();
    expect(
      Math.abs((a?.position.x ?? 0) - (b?.position.x ?? 0)) +
        Math.abs((a?.position.z ?? 0) - (b?.position.z ?? 0)),
    ).toBeGreaterThan(1);
  });

  it("押せるのは自分の手牌とツモ牌だけ", () => {
    // **他家の伏せ牌は中身が 0（一萬）。**押せると一萬を切ってしまう。
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
    ]);
    const all = placementsFor(state);
    expect(all.filter((p) => p.pickable).every((p) => p.seat === 0)).toBe(true);
    expect(all.filter((p) => p.pickable && p.kind === "hand")).toHaveLength(13);
    expect(all.filter((p) => p.pickable && p.kind === "drawn")).toHaveLength(1);
    expect(all.filter((p) => p.kind !== "hand" && p.kind !== "drawn").some((p) => p.pickable)).toBe(false);
  });

  it("山は一続きで、減るのは片端から", () => {
    // **4辺へ1枚ずつ配ると、1枚ツモるたびに4辺が順番に欠ける。**
    const wallOf = (remaining: number) =>
      placementsFor(
        fold([
          roundStart,
          deal,
          { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: remaining },
        ]),
      ).filter((p) => p.kind === "wall");

    // 残り12枚なら、山は1辺か2辺に収まる。**4辺に散っていたら
    // 巡回して配っている。**鍵の集合を比べるだけでは、この誤りを
    // 捕まえられない。
    const few = wallOf(12);
    expect(few).toHaveLength(12);
    expect(new Set(few.map((p) => p.seat)).size).toBeLessThanOrEqual(2);

    // 減った1枚を除けば、残りはそっくり同じ場所にある。
    const before = wallOf(40);
    const after = wallOf(39);
    const beforeKeys = before.map((p) => p.key);
    const afterKeys = after.map((p) => p.key);
    expect(afterKeys.every((key) => beforeKeys.includes(key))).toBe(true);
    expect(beforeKeys.filter((key) => !afterKeys.includes(key))).toHaveLength(1);
  });

  it("山は残り枚数ぶんだけ伏せて置く", () => {
    // **山の中身はサーバから来ない。**見せかけである。
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 12 },
    ]);
    const wall = placementsFor(state).filter((p) => p.kind === "wall");
    expect(wall).toHaveLength(12);
    expect(wall.every((p) => !p.faceUp)).toBe(true);
  });

  it("ドラ表示は卓に置かない", () => {
    // **19枚目以降の河が卓の中央（z=0.15）まで届く。**そこへドラを
    // 置くと重なる。ドラは画面の上段に文字と牌で出す。
    const state = fold([roundStart, deal, { type: "dora_reveal", indicator: 7 }]);
    expect(placementsFor(state).some((p) => p.key.startsWith("dora"))).toBe(false);
  });

  it("手牌は立て、河と副露と山は寝かせる", () => {
    // **牌面は作られた時点で +z を向いている。**寝かせないと河の牌が
    // 卓から生えて立つ。
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 60 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
      { type: "call", seat: 2, from: 1, kind: "pon", tiles: [5, 5, 5] },
    ]);
    const all = placementsFor(state);
    // **副露は卓に寝かせて面を上へ向ける。**立てると卓上の副露に見えず、
    // 倒した牌が卓面から浮く。
    const standing = all.filter(
      (p) => (p.kind === "hand" || p.kind === "drawn") && p.seat !== state.you,
    );
    // **自分の手牌だけは手前へ倒す。**俯瞰のカメラからは、真っ直ぐ立てた
    // 面がほぼ真横になって読めない。
    const mine = all.filter(
      (p) => (p.kind === "hand" || p.kind === "drawn") && p.seat === state.you,
    );
    const lying = all.filter(
      (p) => p.kind === "river" || p.kind === "wall" || p.kind === "meld",
    );
    expect(standing.every((p) => p.rotationX === 0)).toBe(true);
    expect(lying.every((p) => p.rotationX === -Math.PI / 2)).toBe(true);
    expect(mine.length).toBeGreaterThan(0);
    // 立てきってもいないし、寝かせきってもいない。**どちらかに寄せると、
    // 読めないか、他家の伏せ牌や河と見分けがつかなくなる。**
    expect(mine.every((p) => p.rotationX < 0)).toBe(true);
    expect(mine.every((p) => p.rotationX > -Math.PI / 2)).toBe(true);
  });

  it("副露が増えても卓からはみ出さない", () => {
    // **手牌の列を伸ばして置くと、2つ目の副露で卓の外へ出る。**
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 60 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
      { type: "call", seat: 0, from: 1, kind: "pon", tiles: [5, 5, 5] },
      { type: "discard", seat: 0, tile: 0, manner: "tedashi" },
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 59 },
      { type: "discard", seat: 1, tile: 9, manner: "tedashi" },
      { type: "call", seat: 0, from: 1, kind: "pon", tiles: [9, 9, 9] },
      { type: "discard", seat: 0, tile: 1, manner: "tedashi" },
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 58 },
      { type: "discard", seat: 1, tile: 18, manner: "tedashi" },
      { type: "call", seat: 0, from: 1, kind: "pon", tiles: [18, 18, 18] },
      { type: "discard", seat: 0, tile: 2, manner: "tedashi" },
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 57 },
      { type: "discard", seat: 1, tile: 27, manner: "tedashi" },
      { type: "call", seat: 0, from: 1, kind: "pon", tiles: [27, 27, 27] },
    ]);
    const all = placementsFor(state);
    const melds = all.filter((p) => p.kind === "meld");
    expect(melds).toHaveLength(12);
    // **中心だけを見ても足りない。**牌の実寸で卓に収まるか見る。
    expect(all.every((p) => within(p))).toBe(true);
    // **隣の席の列とぶつからない。**
    expect(overlappingPairs(all)).toEqual([]);
  });

  it("リーチ宣言牌が同じ河の隣の牌に食い込まない", () => {
    // **横に倒すと幅が 1.35 になる。**間隔 1 のまま並べると 0.175 食い込む。
    let events: ClientEvent[] = [roundStart, deal];
    for (let turn = 0; turn < 8; turn += 1) {
      const step: ClientEvent[] = [
        { type: "draw", seat: 2, tile: null, source: "wall", wall_remaining: 60 - turn },
        ...(turn === 2 ? [{ type: "riichi", seat: 2, step: "declare" } as ClientEvent] : []),
        { type: "discard", seat: 2, tile: turn, manner: "tedashi" },
      ];
      events = events.concat(step);
    }
    const river = placementsFor(fold(events)).filter((p) => p.kind === "river" && p.seat === 2);
    expect(river).toHaveLength(8);
    expect(river.filter((p) => p.rotationY !== river[0]?.rotationY)).toHaveLength(1);
    expect(overlappingPairs(placementsFor(fold(events)))).toEqual([]);
  });

  it("河が長くなっても、4席のどれとも重ならない", () => {
    // **各席24枚まで捨てる。**3段で収まるうちは気づかないが、
    // 4段目は中心へ寄るので、対面だけでなく隣席の河とも重なる。
    let events: ClientEvent[] = [roundStart, deal];
    for (let turn = 0; turn < 24; turn += 1) {
      for (let seat = 0; seat < 4; seat += 1) {
        const step: ClientEvent[] = [
          {
            type: "draw",
            seat,
            tile: seat === 0 ? 33 : null,
            source: "wall",
            wall_remaining: 70 - turn * 4 - seat,
          },
          { type: "discard", seat, tile: (turn + seat) % 34, manner: "tsumogiri" },
        ];
        events = events.concat(step);
      }
    }
    const all = placementsFor(fold(events));
    const rivers = all.filter((p) => p.kind === "river");
    expect(rivers.filter((p) => p.seat === 0)).toHaveLength(24);
    expect(rivers.filter((p) => p.seat === 2)).toHaveLength(24);
    expect(all.every((p) => within(p))).toBe(true);
    expect(overlappingPairs(all)).toEqual([]);
  });

  it("四副露でも隣の席とぶつからない", () => {
    // 手牌と副露で18枚ぶんになる極端な場合。**そのまま並べると
    // 卓の角で隣席の列と重なる。**
    let events: ClientEvent[] = [roundStart, deal];
    const kinds = [5, 9, 18, 27];
    for (const kind of kinds) {
      const step: ClientEvent[] = [
        { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 60 },
        { type: "discard", seat: 1, tile: kind, manner: "tedashi" },
        { type: "call", seat: 0, from: 1, kind: "minkan", tiles: [kind, kind, kind, kind] },
        { type: "draw", seat: 0, tile: 33, source: "dead_wall", wall_remaining: 60 },
        { type: "discard", seat: 0, tile: 33, manner: "tsumogiri" },
      ];
      events = events.concat(step);
    }
    const all = placementsFor(fold(events));
    expect(all.filter((p) => p.kind === "meld")).toHaveLength(16);
    expect(all.every((p) => within(p))).toBe(true);
    expect(overlappingPairs(all)).toEqual([]);
  });

  it("チーで倒れるのは鳴いた牌そのもの", () => {
    // **同じ牌ばかりのポンでは、どれを倒したか分からない。**
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 3, tile: null, source: "wall", wall_remaining: 60 },
      { type: "discard", seat: 3, tile: 2, manner: "tedashi" },
      // 席0は席3の下家。手牌から 1m と 3m を出し、2m を鳴く。
      { type: "call", seat: 0, from: 3, kind: "chi", tiles: [0, 1, 2] },
    ]);
    const melds = placementsFor(state).filter((p) => p.kind === "meld");
    const sideways = melds.filter((p) => p.rotationY !== river0Facing(p.seat));
    expect(sideways).toHaveLength(1);
    expect(sideways[0]?.encoded).toBe(2);
    // 上家から鳴いたので左端。
    expect(melds[0]?.encoded).toBe(2);
  });

  it("加槓で倒れるのは足した牌ではなく鳴いた牌の位置", () => {
    // **加槓の末尾は後から足した4枚目。**末尾を鳴いた牌として倒すと
    // 加槓だけ別の牌が倒れる。
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 3, tile: null, source: "wall", wall_remaining: 60 },
      { type: "discard", seat: 3, tile: 5, manner: "tedashi" },
      { type: "call", seat: 0, from: 3, kind: "pon", tiles: [5, 5, 5] },
      { type: "call", seat: 0, from: 3, kind: "kakan", tiles: [5, 5, 5, 5] },
    ]);
    const melds = placementsFor(state).filter((p) => p.kind === "meld");
    expect(melds).toHaveLength(4);
    // 上家から鳴いたので左端が倒れる。**4枚目はその上へ積む。**
    const called = melds[0];
    const fourth = melds[3];
    expect(called?.rotationY).not.toBe(0);
    expect(fourth?.rotationY).not.toBe(0);
    expect(fourth?.position.x).toBeCloseTo(called?.position.x ?? 0);
    expect(fourth?.position.z).toBeCloseTo(called?.position.z ?? 0);
    expect(fourth?.position.y).toBeGreaterThan(called?.position.y ?? 0);
    // 真ん中の2枚は立ったまま。
    expect(melds[1]?.rotationY).toBe(0);
    expect(melds[2]?.rotationY).toBe(0);
  });

  it("手前が押せない牌なら、その奥の手牌を拾わない", () => {
    // **河の陰の手牌を押せると、狙っていない牌が飛ぶ。**
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 60 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
    ]);
    const all = placementsFor(state);
    const mine = all.find((p) => p.pickable);
    const river = all.find((p) => p.kind === "river");
    expect(
      pickFrom(
        [
          { key: river?.key ?? "", distance: 1 },
          { key: mine?.key ?? "", distance: 5 },
        ],
        all,
      ),
    ).toBeNull();
    expect(pickFrom([{ key: mine?.key ?? "", distance: 1 }], all)).toBe(mine?.encoded);
    expect(pickFrom([], all)).toBeNull();
  });

  it("鍵は牌ごとに違う", () => {
    // **同じ鍵が2つあると、牌が消えたり重なったりする。**
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 30 },
    ]);
    const keys = placementsFor(state).map((p) => p.key);
    expect(new Set(keys).size).toBe(keys.length);
  });
});
