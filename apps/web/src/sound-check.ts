import { Sfx } from "./audio/sfx";
import type { SoundName } from "./audio/catalog";

/**
 * 合成音を焼いて、振幅と長さを測る。
 *
 * **「例外が出なかった」は「音が出た」ではない。**利得の傾きを1つ間違えると
 * 無音のまま静かに走る。絵で何度もやられた失敗なので、音でも出力そのものを
 * 見る。波形も描くので、目でも確かめられる。
 */

const NAMES: SoundName[] = [
  "clack",
  "draw",
  "call",
  "riichi",
  "dora",
  "agari",
  "ryuukyoku",
];

/** 無音とみなす振幅。ここを下回ったら鳴っていない。 */
const SILENT = 0.01;

const RATE = 44_100;
const SECONDS = 1.2;

async function measure(name: SoundName): Promise<{
  peak: number;
  tailMs: number;
  data: Float32Array;
}> {
  const context = new OfflineAudioContext(1, RATE * SECONDS, RATE);
  const sfx = new Sfx(context);
  sfx.play(name, 0);
  const rendered = await context.startRendering();
  const data = rendered.getChannelData(0);
  let peak = 0;
  let last = 0;
  for (let i = 0; i < data.length; i += 1) {
    const value = Math.abs(data[i] ?? 0);
    if (value > peak) peak = value;
    if (value > SILENT) last = i;
  }
  return { peak, tailMs: (last / RATE) * 1_000, data };
}

function waveform(data: Float32Array): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = 480;
  canvas.height = 48;
  const ctx = canvas.getContext("2d");
  if (ctx === null) {
    return canvas;
  }
  ctx.strokeStyle = "#f0d34f";
  ctx.beginPath();
  const step = Math.ceil(data.length / canvas.width);
  for (let x = 0; x < canvas.width; x += 1) {
    let peak = 0;
    for (let i = x * step; i < (x + 1) * step && i < data.length; i += 1) {
      peak = Math.max(peak, Math.abs(data[i] ?? 0));
    }
    const height = peak * canvas.height;
    ctx.moveTo(x + 0.5, (canvas.height - height) / 2);
    ctx.lineTo(x + 0.5, (canvas.height + height) / 2);
  }
  ctx.stroke();
  return canvas;
}

const report = document.querySelector("#report");
if (report === null) {
  throw new Error("#report が無い");
}

const table = document.createElement("table");
table.innerHTML =
  "<tr><th>音</th><th>最大振幅</th><th>鳴っている長さ</th><th>波形</th></tr>";
let silent = 0;
for (const name of NAMES) {
  const { peak, tailMs, data } = await measure(name);
  if (peak <= SILENT) {
    silent += 1;
  }
  const row = document.createElement("tr");
  const cells = [name, peak.toFixed(3), `${tailMs.toFixed(0)}ms`];
  for (const [index, text] of cells.entries()) {
    const cell = document.createElement("td");
    cell.textContent = text;
    if (index === 1) {
      cell.className = peak > SILENT ? "ok" : "bad";
    }
    row.append(cell);
  }
  const wave = document.createElement("td");
  wave.append(waveform(data));
  row.append(wave);
  table.append(row);
}

const verdict = document.createElement("p");
verdict.id = "verdict";
verdict.textContent =
  silent === 0
    ? `${NAMES.length}件すべて鳴っている`
    : `無音が ${silent} 件ある`;
report.replaceChildren(verdict, table);
