/**
 * 演出の進行を測る時計。
 *
 * テストから時刻を制御できないと演出は検証できないため、実時間を直接
 * 参照せず必ずこの抽象を通す。
 */
export interface Clock {
  now(): number;
}

/** テスト用。時刻を手で進める。 */
export class ManualClock implements Clock {
  #current = 0;

  now(): number {
    return this.#current;
  }

  advance(ms: number): void {
    if (ms < 0) {
      throw new Error(`時計は戻せない（advance(${ms})）`);
    }
    this.#current += ms;
  }

  set(ms: number): void {
    if (ms < this.#current) {
      throw new Error(`時計は戻せない（${this.#current} → ${ms}）`);
    }
    this.#current = ms;
  }
}

export const systemClock: Clock = {
  now: () => performance.now(),
};
