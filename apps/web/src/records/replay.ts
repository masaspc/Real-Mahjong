/**
 * 牌譜の再生。
 *
 * **ソケットの代わりに保存した列を流すだけである。**盤面も演出も対局中と
 * 同じ `Presentation` を通るので、再生のために描画をもう一組作らずに済む。
 *
 * 速さは時計を掛け算して変える。`EffectPlayer` の倍率は「遅れをどう取り
 * 戻すか」の仕組みで、利用者の意図ではない。**そこを触ると、早送り中に
 * 溜まりが減って勝手に等速へ戻る。**
 */

import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import type { Seat } from "../protocol/Seat";
import { Presentation } from "../game/presentation";
import type { Clock } from "../timeline/clock";

/**
 * 元の時計を掛け算した時計。
 *
 * **`now()` は単調に増える必要がある。**倍率を変えた瞬間に時刻が飛ぶと、
 * 再生中の演出が巻き戻ったり終わったことになったりする。切り替えた
 * 時点を継ぎ目にして、そこから先だけ速さを変える。
 */
export class ScaledClock implements Clock {
  readonly #source: Clock;
  #rate: number;
  /** 継ぎ目の、元の時計での時刻。 */
  #sourceMark: number;
  /** 継ぎ目の、こちらの時計での時刻。 */
  #ownMark: number;

  constructor(source: Clock, rate = 1) {
    this.#source = source;
    this.#rate = rate;
    this.#sourceMark = source.now();
    this.#ownMark = 0;
  }

  get rate(): number {
    return this.#rate;
  }

  setRate(rate: number): void {
    if (rate <= 0) {
      throw new Error(`速さは正の数（${rate}）`);
    }
    this.#ownMark = this.now();
    this.#sourceMark = this.#source.now();
    this.#rate = rate;
  }

  now(): number {
    return this.#ownMark + (this.#source.now() - this.#sourceMark) * this.#rate;
  }
}

/** 溜めておく演出の量。これを超えたら次を流し込まない。 */
const FEED_BELOW_MS = 250;

export class Replay {
  readonly #events: ClientEventEnvelope[];
  readonly #clock: ScaledClock;
  #presentation: Presentation;
  #fed = 0;

  constructor(events: ClientEventEnvelope[], you: Seat, source: Clock) {
    this.#events = events;
    this.#clock = new ScaledClock(source);
    this.#presentation = new Presentation(you, this.#clock);
  }

  get presentation(): Presentation {
    return this.#presentation;
  }

  get rate(): number {
    return this.#clock.rate;
  }

  setRate(rate: number): void {
    this.#clock.setRate(rate);
  }

  get total(): number {
    return this.#events.length;
  }

  get fed(): number {
    return this.#fed;
  }

  get done(): boolean {
    return this.#fed >= this.#events.length && this.#presentation.pendingMs === 0;
  }

  /**
   * 局の頭の位置。目次に出す。
   *
   * `round_start` は局の頭そのものなので、これを並べれば「東2局へ飛ぶ」が
   * 作れる。
   */
  get roundStarts(): number[] {
    const marks: number[] = [];
    this.#events.forEach((envelope, index) => {
      if (envelope.event.type === "round_start") {
        marks.push(index);
      }
    });
    return marks;
  }

  /**
   * 流し込みを進める。毎フレーム呼ぶ。
   *
   * **溜まりが少ないときだけ次を渡す。**一度に全部渡すと `EffectPlayer` の
   * 追いつき方針が働いて、演出を丸ごと飛ばして終端へ跳ぶ。
   */
  update(): void {
    while (
      this.#fed < this.#events.length &&
      this.#presentation.pendingMs < FEED_BELOW_MS
    ) {
      const envelope = this.#events[this.#fed];
      if (envelope === undefined) break;
      this.#presentation.receive(envelope);
      this.#fed += 1;
    }
    this.#presentation.update();
  }

  /** いま見えている演出を飛ばして、届いているぶんを見せ切る。 */
  skip(): void {
    this.#presentation.skip();
  }

  /**
   * 指定の位置まで一気に進める。
   *
   * **演出を積まずに盤面だけを作る。**局の頭へ飛ぶのに演出を早回しすると、
   * 半荘の後半では何十秒も待たされる。
   */
  seek(index: number, you: Seat): void {
    const target = Math.max(0, Math.min(index, this.#events.length));
    this.#presentation = new Presentation(you, this.#clock);
    for (let i = 0; i < target; i += 1) {
      const envelope = this.#events[i];
      if (envelope === undefined) break;
      this.#presentation.receive(envelope);
    }
    this.#presentation.skip();
    this.#fed = target;
  }
}
