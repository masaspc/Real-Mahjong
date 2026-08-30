import { describe, expect, it } from "vitest";

import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import { ManualClock } from "../timeline/clock";
import { Replay, ScaledClock } from "./replay";

import seed1 from "../game/__fixtures__/match-seed1.jsonl?raw";

const EVENTS: ClientEventEnvelope[] = seed1
  .trim()
  .split("\n")
  .map((line) => JSON.parse(line) as ClientEventEnvelope);

describe("掛け算した時計", () => {
  it("等速なら元の時計と同じだけ進む", () => {
    const source = new ManualClock();
    const clock = new ScaledClock(source);
    source.advance(100);
    expect(clock.now()).toBe(100);
  });

  it("倍率のぶん速く進む", () => {
    const source = new ManualClock();
    const clock = new ScaledClock(source, 4);
    source.advance(100);
    expect(clock.now()).toBe(400);
  });

  /**
   * **切り替えで時刻が飛ぶと演出が壊れる。**巻き戻れば再生中のものが
   * 戻り、跳べば終わったことになる。継ぎ目から先だけ速さを変える。
   */
  it("速さを変えても時刻は飛ばない", () => {
    const source = new ManualClock();
    const clock = new ScaledClock(source);
    source.advance(100);
    expect(clock.now()).toBe(100);

    clock.setRate(4);
    expect(clock.now()).toBe(100);
    source.advance(10);
    expect(clock.now()).toBe(140);

    clock.setRate(1);
    expect(clock.now()).toBe(140);
    source.advance(10);
    expect(clock.now()).toBe(150);
  });

  it("止まる速さは受け付けない", () => {
    const clock = new ScaledClock(new ManualClock());
    expect(() => clock.setRate(0)).toThrow();
    expect(() => clock.setRate(-1)).toThrow();
  });
});

describe("牌譜の再生", () => {
  it("局の頭が目次になる", () => {
    const replay = new Replay(EVENTS, 0, new ManualClock());
    const marks = replay.roundStarts;
    expect(marks.length).toBeGreaterThan(4);
    for (const index of marks) {
      expect(EVENTS[index]?.event.type).toBe("round_start");
    }
    // 半荘なので、東1局は先頭の方にある。
    expect(marks[0]).toBeLessThan(5);
  });

  /**
   * **一度に全部渡してはいけない。**`EffectPlayer` の追いつき方針が働き、
   * 演出を丸ごと飛ばして終端へ跳ぶ。
   */
  it("溜まりが少ないときだけ次を流す", () => {
    const clock = new ManualClock();
    const replay = new Replay(EVENTS, 0, clock);
    replay.update();
    expect(replay.fed).toBeGreaterThan(0);
    expect(replay.fed).toBeLessThan(EVENTS.length);
  });

  it("時間を進めれば先へ進む", () => {
    const clock = new ManualClock();
    const replay = new Replay(EVENTS, 0, clock);
    replay.update();
    const first = replay.fed;
    for (let i = 0; i < 50; i += 1) {
      clock.advance(200);
      replay.update();
    }
    expect(replay.fed).toBeGreaterThan(first);
  });

  it("速さを上げると同じ実時間で先まで進む", () => {
    const run = (rate: number): number => {
      const clock = new ManualClock();
      const replay = new Replay(EVENTS, 0, clock);
      replay.setRate(rate);
      for (let i = 0; i < 40; i += 1) {
        clock.advance(100);
        replay.update();
      }
      return replay.fed;
    };
    expect(run(4)).toBeGreaterThan(run(1));
  });

  it("局の頭へ飛ぶと、そこまでの盤面ができている", () => {
    const clock = new ManualClock();
    const replay = new Replay(EVENTS, 0, clock);
    const marks = replay.roundStarts;
    const third = marks[2];
    if (third === undefined) throw new Error("局が足りない");

    replay.seek(third, 0);
    expect(replay.fed).toBe(third);
    // **飛んだ先では点棒が動いている。**局を跨いだ証拠。
    const scores = replay.presentation.state.scores;
    expect(scores.reduce((a, b) => a + b, 0)).toBeGreaterThan(0);
    expect(replay.presentation.pendingMs).toBe(0);
  });

  it("飛んだ後も続きから流れる", () => {
    const clock = new ManualClock();
    const replay = new Replay(EVENTS, 0, clock);
    const marks = replay.roundStarts;
    const second = marks[1];
    if (second === undefined) throw new Error("局が足りない");

    replay.seek(second, 0);
    replay.update();
    expect(replay.fed).toBeGreaterThan(second);
  });

  it("最後まで流せば終わる", () => {
    const clock = new ManualClock();
    const replay = new Replay(EVENTS, 0, clock);
    replay.seek(EVENTS.length, 0);
    replay.update();
    expect(replay.done).toBe(true);
  });
});
