import type { AgariResult } from "../protocol/AgariResult";
import type { RyuukyokuKind } from "../protocol/RyuukyokuKind";
import type { Tile } from "../protocol/Tile";
import type { YakuId } from "../protocol/YakuId";
import type { GameState, RoundResult } from "../game/state";

/**
 * 局の結果を出す。
 *
 * **サーバは和了のあと間を置かずに次の局を配る。**役も点数も出さずに
 * 画面が切り替わるので、何で上がったのか分からない。必要な値はすべて
 * `agari` イベントが運んでいる（役と翻、符、点数、裏ドラ、点棒の増減）。
 * 出していなかっただけである。
 *
 * **進行は止めない。**止めるにはサーバ側にも間が要り、凍結済みの
 * `protocol` に演出を1つ足すことになる。ここでは次の局の上に重ねて出し、
 * 読み終わったら消す。
 */

/** 役の表示名。翻数は採点側が決めるので、ここには名前しか持たない。 */
const YAKU_NAMES: Record<YakuId, string> = {
  menzen_tsumo: "門前清自摸和",
  riichi: "立直",
  ippatsu: "一発",
  chankan: "槍槓",
  rinshan_kaihou: "嶺上開花",
  haitei_raoyue: "海底摸月",
  houtei_raoyui: "河底撈魚",
  pinfu: "平和",
  tanyao: "断幺九",
  iipeiko: "一盃口",
  yakuhai_haku: "役牌 白",
  yakuhai_hatsu: "役牌 發",
  yakuhai_chun: "役牌 中",
  yakuhai_round_wind: "場風",
  yakuhai_seat_wind: "自風",
  double_riichi: "ダブル立直",
  chiitoitsu: "七対子",
  toitoi: "対々和",
  sanankou: "三暗刻",
  sanshoku_doukou: "三色同刻",
  sankantsu: "三槓子",
  shousangen: "小三元",
  honroutou: "混老頭",
  sanshoku_doujun: "三色同順",
  ittsu: "一気通貫",
  chanta: "混全帯幺九",
  ryanpeikou: "二盃口",
  junchan: "純全帯幺九",
  honitsu: "混一色",
  chinitsu: "清一色",
  tenhou: "天和",
  chiihou: "地和",
  kokushi_musou: "国士無双",
  kokushi_musou_13: "国士無双十三面待ち",
  suuankou: "四暗刻",
  suuankou_tanki: "四暗刻単騎",
  daisangen: "大三元",
  shousuushii: "小四喜",
  daisuushii: "大四喜",
  tsuuiisou: "字一色",
  ryuuiisou: "緑一色",
  chinroutou: "清老頭",
  chuuren_poutou: "九蓮宝燈",
  chuuren_poutou_9: "純正九蓮宝燈",
  suukantsu: "四槓子",
  dora: "ドラ",
  aka_dora: "赤ドラ",
  ura_dora: "裏ドラ",
};

const RYUUKYOKU_NAMES: Record<RyuukyokuKind, string> = {
  exhaustive: "荒牌平局",
  nine_terminals: "九種九牌",
  four_riichi: "四家立直",
  four_winds: "四風連打",
  four_kans: "四槓散了",
  three_rons: "三家和",
};

/** 結果を出しておく長さ。読み切れて、次の局の邪魔になりすぎない長さ。 */
export const RESULT_SHOWN_MS = 12_000;

/** 満貫以上の呼び名。翻と符から決まる。 */
function limitName(han: number, fu: number): string | null {
  if (han >= 13) return "役満";
  if (han >= 11) return "三倍満";
  if (han >= 8) return "倍満";
  if (han >= 6) return "跳満";
  if (han >= 5) return "満貫";
  // 4翻30符・3翻60符は切り上げ満貫ではなく通常の計算だが、
  // 表示のうえでは満貫と同額になる。ここでは名前を付けない。
  if (han === 4 && fu >= 40) return "満貫";
  if (han === 3 && fu >= 70) return "満貫";
  return null;
}

