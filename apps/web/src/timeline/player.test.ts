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

  /** スキップは待ち時間を捨てる。締切は動かない。 */
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
    expect(player.presented).toHaveLength(60);
  });

  /** 速めた分だけ実時間あたりの消化が増える。 */
  it("a higher rate consumes more events in the same wall time", () => {
    const slow = new ManualClock();
    const normal = new EffectPlayer(slow, defaultCatchUp);
    for (let i = 0; i < 3; i += 1) {
      normal.push(discard(i));
    }
    slow.advance(350);
    normal.update();
    const atNormalRate = normal.presented.length;

    const fast = new ManualClock();
    // しきい値を下げて必ず速まる状態にする
    const hurried = new EffectPlayer(fast, {
      speedUpAfterMs: 100,
      skipAfterMs: 100_000,
    });
    for (let i = 0; i < 3; i += 1) {
      hurried.push(discard(i));
    }
    fast.advance(350);
    hurried.update();

    expect(hurried.playbackRate).toBeGreaterThan(1);
    expect(hurried.presented.length).toBeGreaterThan(atNormalRate);
  });

  /** 表示済みのイベントは順序を保ち、消えない。 */
  it("keeps presented events in arrival order", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock);
    player.push(discard(0));
    player.push(draw(1));
    player.push(discard(2));
    player.skip();
    player.update();

    const seats = player.presented.map((e) =>
      "seat" in e ? (e as { seat: number }).seat : -1,
    );
    expect(seats).toEqual([0, 1, 2]);
  });

  /** 何も積まれていない update は無害。 */
  it("update on an empty queue does nothing", () => {
    const clock = new ManualClock();
    const player = new EffectPlayer(clock);
    player.update();
    player.update();
    expect(player.presented).toHaveLength(0);
    expect(player.pendingMs).toBe(0);
  });
});
