// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Lobby } from "./api";
import { excuse, mountLobby, renderLobby, renderWaiting, SEATS } from "./screen";

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

describe("ロビーの配線", () => {
  type Call = { url: string; init: RequestInit };
  let calls: Call[];
  const store = new Map<string, string>();

  beforeEach(() => {
    calls = [];
    store.clear();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => void store.set(key, value),
      removeItem: (key: string) => void store.delete(key),
    });
    vi.stubGlobal("crypto", {
      randomUUID: () => "11111111-2222-3333-4444-555555555555",
    });
    vi.stubGlobal("fetch", (url: string, init: RequestInit = {}) => {
      calls.push({ url, init });
      const body = url === "/api/rooms" ? { code: "K7QM2X", token: "tok" } : { token: "tok" };
      return Promise.resolve({
        ok: true,
        status: 200,
        json: () => Promise.resolve(body),
        text: () => Promise.resolve(""),
      });
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  function press(label: string): void {
    const button = [...root.querySelectorAll("button")].find(
      (b) => b.textContent === label,
    );
    if (!button) throw new Error(`${label} が無い`);
    button.dispatchEvent(new Event("click"));
  }

  async function settle(): Promise<void> {
    for (let i = 0; i < 20; i += 1) {
      await Promise.resolve();
    }
  }

  /**
   * **鍵は部屋を作る時点で渡さないと間に合わない。**卓が立つときに牌譜の
   * 見出しへ入るので、後から渡しても牌譜が自分のものにならない。
   */
  it("部屋を作るとき browser の鍵を渡す", async () => {
    void mountLobby(root);
    press("部屋を作る");
    await settle();

    const made = calls.find((call) => call.url === "/api/rooms");
    expect(made, "部屋を作る要求が飛んでいない").toBeTruthy();
    const headers = made?.init.headers as Record<string, string>;
    expect(headers["X-Mahjong-Player"]).toHaveLength(32);
  });

  it("部屋に入るときも鍵を渡す", async () => {
    void mountLobby(root);
    const code = root.querySelector<HTMLInputElement>(".lobby-code-input");
    if (!code) throw new Error("合言葉の欄が無い");
    code.value = "K7QM2X";
    press("部屋に入る");
    await settle();

    const joined = calls.find((call) => call.url.includes("/join"));
    expect(joined, "入室の要求が飛んでいない").toBeTruthy();
    const headers = joined?.init.headers as Record<string, string>;
    expect(headers["X-Mahjong-Player"]).toHaveLength(32);
  });

  it("ひとりで打つは、作ってすぐ始める", async () => {
    void mountLobby(root);
    press("ひとりで打つ");
    await settle();

    expect(calls.map((call) => call.url)).toEqual([
      "/api/rooms",
      "/api/rooms/K7QM2X/start",
    ]);
  });
});
