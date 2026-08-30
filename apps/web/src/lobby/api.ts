/**
 * 部屋の口を叩く。
 *
 * **対局の通信とは別系統である。**卓のやり取りは WebSocket で
 * `crates/protocol` の型に従うが、部屋は普通の HTTP JSON でやり取りする。
 * 凍結された型に部屋の概念を持ち込まないための線引きで、
 * `docs/superpowers/specs/2026-08-30-rooms-and-seating-design.md` で決めた。
 */

/** 席の証明を運ぶヘッダ。サーバの `TOKEN_HEADER` と対になる。 */
const TOKEN_HEADER = "X-Mahjong-Token";

const TOKEN_KEY = "real-mahjong.token";
const CODE_KEY = "real-mahjong.code";

export type MemberView = {
  name: string;
  host: boolean;
  present: boolean;
};

export type Lobby = {
  code: string;
  state: "waiting" | "playing";
  you: MemberView;
  members: MemberView[];
  can_start: boolean;
};

/**
 * サーバが断った理由。
 *
 * **`slug` は分岐のための名前で、人に見せる文ではない。**表示文を
 * サーバが持つと、言い回しを直すたびにサーバを出し直すことになる。
 */
export class RoomError extends Error {
  readonly slug: string;
  readonly status: number;

  constructor(slug: string, status: number) {
    super(slug);
    this.name = "RoomError";
    this.slug = slug;
    this.status = status;
  }
}

async function ask<T>(
  path: string,
  init: { method?: string; token?: string | null; body?: unknown } = {},
): Promise<T> {
  const headers: Record<string, string> = {};
  if (init.body !== undefined) headers["Content-Type"] = "application/json";
  if (init.token) headers[TOKEN_HEADER] = init.token;

  const response = await fetch(path, {
    method: init.method ?? "GET",
    headers,
    body: init.body === undefined ? undefined : JSON.stringify(init.body),
  });
  // **本文が無くても落とさない。**断り方が変わっても、状態番号だけは残る。
  const payload = await response.json().catch(() => null);
  if (!response.ok) {
    const slug =
      payload && typeof payload === "object" && "error" in payload
        ? String((payload as { error: unknown }).error)
        : "unknown";
    throw new RoomError(slug, response.status);
  }
  return payload as T;
}

export function createRoom(name: string): Promise<{ code: string; token: string }> {
  return ask("/api/rooms", { method: "POST", body: { name } });
}

export function joinRoom(code: string, name: string): Promise<{ token: string }> {
  // 合言葉は小文字で入れられても通す。**字を揃えるのは入口の仕事。**
  return ask(`/api/rooms/${encodeURIComponent(code.toUpperCase())}/join`, {
    method: "POST",
    body: { name },
  });
}

export function lookRoom(code: string, token: string): Promise<Lobby> {
  return ask(`/api/rooms/${encodeURIComponent(code)}`, { token });
}

export function startRoom(code: string, token: string): Promise<{ state: string }> {
  return ask(`/api/rooms/${encodeURIComponent(code)}/start`, {
    method: "POST",
    token,
  });
}

/** 覚えている席の証明。 */
export function loadSeat(): { code: string; token: string } | null {
  const token = localStorage.getItem(TOKEN_KEY);
  const code = localStorage.getItem(CODE_KEY);
  return token && code ? { code, token } : null;
}

export function saveSeat(code: string, token: string): void {
  localStorage.setItem(TOKEN_KEY, token);
  localStorage.setItem(CODE_KEY, code);
}

export function clearSeat(): void {
  localStorage.removeItem(TOKEN_KEY);
  localStorage.removeItem(CODE_KEY);
}
