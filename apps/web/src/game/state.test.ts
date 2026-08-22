import { describe, expect, it } from "vitest";

import type { ClientEvent } from "../protocol/ClientEvent";
import { apply, emptyState } from "./state";

let seq = 0;
function fold(events: ClientEvent[], nowMs = 0) {
  let state = emptyState(0);
  for (const event of events) {
    state = apply(state, { seq: (seq += 1), event }, nowMs);
  }
  return state;
}

const roundStart: ClientEvent = {
  type: "round_start",
  round: { wind: "East", number: 1 },
  dealer: 0,
  honba: 0,
  riichi_sticks: 0,
  scores: [25000, 25000, 25000, 25000],
  seed_commit: "abc",
};

const deal: ClientEvent = {
  type: "deal",
  your_hand: [0, 1, 2, 9, 10, 11, 18, 19, 20, 27, 27, 30, 30],
  hand_sizes: [13, 13, 13, 13],
  dora_indicator: 5,
};

describe("盤面の組み立て", () => {
  it("配牌で手牌が入る", () => {
    const state = fold([roundStart, deal]);
    expect(state.hand).toHaveLength(13);
    expect(state.doraIndicators).toEqual([5]);
    expect(state.scores).toEqual([25000, 25000, 25000, 25000]);
  });

  it("自分のツモは手牌と分けて持つ", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
    ]);
    expect(state.hand).toHaveLength(13);
    expect(state.drawn).toBe(33);
    expect(state.wallRemaining).toBe(69);
  });

  it("他家のツモは枚数だけ動かす", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
    ]);
    expect(state.drawn).toBeNull();
    expect(state.seats[1]?.handSize).toBe(14);
  });

  it("自分の打牌でツモ牌が消え、河へ積まれる", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 0, tile: 33, manner: "tsumogiri" },
    ]);
    expect(state.drawn).toBeNull();
    expect(state.hand).toHaveLength(13);
    expect(state.seats[0]?.river.map((d) => d.tile)).toEqual([33]);
  });

  it("手牌から切ると、ツモ牌が手牌へ入る", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 0, tile: 0, manner: "tedashi" },
    ]);
    expect(state.drawn).toBeNull();
    expect(state.hand).toHaveLength(13);
    expect(state.hand).toContain(33);
    expect(state.hand).not.toContain(0);
  });

  it("鳴かれた牌は河から取り除かれる", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
      // **`tiles` は副露の全部。**ポンなら手牌からの2枚＋鳴いた1枚。
      { type: "call", seat: 2, from: 1, kind: "pon", tiles: [5, 5, 5] },
    ]);
    expect(state.seats[1]?.river).toHaveLength(0);
    expect(state.seats[2]?.melds).toHaveLength(1);
    expect(state.seats[2]?.melds[0]?.tiles).toEqual([5, 5, 5]);
  });

  it("リーチが成立すると1000点が出て供託が増える", () => {
    // **イベントは金額を運ばない。**こちらで同じことをしないと、
    // 局が終わるまで画面の点数がずれ続ける。
    const state = fold([
      roundStart,
      deal,
      { type: "riichi", seat: 2, step: "declare" },
      { type: "riichi", seat: 2, step: "accepted" },
    ]);
    expect(state.scores[2]).toBe(24000);
    expect(state.sticks).toBe(1);
  });

  it("同じ牌を手にも持っているとき、ツモ切りを取り違えない", () => {
    // 5m を手に持ったまま 5m をツモり、手出しで別の牌を切る。
    const withFive: ClientEvent = {
      type: "deal",
      your_hand: [4, 0, 1, 2, 9, 10, 11, 18, 19, 20, 27, 30, 31],
      hand_sizes: [13, 13, 13, 13],
      dora_indicator: 8,
    };
    const state = fold([
      withFive,
      { type: "draw", seat: 0, tile: 4, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 0, tile: 31, manner: "tedashi" },
    ]);
    expect(state.drawn).toBeNull();
    expect(state.hand).toHaveLength(13);
    expect(state.hand.filter((t) => t === 4)).toHaveLength(2);
    expect(state.hand).not.toContain(31);
  });

  it("ポンで手牌から出るのは2枚だけ", () => {
    // **同じ牌をもう1枚持っていても、減るのは2枚。**
    // `tiles` を丸ごと引くと1枚多く消える。
    const withThree: ClientEvent = {
      type: "deal",
      your_hand: [5, 5, 5, 0, 1, 2, 9, 10, 11, 18, 19, 27, 30],
      hand_sizes: [13, 13, 13, 13],
      dora_indicator: 8,
    };
    const state = fold([
      roundStart,
      withThree,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
      { type: "call", seat: 0, from: 1, kind: "pon", tiles: [5, 5, 5] },
    ]);
    expect(state.hand).toHaveLength(11);
    expect(state.hand.filter((t) => t === 5)).toHaveLength(1);
    expect(state.seats[1]?.river).toHaveLength(0);
  });

  it("他家のポンで減る枚数は2枚", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
      { type: "call", seat: 2, from: 1, kind: "pon", tiles: [5, 5, 5] },
    ]);
    expect(state.seats[2]?.handSize).toBe(11);
  });

  it("槓に使わなかったツモ牌が嶺上ツモで消えない", () => {
    // 手牌の4枚で暗槓する。直前のツモ牌は槓と無関係なので手牌へ入る。
    const withFour: ClientEvent = {
      type: "deal",
      your_hand: [5, 5, 5, 5, 0, 1, 2, 9, 10, 11, 18, 19, 20],
      hand_sizes: [13, 13, 13, 13],
      dora_indicator: 8,
    };
    const state = fold([
      roundStart,
      withFour,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
      { type: "call", seat: 0, from: 0, kind: "ankan", tiles: [5, 5, 5, 5] },
      { type: "draw", seat: 0, tile: 6, source: "dead_wall", wall_remaining: 69 },
    ]);
    expect(state.drawn).toBe(6);
    expect(state.hand).toHaveLength(10);
    expect(state.hand).toContain(33);
    expect(state.hand.filter((t) => t === 5)).toHaveLength(0);
  });

  it("加槓の4枚目がツモ牌でも手牌が狂わない", () => {
    // **4枚目はたいていツモってきた牌。**手牌だけを見ると取り除けず、
    // 1枚多いまま残る。
    // **5m は4枚しかない。**手に2枚、ポンで1枚もらい、4枚目をツモる。
    const withPair: ClientEvent = {
      type: "deal",
      your_hand: [5, 5, 0, 1, 2, 9, 10, 11, 18, 19, 20, 27, 30],
      hand_sizes: [13, 13, 13, 13],
      dora_indicator: 8,
    };
    const state = fold([
      roundStart,
      withPair,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
      { type: "call", seat: 0, from: 1, kind: "pon", tiles: [5, 5, 5] },
      // 鳴いた席はそのまま打つ。
      { type: "discard", seat: 0, tile: 0, manner: "tedashi" },
      // 巡ってきて4枚目をツモり、加槓する。
      { type: "draw", seat: 0, tile: 5, source: "wall", wall_remaining: 68 },
      { type: "call", seat: 0, from: 1, kind: "kakan", tiles: [5, 5, 5, 5] },
    ]);
    expect(state.drawn).toBeNull();
    expect(state.hand).toHaveLength(10);
    expect(state.hand.filter((t) => t === 5)).toHaveLength(0);
  });

  it("暗槓の4枚目がツモ牌でも手牌が狂わない", () => {
    const withThree: ClientEvent = {
      type: "deal",
      your_hand: [5, 5, 5, 0, 1, 2, 9, 10, 11, 18, 19, 27, 30],
      hand_sizes: [13, 13, 13, 13],
      dora_indicator: 8,
    };
    const state = fold([
      roundStart,
      withThree,
      { type: "draw", seat: 0, tile: 5, source: "wall", wall_remaining: 69 },
      { type: "call", seat: 0, from: 0, kind: "ankan", tiles: [5, 5, 5, 5] },
      { type: "draw", seat: 0, tile: 33, source: "dead_wall", wall_remaining: 69 },
    ]);
    expect(state.hand).toHaveLength(10);
    expect(state.hand.filter((t) => t === 5)).toHaveLength(0);
    expect(state.drawn).toBe(33);
    expect(state.seats[0]?.melds[0]?.kind).toBe("ankan");
  });

  it("暗槓は河を触らず、手牌から4枚出る", () => {
    // **暗槓は from が自分自身。**河を消すと無関係な牌が消える。
    const withFour: ClientEvent = {
      type: "deal",
      your_hand: [5, 5, 5, 5, 0, 1, 2, 9, 10, 11, 18, 19, 27],
      hand_sizes: [13, 13, 13, 13],
      dora_indicator: 8,
    };
    const state = fold([
      roundStart,
      withFour,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 0, tile: 33, manner: "tsumogiri" },
      { type: "call", seat: 0, from: 0, kind: "ankan", tiles: [5, 5, 5, 5] },
    ]);
    expect(state.hand.filter((t) => t === 5)).toHaveLength(0);
    expect(state.hand).toHaveLength(9);
    expect(state.seats[0]?.river).toHaveLength(1);
    expect(state.seats[0]?.melds[0]?.kind).toBe("ankan");
  });

  it("加槓は元のポンを置き換え、手牌から1枚だけ出る", () => {
    const withPonAndFourth: ClientEvent = {
      type: "deal",
      your_hand: [5, 5, 5, 0, 1, 2, 9, 10, 11, 18, 19, 27, 30],
      hand_sizes: [13, 13, 13, 13],
      dora_indicator: 8,
    };
    const state = fold([
      roundStart,
      withPonAndFourth,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
      { type: "call", seat: 0, from: 1, kind: "pon", tiles: [5, 5, 5] },
      { type: "call", seat: 0, from: 1, kind: "kakan", tiles: [5, 5, 5, 5] },
    ]);
    expect(state.seats[0]?.melds).toHaveLength(1);
    expect(state.seats[0]?.melds[0]?.kind).toBe("kakan");
    expect(state.hand).toHaveLength(10);
    expect(state.seats[1]?.river).toHaveLength(0);
  });

  it("リーチ宣言牌に印が付く", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "riichi", seat: 2, step: "declare" },
      { type: "draw", seat: 2, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 2, tile: 9, manner: "tedashi" },
    ]);
    expect(state.seats[2]?.river[0]?.riichi).toBe(true);
  });

  it("リーチが成立すると席に印が付く", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "riichi", seat: 2, step: "declare" },
      { type: "riichi", seat: 2, step: "accepted" },
    ]);
    expect(state.seats[2]?.riichi).toBe(true);
  });

  it("要求は締切の絶対時刻を持つ", () => {
    const state = fold(
      [
        roundStart,
        deal,
        {
          type: "request_action",
          window_id: 3,
          options: [{ type: "discard", allowed: [0, 1], riichi_allowed: [] }],
          deadline_ms: 5000,
        },
      ],
      1_000,
    );
    expect(state.pending?.windowId).toBe(3);
    expect(state.pending?.deadlineAt).toBe(6_000);
  });

  it("打牌すると要求が消える", () => {
    const state = fold([
      roundStart,
      deal,
      {
        type: "request_action",
        window_id: 3,
        options: [{ type: "discard", allowed: [0], riichi_allowed: [] }],
        deadline_ms: 5000,
      },
      { type: "discard", seat: 0, tile: 0, manner: "tedashi" },
    ]);
    expect(state.pending).toBeNull();
  });

  it("新ドラが増える", () => {
    const state = fold([roundStart, deal, { type: "dora_reveal", indicator: 7 }]);
    expect(state.doraIndicators).toEqual([5, 7]);
  });

  it("局が変わると河と手牌が消える", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 0, tile: 33, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 0, tile: 33, manner: "tsumogiri" },
      roundStart,
    ]);
    expect(state.seats[0]?.river).toHaveLength(0);
    expect(state.hand).toHaveLength(0);
    expect(state.doraIndicators).toEqual([]);
  });

  it("終局で順位が入る", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "match_end", final_scores: [30000, 25000, 24000, 21000], placements: [1, 2, 3, 4] },
    ]);
    expect(state.phase).toBe("matchOver");
    expect(state.finalScores).toEqual([30000, 25000, 24000, 21000]);
  });

  it("カンの宣言だけでは盤面が動かない", () => {
    // **KanDeclared は宣言。**成立は直後の Call が運ぶ。ここで手牌を
    // 減らすと、槍槓で不成立になったときに戻せない。
    const before = fold([roundStart, deal]);
    const after = fold([
      roundStart,
      deal,
      { type: "kan_declared", seat: 0, kind: "ankan", tile: 5 },
    ]);
    expect(after.hand).toEqual(before.hand);
    expect(after.seats[0]?.melds).toHaveLength(0);
    expect(after.seats[0]?.river).toHaveLength(0);
  });

  it("シードの開示では盤面が動かない", () => {
    const before = fold([roundStart, deal]);
    const after = fold([roundStart, deal, { type: "seed_reveal", seeds: ["abc"] }]);
    expect(after.hand).toEqual(before.hand);
    expect(after.scores).toEqual(before.scores);
  });

  it("リーチ後の局終了でサーバの確定点に揃う", () => {
    // **accepted で一時的に動かした点数は、round_end が上書きする。**
    // 二重に引かないことをここで固定する。
    const state = fold([
      roundStart,
      deal,
      { type: "riichi", seat: 2, step: "declare" },
      { type: "riichi", seat: 2, step: "accepted" },
      {
        type: "round_end",
        scores: [25000, 25000, 24000, 26000],
        next: {
          type: "next",
          round: { wind: "East", number: 2 },
          dealer: 1,
          honba: 0,
          riichi_sticks: 0,
        },
        reason: "dealer_loss",
      },
    ]);
    expect(state.scores).toEqual([25000, 25000, 24000, 26000]);
  });

  it("連番を覚える", () => {
    const state = fold([roundStart, deal]);
    expect(state.lastSeq).not.toBeNull();
  });
});

