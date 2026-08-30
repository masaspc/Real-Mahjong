#!/usr/bin/env node
/**
 * 動いている画面を撮る。
 *
 * ```text
 * node tools/shoot.mjs "http://127.0.0.1:8080/?table=x" out.png [待つミリ秒]
 * ```
 *
 * **`--screenshot` と `--virtual-time-budget` では対局の画面が撮れない。**
 * 卓は `requestAnimationFrame` を止めないので仮想時間が尽きず、Chrome は
 * いつまでも終了しない（実測で2分待っても返らない）。`preview.html` に
 * `still=1` を足したのはこの回避で、動く画面には使えない。
 *
 * こちらは DevTools プロトコルで繋いで「今の絵」を要求する。描画ループが
 * 回っていてよい。何秒経った時点かを指定できるので、対局の途中を並べて
 * 見られる。
 *
 * 依存は足していない。Node 22 の組み込み `WebSocket` だけを使う。
 */

import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const [url, out, waitMs = "8000", clipArg] = process.argv.slice(2);
if (url === undefined || out === undefined) {
  console.error(
    "使い方: node tools/shoot.mjs <url> <out.png> [待つミリ秒] [x,y,幅,高さ,倍率]",
  );
  process.exit(2);
}

/**
 * 一部だけを拡大して撮る。
 *
 * **全体像では小さすぎて読めない箇所がある。**上端の帯やボタンの中の牌は
 * 1280x800 の中では数十画素しかなく、化けていても気付けない。切り出して
 * 倍率を上げれば、そこだけを人の目で確かめられる。
 */
function clipOf(text) {
  if (text === undefined) {
    return undefined;
  }
  const [x, y, width, height, scale = "1"] = text.split(",").map(Number);
  return { x, y, width, height, scale };
}

const CHROME =
  process.env["CHROME"] ??
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const PORT = Number(process.env["CDP_PORT"] ?? 9222);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const chrome = spawn(
  CHROME,
  [
    "--headless=new",
    "--disable-gpu",
    // WebGL はソフトウェア実装で動かす。GPU の無い環境でも同じ絵になる。
    "--use-angle=swiftshader",
    "--enable-unsafe-swiftshader",
    "--window-size=1280,800",
    `--remote-debugging-port=${PORT}`,
    "--user-data-dir=/tmp/real-mahjong-shoot",
    "about:blank",
  ],
  { stdio: "ignore" },
);

/** DevTools が口を開けるまで待つ。 */
async function endpoint() {
  for (let i = 0; i < 50; i += 1) {
    try {
      const res = await fetch(`http://127.0.0.1:${PORT}/json/version`);
      return (await res.json()).webSocketDebuggerUrl;
    } catch {
      await sleep(200);
    }
  }
  throw new Error("DevTools に繋がらない");
}

class Cdp {
  #ws;
  #next = 1;
  #pending = new Map();

  constructor(ws) {
    this.#ws = ws;
    ws.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      const resolve = this.#pending.get(message.id);
      if (resolve !== undefined) {
        this.#pending.delete(message.id);
        resolve(message.result);
      }
    });
  }

  static async open(wsUrl) {
    const ws = new WebSocket(wsUrl);
    await new Promise((resolve, reject) => {
      ws.addEventListener("open", resolve, { once: true });
      ws.addEventListener("error", reject, { once: true });
    });
    return new Cdp(ws);
  }

  send(method, params = {}, sessionId) {
    const id = this.#next++;
    return new Promise((resolve) => {
      this.#pending.set(id, resolve);
      this.#ws.send(JSON.stringify({ id, method, params, sessionId }));
    });
  }

  close() {
    this.#ws.close();
  }
}

const browser = await Cdp.open(await endpoint());
const { targetId } = await browser.send("Target.createTarget", { url });
const { sessionId } = await browser.send("Target.attachToTarget", {
  targetId,
  flatten: true,
});

// **撮る前に画面を押せるようにする。**待合のように「状態を作らないと
// 出てこない画面」は、URL を開くだけでは辿り着けない。環境変数
// `SHOOT_EVAL` に JavaScript を渡すと、読み込み直後に一度だけ走らせる。
if (process.env.SHOOT_EVAL) {
  await browser.send(
    "Runtime.evaluate",
    { expression: process.env.SHOOT_EVAL, awaitPromise: true },
    sessionId,
  );
}

await sleep(Number(waitMs));

const clip = clipOf(clipArg);
const { data } = await browser.send(
  "Page.captureScreenshot",
  clip === undefined ? {} : { clip, captureBeyondViewport: true },
  sessionId,
);
writeFileSync(out, Buffer.from(data, "base64"));

// **中身が空でないことを言い切る。**「png が出来た」は検証にならない。
// 一度、真っ白な png を撮って合格にしたことがある。
const bytes = Buffer.from(data, "base64").length;
console.log(`${out} ${bytes} バイト`);

browser.close();
chrome.kill();
// 切り出しは小さいので、下限も小さくする。
process.exit(bytes > (clip === undefined ? 20000 : 2000) ? 0 : 1);
