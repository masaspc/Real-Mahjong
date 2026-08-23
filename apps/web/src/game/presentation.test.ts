import { describe, expect, it } from "vitest";

import { Presentation } from "./presentation";
import { ManualClock } from "../timeline/clock";
import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import type { Seat } from "../protocol/Seat";
import type { Tile } from "../protocol/Tile";

/**
 * **`as unknown as` で型検査を黙らせてはならない。**
 *
 * 通してしまうと、実在しない欄（`discard` の `riichi`）や無い値
 * （`manner: "tegiri"`。正しくは `tedashi` か `tsumogiri`）に気付けない。
 * ここは素直に型を満たす。`Seat` も `Tile` も素の `number` なので
 * キャストは要らない。
 */

/** 打牌。演出は 350ms。 */
function discard(seq: number, seat: Seat, tile: Tile): ClientEventEnvelope {
  return { seq, event: { type: "discard", seat, tile, manner: "tedashi" } };
}

/** ツモ。演出は 250ms。他家のツモ牌は見えないので `tile` は null。 */
function draw(seq: number, seat: Seat, wallRemaining = 69): ClientEventEnvelope {
  return {
    seq,
    event: {
      type: "draw",
      seat,
      tile: seat === 0 ? 4 : null,
      source: "wall",
      wall_remaining: wallRemaining,
    },
  };
}

/** 演出を持たないイベント。リーチの成立は点棒が動くだけで場は止まらない。 */
function riichiAccepted(seq: number, seat: Seat): ClientEventEnvelope {
  return { seq, event: { type: "riichi", seat, step: "accepted" } };
}

/** 行動要求。締切が受信時刻を基準にしているかを見るために使う。 */
function requestAction(seq: number, deadlineMs: number): ClientEventEnvelope {
  return {
    seq,
    event: { type: "request_action", window_id: 1, options: [], deadline_ms: deadlineMs },
  };
}

describe("Presentation", () => {
  it("演出が終わるまで盤面へ出さない", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));

    p.update();
    expect(p.state.seats[1].river).toHaveLength(0);

    clock.advance(349);
    p.update();
    expect(p.state.seats[1].river).toHaveLength(0);

    clock.advance(1);
    p.update();
    expect(p.state.seats[1].river).toHaveLength(1);
  });

  it("受信した seq は演出を待たずに進む", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(7, 1, 5));

    // **再接続はここを見る。**表示に合わせて遅らせると、演出中に切れた
    // ときに同じイベントを取り直して二重に積む。
    expect(p.receivedSeq).toBe(7);
    expect(p.state.lastSeq).toBeNull();
  });

  it("溜まった演出を早送りできる", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));
    p.receive(discard(2, 2, 6));

    p.skip();
    p.update();
    expect(p.state.seats[1].river).toHaveLength(1);
    expect(p.state.seats[2].river).toHaveLength(1);
    expect(p.pendingMs).toBe(0);
  });

  it("復帰は演出を捨てて最新へ飛ぶ", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(draw(1, 0));
    p.receive(discard(2, 0, 5));

    p.jumpToLatest();
    p.update();
    expect(p.state.lastSeq).toBe(2);
    expect(p.pendingMs).toBe(0);
  });

  it("大きく遅れたら演出を捨てて追いつく", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    // 350ms × 20 = 7,000ms ぶん積む。既定の skipAfterMs は 6,000。
    for (let i = 1; i <= 20; i += 1) {
      p.receive(discard(i, 1, 5));
    }
    p.update();
    expect(p.state.seats[1].river).toHaveLength(20);
  });

  it("同じイベントを二度畳まない", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));
    clock.advance(350);

    p.update();
    p.update();
    p.update();
    expect(p.state.seats[1].river).toHaveLength(1);
  });

  it("締切は受信した時刻を基準にする", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    // 打牌の演出（350ms）を挟んでから行動要求が届く。
    p.receive(discard(1, 1, 5));
    clock.advance(1_000);
    p.receive(requestAction(2, 20_000));

    // 受信は 1,000ms の時点。表示はここから更に遅れる。
    clock.advance(5_000);
    p.update();

    // **表示時刻（6,000）を基準にすると 26,000 になる。**演出を見ている間に
    // 締切が後ろへずれ、実際には切れているのに時間が残って見える。
    expect(p.state.pending?.deadlineAt).toBe(21_000);
  });

  it("演出を持たないイベントは待たせない", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    // リーチの成立は effectOf が null を返すので 0ms。
    p.receive(riichiAccepted(0, 1));

    p.update();
    expect(p.state.lastSeq).toBe(0);
  });

  it("先頭が待っている間は後ろも出さない", () => {
    const clock = new ManualClock();
    const p = new Presentation(0, clock);
    p.receive(discard(1, 1, 5));
    p.receive(draw(2, 2));

    clock.advance(300);
    p.update();
    // **順序が入れ替わってはいけない。**ツモは 250ms だが、前の打牌が
    // 終わっていないので出せない。
    expect(p.state.lastSeq).toBeNull();
  });
  describe("再生中のもの", () => {
    it("再生中のイベントと、適用後の盤面を出す", () => {
      const clock = new ManualClock();
      const p = new Presentation(0, clock);
      p.receive(discard(1, 1, 5));

      clock.advance(100);
      p.update();

      const active = p.active;
      // 表示中の盤面にはまだ無い。
      expect(p.state.seats[1].river).toHaveLength(0);
      // 適用後の盤面には有る。**この差が動きの起点と着地になる。**
      expect(active?.nextState.seats[1].river).toHaveLength(1);
      expect(active?.elapsedMs).toBe(100);
      expect(active?.durationMs).toBe(350);
    });

    it("再生し終えたら再生中のものは無い", () => {
      const clock = new ManualClock();
      const p = new Presentation(0, clock);
      p.receive(discard(1, 1, 5));

      clock.advance(350);
      p.update();

      expect(p.active).toBeNull();
      expect(p.state.seats[1].river).toHaveLength(1);
    });

    it("何度読んでも表示は進まない", () => {
      const clock = new ManualClock();
      const p = new Presentation(0, clock);
      p.receive(discard(1, 1, 5));
      p.update();

      // **毎フレーム読むので、読むたびに表示が進んでは困る。**
      p.active;
      p.active;
      expect(p.state.seats[1].river).toHaveLength(0);
    });

    it("次のイベントへ移ると適用後の盤面も入れ替わる", () => {
      const clock = new ManualClock();
      const p = new Presentation(0, clock);
      p.receive(discard(1, 1, 5));
      p.receive(discard(2, 2, 6));

      // **境目をまたいで2度読む。**1度しか読まない試験では、控えを
      // 作り直さない実装でも通ってしまう。
      p.update();
      expect(p.active?.nextState.seats[1].river).toHaveLength(1);

      clock.advance(350);
      p.update();

      // 控えを作り直さないと、1件目の盤面を返し続ける。
      const active = p.active;
      expect(active?.nextState.seats[2].river).toHaveLength(1);
      expect(active?.nextState.seats[1].river).toHaveLength(1);
    });
  });
});

