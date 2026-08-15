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

/** 許すのはこれだけ。**増やすときは人が判断する。** */
const ALLOWED = ["cc0", "pd", "public domain"];

async function categoryFiles() {
  const url = `${API}?action=query&list=categorymembers`
    + `&cmtitle=${encodeURIComponent("Category:SVG Planar illustrations of Mahjong tiles")}`
    + `&cmlimit=500&cmtype=file&format=json&origin=*`;
  const json = await (await fetch(url)).json();
  return json.query.categorymembers.map((m) => m.title);
}

async function info(title) {
  const url = `${API}?action=query&titles=${encodeURIComponent(title)}`
    + `&prop=imageinfo&iiprop=url|size|extmetadata&format=json&origin=*`;
  const json = await (await fetch(url)).json();
  const page = Object.values(json.query.pages)[0];
  const ii = page.imageinfo[0];
  const meta = ii.extmetadata ?? {};
  return {
    url: ii.url,
    width: ii.width,
    height: ii.height,
    license: (meta.LicenseShortName?.value ?? "").toLowerCase(),
    author: (meta.Artist?.value ?? "").replace(/<[^>]*>/g, "").trim(),
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

  // **ここで止める。**許諾が合わないものを1件でも通したら意味がない。
  if (!ALLOWED.some((ok) => meta.license.includes(ok))) {
    throw new Error(`${title} の許諾が許可外: ${meta.license}`);
  }
  if (meta.width !== 75 || meta.height !== 95) {
    throw new Error(`${title} の寸法が違う: ${meta.width}x${meta.height}`);
  }

  const svg = await (await fetch(meta.url)).text();
  await writeFile(`apps/web/src/assets/tiles/${name}.svg`, svg);
  credits.push(`| ${name}.svg | ${title.replace("File:", "")} | ${meta.author} | ${meta.license} | ${meta.width}x${meta.height} |`);
  console.log(`取り込み: ${name}.svg  ${meta.license}`);
}

await writeFile(
  "apps/web/src/assets/tiles/CREDITS.md",
  `# 牌図の出所\n\n取得日: ${new Date().toISOString().slice(0, 10)}\n`
  + `出所: Wikimedia Commons\n\n`
  + `| 保存名 | 元ファイル | 作者 | 許諾 | 寸法 |\n|---|---|---|---|---|\n`
  + credits.join("\n") + "\n",
);
console.log(`${credits.length} 件`);
