import type { ClientEvent } from "../protocol/ClientEvent";
import type { Timeline } from "./timeline";

/** 演出が盤面を参照するための最小限の窓口。 */
export interface EffectContext {
  /** 視点となる席から見た相対位置。描画層が実装する。 */
  seatOf(viewer: number): number;
}

export interface EffectPlugin {
  readonly id: string;
  match(event: ClientEvent, ctx: EffectContext): boolean;
  play(event: ClientEvent, ctx: EffectContext): Timeline<unknown>;
}

/**
 * 演出プラグインの登録簿。
 *
 * キャラ演出は別バンドルとして後から register する。**キャラを一切
 * ロードしなくても基本演出だけでゲームが成立する**ことが要件である
 * （仕様 7.3）。素材が揃うまで開発が止まらず、キャラを切った軽量モードも
 * 自然に手に入る。
 */
export class EffectRegistry {
  readonly #plugins: EffectPlugin[] = [];

  register(plugin: EffectPlugin): void {
    if (this.has(plugin.id)) {
      throw new Error(`演出プラグイン '${plugin.id}' が二重に登録された`);
    }
    this.#plugins.push(plugin);
  }

  has(id: string): boolean {
    return this.#plugins.some((p) => p.id === id);
  }

  /** そのイベントに反応するプラグインを登録順に返す。 */
  pluginsFor(event: ClientEvent, ctx: EffectContext): EffectPlugin[] {
    return this.#plugins.filter((p) => p.match(event, ctx));
  }
}
