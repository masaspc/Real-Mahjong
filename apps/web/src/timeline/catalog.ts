import type { ClientEvent } from "../protocol/ClientEvent";

/**
 * 局の進行を止める演出の種類。
 *
 * **値は Rust の protocol::effect と一致していなければならない。**
 * サーバはこの表で思考時間の締切を計算し、クライアントは同じ表で再生する。
 * ずれると、演出を見ている間に持ち時間が削られるという理不尽が生まれる。
 */
export type EffectKind =
  | "draw"
  | "discard"
  | "pon"
  | "chi"
  | "kan"
  | "riichi_declare"
  | "dora_reveal";

const DURATIONS: Record<EffectKind, number> = {
  draw: 250,
  discard: 350,
  pon: 700,
  chi: 700,
  kan: 1100,
  riichi_declare: 1800,
  dora_reveal: 800,
};

export function effectDurationMs(kind: EffectKind): number {
  return DURATIONS[kind];
}

/** そのイベントが進行を止める演出を伴うか。伴わなければ null。 */
export function effectOf(event: ClientEvent): EffectKind | null {
  switch (event.type) {
    case "draw":
      return "draw";
    case "discard":
      return "discard";
    case "dora_reveal":
      return "dora_reveal";
    case "riichi":
      // 成立側は点棒の移動のみで、局の進行を止める演出を持たない。
      return event.step === "declare" ? "riichi_declare" : null;
    case "call":
    case "kan_declared":
      switch (event.kind) {
        case "chi":
          return "chi";
        case "pon":
          return "pon";
        default:
          return "kan";
      }
    default:
      return null;
  }
}

/** 直前に届いた一連のイベントの演出時間の合計。 */
export function leadInMs(events: ClientEvent[]): number {
  let total = 0;
  for (const event of events) {
    const kind = effectOf(event);
    if (kind !== null) {
      total += effectDurationMs(kind);
    }
  }
  return total;
}
