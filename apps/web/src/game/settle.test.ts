import { describe, expect, it } from "vitest";

import { Presentation } from "./presentation";
import { ManualClock } from "../timeline/clock";
import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import type { Seat } from "../protocol/Seat";
import type { Tile } from "../protocol/Tile";

function discard(seq: number, seat: Seat, tile: Tile): ClientEventEnvelope {
  return { seq, event: { type: "discard", seat, tile, manner: "tedashi" } };
}

/**
 * 演出を途中でやめる経路は3つある。
 *
 * `?effects=off`・手で叩いた早送り・6秒超の自動切り捨て。いずれも
 * `active` が `null` になっていなければ、**代理の牌が空中で止まる。**
 *
 * 再接続はここに含めない。Wave 3f で、取り直した backlog は通常の加速と
 * 6秒判定に委ねる設計にした（`main.ts` の `onStatus` にその旨がある）。
 */
describe("演出を畳む経路", () => {
  it("手で早送りすると再生中のものが無くなる", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));
    p.update();
    expect(p.active).not.toBeNull();

    p.skip();
    expect(p.active).toBeNull();
    expect(p.state.seats[1].river).toHaveLength(1);
  });

  it("effects=off の経路でも、受け取った端から畳まれる", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    // `?effects=off` は受信のたびに skip を呼ぶ。**別経路を作らない。**
    p.receive(discard(1, 1, 5));
    p.skip();
    p.receive(discard(2, 2, 6));
    p.skip();

    expect(p.active).toBeNull();
    expect(p.state.seats[1].river).toHaveLength(1);
    expect(p.state.seats[2].river).toHaveLength(1);
  });

  it("6秒を超えて溜まると自動で切り捨て、再生中のものが無くなる", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    // 350ms x 20 = 7,000ms。既定の skipAfterMs は 6,000。
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
