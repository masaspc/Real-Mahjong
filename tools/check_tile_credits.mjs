#!/usr/bin/env node
/**
 * 牌図の許諾を機械で確かめる。
 *
 * **許諾の確認を人の目に任せない。**このウェーブで二度誤った。一度目は系統の
 * 違う CC BY-SA のファイルを「パブリックドメイン」と一括で書いた。二度目は
 * 東風だけが GPL であることを見落とした（取り込みスクリプトの検査が捕まえた）。
 *
 * 取り込みは一度きりだが、確認は毎回できる。`CREDITS.md` の表を読み、
 *
 * - 表に書かれた牌図がすべて実在するか
 * - 実在する牌図がすべて表に載っているか
 * - 許諾が許可済みの一覧に入っているか
 * - 寸法が 75x95 で揃っているか
 *
 * を見る。ひとつでも外れたら 1 で終わる。
 */

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const DIR = "apps/web/src/assets/tiles";
const EXPECTED = 34;
const SIZE = "75x95";

/**
 * 許可済みの許諾。
 *
 * **`gpl` は人間が個別に承認したものである。**東風（`east.svg`）だけが
 * GPL でしか手に入らなかった。他の系統を混ぜる判断はここを書き換える人が
 * 負う。勝手に増やさない。
 */
const ALLOWED = ["cc0", "pd", "public domain", "gpl"];

function fail(message) {
  console.error(`牌図の許諾: ${message}`);
  process.exitCode = 1;
}

const credits = readFileSync(join(DIR, "CREDITS.md"), "utf8");

/** 表の行だけを拾う。見出しと区切りは列の中身で弾ける。 */
const rows = credits
  .split("\n")
  .filter((line) => line.startsWith("|"))
  .map((line) =>
    line
      .split("|")
      .slice(1, -1)
      .map((cell) => cell.trim()),
  )
  .filter((cells) => cells.length === 5 && cells[0].endsWith(".svg"));

if (rows.length !== EXPECTED) {
  fail(`表の行が ${EXPECTED} でない: ${rows.length}`);
}

const listed = new Set();
for (const [name, , , license, size] of rows) {
  listed.add(name);
  const normalized = license.toLowerCase();
  if (!ALLOWED.some((ok) => normalized.includes(ok))) {
    fail(`${name} の許諾が許可外: ${license}`);
  }
  if (size !== SIZE) {
    fail(`${name} の寸法が ${SIZE} でない: ${size}`);
  }
}

const present = new Set(
  readdirSync(DIR).filter((name) => name.endsWith(".svg")),
);

for (const name of listed) {
  if (!present.has(name)) {
    fail(`${name} が表にあるが実体が無い`);
  }
}
for (const name of present) {
  if (!listed.has(name)) {
    fail(`${name} が実体としてあるが表に無い`);
  }
}

if (process.exitCode === undefined || process.exitCode === 0) {
  console.log(`牌図の許諾: ${rows.length} 件すべて確認`);
}