describe("演出の出始めを知らせる", () => {
  /** 知らせを受け取った順に控える。 */
  function record(presentation: Presentation): {
    names: string[];
    skipped: boolean[];
  } {
    const names: string[] = [];
    const skipped: boolean[] = [];
    presentation.onStart((event, wasSkipped) => {
      names.push(event.type);
      skipped.push(wasSkipped);
    });
    return { names, skipped };
  }

  it("演出の頭で1度だけ知らせる", () => {
    // **畳み終わり（演出の終わり）ではない。**そこで鳴らすと、鳴きの声が
    // 牌を倒し終えてから聞こえる。
    const clock = new ManualClock();
    const presentation = new Presentation(0, clock);
    const seen = record(presentation);

    presentation.receive(discard(1, 1, 0));
    presentation.update();
    expect(seen.names).toEqual(["discard"]);

    // 演出の途中で何度更新しても増えない。
    clock.advance(100);
    presentation.update();
    clock.advance(100);
    presentation.update();
    expect(seen.names).toEqual(["discard"]);

    // 終わって次が始まれば、次のぶんが来る。
    presentation.receive(draw(2, 2));
    clock.advance(200);
    presentation.update();
    expect(seen.names).toEqual(["discard", "draw"]);
  });

  it("演出を持たないイベントも知らせる", () => {
    // 和了と流局は演出時間を持たないが、音は鳴らす。
    const clock = new ManualClock();
    const presentation = new Presentation(0, clock);
    const seen = record(presentation);

    presentation.receive(riichiAccepted(1, 1));
    presentation.update();
    expect(seen.names).toEqual(["riichi"]);
  });

  it("まとめて出たときは、その印が付く", () => {
    // **再接続やタブ復帰では数十件が一度に出る。**そのまま鳴らすと打牌音が
    // 束になって弾ける。受け取る側が間引けるように伝える。
    const clock = new ManualClock();
    const presentation = new Presentation(0, clock);
    const seen = record(presentation);

    for (let i = 0; i < 6; i += 1) {
      presentation.receive(discard(i + 1, 1, 0));
    }
    presentation.skip();

    expect(seen.names).toHaveLength(6);
    expect(seen.skipped.slice(1).every(Boolean), "束の印が付いていない").toBe(
      true,
    );
  });

  it("1件ずつ普通に出たものには束の印を付けない", () => {
    const clock = new ManualClock();
    const presentation = new Presentation(0, clock);
    const seen = record(presentation);

    presentation.receive(discard(1, 1, 0));
    presentation.update();
    clock.advance(350);
    presentation.receive(discard(2, 2, 1));
    presentation.update();

    expect(seen.names).toHaveLength(2);
    expect(seen.skipped).toEqual([false, false]);
  });
});
