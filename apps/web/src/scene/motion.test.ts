import { describe, expect, it } from "vitest";

import { motionsFor, poseAt, reconcileMoving } from "./motion";
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
 * 手牌を配るのは `deal` である。
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
 * 現れない形になる。
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
  return { before: state, after: apply(state, envelope(9, event), 0) };
}

/** 出し入れの判断だけを見る試験のための、中身が空の動き。 */
function motionOf(id: string): Motion {
  return {
    id,
    fromKey: `${id}-from`,
    toKey: `${id}-to`,
    encoded: 0,
    faceUp: false,
    from: { position: { x: 0, y: 0, z: 0 }, rotationX: 0, rotationY: 0 },
    to: { position: { x: 0, y: 0, z: 0 }, rotationX: 0, rotationY: 0 },
    lift: 0,
  };
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
    // 13枚が14枚になると残りの13枚も動く。
    expect(motions.length).toBeGreaterThan(1);
  });

  it("山が一度に複数減ったら、消えた端のうち最も小さい番号から動く", () => {
    // **1枚しか消えない場合では、最小を選ぶ規則が効いているか分からない。**
    // slot = WALL_SLOTS - wallRemaining + index なので、残りが減るほど
    // 開始の番号が上がる。消えるのは常に小さい側である。
    const event: ClientEvent = {
      type: "draw",
      seat: 0,
      tile: 4,
      source: "wall",
      wall_remaining: 67,
    };
    const { before, after } = step(started(), event);
    const motions = motionsFor(
      placementsFor(before, 0),
      placementsFor(after, 0),
      event,
      0,
    );

    const drawn = motions.find((m) => m.toKey === "drawn-0");
    // 70 -> 67 なので、消えるのは slot 66, 67, 68。最も小さいものを使う。
    expect(drawn?.fromKey).toBe("wall-66");
  });

  it("そのまま居座る牌以外は、すべて動きで説明できる", () => {
    // **これがこのウェーブの Goal そのものである。**
    //
    // 「位置が変わったものに動きがあるか」では足りない。前の盤面に鍵が
    // 無い牌（ツモ牌が手牌へ吸収されるなど）を飛ばしてしまい、**瞬間移動が
    // 起きるまさにその場所を検査しない。**
    const scenarios: { name: string; from: GameState; event: ClientEvent }[] = [
      {
        name: "自分のツモ",
        from: started(),
        event: { type: "draw", seat: 0, tile: 4, source: "wall", wall_remaining: 69 },
      },
      {
        name: "ツモ牌を持ったままの自分のツモ",
        from: drawnMine(),
        event: { type: "draw", seat: 0, tile: 8, source: "dead_wall", wall_remaining: 68 },
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

    // 0.9pi から -0.9pi へは、0 を通る 1.8pi ではなく pi を跨ぐ 0.2pi が近い。
    // **素直に線形補間すると牌が一周する。**
    const mid = poseAt(motion, 0.5).rotationY;
    expect(Math.abs(mid)).toBeGreaterThan(Math.PI * 0.9);
  });

  it("代理は、続いている動きだけ残して他は捨てる", () => {
    const a = motionOf("a");
    const b = motionOf("b");

    const first = reconcileMoving([], [a, b]);
    expect(first.create.sort()).toEqual(["a", "b"]);
    expect(first.drop).toEqual([]);

    const next = reconcileMoving(["a", "b"], [a, motionOf("c")]);
    expect(next.keep).toEqual(["a"]);
    expect(next.drop).toEqual(["b"]);
    expect(next.create).toEqual(["c"]);
  });

  it("動きが無くなったら代理を全部捨てる", () => {
    // **1枚でも残ると、牌が空中で止まったままになる。**
    const plan = reconcileMoving(["a", "b"], []);
    expect(plan.drop.sort()).toEqual(["a", "b"]);
    expect(plan.keep).toEqual([]);
    expect(plan.create).toEqual([]);
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
    const { before, after } = step(drawnOther(), event);
    const motions = motionsFor(
      placementsFor(before, 0),
      placementsFor(after, 0),
      event,
      0,
    );

    const toRiver = motions.find((m) => m.toKey === "river-1-0");
    expect(toRiver).toBeDefined();
    // 手牌は立ち、河は寝る。**姿勢を補間しないと途中で牌が刺さって見える。**
    expect(toRiver?.from.rotationX).not.toBe(toRiver?.to.rotationX);
  });
});
