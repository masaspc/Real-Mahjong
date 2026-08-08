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