describe("手番", () => {
  it("ツモで手番が移る", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 2, tile: null, source: "wall", wall_remaining: 69 },
    ]);
    expect(state.turn).toBe(2);
  });

  it("捨てても手番は移らない。次のツモまでは捨てた席のまま", () => {
    // **鳴きの反応を待っている間である。**ここで手番を進めると、誰の
    // 反応を待っているのか分からなくなる。
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 2, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 2, tile: 5, manner: "tedashi" },
    ]);
    expect(state.turn).toBe(2);
  });

  it("鳴くと鳴いた席が手番になる", () => {
    // **ツモを経由しない。**ここで移さないと、捨てた側を待ち続けている
    // ように見える。
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
      { type: "call", seat: 3, from: 1, kind: "pon", tiles: [5, 5, 5] },
    ]);
    expect(state.turn).toBe(3);
  });

  it("局が終わると手番が消える", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 2, tile: null, source: "wall", wall_remaining: 69 },
      {
        type: "ryuukyoku",
        kind: "exhaustive",
        initiator: null,
        tenpai: [false, false, false, false],
        revealed_hands: [],
        nagashi_winners: [],
        settlement: { delta: [0, 0, 0, 0], entries: [] },
      },
    ]);
    expect(state.turn).toBeNull();
  });
});

describe("鳴いたあとの表示", () => {
  it("鳴きが成立すると『席N が捨てた』の元が消える", () => {
    // **これは実際に遊んでいて詰まった。**ポンした直後、帯が出たまま
    // 残り、その状態で自分の打牌要求が届く。帯があると画面は鳴きの
    // 反応待ちに見えるのに、押せるボタンは1つも無い。何を待たれて
    // いるのか分からなくなる。
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
      { type: "call", seat: 0, from: 1, kind: "pon", tiles: [5, 5, 5] },
    ]);
    expect(state.lastDiscard).toBeNull();
  });

  it("鳴かれた側でない他家が鳴いても帯は消える", () => {
    const state = fold([
      roundStart,
      deal,
      { type: "draw", seat: 1, tile: null, source: "wall", wall_remaining: 69 },
      { type: "discard", seat: 1, tile: 5, manner: "tedashi" },
      { type: "call", seat: 2, from: 1, kind: "pon", tiles: [5, 5, 5] },
    ]);
    expect(state.lastDiscard).toBeNull();
  });
});
