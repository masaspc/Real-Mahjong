import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import type { Seat } from "../protocol/Seat";
import { EffectPlayer, defaultCatchUp } from "../timeline/player";
import type { CatchUpPolicy } from "../timeline/player";
import type { Clock } from "../timeline/clock";
import { apply, emptyState } from "./state";
import type { GameState } from "./state";

/**
 * 受信したイベントを、演出の時間ぶん遅らせて盤面に見せる。
 *
 * **論理状態と表示状態を分ける。**イベントは届いた時点で確定しているが、
 * 見せるのは演出が終わってからである。この分離が無いと、再接続やタブ復帰で
 * 「まだ演出を見ていない」ことを理由に盤面が巻き戻る。
 *
 * 締切には触れない。サーバが `deadline_for` で演出ぶん（lead_in）を既に
 * 足しているので、**遅らせても思考時間は減らない。**
 */
export class Presentation {
  readonly #player: EffectPlayer;
  readonly #clock: Clock;
  #state: GameState;
  /** 受信済みの最大 seq。**再接続はこれを送る。**表示より先を行く。 */
  #receivedSeq: number | null = null;
  /**
   * 受信順の控え。`EffectPlayer` は seq を持たないのでこちらで持つ。
   *
   * **受信した時刻も一緒に控える。**`apply` はこれを `deadlineAt` の基準に
   * 使うので、畳むときの時刻を渡すと締切が演出待ちのぶん後ろへずれる。
   */
  #envelopes: { envelope: ClientEventEnvelope; receivedAt: number }[] = [];
  /** すでに畳んだ件数。二重に畳まないための印。 */
  #folded = 0;

  constructor(you: Seat, clock: Clock, policy: CatchUpPolicy = defaultCatchUp) {
    this.#player = new EffectPlayer(clock, policy);
    this.#clock = clock;
    this.#state = emptyState(you);
  }

  get state(): GameState {
    return this.#state;
  }

  get receivedSeq(): number | null {
    return this.#receivedSeq;
  }

  get pendingMs(): number {
    return this.#player.pendingMs;
  }

  get playbackRate(): number {
    return this.#player.playbackRate;
  }

  receive(envelope: ClientEventEnvelope): void {
    this.#envelopes.push({ envelope, receivedAt: this.#clock.now() });
    this.#receivedSeq = envelope.seq;
    this.#player.push(envelope.event);
  }

  /** 出し終えたぶんを盤面へ畳む。毎フレーム呼ぶ。 */
  update(): void {
    this.#player.update();
    this.#fold();
  }

  /** 溜まっている演出を捨てて、いますぐ全部見せる。 */
  skip(): void {
    this.#player.skip();
    this.#fold();
  }

  /** 明示的な再同期。演出を捨てて最新状態へ飛ぶ。 */
  jumpToLatest(): void {
    this.#player.jumpToLatest();
    this.#fold();
  }

  /**
   * 再生器が出し終えた件数まで畳む。
   *
   * **件数で進める。**`presented` は push した順にそのまま並ぶので、
   * 控えておいた封筒の同じ位置が対応する。イベントの中身で突き合わせると
   * 同型のイベント（同じ席の同じ牌の打牌）で取り違える。
   */
  #fold(): void {
    const done = this.#player.presented.length;
    for (; this.#folded < done; this.#folded += 1) {
      const item = this.#envelopes[this.#folded];
      if (item === undefined) {
        return;
      }
      // **受信時刻を渡す。**`performance.now()` を直に読んではならない。
      // 実時間を握るのは `Clock` だけであり、締切の基準は表示ではなく受信である。
      this.#state = apply(this.#state, item.envelope, item.receivedAt);
    }
  }
}
