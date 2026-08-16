import { describe, expect, it } from "vitest";

import { tileFaceSvg, tileBackSvg } from "./tile-face";

/**
 * FNV-1a（32bit）。文字コードを1つずつ XOR してから素数倍する、
 * 定数個の演算で終わる単純なハッシュ。
 *
 * `seed` を変えると独立した32bit値が得られる（`mark` はこれを2回呼んで
 * 衝突しにくくしている）。
 */
function fnv1a(input: string, seed: number): number {
  let hash = seed;
  for (let i = 0; i < input.length; i += 1) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/**
 * 牌ごとの絵を短い印にする。
 *
 * **絵そのものを見本にすると差分が読めない。**印だけを見本にすれば、
 * 入れ替わりや取り違えは検出でき、履歴も膨らまない。
 *
 * **`node:crypto` は使わない。**この web パッケージは `tsconfig.json` で
 * `types: []` を指定しており、Node の型を意図的に持たない（ブラウザ向けの
 * パッケージであるため）。ここに必要なのは「同じ文字列からは必ず同じ印が
 * 出る」ことだけで暗号強度は要らないので、依存を増やさず自前の FNV-1a で
 * 済ませる。32bit 単発では衝突の余地が大きいため、異なるシードで2回かけ、
 * 64bit 相当（16桁の16進）にしている。実際に衝突しないことは下の試験
 * 「38件の印がすべて異なる」で確かめる。
 */
function mark(svg: string): string {
  const a = fnv1a(svg, 0x811c9dc5);
  const b = fnv1a(svg, 0x9e3779b9);
  return a.toString(16).padStart(8, "0") + b.toString(16).padStart(8, "0");
}

describe("牌姿一覧", () => {
  it("37種と裏面の絵が、承認された対応から動いていない", () => {
    const sheet: Record<string, string> = {};
    for (let tile = 0; tile <= 36; tile += 1) {
      sheet[String(tile)] = mark(tileFaceSvg(tile));
    }
    sheet["back"] = mark(tileBackSvg());
    // **この試験が捕まえるのは `SOURCES` の並び違いと取り違え、素材の
    // 差し替えである。**`tileFaceSvg` は `SOURCES` を直接引くだけで、
    // 3D のアトラスの行・列の計算（`scene/atlas.ts` の `uvOffsetOf`）を
    // 経由しない。**よってアトラスの行の反転はここでは捕まえられない**
    // ——それは `scene/atlas.test.ts` の役目である。重く不安定な3D撮影に
    // 依存せず、3Dを経由しないまま牌の対応が保たれているかを見るのが
    // この試験の役割。
    expect(sheet).toMatchSnapshot();
  });

  it("37種と裏面、38件の印がすべて異なる", () => {
    // **弱いハッシュだと衝突し、違う牌が同じ印になりうる。**上の試験は
    // 印だけを見本にしているため、衝突していれば取り違えを見逃す。
    // ここで38件が38通りであることを別途確かめる。
    const marks = new Set<string>();
    for (let tile = 0; tile <= 36; tile += 1) {
      marks.add(mark(tileFaceSvg(tile)));
    }
    marks.add(mark(tileBackSvg()));
    expect(marks.size).toBe(38);
  });
});
