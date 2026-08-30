// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";

import type { Lobby } from "./api";
import { excuse, renderLobby, renderWaiting, SEATS } from "./screen";

function lobbyOf(members: Lobby["members"], can_start = true): Lobby {
  const you = members[0] ?? { name: "まさ", host: true, present: true };
  return { code: "K7QM2X", state: "waiting", you, members, can_start };
}

let root: HTMLElement;

beforeEach(() => {
  document.body.replaceChildren();
  root = document.createElement("div");
  document.body.append(root);
});

describe("ロビー", () => {
  it("入口は3つ", () => {
    renderLobby(root, { alone: () => {}, create: () => {}, join: () => {} });
    const labels = [...root.querySelectorAll("button")].map((b) => b.textContent);
    expect(labels).toEqual(["ひとりで打つ", "部屋を作る", "部屋に入る"]);
  });

  it("名前と合言葉をそのまま渡す", () => {
    const seen: string[] = [];
    renderLobby(root, {
      alone: () => {},
      create: () => {},
      join: (code, name) => seen.push(code, name),
    });
    const inputs = root.querySelectorAll("input");
    (inputs[0] as HTMLInputElement).value = "まさ";
    (inputs[1] as HTMLInputElement).value = "k7qm2x";
    [...root.querySelectorAll("button")][2]?.dispatchEvent(new Event("click"));
    expect(seen).toEqual(["k7qm2x", "まさ"]);
  });
});

describe("待合", () => {
  it("枠は常に4つ。空きは CPU が入ると言う", () => {
    renderWaiting(root, lobbyOf([{ name: "まさ", host: true, present: true }]), {
      start: () => {},
      leave: () => {},
      copy: () => {},
    });
    const slots = root.querySelectorAll(".slot");
    expect(slots.length).toBe(SEATS);
    expect(slots[1]?.textContent).toContain("CPU");
  });

  it("部屋主にだけ開始が出る", () => {
    const handlers = { start: () => {}, leave: () => {}, copy: () => {} };
    const members = [{ name: "まさ", host: true, present: true }];

    renderWaiting(root, lobbyOf(members, true), handlers);
    expect(root.textContent).toContain("開始");

    renderWaiting(root, lobbyOf(members, false), handlers);
    expect(root.textContent).not.toContain("開始");
    expect(root.textContent).toContain("部屋主が始めるのを待っています");
  });

  it("黙っている人には離席中が付く", () => {
    renderWaiting(
      root,
      lobbyOf([
        { name: "まさ", host: true, present: true },
        { name: "たろう", host: false, present: false },
      ]),
      { start: () => {}, leave: () => {}, copy: () => {} },
    );
    const slots = [...root.querySelectorAll(".slot")];
    expect(slots[0]?.textContent).not.toContain("離席中");
    expect(slots[1]?.textContent).toContain("離席中");
  });

  it("合言葉を押すと写す", () => {
    const copied: string[] = [];
    renderWaiting(root, lobbyOf([{ name: "まさ", host: true, present: true }]), {
      start: () => {},
      leave: () => {},
      copy: (text) => copied.push(text),
    });
    root.querySelector<HTMLElement>(".lobby-code")?.dispatchEvent(new Event("click"));
    expect(copied).toEqual(["K7QM2X"]);
  });

  /**
   * **名前は他人が入力した文字列である。**待合が唯一の流入口なので、
   * ここが `innerHTML` になっていないことを見張る。
   */
  it("名前は文字として入る。印にはならない", () => {
    const attack = "<img src=x onerror=alert(1)>";
    renderWaiting(root, lobbyOf([{ name: attack, host: false, present: true }]), {
      start: () => {},
      leave: () => {},
      copy: () => {},
    });
    expect(root.querySelectorAll("img").length).toBe(0);
    expect(root.querySelector(".slot-name")?.textContent).toBe(attack);
  });
});

describe("断りの言い換え", () => {
  it("知らない理由でも黙らない", () => {
    // **表示文は画面が持つ。**サーバは分岐のための名前しか返さない。
    expect(excuse("room_full")).toContain("4人");
    expect(excuse("no_such_room")).toContain("見つかりません");
    expect(excuse("なにこれ")).not.toBe("");
  });
});

describe("待合の枠", () => {
  it("4人埋まれば空きは出ない", () => {
    const members = ["1", "2", "3", "4"].map((name) => ({
      name,
      host: name === "1",
      present: true,
    }));
    renderWaiting(root, lobbyOf(members), {
      start: () => {},
      leave: () => {},
      copy: () => {},
    });
    expect(root.querySelectorAll(".slot.empty").length).toBe(0);
    expect(root.querySelectorAll(".slot").length).toBe(SEATS);
  });
});
