import type { ClientEvent } from "../protocol/ClientEvent";
import { effectDurationMs, effectOf } from "./catalog";
import type { Clock } from "./clock";

/**
 * 演出が論理状態からどれだけ遅れたら、どう取り戻すか。
 *
 * 少しの遅れは再生を速めて吸収し、大きく遅れたら演出を捨てて状態だけ合わせる。
 */
export type CatchUpPolicy = {
  /** この時間ぶん溜まったら再生を速める */
  speedUpAfterMs: number;
  /** この時間ぶん溜まったら演出を飛ばす */
  skipAfterMs: number;
};

export const defaultCatchUp: CatchUpPolicy = {
  speedUpAfterMs: 1_500,
  skipAfterMs: 6_000,
};

type Queued = {
  event: ClientEvent;
  durationMs: number;
};

/**
 * 受信イベントを演出として再生する。
 *
 * 論理状態は受信した時点で確定しており、`presented` はそこから遅れて
 * 追いつく表示側の状態である（仕様 6.3）。この分離が無いと再接続と
 * タブ復帰で破綻する。
 */
export class EffectPlayer {
  readonly #clock: Clock;
  readonly #policy: CatchUpPolicy;
  #queue: Queued[] = [];
  #shown: ClientEvent[] = [];
  /** 現在再生中の演出が始まった時刻 */
  #startedAt: number | null = null;
  #rate = 1;

  constructor(clock: Clock, policy: CatchUpPolicy = defaultCatchUp) {
    this.#clock = clock;
    this.#policy = policy;
  }

  get presented(): readonly ClientEvent[] {
    return this.#shown;
  }

  /** まだ再生し終えていない演出の合計時間。 */
  get pendingMs(): number {
    return this.#queue.reduce((sum, item) => sum + item.durationMs, 0);
  }

  get playbackRate(): number {
    return this.#rate;
  }

  push(event: ClientEvent): void {
    const kind = effectOf(event);
    const wasIdle = this.#queue.length === 0;
    this.#queue.push({
      event,
      durationMs: kind === null ? 0 : effectDurationMs(kind),
    });
    // 演出の開始は「キューの先頭になった時点」である。update() が
    // 呼ばれるまで待つと、呼び出し間隔しだいで待ち時間が始まらない。
    if (wasIdle && this.#startedAt === null) {
      this.#startedAt = this.#clock.now();
    }
  }

  /**
   * 待ち時間を捨てて、いま溜まっている分をすべて表示する。
   *
   * ユーザーが画面を叩いたときの早送りに使う。**締切は動かない**ので、
   * 速く打てるようになるだけで有利にも不利にもならない（仕様 6.3）。
   */
  skip(): void {
    for (const item of this.#queue) {
      this.#shown.push(item.event);
    }
    this.#queue = [];
    this.#startedAt = null;
  }

  /** 再接続やリロードからの復帰。演出を捨てて最新状態へ飛ぶ。 */
  jumpToLatest(): void {
    this.skip();
    this.#rate = 1;
  }

  update(): void {
    this.#rate = this.#rateFor(this.pendingMs);

    if (this.pendingMs >= this.#policy.skipAfterMs) {
      // 大きく遅れている。演出を飛ばして状態だけ合わせる。
      this.skip();
      return;
    }

    for (;;) {
      const head = this.#queue[0];
      if (head === undefined) {
        this.#startedAt = null;
        return;
      }

      // 進行を止めないイベントは待たずに通す。
      if (head.durationMs === 0) {
        this.#queue.shift();
        this.#shown.push(head.event);
        continue;
      }

      // push で必ず設定されるが、型の上では null を排除しておく。
      this.#startedAt ??= this.#clock.now();

      const elapsed = (this.#clock.now() - this.#startedAt) * this.#rate;
      if (elapsed < head.durationMs) {
        return;
      }

      this.#queue.shift();
      this.#shown.push(head.event);
      // 余った時間を次の演出へ持ち越す。
      this.#startedAt += head.durationMs / this.#rate;
    }
  }

  #rateFor(pending: number): number {
    if (pending < this.#policy.speedUpAfterMs) {
      return 1;
    }
    // 遅れに比例して速める。上限は4倍。
    const excess = pending / this.#policy.speedUpAfterMs;
    return Math.min(4, excess);
  }
}
