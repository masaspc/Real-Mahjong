import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { listRecords, playerKey, readRecord, recordEvents } from "./api";
import { RoomError } from "../lobby/api";

type Call = { url: string; init: RequestInit };

function serve(status: number, body: unknown, text?: string): Call[] {
  const calls: Call[] = [];
  vi.stubGlobal("fetch", (url: string, init: RequestInit = {}) => {
    calls.push({ url, init });
    return Promise.resolve({
      ok: status >= 200 && status < 300,
      status,
      json: () => Promise.resolve(body),
      text: () => Promise.resolve(text ?? ""),
    });
  });
  return calls;
}

const store = new Map<string, string>();

beforeEach(() => {
  store.clear();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
    removeItem: (key: string) => void store.delete(key),
  });
  vi.stubGlobal("crypto", { randomUUID: () => "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee" });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("browser の鍵", () => {
  it("無ければ作って覚える", () => {
    const first = playerKey();
    expect(first).toHaveLength(32);
    expect(store.get("real-mahjong.player")).toBe(first);
  });

  it("2度呼んでも同じ鍵", () => {
    // **毎回作り直すと、対局のたびに別人になる。**一覧が1件ずつに割れる。
    expect(playerKey()).toBe(playerKey());
  });

  it("既にあるものを上書きしない", () => {
    store.set("real-mahjong.player", "むかしの鍵");
    expect(playerKey()).toBe("むかしの鍵");
  });
});

describe("牌譜の一覧", () => {
  it("鍵はヘッダで送る", () => {
    // **クエリ文字列に置かない。**アクセスログや Referer に残る。
    const calls = serve(200, { records: [] });
    return listRecords().then(() => {
      expect(calls[0]?.url).toBe("/api/records");
      const headers = calls[0]?.init.headers as Record<string, string>;
      expect(headers["X-Mahjong-Player"]).toHaveLength(32);
      expect(calls[0]?.url).not.toContain(headers["X-Mahjong-Player"]);
    });
  });

  it("空でも落ちない", async () => {
    serve(200, {});
    await expect(listRecords()).resolves.toEqual([]);
  });

  it("断られた理由が分岐に使える形で出る", async () => {
    serve(401, { error: "bad_player" });
    await expect(listRecords()).rejects.toBeInstanceOf(RoomError);
  });
});

describe("牌譜の中身", () => {
  it("見出しは席の証明で引く", async () => {
    const calls = serve(200, { id: "abc", you: 2, players: [], started_ms: 1 });
    const head = await readRecord("abc", "tok");
    expect(head.you).toBe(2);
    expect(calls[0]?.url).toBe("/api/records/abc");
    const headers = calls[0]?.init.headers as Record<string, string>;
    expect(headers["X-Mahjong-Token"]).toBe("tok");
  });

  it("席の証明が無くても鍵で開ける", async () => {
    // **証明は部屋ごとに配られる。**対局が終わって画面を閉じれば
    // 手元に残らない。一覧に出ているのに開けない、では困る。
    const calls = serve(200, { id: "abc", you: 1, players: [], started_ms: 1 });
    await readRecord("abc");
    const headers = calls[0]?.init.headers as Record<string, string>;
    expect(headers["X-Mahjong-Player"]).toHaveLength(32);
    expect(headers["X-Mahjong-Token"]).toBeUndefined();
  });

  it("本文は JSONL として読む", async () => {
    const lines = [
      '{"seq":0,"event":{"type":"dora_reveal","indicator":3}}',
      '{"seq":1,"event":{"type":"dora_reveal","indicator":4}}',
    ].join("\n");
    serve(200, null, lines);
    const events = await recordEvents("abc", "tok");
    expect(events).toHaveLength(2);
    expect(events[1]?.seq).toBe(1);
  });

  it("読めない行は捨てる。1行の壊れで対局を諦めない", async () => {
    serve(200, null, '{"seq":0,"event":{"type":"dora_reveal","indicator":3}}\nこわれ\n\n');
    const events = await recordEvents("abc", "tok");
    expect(events).toHaveLength(1);
  });

  it("id は URL に入れる前に逃がす", async () => {
    const calls = serve(200, null, "");
    await recordEvents("a/b?c", "tok");
    expect(calls[0]?.url).toBe("/api/records/a%2Fb%3Fc/events");
  });
});
