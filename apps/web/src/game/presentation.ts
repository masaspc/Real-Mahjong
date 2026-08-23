import type { ClientEvent } from "../protocol/ClientEvent";
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
  /** `active` の作り直しを避けるための控え。畳んだ件数が変われば作り直す。 */
  #activeAt: number | null = null;
  #activeNext: GameState | null = null;
  /**
   * 出し始めたイベントの件数。**畳んだ件数ではない。**
   *
   * 音は演出の頭で鳴る。畳み終わり（演出の終わり）を待つと、鳴きの声が
   * 牌を倒し終えてから聞こえる。
   */
  #started = 0;
  #onStart: ((event: ClientEvent, skipped: boolean) => void) | null = null;

  constructor(you: Seat, clock: Clock, policy: CatchUpPolicy = defaultCatchUp) {
    this.#player = new EffectPlayer(clock, policy);
    this.#clock = clock;
    this.#state = emptyState(you);
  }

  get state(): GameState {
    return this.#state;
  }

  /**
   * イベントが出始めたときに1度だけ呼ぶ相手を決める。
   *
   * 第2引数は「まとめて出た」印である。再接続やタブ復帰では溜まりを一気に
   * 捨てるので、**そのまま鳴らすと数十発の音が同時に出る。**呼ばれた側が
   * 間引けるように伝える。
   */
  onStart(listener: (event: ClientEvent, skipped: boolean) => void): void {
    this.#onStart = listener;
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

  /**
   * いま再生中のイベントと、それを適用した後の盤面。
   *
   * **`state` が動きの起点、`nextState` が着地点である。**再生中は
   * `state` にまだ入っていないので、両方を配置へ直せば差が動きになる。
   */
  get active(): {
    event: ClientEvent;
    elapsedMs: number;
    durationMs: number;
    nextState: GameState;
  } | null {
    const current = this.#player.current;
    if (current === null) {
      return null;
    }
    // 再生中のものは「次に畳むもの」である。
    const item = this.#envelopes[this.#folded];
    if (item === undefined) {
      return null;
    }
    // **毎フレーム畳み直さない。**apply は状態を深く複製するので、
    // 同じイベントのあいだは作った結果を使い回す。
    if (this.#activeAt !== this.#folded || this.#activeNext === null) {
      this.#activeAt = this.#folded;
      this.#activeNext = apply(this.#state, item.envelope, item.receivedAt);
    }
    return {
      event: current.event,
      elapsedMs: current.elapsedMs,
      durationMs: current.durationMs,
      nextState: this.#activeNext,
    };
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
    this.#announce();
  }

  /** 溜まっている演出を捨てて、いますぐ全部見せる。 */
  skip(): void {
    this.#player.skip();
    this.#fold();
    this.#announce();
  }

  /** 明示的な再同期。演出を捨てて最新状態へ飛ぶ。 */
  jumpToLatest(): void {
    this.#player.jumpToLatest();
    this.#fold();
    this.#announce();
  }

  /**
   * 新しく出始めたイベントを知らせる。
   *
   * **出し始めた件数は「畳んだ件数 + 再生中の1件」である。**畳んだ件数だけ
   * 見ると、いま再生している演出が数えられない。
   */
  #announce(): void {
    const started = this.#folded + (this.#player.current === null ? 0 : 1);
    const listener = this.#onStart;
    if (listener === null) {
      this.#started = started;
      return;
    }
    // 1回の更新で2件以上進んだなら、演出を飛ばしている。
    const skipped = started - this.#started > 1;
    for (; this.#started < started; this.#started += 1) {
      const item = this.#envelopes[this.#started];
      if (item === undefined) {
        return;
      }
      listener(item.envelope.event, skipped);
    }
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
