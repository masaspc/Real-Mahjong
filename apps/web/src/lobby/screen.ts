/**
 * ロビーと待合。**対局が始まるまでの画面である。**
 *
 * 描画は `renderLobby` / `renderWaiting` の2本に閉じ、進行の判断は
 * `mountLobby` が持つ。試験は前者を DOM の上で直接叩く。
 */

import { playerKey } from "../records/api";
import type { Lobby, MemberView } from "./api";
import {
  clearSeat,
  createRoom,
  joinRoom,
  loadSeat,
  lookRoom,
  RoomError,
  saveSeat,
  startRoom,
} from "./api";

/** 待合を引く間隔。 */
export const POLL_MS = 1_000;

/** 卓に着ける人数。空いた席は CPU が埋める。 */
export const SEATS = 4;

/** 断りの名前を人の言葉にする。**表示文は画面が持つ。** */
export function excuse(slug: string): string {
  switch (slug) {
    case "no_such_room":
      return "その合言葉の部屋は見つかりません";
    case "room_full":
      return "その部屋はもう4人います";
    case "already_started":
      return "その部屋はもう始まっています";
    case "bad_token":
      return "この部屋の席がありません。作り直してください";
    case "not_host":
      return "開始できるのは部屋を作った人だけです";
    default:
      return "つながりませんでした。少し待って試してください";
  }
}

function node<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  if (className) element.className = className;
  // **名前は他人が入力した文字列である。**innerHTML に混ぜない。
  if (text !== undefined) element.textContent = text;
  return element;
}

function field(label: string, placeholder: string, max: number): HTMLInputElement {
  const input = node("input");
  input.type = "text";
  input.placeholder = placeholder;
  input.maxLength = max;
  input.setAttribute("aria-label", label);
  return input;
}

export type LobbyHandlers = {
  alone(name: string): void;
  create(name: string): void;
  join(code: string, name: string): void;
  records(): void;
};

/** ロビー。入口は3つだけ。 */
export function renderLobby(
  root: HTMLElement,
  handlers: LobbyHandlers,
  notice?: string,
): void {
  const panel = node("div", "lobby");
  panel.append(node("h1", "lobby-title", "麻雀"));

  const name = field("あなたの名前", "名前（12文字まで）", 12);
  name.className = "lobby-name";
  panel.append(name);

  const alone = node("button", "lobby-button primary", "ひとりで打つ");
  alone.addEventListener("click", () => handlers.alone(name.value));

  const create = node("button", "lobby-button", "部屋を作る");
  create.addEventListener("click", () => handlers.create(name.value));

  panel.append(alone, create);

  const row = node("div", "lobby-join");
  const code = field("合言葉", "合言葉", 6);
  code.className = "lobby-code-input";
  const join = node("button", "lobby-button", "部屋に入る");
  join.addEventListener("click", () => handlers.join(code.value, name.value));
  row.append(code, join);
  panel.append(row);

  const records = node("button", "lobby-button quiet", "牌譜を見る");
  records.addEventListener("click", () => handlers.records());
  panel.append(records);

  if (notice) {
    panel.append(node("p", "lobby-notice", notice));
  }
  root.replaceChildren(panel);
}

export type WaitingHandlers = {
  start(): void;
  leave(): void;
  copy(code: string): void;
};

function memberSlot(member: MemberView | undefined): HTMLElement {
  if (!member) {
    // **空席は隠さない。**「あと何人入れるか」と「CPU が入る」を同時に言う。
    return node("li", "slot empty", "空き（CPU が入ります）");
  }
  const item = node("li", `slot${member.present ? " present" : ""}`);
  item.append(node("span", "slot-name", member.name));
  if (member.host) item.append(node("span", "slot-mark", "部屋主"));
  if (!member.present) item.append(node("span", "slot-mark away", "離席中"));
  return item;
}

