import type { ActionOption } from "../protocol/ActionOption";
import type { ClientEvent } from "../protocol/ClientEvent";
import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import type { MeldKind } from "../protocol/MeldKind";
import type { Round } from "../protocol/Round";
import type { Seat } from "../protocol/Seat";
import type { Tile } from "../protocol/Tile";
import { kindOf, sortTiles } from "./tiles";

/** リーチ棒。**成立の時点で宣言者から出る。** */
const RIICHI_STICK = 1_000;

/** 河に積まれた1枚。 */
export type Discarded = {
  tile: Tile;
  /** リーチ宣言牌。横向きに描く。 */
  riichi: boolean;
};

export type MeldView = {
  kind: MeldKind;
  tiles: Tile[];
  from: Seat;
};

export type SeatView = {
  handSize: number;
  river: Discarded[];
  melds: MeldView[];
  riichi: boolean;
  /** 次の打牌がリーチ宣言牌になる席。 */
  declaring: boolean;
};

/** いま自分が選べること。 */
export type Pending = {
  windowId: number;
  options: ActionOption[];
  /** 締切の絶対時刻（`performance.now()` と同じ尺度）。 */
  deadlineAt: number;
};

export type GameState = {
  you: Seat;
  seats: [SeatView, SeatView, SeatView, SeatView];
  /** 自分の手牌。ツモ牌は含まない。 */
  hand: Tile[];
  /** 自分のツモ牌。**手牌と分けて持つと、ツモ切りが描き分けられる。** */
  drawn: Tile | null;
  round: Round | null;
  dealer: Seat;
  honba: number;
  sticks: number;
  scores: number[];
  doraIndicators: Tile[];
  wallRemaining: number;
  pending: Pending | null;
  lastSeq: number | null;
  phase: "waiting" | "playing" | "matchOver";
  /**
   * 直前に切られた牌。**鳴くかどうかを決めるのに要る。**
   *
   * 相手が何を切ったのかが分からないと、ポンやチーのボタンが出ても何を
   * 鳴くのか判断できない。次のツモか鳴きの成立で消える。
   */
  lastDiscard: { seat: Seat; tile: Tile } | null;
  /** 和了や流局の要約。画面の帯に出す。 */
  notice: string | null;
  finalScores: number[] | null;
};

/**
 * 席を取り出す。
 *
 * **`noUncheckedIndexedAccess` が効いているので添字は undefined を含む。**
 * 席は必ず4つあるので実際には外れないが、型を黙らせるために一箇所へ寄せる。
 */
function seatOf(state: GameState, seat: Seat): SeatView {
  const found = state.seats[seat];
  if (!found) {
    throw new Error(`席が範囲外: ${seat}`);
  }
  return found;
}

/**
 * 自分の持ち牌から1枚取り除く。
 *
 * **ツモ牌は手牌と分けて持っている。**加槓の4枚目や暗槓の4枚目は
 * たいていツモってきた牌なので、手牌だけを見ると取り除けず、
 * 手牌が1枚多いまま残る。
 */
function takeFromMine(state: GameState, tile: Tile): void {
  const at = state.hand.indexOf(tile);
  if (at >= 0) {
    state.hand.splice(at, 1);
    return;
  }
  if (state.drawn === tile) {
    state.drawn = null;
  }
}

function emptySeat(): SeatView {
  return { handSize: 0, river: [], melds: [], riichi: false, declaring: false };
}

export function emptyState(you: Seat): GameState {
  return {
    you,
    seats: [emptySeat(), emptySeat(), emptySeat(), emptySeat()],
    hand: [],
    drawn: null,
    round: null,
    dealer: 0,
    honba: 0,
    sticks: 0,
    scores: [25000, 25000, 25000, 25000],
    doraIndicators: [],
    wallRemaining: 70,
    pending: null,
    lastDiscard: null,
    lastSeq: null,
    phase: "waiting",
    notice: null,
    finalScores: null,
  };
}

function clone(state: GameState): GameState {
  return {
    ...state,
    seats: state.seats.map((s) => ({
      ...s,
      river: [...s.river],
      melds: [...s.melds],
    })) as GameState["seats"],
    hand: [...state.hand],
    doraIndicators: [...state.doraIndicators],
    scores: [...state.scores],
  };
}

