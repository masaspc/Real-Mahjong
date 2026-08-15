import type { Tile } from "../protocol/Tile";
import type { Command } from "../protocol/Command";
import type { GameState } from "../game/state";
import { tileLabel } from "../game/tiles";

/** 押せるボタン1つぶん。 */
export type Choice = {
  label: string;
  /** ボタンに並べて見せる牌。**文字だけでは何を鳴くのか分からない。** */
  tiles: Tile[];
  command: Command;
};

/**
 * いま選べる操作を、押したら送るコマンドとして並べる。
 *
 * **DOM を触らない。**大明槓・暗槓・加槓・ロン・ツモは滅多に出ないので、
 * その場面を引くまで遊んで確かめるのは現実的でない。ここを純粋な関数に
 * しておけば、送信の形を場面に依存せず固定できる。
 *
 * **送り方が3通りあることに注意。**大明槓・チー・ポン・ロン・見送りは
 * `call_response`、暗槓と加槓とツモと九種九牌は専用のコマンドである。
 */
export function actionsFor(state: GameState): Choice[] {
  const pending = state.pending;
  if (!pending) {
    return [];
  }
  const windowId = pending.windowId;
  const choices: Choice[] = [];

  // **打牌を求められていなければ反応ウィンドウ。**そのときだけ見送れる。
  // 自分の番に「見送り」は意味を持たない。必ず何かを切る。
  const isReaction = !pending.options.some((option) => option.type === "discard");

  for (const option of pending.options) {
    switch (option.type) {
      case "chi":
        for (const tiles of option.candidates) {
          choices.push({
            label: "チー",
            tiles,
            command: { type: "call_response", window_id: windowId, response: { type: "chi", tiles } },
          });
        }
        break;

      case "pon":
        for (const tiles of option.candidates) {
          choices.push({
            label: "ポン",
            tiles,
            command: { type: "call_response", window_id: windowId, response: { type: "pon", tiles } },
          });
        }
        break;

      case "kan":
        for (const candidate of option.candidates) {
          if (candidate.type === "minkan") {
            choices.push({
              label: "大明槓",
              tiles: [],
              command: { type: "call_response", window_id: windowId, response: { type: "kan" } },
            });
          } else if (candidate.type === "ankan") {
            choices.push({
              label: "暗槓",
              tiles: [candidate.kind],
              command: { type: "ankan", kind: candidate.kind },
            });
          } else {
            choices.push({
              label: "加槓",
              tiles: [candidate.tile],
              command: { type: "kakan", tile: candidate.tile },
            });
          }
        }
        break;

      case "ron":
        choices.push({
          label: "ロン",
          tiles: [],
          command: { type: "call_response", window_id: windowId, response: { type: "ron" } },
        });
        break;

      case "tsumo":
        choices.push({ label: "ツモ",
          tiles: [], command: { type: "tsumo" } });
        break;

      case "kyuushu":
        choices.push({ label: "九種九牌",
          tiles: [], command: { type: "kyuushu" } });
        break;

      default:
        break;
    }
  }

  // **`ActionOption::Pass` はエンジンが一度も出さない。**それでも
  // `CallResponse::Pass` はいつでも受理される（`response_is_offered` が
  // Pass だけ無条件に真）。見送りが押せないと、鳴きたくない席は
  // 時間切れまで待つことになり、他の3人まで待たされる。
  if (isReaction && choices.length > 0) {
    choices.push({
      label: "見送り",
          tiles: [],
      command: { type: "call_response", window_id: windowId, response: { type: "pass" } },
    });
  }
  return choices;
}

/** 押せる牌。押したら送るコマンドを添える。 */
export function discardChoices(state: GameState, riichiReady: boolean): Map<number, Command> {
  const map = new Map<number, Command>();
  const option = state.pending?.options.find((o) => o.type === "discard");
  if (!option || option.type !== "discard") {
    return map;
  }
  const allowed = riichiReady ? option.riichi_allowed : option.allowed;
  for (const tile of allowed) {
    map.set(tile, { type: "discard", tile, riichi: riichiReady });
  }
  return map;
}

/** リーチを宣言できるか。 */
export function canDeclareRiichi(state: GameState): boolean {
  const option = state.pending?.options.find((o) => o.type === "discard");
  return option?.type === "discard" && option.riichi_allowed.length > 0;
}
