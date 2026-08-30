/**
 * 牌譜の口を叩く。
 *
 * **証明が2種類ある。**席の証明（`X-Mahjong-Token`）は1対局にしか効かず、
 * browser の鍵（`X-Mahjong-Player`）が対局をまたぐ名札になる。前者で
 * 1件を開き、後者で一覧を引く。
 *
 * 設計は `docs/superpowers/specs/2026-08-30-records-design.md`。
 */

import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import { RoomError } from "../lobby/api";

const TOKEN_HEADER = "X-Mahjong-Token";
const PLAYER_HEADER = "X-Mahjong-Player";
const PLAYER_KEY = "real-mahjong.player";

export type RecordCard = {
  id: string;
  players: string[];
  started_ms: number;
  ended_ms: number | null;
  result: { final_scores: number[]; placements: number[] } | null;
};

export type RecordHead = RecordCard & { you: number };

/**
 * この browser を指す鍵。
 *
 * **アカウントが入るまでの繋ぎである。**`localStorage` を消すと一覧が
 * 消え、別の機械からは見えない。アカウントが入ったら、この鍵を利用者へ
 * 結び直す。
 */
export function playerKey(): string {
  let key = localStorage.getItem(PLAYER_KEY);
  if (!key) {
    key = crypto.randomUUID().replaceAll("-", "");
    localStorage.setItem(PLAYER_KEY, key);
  }
  return key;
}

async function ask(path: string, headers: Record<string, string>): Promise<Response> {
  const response = await fetch(path, { headers });
  if (!response.ok) {
    const payload: unknown = await response.json().catch(() => null);
    const slug =
      payload && typeof payload === "object" && "error" in payload
        ? String((payload as { error: unknown }).error)
        : "unknown";
    throw new RoomError(slug, response.status);
  }
  return response;
}

/** その browser が打った対局。新しい順。 */
export async function listRecords(): Promise<RecordCard[]> {
  const response = await ask("/api/records", { [PLAYER_HEADER]: playerKey() });
  const body = (await response.json()) as { records?: RecordCard[] };
  return body.records ?? [];
}

/**
 * 1対局の見出し。自分がどの席だったかを含む。
 *
 * **鍵だけで開ける。**席の証明は部屋ごとに配られるので、対局が終わって
 * 画面を閉じれば手元に残らない。一覧に出ているのに開けない、では困る。
 */
export async function readRecord(id: string, token?: string): Promise<RecordHead> {
  const response = await ask(`/api/records/${encodeURIComponent(id)}`, credentials(token));
  return (await response.json()) as RecordHead;
}

function credentials(token?: string): Record<string, string> {
  const headers: Record<string, string> = { [PLAYER_HEADER]: playerKey() };
  if (token) headers[TOKEN_HEADER] = token;
  return headers;
}

/**
 * 牌譜の本文。**自分の席の視界に射影されたもの。**
 *
 * JSONL で返る。読めない行は捨てる——1行の壊れで対局全体を諦めない。
 */
export async function recordEvents(
  id: string,
  token?: string,
): Promise<ClientEventEnvelope[]> {
  const response = await ask(
    `/api/records/${encodeURIComponent(id)}/events`,
    credentials(token),
  );
  const text = await response.text();
  const events: ClientEventEnvelope[] = [];
  for (const line of text.split("\n")) {
    if (line.trim() === "") continue;
    try {
      events.push(JSON.parse(line) as ClientEventEnvelope);
    } catch {
      // 読めない行は捨てる。
    }
  }
  return events;
}
