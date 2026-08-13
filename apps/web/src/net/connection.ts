import type { ClientEventEnvelope } from "../protocol/ClientEventEnvelope";
import type { Command } from "../protocol/Command";

/** 接続先の URL を組み立てる。 */
export function buildUrl(base: string, table: string, lastSeq: number | null): string {
  const params = new URLSearchParams({ table });
  // **0 を「無い」と取り違えると、対局の頭から送り直される。**
  if (lastSeq !== null) {
    params.set("last_seq", String(lastSeq));
  }
  return `${base}?${params.toString()}`;
}

export type Connection = {
  send(command: Command): void;
  close(): void;
};

export type Options = {
  base: string;
  table: string;
  /** いま届いている連番。再接続のときに渡す。 */
  lastSeq(): number | null;
  onEvent(envelope: ClientEventEnvelope): void;
  onStatus(text: string): void;
};

/**
 * 繋ぎ、切れたら繋ぎ直す。
 *
 * **切れたら最後に受け取った連番から送り直してもらう。**対局の頭から
 * やり直さないので、再読み込みしても続きから遊べる。
 */
export function connect(options: Options): Connection {
  let socket: WebSocket | null = null;
  let closed = false;
  let retry = 0;

  const open = () => {
    if (closed) {
      return;
    }
    socket = new WebSocket(buildUrl(options.base, options.table, options.lastSeq()));
    socket.onopen = () => {
      retry = 0;
      options.onStatus("接続");
    };
    socket.onmessage = (message) => {
      options.onEvent(JSON.parse(message.data as string) as ClientEventEnvelope);
    };
    socket.onclose = () => {
      if (closed) {
        return;
      }
      retry += 1;
      const wait = Math.min(1000 * retry, 5000);
      options.onStatus(`切断。${wait / 1000}秒後に繋ぎ直します`);
      setTimeout(open, wait);
    };
  };

  open();

  return {
    send(command) {
      if (socket && socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify(command));
      }
    },
    close() {
      closed = true;
      socket?.close();
    },
  };
}
