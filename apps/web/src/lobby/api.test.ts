import { afterEach, describe, expect, it, vi } from "vitest";

import { createRoom, joinRoom, lookRoom, RoomError, startRoom } from "./api";

type Call = { url: string; init: RequestInit };

function serve(status: number, body: unknown): Call[] {
  const calls: Call[] = [];
  vi.stubGlobal("fetch", (url: string, init: RequestInit = {}) => {
    calls.push({ url, init });
    return Promise.resolve({
      ok: status >= 200 && status < 300,
      status,
      json: () => Promise.resolve(body),
    });
  });
  return calls;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("部屋の口", () => {
  it("部屋を作るとコードとトークンが返る", async () => {
    const calls = serve(200, { code: "K7QM2X", token: "9f3c" });
    const made = await createRoom("まさ");
    expect(made.code).toBe("K7QM2X");
    expect(calls[0]?.url).toBe("/api/rooms");
    expect(calls[0]?.init.method).toBe("POST");
    expect(calls[0]?.init.body).toBe(JSON.stringify({ name: "まさ" }));
  });

  it("席の証明はヘッダで送る", async () => {
    // **クエリ文字列に置かない。**アクセスログや Referer に席の証明が残る。
    const calls = serve(200, { code: "K7QM2X", state: "waiting" });
    await lookRoom("K7QM2X", "9f3c");
    expect(calls[0]?.url).toBe("/api/rooms/K7QM2X");
    expect(calls[0]?.url).not.toContain("9f3c");
    const headers = calls[0]?.init.headers as Record<string, string>;
    expect(headers["X-Mahjong-Token"]).toBe("9f3c");
  });

  it("開始もヘッダで送り、本文は持たない", async () => {
    const calls = serve(200, { state: "playing" });
    await startRoom("K7QM2X", "9f3c");
    expect(calls[0]?.init.method).toBe("POST");
    expect(calls[0]?.init.body).toBeUndefined();
    const headers = calls[0]?.init.headers as Record<string, string>;
    expect(headers["X-Mahjong-Token"]).toBe("9f3c");
  });

  it("合言葉は大文字に揃えてから送る", async () => {
    // 口で伝えられた合言葉を小文字で打つ人がいる。**入口で揃える。**
    const calls = serve(200, { token: "1a8e" });
    await joinRoom("k7qm2x", "たろう");
    expect(calls[0]?.url).toBe("/api/rooms/K7QM2X/join");
  });

  it("断られた理由が分岐に使える形で出てくる", async () => {
    serve(409, { error: "room_full" });
    await expect(joinRoom("K7QM2X", "5人目")).rejects.toMatchObject({
      slug: "room_full",
      status: 409,
    });
  });

  it("本文が読めなくても落ちない", async () => {
    // **断り方が変わっても状態番号だけは残る。**画面が真っ白にならない。
    vi.stubGlobal("fetch", () =>
      Promise.resolve({
        ok: false,
        status: 502,
        json: () => Promise.reject(new Error("本文が無い")),
      }),
    );
    const error = await createRoom("まさ").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(RoomError);
    expect((error as RoomError).status).toBe(502);
    expect((error as RoomError).slug).toBe("unknown");
  });
});
