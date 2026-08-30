import { Euler, Vector3 } from "three";
import { describe, expect, it } from "vitest";

import { ROTATION_ORDER, placementsFor } from "./placement";
import type { GameState, SeatView } from "../game/state";
import type { Seat } from "../protocol/Seat";
import type { Tile } from "../protocol/Tile";

/**
 * 卓に寝かせた牌が、どの席でも牌面を上へ向けていることを見張る。
 *
 * **これは実際に壊れていた。**回転の合成順が既定の `"XYZ"` のままだったので、
 * 席の向きを回してから寝かせるぶんが世界の軸にかかり、
 *
 * - 対面（席2）の河は牌面が真下を向いて伏せ牌になった
 * - 左右（席1/3）の河と山は牌面が真横を向き、無地の板の列に見えた
 *
 * 型検査も 220 件の試験も通ったまま、**相手の捨て牌が一枚も読めなかった。**
 * 画素を見るまで誰も気付かなかったので、ここで数値として固定する。
 */

function seat(river: Tile[]): SeatView {
  return {
    handSize: 13,
    river: river.map((tile) => ({ tile, riichi: false })),
    melds: [],
    riichi: false,
    declaring: false,
  };
}

function stateWithRivers(): GameState {
  return {
    you: 0 as Seat,
    players: [],
    seats: [
      seat([0 as Tile, 1 as Tile]),
      seat([2 as Tile, 3 as Tile]),
      seat([4 as Tile, 5 as Tile]),
      seat([6 as Tile, 7 as Tile]),
    ],
    hand: [],
    drawn: null,
    round: { wind: "East", number: 1 },
    dealer: 0 as Seat,
    honba: 0,
    sticks: 0,
    scores: [25000, 25000, 25000, 25000],
    doraIndicators: [],
    wallRemaining: 70,
    pending: null,
    lastSeq: null,
    phase: "playing",
    lastDiscard: null,
    recentDiscard: null,
    turn: null,
    result: null,
    notice: null,
    finalScores: null,
    placements: null,
    rules: null,
  };
}

/** 牌面（+z）が回転後にどちらを向くか。 */
function faceNormal(rotationX: number, rotationY: number): Vector3 {
  return new Vector3(0, 0, 1).applyEuler(
    new Euler(rotationX, rotationY, 0, ROTATION_ORDER),
  );
}

describe("卓に置いた牌の向き", () => {
  it("四席すべての河が牌面を上へ向ける", () => {
    const placements = placementsFor(stateWithRivers());
    const rivers = placements.filter((p) => p.kind === "river");
    expect(rivers).toHaveLength(8);
    for (const placement of rivers) {
      const normal = faceNormal(placement.rotationX, placement.rotationY);
      expect(
        normal.y,
        `席${placement.seat} の河が上を向いていない: ${normal.toArray().join(",")}`,
      ).toBeCloseTo(1, 5);
    }
  });

  it("山はどの席でも牌面を下へ向ける", () => {
    // 山は伏せて積む。横を向いていると、無地の板が立っているように見える。
    const walls = placementsFor(stateWithRivers()).filter(
      (p) => p.kind === "wall",
    );
    expect(walls.length).toBeGreaterThan(0);
    for (const placement of walls) {
      const normal = faceNormal(placement.rotationX, placement.rotationY);
      expect(Math.abs(normal.y), `山が横を向いている`).toBeCloseTo(1, 5);
    }
  });

  it("既定の合成順ならこの不変条件は破れる", () => {
    // **なぜ順序を指定しているのかをここに残す。**`ROTATION_ORDER` を
    // 消して既定へ戻すと、対面の河は裏返り、左右の河は横を向く。
    const opposite = new Vector3(0, 0, 1).applyEuler(
      new Euler(-Math.PI / 2, Math.PI, 0, "XYZ"),
    );
    expect(opposite.y).toBeCloseTo(-1, 5);
    const rightward = new Vector3(0, 0, 1).applyEuler(
      new Euler(-Math.PI / 2, Math.PI / 2, 0, "XYZ"),
    );
    expect(Math.abs(rightward.y)).toBeCloseTo(0, 5);
  });
});

describe("直前の捨て牌を光らせる", () => {
  it("印の指す1枚だけが光る", () => {
    const state = stateWithRivers();
    state.recentDiscard = { seat: 2 as Seat, index: 1 };
    const lit = placementsFor(state).filter((p) => p.emphasis);
    expect(lit).toHaveLength(1);
    expect(lit[0]?.key).toBe("river-2-1");
  });

  it("印が無ければどれも光らない", () => {
    const state = stateWithRivers();
    state.recentDiscard = null;
    expect(placementsFor(state).some((p) => p.emphasis)).toBe(false);
  });

  it("手牌も山も副露も光らない", () => {
    // **席と番号だけで照合すると、同じ番号の手牌まで光る。**
    const state = stateWithRivers();
    state.recentDiscard = { seat: 0 as Seat, index: 0 };
    const lit = placementsFor(state).filter((p) => p.emphasis);
    expect(lit.every((p) => p.kind === "river")).toBe(true);
  });
});
