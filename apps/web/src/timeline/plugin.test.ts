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

  /** キャラを一切ロードしなくてもゲームは成立しなければならない。 */
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

  /** 後から差した演出も seek 可能なタイムラインを返す。 */
  it("plugins produce seekable timelines", () => {
    const registry = new EffectRegistry();
    registry.register(plugin("base", () => true));
    const found = registry.pluginsFor(discard, ctx);
    const timeline = found[0]!.play(discard, ctx);
    expect(timeline.durationMs).toBe(100);
    expect(timeline.seek(50)).toBe("base");
    expect(timeline.seek(0)).toBe("base");
  });
});