/** 待合。 */
export function renderWaiting(
  root: HTMLElement,
  lobby: Lobby,
  handlers: WaitingHandlers,
  notice?: string,
): void {
  const panel = node("div", "lobby");
  panel.append(node("p", "lobby-lead", "この合言葉を相手に伝えてください"));

  const code = node("button", "lobby-code", lobby.code);
  code.title = "押すと写します";
  code.addEventListener("click", () => handlers.copy(lobby.code));
  panel.append(code);

  const list = node("ul", "slots");
  // **形が違う応答でも枠は出す。**ここで落ちると1秒ごとの見張りが止まり、
  // 待合が固まったまま何も言わない画面になる。
  const members = lobby.members ?? [];
  for (let index = 0; index < SEATS; index += 1) {
    list.append(memberSlot(members[index]));
  }
  panel.append(list);

  if (lobby.can_start) {
    const start = node("button", "lobby-button primary", "開始（空席は CPU）");
    start.addEventListener("click", () => handlers.start());
    panel.append(start);
  } else {
    panel.append(node("p", "lobby-wait", "部屋主が始めるのを待っています"));
  }

  const leave = node("button", "lobby-button quiet", "やめる");
  leave.addEventListener("click", () => handlers.leave());
  panel.append(leave);

  if (notice) {
    panel.append(node("p", "lobby-notice", notice));
  }
  root.replaceChildren(panel);
}

/** ロビーを抜けた先。 */
export type Entry =
  | { kind: "play"; token: string }
  | { kind: "records" };

/**
 * ロビーを動かし、卓が立ったら席の証明を返す。
 *
 * **覚えている席から始める。**再読み込みで対局へ戻れることが、
 * 遊びやすさに直結する。
 */
export function mountLobby(root: HTMLElement): Promise<Entry> {
  return new Promise((resolve) => {
    let timer: ReturnType<typeof setTimeout> | null = null;

    const stop = (): void => {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    };

    const toLobby = (notice?: string): void => {
      stop();
      clearSeat();
      renderLobby(
        root,
        {
          alone: (name) => {
            void begin(name, true);
          },
          create: (name) => {
            void begin(name, false);
          },
          join: (code, name) => {
            void enter(code, name);
          },
          records: () => {
            stop();
            resolve({ kind: "records" });
          },
        },
        notice,
      );
    };

    const begin = async (name: string, alone: boolean): Promise<void> => {
      try {
        const made = await createRoom(name, playerKey());
        saveSeat(made.code, made.token);
        if (alone) {
          await startRoom(made.code, made.token);
          resolve({ kind: "play", token: made.token });
          return;
        }
        watch(made.code, made.token);
      } catch (error) {
        toLobby(excuse(error instanceof RoomError ? error.slug : "unknown"));
      }
    };

    const enter = async (code: string, name: string): Promise<void> => {
      const wanted = code.trim().toUpperCase();
      if (wanted.length === 0) {
        toLobby("合言葉を入れてください");
        return;
      }
      try {
        const joined = await joinRoom(wanted, name, playerKey());
        saveSeat(wanted, joined.token);
        watch(wanted, joined.token);
      } catch (error) {
        toLobby(excuse(error instanceof RoomError ? error.slug : "unknown"));
      }
    };

    const watch = (code: string, token: string, notice?: string): void => {
      stop();
      const tick = async (): Promise<void> => {
        let lobby: Lobby;
        try {
          lobby = await lookRoom(code, token);
        } catch (error) {
          // **知らないトークンはロビーへ戻す。**掃かれた部屋の証明を
          // 握ったまま待ち続けても、永久に始まらない。
          toLobby(excuse(error instanceof RoomError ? error.slug : "unknown"));
          return;
        }
        if (lobby.state === "playing") {
          stop();
          resolve({ kind: "play", token });
          return;
        }
        try {
          renderWaiting(
          root,
          lobby,
          {
            start: () => {
              void startRoom(code, token).catch((error: unknown) => {
                watch(code, token, excuse(error instanceof RoomError ? error.slug : "unknown"));
              });
            },
            leave: () => toLobby(),
            copy: (text) => {
              void navigator.clipboard?.writeText(text);
            },
          },
          notice,
          );
        } catch (error) {
          // **描けなくても見張りは続ける。**次の応答で直るかもしれない。
          console.error("待合を描けなかった", error);
        }
        notice = undefined;
        timer = setTimeout(() => void tick(), POLL_MS);
      };
      void tick();
    };

    const remembered = loadSeat();
    if (remembered) {
      watch(remembered.code, remembered.token);
    } else {
      toLobby();
    }
  });
}
