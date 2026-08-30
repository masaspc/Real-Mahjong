import { describe, expect, it, vi } from "vitest";

import { buildUrl } from "./connection";

describe("接続先の組み立て", () => {
  it("卓の id を載せる", () => {
    expect(buildUrl("ws://h/ws", "abc", null)).toBe("ws://h/ws?token=abc");
  });

  it("連番があれば載せる", () => {
    expect(buildUrl("ws://h/ws", "abc", 42)).toBe("ws://h/ws?token=abc&last_seq=42");
  });

  it("連番が 0 でも載せる", () => {
    // **0 を「無い」と取り違えると、対局の頭から送り直される。**
    expect(buildUrl("ws://h/ws", "abc", 0)).toBe("ws://h/ws?token=abc&last_seq=0");
  });

  it("卓の id を URL に安全な形へ直す", () => {
    expect(buildUrl("ws://h/ws", "a b&c", null)).toBe("ws://h/ws?token=a+b%26c");
  });
});
