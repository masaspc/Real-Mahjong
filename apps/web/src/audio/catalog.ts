import type { ClientEvent } from "../protocol/ClientEvent";

/**
 * イベントと音の対応。
 *
 * **ここは音を鳴らさない。**どのイベントでどの音をいつ鳴らすかだけを決める。
 * 合成そのもの（`sfx.ts`）は `AudioContext` を要るので試験にかけられないが、
 * 対応表は純粋な関数にしておけば固定できる。
 */

export type SoundName =
  | "clack"
  | "draw"
  | "call"
  | "riichi"
  | "dora"
  | "agari"
  | "ryuukyoku";

export type Cue = {
  name: SoundName;
  /**
   * 演出が始まってから鳴るまで。
   *
   * **打牌の音は牌が河に着いた時に鳴る。**演出の頭で鳴らすと、まだ手元に
   * ある牌から音がする。打牌の演出は 350ms なので、着地に合わせて遅らせる。
   * 鳴きと立直は宣言そのものなので頭で鳴らす。
   */
  delayMs: number;
};

export function soundOf(event: ClientEvent): Cue | null {
  switch (event.type) {
    case "discard":
      return { name: "clack", delayMs: 300 };
    case "draw":
      return { name: "draw", delayMs: 200 };
    case "call":
      // **鳴きの音は宣言のときに1度だけ。**加槓・暗槓は `kan_declared` が
      // 宣言を持ち、この `call` は帳簿上の記録なので、そちらでは鳴らさない。
      return event.kind === "chi" || event.kind === "pon"
        ? { name: "call", delayMs: 0 }
        : null;
    case "kan_declared":
      return { name: "call", delayMs: 0 };
    case "riichi":
      return event.step === "declare" ? { name: "riichi", delayMs: 0 } : null;
    case "dora_reveal":
      return { name: "dora", delayMs: 0 };
    case "agari":
      return { name: "agari", delayMs: 0 };
    case "ryuukyoku":
      return { name: "ryuukyoku", delayMs: 0 };
    default:
      return null;
  }
}