type Make = {
  node<K extends keyof HTMLElementTagNameMap>(
    tag: K,
    className?: string,
    text?: string,
  ): HTMLElementTagNameMap[K];
  tileNode(tile: Tile): HTMLElement;
};

/** 1人ぶんの和了。 */
function agariBlock(result: AgariResult, viewer: number, make: Make): HTMLElement {
  const box = make.node("div", "result-win");

  const who = result.seat === viewer ? "あなた" : `席${result.seat}`;
  const how = result.from === null ? "ツモ" : `ロン（席${result.from}）`;
  box.append(make.node("div", "result-who", `${who}の和了 ・ ${how}`));

  // 手牌。**和了牌を最後に離して置く。**どれで上がったのかが読めない。
  const hand = make.node("div", "result-hand");
  for (const tile of result.hand) {
    hand.append(make.tileNode(tile));
  }
  for (const meld of result.melds) {
    const group = make.node("span", "result-meld");
    for (const tile of meld.tiles) {
      group.append(make.tileNode(tile));
    }
    hand.append(group);
  }
  const winning = make.node("span", "result-winning");
  winning.append(make.tileNode(result.win_tile));
  hand.append(winning);
  box.append(hand);

  const yaku = make.node("ul", "result-yaku");
  for (const [id, han] of result.yaku) {
    const row = make.node("li");
    row.append(
      make.node("span", "result-yaku-name", YAKU_NAMES[id] ?? id),
      make.node("span", "result-yaku-han", `${han}翻`),
    );
    yaku.append(row);
  }
  box.append(yaku);

  const limit = limitName(result.han, result.fu);
  box.append(
    make.node(
      "div",
      "result-score",
      `${result.fu}符 ${result.han}翻${limit ? ` ${limit}` : ""} ${result.score.toLocaleString()}点`,
    ),
  );

  if (result.ura_indicators !== null && result.ura_indicators.length > 0) {
    const ura = make.node("div", "result-ura");
    ura.append(make.node("span", "result-label", "裏ドラ表示"));
    for (const tile of result.ura_indicators) {
      ura.append(make.tileNode(tile));
    }
    box.append(ura);
  }

  return box;
}

/** 点棒の増減。4席ぶん。 */
function deltaBlock(
  result: RoundResult,
  state: GameState,
  make: Make,
): HTMLElement {
  const row = make.node("div", "result-delta");
  for (let seat = 0; seat < 4; seat += 1) {
    const delta = result.delta[seat] ?? 0;
    const sign = delta > 0 ? "+" : "";
    const cell = make.node(
      "span",
      `result-delta-cell${delta > 0 ? " plus" : delta < 0 ? " minus" : ""}`,
    );
    cell.append(
      make.node("b", undefined, seat === state.you ? "あなた" : `席${seat}`),
      make.node("span", undefined, `${sign}${delta.toLocaleString()}`),
    );
    row.append(cell);
  }
  return row;
}

/** 結果の板。呼ぶ側が DOM の作り方を渡す（`board.ts` と作法を揃えるため）。 */
export function resultPanel(
  result: RoundResult,
  state: GameState,
  make: Make,
  onClose: () => void,
): HTMLElement {
  const panel = make.node("section", "result");

  if (result.kind === "agari") {
    for (const one of result.results) {
      panel.append(agariBlock(one, state.you, make));
    }
  } else {
    const name =
      result.ryuukyoku === null ? "流局" : RYUUKYOKU_NAMES[result.ryuukyoku];
    panel.append(make.node("div", "result-who", name));
    const tenpai = result.tenpai
      .map((ok, seat) => `${seat === state.you ? "あなた" : `席${seat}`}: ${ok ? "聴牌" : "不聴"}`)
      .join(" / ");
    panel.append(make.node("div", "result-tenpai", tenpai));
  }

  panel.append(deltaBlock(result, state, make));

  const close = make.node("button", "action result-close", "閉じる");
  close.addEventListener("click", onClose);
  panel.append(close);
  return panel;
}
