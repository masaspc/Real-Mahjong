import type { ClientEvent } from "./protocol/ClientEvent";
import type { Command } from "./protocol/Command";
import type { Tile } from "./protocol/Tile";

/**
 * 生成された型が期待どおりの形であることを、コンパイル時に確かめるための足場。
 * protocol 側の変更で形が崩れたら typecheck が落ちる。
 */
export function describeEvent(event: ClientEvent): string {
  return event.type;
}

export function discardCommand(tile: Tile, riichi: boolean): Command {
  return { type: "discard", tile, riichi };
}

export function respondPass(windowId: number): Command {
  return {
    type: "call_response",
    window_id: windowId,
    response: { type: "pass" },
  };
}

/**
 * 自席のツモ牌のみが Some になる。他家のツモは null で届く。
 * この形が崩れたら、視界フィルタの前提が壊れているということ。
 */
export function drawnTile(event: ClientEvent): Tile | null {
  return event.type === "draw" ? event.tile : null;
}

document.querySelector("#app")!.textContent = "Real Mahjong";
