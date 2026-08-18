// Wikimedia Commons から牌図を取り込む。
//
// **許諾を機械で検査する。**人が目で見て「パブリックドメイン」と書くと、
// 系統の違うファイルが混ざったときに気付けない。実際に一度誤った。

import { writeFile, mkdir } from "node:fs/promises";

const API = "https://commons.wikimedia.org/w/api.php";

/** Unicode の符号位置と牌の対応。34種。 */
const TILES = [
  ["1F000", "east"], ["1F001", "south"], ["1F002", "west"], ["1F003", "north"],
  ["1F004", "red"], ["1F005", "green"], ["1F006", "white"],
  ["1F007", "m1"], ["1F008", "m2"], ["1F009", "m3"], ["1F00A", "m4"],
  ["1F00B", "m5"], ["1F00C", "m6"], ["1F00D", "m7"], ["1F00E", "m8"],
  ["1F00F", "m9"],
  ["1F010", "s1"], ["1F011", "s2"], ["1F012", "s3"], ["1F013", "s4"],
  ["1F014", "s5"], ["1F015", "s6"], ["1F016", "s7"], ["1F017", "s8"],
  ["1F018", "s9"],
  ["1F019", "p1"], ["1F01A", "p2"], ["1F01B", "p3"], ["1F01C", "p4"],
  ["1F01D", "p5"], ["1F01E", "p6"], ["1F01F", "p7"], ["1F020", "p8"],
  ["1F021", "p9"],
];

/**
 * 許すのはこれだけ。**増やすときは人が判断する。**
 *
 * gpl を含めるのは人の判断による。File:U+1F000 MJEastwind.svg（東風）だけが
 * GPL のみで、同じ作者の他の牌が持つ PD の記載を欠いている。**配布するときは
 * GPL の義務が生じる。**手元で遊ぶ間は問題にならないが、公開する前に見直すこと。
 */
const ALLOWED = ["cc0", "pd", "public domain", "gpl"];

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Wikimedia は連続リクエストをレート制限する（"You are making too many
 * requests" というプレーンテキストを返し、JSON として壊れる）。値や許諾判定
 * ロジックには関係ない技術的な再試行なので、ここだけリトライを入れる。
 */
async function fetchWithRetry(url, parse) {
  const MAX_ATTEMPTS = 10;
  for (let attempt = 0; attempt < MAX_ATTEMPTS; attempt++) {
    const res = await fetch(url, {
      headers: {
        // Wikimedia のベストプラクティスに従い、識別可能な User-Agent を送る。
        "User-Agent": "RealMahjongTileFetch/1.0 (local dev script; contact: masaxeon@gmail.com)",
      },
    });
    const text = await res.text();
    // **状態行を見ずに本文を解釈しない。**404 や 500 の本文は HTML なので
    // `JSON.parse` が投げ、リトライの側に落ちる。10回ぶん待ってから
    // 「リトライしても取得できない」とだけ言うので、URL の綴り間違いなのか
    // 混雑なのかが分からない。ここで切り分ける。
    if (!res.ok) {
      // 429（過負荷）と 5xx は待てば直る。それ以外は待っても直らない。
      if (res.status !== 429 && res.status < 500) {
        throw new Error(`取得できない (HTTP ${res.status}): ${url}`);
      }
      if (attempt === MAX_ATTEMPTS - 1) {
        throw new Error(`リトライしても取得できない (HTTP ${res.status}): ${url}`);
      }
      await sleep(Math.min(3000 * (attempt + 1), 30000));
      continue;
    }
    try {
      return parse(text);
    } catch {
      if (attempt === MAX_ATTEMPTS - 1) {
        throw new Error(`リトライしても取得できない: ${url}\n${text.slice(0, 200)}`);
      }
      await sleep(Math.min(3000 * (attempt + 1), 30000));
    }
  }
}

/**
 * 作者名を1行に均す。
 *
 * Commons の `Artist` は HTML であり、作者名のほかに Inkscape の定型文が
 * 続く版がある。タグを剥がしただけだと
 * 「Shizhao  This W3C-unspecified vector image was created with Inkscape .」
 * のような行が `CREDITS.md` の表に入る。**`|` が混ざると表そのものが崩れる**
 * ので、ここで潰しておく。
 */
function cleanAuthor(html) {
  return html
    .replace(/<[^>]*>/g, " ")
    .replace(/This [^.]*vector image was created with [^.]*\./gi, "")
    .replace(/\|/g, "/")
    .replace(/\s+/g, " ")
    .trim();
}

async function categoryFiles() {
  const url = `${API}?action=query&list=categorymembers`
    + `&cmtitle=${encodeURIComponent("Category:SVG Planar illustrations of Mahjong tiles")}`
    + `&cmlimit=500&cmtype=file&format=json&origin=*`;
  const json = await fetchWithRetry(url, JSON.parse);
  return json.query.categorymembers.map((m) => m.title);
}

async function info(title) {
  const url = `${API}?action=query&titles=${encodeURIComponent(title)}`
    + `&prop=imageinfo&iiprop=url|size|extmetadata&format=json&origin=*`;
  const json = await fetchWithRetry(url, JSON.parse);
  const page = Object.values(json.query.pages)[0];
  const ii = page.imageinfo[0];
  const meta = ii.extmetadata ?? {};
  return {
    url: ii.url,
    width: ii.width,
    height: ii.height,
    license: (meta.LicenseShortName?.value ?? "").toLowerCase(),
    author: cleanAuthor(meta.Artist?.value ?? ""),
  };
}

const titles = await categoryFiles();
await mkdir("apps/web/src/assets/tiles", { recursive: true });
const credits = [];

for (const [code, name] of TILES) {
  const title = titles.find((t) => t.startsWith(`File:U+${code} `));
  if (!title) {
    throw new Error(`U+${code} のファイルが見つからない`);
  }
  const meta = await info(title);
  await sleep(1000);

  // **ここで止める。**許諾が合わないものを1件でも通したら意味がない。
  if (!ALLOWED.some((ok) => meta.license.includes(ok))) {
    throw new Error(`${title} の許諾が許可外: ${meta.license}`);
  }
  if (meta.width !== 75 || meta.height !== 95) {
    throw new Error(`${title} の寸法が違う: ${meta.width}x${meta.height}`);
  }

  const svg = await fetchWithRetry(meta.url, (text) => {
    if (!text.includes("<svg")) {
      throw new Error("svg でない応答");
    }
    return text;
  });
  await writeFile(`apps/web/src/assets/tiles/${name}.svg`, svg);
  credits.push(`| ${name}.svg | ${title.replace("File:", "")} | ${meta.author} | ${meta.license} | ${meta.width}x${meta.height} |`);
  console.log(`取り込み: ${name}.svg  ${meta.license}`);
  await sleep(1500);
}

await writeFile(
  "apps/web/src/assets/tiles/CREDITS.md",
  `# 牌図の出所\n\n取得日: ${new Date().toISOString().slice(0, 10)}\n`
  + `出所: Wikimedia Commons\n\n`
  + `**注意: east.svg（東風）のみ GPL である。**他は PD か CC0。配布するときは GPL の義務が生じる。\n\n`
  + `| 保存名 | 元ファイル | 作者 | 許諾 | 寸法 |\n|---|---|---|---|---|\n`
  + credits.join("\n") + "\n",
);
console.log(`${credits.length} 件`);
