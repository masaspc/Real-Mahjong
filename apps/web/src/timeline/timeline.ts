/**
 * 宣言的な演出タイムライン。
 *
 * 時刻 t を与えれば状態が決まる形にしてある。async/await で書くと
 * スキップ・早送り・再接続時の追いつきが実装できなくなるため、
 * この形は設計上の必須要件である（仕様 7.3）。
 */
export interface Timeline<S> {
  readonly durationMs: number;
  seek(t: number): S;
}

export type Easing = (t: number) => number;

export const linear: Easing = (t) => t;

export const easeOutCubic: Easing = (t) => 1 - (1 - t) ** 3;

function clamp01(value: number): number {
  if (value < 0) return 0;
  if (value > 1) return 1;
  return value;
}

export function constant<S>(value: S, durationMs: number): Timeline<S> {
  return {
    durationMs,
    seek: () => value,
  };
}

export function delay(durationMs: number): Timeline<null> {
  return constant(null, durationMs);
}

export function tween(
  from: number,
  to: number,
  durationMs: number,
  ease: Easing = linear,
): Timeline<number> {
  return {
    durationMs,
    seek(t: number): number {
      if (durationMs <= 0) return to;
      const progress = ease(clamp01(t / durationMs));
      return from + (to - from) * progress;
    },
  };
}

export function sequence<S>(parts: Timeline<S>[]): Timeline<S> {
  const total = parts.reduce((sum, part) => sum + part.durationMs, 0);
  return {
    durationMs: total,
    seek(t: number): S {
      if (parts.length === 0) {
        throw new Error("空の sequence は seek できない");
      }
      let remaining = t;
      for (const part of parts) {
        if (remaining <= part.durationMs) {
          return part.seek(remaining);
        }
        remaining -= part.durationMs;
      }
      const last = parts[parts.length - 1] as Timeline<S>;
      return last.seek(last.durationMs);
    },
  };
}

export function parallel<S extends object>(parts: {
  [K in keyof S]: Timeline<S[K]>;
}): Timeline<S> {
  const entries = Object.entries(parts) as [keyof S, Timeline<S[keyof S]>][];
  const total = entries.reduce(
    (max, [, part]) => Math.max(max, part.durationMs),
    0,
  );
  return {
    durationMs: total,
    seek(t: number): S {
      const out = {} as S;
      for (const [key, part] of entries) {
        out[key] = part.seek(t);
      }
      return out;
    },
  };
}
