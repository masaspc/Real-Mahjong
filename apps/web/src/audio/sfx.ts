import type { SoundName } from "./catalog";

/**
 * 効果音。**音源ファイルを持たない。**
 *
 * 牌の音も鐘も、その場で合成する。素材を持ち込まないので許諾の吟味が要らず、
 * 音の長さや高さをその場で直せる。牌図で二度誤った許諾の確認を、音でもう一度
 * 繰り返す理由は無い。
 *
 * **`AudioContext` は最初の操作まで作らない。**ブラウザは利用者の操作なしに
 * 音を出すことを許さない。読み込んだ瞬間に作ると `suspended` のまま生まれ、
 * 「再開したのに鳴らない」状態を持ち歩くことになる。
 */

const STORAGE_KEY = "real-mahjong.muted";

export class Sfx {
  #context: BaseAudioContext | null = null;
  #master: GainNode | null = null;
  #muted: boolean;

  /**
   * @param context 差し替え用。**波形を確かめるために `OfflineAudioContext`
   * を渡せるようにしてある。**「例外を出さずに走った」と「本当に音が出て
   * いる」は別のことで、絵のときに何度も痛い目を見た。`sound-check.html`
   * がこれを使って各音の振幅を測る。
   */
  constructor(context?: BaseAudioContext) {
    this.#muted = globalThis.localStorage?.getItem(STORAGE_KEY) === "1";
    if (context !== undefined) {
      this.#context = context;
      this.#master = context.createGain();
      this.#master.gain.value = 1;
      this.#master.connect(context.destination);
    }
  }

  get muted(): boolean {
    return this.#muted;
  }

  set muted(value: boolean) {
    this.#muted = value;
    globalThis.localStorage?.setItem(STORAGE_KEY, value ? "1" : "0");
    if (this.#master !== null && this.#context !== null) {
      this.#master.gain.setValueAtTime(value ? 0 : 1, this.#context.currentTime);
    }
  }

  /**
   * 利用者の操作のたびに呼ぶ。
   *
   * **鳴らす直前に作ってはいけない。**`AudioContext` は操作の文脈でしか
   * 開始できないので、盤面の更新から作ると `suspended` のままになる。
   */
  unlock(): void {
    if (this.#context === null) {
      const Ctor =
        globalThis.AudioContext ??
        (globalThis as unknown as { webkitAudioContext?: typeof AudioContext })
          .webkitAudioContext;
      if (Ctor === undefined) {
        return;
      }
      this.#context = new Ctor();
      this.#master = this.#context.createGain();
      this.#master.gain.value = this.#muted ? 0 : 1;
      this.#master.connect(this.#context.destination);
    }
    const live = this.#context as AudioContext;
    if (live.state === "suspended") {
      void live.resume();
    }
  }

  play(name: SoundName, delayMs = 0): void {
    const context = this.#context;
    const master = this.#master;
    if (context === null || master === null || this.#muted) {
      return;
    }
    const at = context.currentTime + delayMs / 1000;
    switch (name) {
      case "clack":
        this.#clack(at, 0.5, 2_400);
        break;
      case "draw":
        // ツモは4人ぶん鳴るので、打牌より小さく短く。
        this.#clack(at, 0.16, 3_200);
        break;
      case "call":
        // 2度打ちつける。宣言と、卓に倒す音。
        this.#clack(at, 0.55, 2_000);
        this.#clack(at + 0.09, 0.45, 2_600);
        break;
      case "riichi":
        this.#bell(at, [880, 1_320], 0.9, 0.28);
        break;
      case "dora":
        this.#bell(at, [1_320], 0.45, 0.16);
        break;
      case "agari":
        // 上がっていく4音。和了は嬉しい出来事なので、他より長く鳴らす。
        [523.25, 659.25, 783.99, 1_046.5].forEach((hz, index) => {
          this.#bell(at + index * 0.11, [hz], 0.7, 0.24);
        });
        break;
      case "ryuukyoku":
        // 下がる2音。誰も上がらなかった、という響きにする。
        this.#bell(at, [392], 0.5, 0.18);
        this.#bell(at + 0.16, [329.63], 0.7, 0.18);
        break;
    }
  }

  /**
   * 牌の音。
   *
   * 短い雑音を帯域で削り、指数で落とす。象牙どうしが当たる音は倍音が
   * 詰まった一瞬の破裂なので、正弦を重ねるより雑音を削るほうが近い。
   */
  #clack(at: number, gain: number, hz: number): void {
    const context = this.#context;
    const master = this.#master;
    if (context === null || master === null) {
      return;
    }
    const length = Math.floor(context.sampleRate * 0.09);
    const buffer = context.createBuffer(1, length, context.sampleRate);
    const data = buffer.getChannelData(0);
    for (let i = 0; i < length; i += 1) {
      // 末尾へ向けて急に落とす。尾を引くと拍手のように聞こえる。
      const decay = Math.exp(-i / (length * 0.16));
      data[i] = (Math.random() * 2 - 1) * decay;
    }
    const source = context.createBufferSource();
    source.buffer = buffer;

    const band = context.createBiquadFilter();
    band.type = "bandpass";
    band.frequency.value = hz;
    band.Q.value = 1.1;

    // 木と象牙の胴。これが無いと乾いた砂の音になる。
    const body = context.createOscillator();
    body.type = "triangle";
    body.frequency.setValueAtTime(320, at);
    body.frequency.exponentialRampToValueAtTime(140, at + 0.06);
    const bodyGain = context.createGain();
    bodyGain.gain.setValueAtTime(gain * 0.5, at);
    bodyGain.gain.exponentialRampToValueAtTime(0.0001, at + 0.07);

    const level = context.createGain();
    level.gain.setValueAtTime(gain, at);
    level.gain.exponentialRampToValueAtTime(0.0001, at + 0.09);

    source.connect(band).connect(level).connect(master);
    body.connect(bodyGain).connect(master);
    source.start(at);
    body.start(at);
    body.stop(at + 0.08);
  }

  /** 鐘。正弦を重ねて指数で落とす。 */
  #bell(at: number, partials: number[], gain: number, seconds: number): void {
    const context = this.#context;
    const master = this.#master;
    if (context === null || master === null) {
      return;
    }
    for (const [index, hz] of partials.entries()) {
      const osc = context.createOscillator();
      osc.type = "sine";
      osc.frequency.value = hz;
      const level = context.createGain();
      const share = gain / (index + 1.6);
      level.gain.setValueAtTime(0.0001, at);
      level.gain.exponentialRampToValueAtTime(share, at + 0.008);
      level.gain.exponentialRampToValueAtTime(0.0001, at + seconds);
      osc.connect(level).connect(master);
      osc.start(at);
      osc.stop(at + seconds + 0.02);
    }
  }
}