/** 1件のイベントを畳む。**状態を書き換えず、新しい状態を返す。** */
export function apply(
  previous: GameState,
  envelope: ClientEventEnvelope,
  nowMs: number,
): GameState {
  const state = clone(previous);
  state.lastSeq = envelope.seq;
  const event: ClientEvent = envelope.event;

  switch (event.type) {
    case "match_start":
      state.you = event.you;
      state.phase = "playing";
      break;

    case "round_start": {
      const you = state.you;
      const carried = { ...emptyState(you), lastSeq: state.lastSeq, phase: "playing" as const };
      Object.assign(state, carried);
      state.round = event.round;
      state.dealer = event.dealer;
      state.honba = event.honba;
      state.sticks = event.riichi_sticks;
      state.scores = [...event.scores];
      state.notice = null;
      break;
    }

    case "deal":
      state.hand = sortTiles(event.your_hand);
      state.doraIndicators = [event.dora_indicator];
      for (let i = 0; i < 4; i += 1) {
        seatOf(state, i as Seat).handSize = event.hand_sizes[i] ?? 13;
      }
      break;

    case "draw":
      state.lastDiscard = null;
      state.wallRemaining = event.wall_remaining;
      if (event.seat === state.you && event.tile !== null) {
        // **前のツモ牌が残っていれば手牌へ入れる。**カンのあとの嶺上ツモで
        // 上書きすると、槓に使わなかったツモ牌が消える。手牌の4枚で
        // 暗槓したときがそれにあたる。
        if (state.drawn !== null) {
          state.hand.push(state.drawn);
          state.hand = sortTiles(state.hand);
        }
        state.drawn = event.tile;
      } else {
        seatOf(state, event.seat).handSize += 1;
      }
      break;

    case "discard": {
      const seat = seatOf(state, event.seat);
      seat.river.push({ tile: event.tile, riichi: seat.declaring });
      seat.declaring = false;
      state.lastDiscard = { seat: event.seat, tile: event.tile };
      if (event.seat === state.you) {
        // **ツモ切りかどうかはイベントが持っている。**牌の一致で当てると、
        // 同じ牌を手にも持っているときに取り違える。
        if (event.manner === "tsumogiri") {
          state.drawn = null;
        } else {
          const at = state.hand.indexOf(event.tile);
          if (at >= 0) {
            state.hand.splice(at, 1);
          }
          if (state.drawn !== null) {
            state.hand.push(state.drawn);
            state.drawn = null;
          }
          state.hand = sortTiles(state.hand);
        }
        state.pending = null;
      } else {
        seat.handSize -= 1;
      }
      break;
    }

    case "riichi":
      if (event.step === "declare") {
        seatOf(state, event.seat).declaring = true;
      } else {
        seatOf(state, event.seat).riichi = true;
        // **成立の時点で棒が出る。**エンジンはここで 1000 点を引き、
        // 供託を1本増やす。イベントは金額を運ばないので、こちらで
        // 同じことをしないと局が終わるまで画面の点数がずれ続ける。
        state.scores[event.seat] = (state.scores[event.seat] ?? 0) - RIICHI_STICK;
        state.sticks += 1;
      }
      break;

    case "call": {
      const caller = seatOf(state, event.seat);
      // **`tiles` は副露の全部であって、手牌から出た分ではない。**
      // 取り違えると手牌の枚数が狂う。
      let fromHand: Tile[] = [...event.tiles];

      if (event.kind === "ankan") {
        // 暗槓は `from` が自分自身。**河を触ってはならない。**
        // 4枚とも手牌から出る。
      } else if (event.kind === "kakan") {
        // 加槓は元のポンに4枚目を足したもの。鳴いた牌はポンの時点で
        // 河から消えている。**ここで消すと無関係な牌が消える。**
        // 手牌から出るのは足した1枚だけ。
        fromHand = event.tiles.slice(-1);
      } else {
        // チー・ポン・大明槓。鳴かれた牌は打った席の河から消える。
        const source = seatOf(state, event.from);
        const called = source.river.pop()?.tile;
        const at = called === undefined ? -1 : fromHand.indexOf(called);
        if (at >= 0) {
          fromHand.splice(at, 1);
        }
      }

      if (event.kind === "kakan") {
        // **元のポンを置き換える。**足すと、ポンと加槓が二重に並ぶ。
        const fourth = event.tiles[0];
        const at =
          fourth === undefined
            ? -1
            : caller.melds.findIndex((meld) => {
                const head = meld.tiles[0];
                return meld.kind === "pon" && head !== undefined && kindOf(head) === kindOf(fourth);
              });
        const upgraded = { kind: event.kind, tiles: event.tiles, from: event.from };
        if (at >= 0) {
          caller.melds[at] = upgraded;
        } else {
          caller.melds.push(upgraded);
        }
      } else {
        caller.melds.push({ kind: event.kind, tiles: event.tiles, from: event.from });
      }

      if (event.seat === state.you) {
        for (const tile of fromHand) {
          takeFromMine(state, tile);
        }
        state.pending = null;
      } else {
        caller.handSize -= fromHand.length;
      }
      break;
    }

    case "dora_reveal":
      state.doraIndicators.push(event.indicator);
      break;

    case "request_action":
      state.pending = {
        windowId: event.window_id,
        options: event.options,
        deadlineAt: nowMs + event.deadline_ms,
      };
      break;

    case "action_passed":
      if (event.seat === state.you) {
        state.pending = null;
      }
      break;

    case "agari": {
      const winners = event.results.map((r) => `席${r.seat}`).join("・");
      state.notice = `${winners} 和了`;
      state.pending = null;
      break;
    }

    case "ryuukyoku":
      state.notice = "流局";
      state.pending = null;
      break;

    case "round_end":
      state.scores = [...event.scores];
      state.pending = null;
      break;

    case "match_end":
      state.phase = "matchOver";
      state.finalScores = [...event.final_scores];
      state.notice = "終局";
      state.pending = null;
      break;

    default:
      break;
  }

  return state;
}
