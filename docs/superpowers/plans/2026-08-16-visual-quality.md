# 見た目の完成度を上げる 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 牌を本物の牌図で描き、操作と情報の障害を取り除き、崩れを機械が検出できるようにする。

**Architecture:** 牌面は Wikimedia Commons の CC0 素材へ差し替え、2D と 3D で同じ素材を使う。取り込みは許諾を機械的に検査するスクリプトで行い、人の目に任せない。崩れの検出は WebGL を経由しない経路に置く。

**Tech Stack:** TypeScript / Vitest / Three.js / Node（取り込みスクリプト）

## Global Constraints

- **許諾の確認を人の目に任せない。**取り込みスクリプトは各ファイルの許諾を検査し、CC0 か PD でなければ中断する
- **牌面の定義を2つ持たない。**2D の盤面と 3D の卓は同じ素材を使う
- **崩れの検出を WebGL に依存させない。**牌の化けは 3D を経由せず検出する
- 実時間を直接読まない。時刻は `Clock` を通す
- 既存の 209 件の試験の期待値を緩めない
- コミットは対象を明示する。`git add -A` は使わない

---

### Task 1: 牌の側面と背の描画を直す

**牌を作り直したときに、側面と背が壊れた。**撮影して初めて分かった。伏せ牌が
黒い塊になり、山の側面が白い筋を引いている。

原因は UV の割り当てである。`paint()` は頂点の x/y をそのままセル内の位置へ
写すが、**側面と面取りの頂点は牌の縁にあるため、セルの端の画素が厚み方向へ
引き伸ばされる。**牌体のセルは無地なので、本来は1点を指せば足りる。

**Files:**
- Modify: `apps/web/src/scene/tile-geometry.ts`
- Modify: `apps/web/src/scene/tile-geometry.test.ts`

**Interfaces:**
- Produces: 変更なし（`createTileGeometry` `applyFaceUv` の形は同じ）

- [ ] **Step 1: 失敗する試験を書く**

`tile-geometry.test.ts` の `describe("createTileGeometry")` の中へ足す。

```ts
  it("側面は引き伸ばさず、1点の色で塗る", () => {
    // **側面の頂点をセル全体へ広げると、縁の画素が厚み方向へ伸びて筋になる。**
    // 牌体のセルは無地なので、すべて同じ1点を指せばよい。
    const geometry = createTileGeometry();
    const position = geometry.getAttribute("position");
    const uv = geometry.getAttribute("uv");
    const half = TILE.depth / 2;

    const sides: number[] = [];
    for (let i = 0; i < position.count; i += 1) {
      const z = position.getZ(i);
      if (z <= half - 0.07 && z >= -half + 0.07) {
        sides.push(i);
      }
    }
    expect(sides.length).toBeGreaterThan(0);

    const first = sides[0]!;
    for (const i of sides) {
      expect(uv.getX(i)).toBeCloseTo(uv.getX(first), 6);
      expect(uv.getY(i)).toBeCloseTo(uv.getY(first), 6);
    }
  });
```

- [ ] **Step 2: 試験が落ちることを確かめる**

Run: `pnpm --dir apps/web test src/scene/tile-geometry.test.ts`
Expected: FAIL（側面の UV がばらついている）

- [ ] **Step 3: 実装する**

`tile-geometry.ts` に、1点だけを指す塗り方を足し、側面に使う。

```ts
/**
 * 指定した頂点を、セルの中心1点へ寄せる。
 *
 * **無地の面を引き伸ばさない。**牌体のセルは一様なので、位置に応じて
 * 広げるとセルの縁の画素を拾い、厚み方向へ筋が出る。
 */
function paintFlat(
  geometry: BufferGeometry,
  indices: number[],
  cell: number,
): void {
  const uv = geometry.getAttribute("uv");
  if (uv === undefined) {
    throw new Error("uv 属性を持たないジオメトリには適用できない");
  }
  const { u, v, du, dv } = uvOffsetOf(cell);
  for (const i of indices) {
    uv.setXY(i, u + du / 2, v + dv / 2);
  }
  uv.needsUpdate = true;
}
```

`createTileGeometry` の中で、側面だけをこれに替える。

```ts
  paintFlat(geometry, side, BODY_INDEX);
  paint(geometry, back, BACK_INDEX);
  paint(geometry, front, BODY_INDEX);
```

- [ ] **Step 4: 試験が通ることを確かめる**

このタスクで足した1件だけを見る。

Run: `pnpm --dir apps/web test src/scene/tile-geometry.test.ts -t 側面`
Expected: 1 passed

ファイル全体と、全体の退行も見る。

Run: `pnpm --dir apps/web test src/scene/tile-geometry.test.ts`
Expected: 6 passed

Run: `pnpm --dir apps/web test`
Expected: 210 passed

- [ ] **Step 5: コミット**

```bash
git add apps/web/src/scene/tile-geometry.ts apps/web/src/scene/tile-geometry.test.ts
git commit -m "fix(web): 牌の側面が筋を引くのを直す

側面の頂点をセル全体へ広げていたため、無地のはずの牌体で縁の画素が厚み方向へ
引き伸ばされていた。**撮影して初めて分かった。**セルの中心1点へ寄せる。"
```

---

### Task 2: 1コマだけ描いて止まる口を設ける

**撮影のたびに数分かかるのは、描画ループが仮想時間ぶん回り続けるからである。**
`?still=1` を与えたら1コマ描いて止まるようにする。見本の撮影が速く、かつ
毎回同じ絵になる。

**Files:**
- Modify: `apps/web/src/preview.ts`

**Interfaces:**
- Produces: `preview.html?still=1` で描画ループが1コマで止まる

- [ ] **Step 1: 実装する**

`preview.ts` の描画ループを次のようにする。

```ts
/**
 * `?still=1` なら1コマだけ描いて止める。
 *
 * **撮影のために回し続けない。**仮想時間で撮ると、止まらないループは
 * 千コマ単位で描き直され、ソフトウェア描画では終わらない。
 */
const still = new URLSearchParams(location.search).get("still") === "1";

function frame(): void {
  scene.render();
  if (!still) {
    requestAnimationFrame(frame);
  }
}
requestAnimationFrame(frame);
```

- [ ] **Step 2: 型検査とビルドが通ることを確かめる**

Run: `pnpm --dir apps/web typecheck && pnpm --dir apps/web build`
Expected: エラー0件でビルド成功

- [ ] **Step 3: 撮影が速くなることを確かめる**

Run: サーバを起動して
```bash
OUT=$(mktemp -d)
time "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless=new --disable-gpu --use-angle=swiftshader --enable-unsafe-swiftshader \
  --window-size=1280,720 --virtual-time-budget=8000 \
  --screenshot="$OUT/still.png" "http://127.0.0.1:8080/preview.html?viewer=0&still=1"
echo "$OUT"
```
Expected: png が書き出され、**60秒以内に終わる。**終わらなければ失敗

- [ ] **Step 4: コミット**

```bash
git add apps/web/src/preview.ts
git commit -m "feat(web): 1コマだけ描いて止まる口を設ける

撮影のために描画ループを回し続けない。仮想時間で撮ると、止まらないループは
千コマ単位で描き直され、ソフトウェア描画では終わらない。"
```

---

### Task 3: 操作と情報の障害を洗い出す

**素材を直す前に、使えない箇所を潰す。**利用者は「絵」「手応え」「操作の
分かりにくさ」「細部の粗さ」の4つすべてを不満に挙げている。絵だけ直しても
使えなさは残る。

**Files:**
- Create: `docs/superpowers/notes/2026-08-16-playthrough.md`

**Interfaces:**
- Produces: 障害の一覧（次のタスクで直す対象）

- [ ] **Step 1: 半荘を通して記録する**

サーバを起動し、`http://127.0.0.1:8080/` で半荘を1回打つ。**次の観点で、
気付いたことをすべて書き出す。**直す判断はここではしない。

- 何を選べるのかが画面から分かるか（鳴き・リーチ・ツモ・流局）
- 何が起きたのかが分かるか（誰が何を切った、誰が鳴いた、点数がどう動いた）
- 押したものが押せたと分かるか
- 待たされている間、何を待っているのか分かるか
- 局が変わったこと、半荘が終わったことが分かるか

- [ ] **Step 2: 一覧にする**

`docs/superpowers/notes/2026-08-16-playthrough.md` に、次の形で書く。

```markdown
| # | 起きたこと | 何が困るか | 直す/直さない | 理由 |
|---|---|---|---|---|
| 1 | ... | ... | 直す | ... |
```

**「直さない」を選ぶときは理由を書く。**後のウェーブへ送るのか、仕様として
正しいのかを区別する。

- [ ] **Step 3: コミット**

```bash
git add docs/superpowers/notes/2026-08-16-playthrough.md
git commit -m "docs: 半荘を通して操作と情報の障害を洗い出す

絵だけ直しても使えなさは残る。素材へ手を付ける前に、何が分からないかを
一覧にする。"
```

---

### Task 4: 洗い出した障害を直す

Task 3 の一覧のうち「直す」としたものを直す。**一覧に無いものは直さない。**
範囲が膨らむと、この計画のどこで終わるのかが決まらなくなる。

**Files:**
- Modify: `apps/web/src/ui/board.ts`
- Modify: `apps/web/src/ui/board.css`
- Modify: `docs/superpowers/notes/2026-08-16-playthrough.md`

**Interfaces:**
- Consumes: Task 3 の一覧

- [ ] **Step 1: 直す**

一覧の上から順に直す。**1件直すごとに、一覧の該当行へ「直した」と印を付ける。**

- [ ] **Step 2: 既存の試験が壊れていないことを確かめる**

Run: `pnpm --dir apps/web test`
Expected: 210 passed

Run: `pnpm --dir apps/web typecheck && pnpm --dir apps/web build`
Expected: エラー0件でビルド成功

- [ ] **Step 3: コミット**

```bash
git add apps/web/src/ui/board.ts apps/web/src/ui/board.css docs/superpowers/notes/2026-08-16-playthrough.md
git commit -m "fix(web): 半荘で見つけた操作と情報の障害を直す"
```

---

### Task 5: 牌図を取り込むスクリプト

**許諾の確認を人の目に任せない。**設計の初版は、系統の違うファイルを一括で
「パブリックドメイン」と書いて誤った。実際には CC BY-SA 4.0 のものが混ざって
いた。**同じ誤りを二度と起こさないため、機械が検査する。**

**Files:**
- Create: `tools/fetch_tiles.mjs`
- Create: `apps/web/src/assets/tiles/CREDITS.md`（スクリプトが生成する）

**Interfaces:**
- Produces: `apps/web/src/assets/tiles/U+1F0XX.svg` 34件と `CREDITS.md`

- [ ] **Step 1: スクリプトを書く**

`tools/fetch_tiles.mjs`：

```js
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
```

- [ ] **Step 2: 実行する**

Run: `node tools/fetch_tiles.mjs`
Expected: 「34 件」と表示され、`apps/web/src/assets/tiles/` に 34 個の svg と
`CREDITS.md` ができる。**1件でも許諾が合わなければ例外で止まる**

- [ ] **Step 3: 許諾がすべて許可内であることを確かめる**

Run: `grep -c "cc0\|public domain" apps/web/src/assets/tiles/CREDITS.md`
Expected: 34

- [ ] **Step 4: 検査が本当に効くことを確かめる**

`ALLOWED` を一時的に `["絶対に一致しない"]` へ変え、実行して止まることを見る。
**確かめたら必ず元へ戻す。**

Run: `node tools/fetch_tiles.mjs`
Expected: 「許諾が許可外」で異常終了する

- [ ] **Step 5: コミット**

```bash
git add tools/fetch_tiles.mjs apps/web/src/assets/tiles
git commit -m "feat: 牌図を許諾を検査しながら取り込む

**許諾の確認を人の目に任せない。**設計の初版は系統の違うファイルを一括で
パブリックドメインと書いて誤り、実際には CC BY-SA 4.0 が混ざっていた。
CC0 か PD 以外は取り込まず、寸法が違っても止める。"
```

---

### Task 6: 牌面を取り込んだ素材へ差し替える

**2D と 3D で同じ素材を使う。**`face-atlas.ts` は「牌の面は `ui/tile-face.ts` が
唯一の定義である」と明記している。3D だけ差し替えると、鳴きのボタン（2D）と
卓の牌（3D）で白や一索の見た目が食い違う。

**Files:**
- Modify: `apps/web/src/ui/tile-face.ts`
- Modify: `apps/web/src/ui/tile-face.test.ts`

**Interfaces:**
- Consumes: `apps/web/src/assets/tiles/*.svg`（Task 5）
- Produces: `tileFaceSvg(tile)` が取り込んだ素材を返す（形は変えない）

- [ ] **Step 1: 素材を読み込む対応表を作る**

`tile-face.ts` の先頭で、34種を `?raw` で読み込む。**通信を増やさない。**

```ts
import east from "../assets/tiles/east.svg?raw";
import south from "../assets/tiles/south.svg?raw";
// ...34件すべて

/** 符号 0..=33 の順に並べる。0-8=萬 9-17=筒 18-26=索 27-33=字。 */
const SOURCES: readonly string[] = [
  m1, m2, m3, m4, m5, m6, m7, m8, m9,
  p1, p2, p3, p4, p5, p6, p7, p8, p9,
  s1, s2, s3, s4, s5, s6, s7, s8, s9,
  east, south, west, north, white, green, red,
];
```

- [ ] **Step 2: `tileFaceSvg` を差し替える**

```ts
export function tileFaceSvg(tile: Tile): string {
  const kind = kindOf(tile);
  const source = SOURCES[kind];
  if (source === undefined) {
    throw new Error(`牌の符号が範囲外: ${tile}`);
  }
  return isRed(tile) ? reddened(source) : source;
}
```

`reddened` は**このタスクで素通しとして定義する。**Task 7 で中身を入れる。

```ts
/** 赤ドラの絵。**Task 7 で素材の中身を読んでから実装する。** */
function reddened(source: string): string {
  return source;
}
```

**呼ぶ側だけ先に書いて関数を後のタスクへ回さない。**コンパイルが通らず、
このタスク単体で完了できなくなる。Task 7 までは赤ドラが通常の五と同じ絵に
なることを受け入れる。

- [ ] **Step 3: 試験を新しい形へ直す**

`tile-face.test.ts` の、自前描画の中身（円の数、竹の数、白の枠）を見ている
試験は成り立たなくなる。**次の形へ置き換える。**

```ts
  it("34種すべてに素材がある", () => {
    for (let kind = 0; kind < 34; kind += 1) {
      expect(tileFaceSvg(kind).length).toBeGreaterThan(0);
    }
  });

  it("種類ごとに違う絵である", () => {
    const seen = new Set<string>();
    for (let kind = 0; kind < 34; kind += 1) {
      seen.add(tileFaceSvg(kind));
    }
    // **34種が34通りでなければ、どれかが取り違えられている。**
    expect(seen.size).toBe(34);
  });

  it("範囲外は拒む", () => {
    expect(() => tileFaceSvg(99)).toThrow();
  });
```

- [ ] **Step 4: 試験が通ることを確かめる**

Run: `pnpm --dir apps/web test src/ui/tile-face.test.ts`
Expected: 3 passed

Run: `pnpm --dir apps/web test`
Expected: 203 passed

（`tile-face.test.ts` は現在10件ある。自前描画の中身を見る試験が消えて3件に
なるため、全体は 210 から 203 へ減る）

- [ ] **Step 5: コミット**

```bash
git add apps/web/src/ui/tile-face.ts apps/web/src/ui/tile-face.test.ts
git commit -m "feat(web): 牌面を取り込んだ牌図へ差し替える

2D と 3D で同じ素材を使う。face-atlas.ts が「牌の面は tile-face.ts が唯一の
定義である」と書いているとおり、片方だけ差し替えると見た目が食い違う。"
```

---

### Task 7: 赤ドラを作る

**色の一括置換はしない。**SVG には複数の `fill`/`stroke`、埋め込みの style が
あり得る。中身を確かめずに置換すると、背景まで赤くなるか、何も変わらない。

**Files:**
- Modify: `apps/web/src/ui/tile-face.ts`
- Modify: `apps/web/src/ui/tile-face.test.ts`

**Interfaces:**
- Produces: `tileFaceSvg` が赤ドラ（34/35/36）で通常の五と違う絵を返す

- [ ] **Step 1: 素材の中身を読む**

```bash
head -20 apps/web/src/assets/tiles/m5.svg
head -20 apps/web/src/assets/tiles/p5.svg
head -20 apps/web/src/assets/tiles/s5.svg
```

**色を持つ要素を特定してから置換規則を書く。**3ファイルで構造が違う場合は
ファイルごとに規則を分ける。

- [ ] **Step 2: 試験を書く**

```ts
  it("赤ドラは通常の五と違う絵になる", () => {
    expect(tileFaceSvg(34)).not.toEqual(tileFaceSvg(4));
    expect(tileFaceSvg(35)).not.toEqual(tileFaceSvg(13));
    expect(tileFaceSvg(36)).not.toEqual(tileFaceSvg(22));
  });

  it("赤ドラでも牌の形は保たれる", () => {
    // **背景まで赤く塗ると牌に見えない。**元と同じ長さの範囲に収まることで、
    // 丸ごと置き換えていないことを見る。
    for (const [red, plain] of [[34, 4], [35, 13], [36, 22]] as const) {
      const a = tileFaceSvg(red).length;
      const b = tileFaceSvg(plain).length;
      expect(Math.abs(a - b)).toBeLessThan(b * 0.2);
    }
  });
```

- [ ] **Step 3: 実装する**

Step 1 で読んだ結果に基づいて `reddened` を書く。**素材の実際の色を見てから
書くこと。**推測で色名を決めない。

- [ ] **Step 4: 試験が通ることを確かめる**

このタスクで足した2件だけを見る。

Run: `pnpm --dir apps/web test src/ui/tile-face.test.ts -t 赤ドラ`
Expected: 2 passed

ファイル全体と、全体の退行も見る。

Run: `pnpm --dir apps/web test src/ui/tile-face.test.ts`
Expected: 5 passed

Run: `pnpm --dir apps/web test`
Expected: 205 passed

- [ ] **Step 5: コミット**

```bash
git add apps/web/src/ui/tile-face.ts apps/web/src/ui/tile-face.test.ts
git commit -m "feat(web): 赤ドラを素材から作る

色の一括置換はしない。3ファイルの中身を読み、色を持つ要素を特定してから
置換規則を書いた。背景まで赤くならないことを試験で固定する。"
```

---

### Task 8: 牌姿一覧で崩れを検出する

**牌の化けを WebGL 抜きで捕まえる。**3D の撮影に依存させない。34種＋赤ドラ3種
＋裏面の対応を記録し、入れ替わったら落とす。

**Files:**
- Create: `apps/web/src/ui/tile-face.snapshot.test.ts`
- Create: `apps/web/src/ui/__snapshots__/tile-face.snapshot.test.ts.snap`（試験が生成する）

**Interfaces:**
- Consumes: `tileFaceSvg`（Task 6・7）

- [ ] **Step 1: 試験を書く**

```ts
import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";

import { tileFaceSvg, tileBackSvg } from "./tile-face";

/**
 * 牌ごとの絵を短い印にする。
 *
 * **絵そのものを見本にすると差分が読めない。**印だけを見本にすれば、
 * 入れ替わりや取り違えは検出でき、履歴も膨らまない。
 */
function mark(svg: string): string {
  return createHash("sha256").update(svg).digest("hex").slice(0, 12);
}

describe("牌姿一覧", () => {
  it("37種と裏面の絵が、承認された対応から動いていない", () => {
    const sheet: Record<string, string> = {};
    for (let tile = 0; tile <= 36; tile += 1) {
      sheet[String(tile)] = mark(tileFaceSvg(tile));
    }
    sheet["back"] = mark(tileBackSvg());
    // **アトラスの行が反転したときも、牌を取り違えたときも、ここが動く。**
    expect(sheet).toMatchSnapshot();
  });
});
```

- [ ] **Step 2: 見本を作る**

Run: `pnpm --dir apps/web test src/ui/tile-face.snapshot.test.ts`
Expected: 1 passed（見本が生成される）

- [ ] **Step 3: 試験が本当に効くことを確かめる**

`tile-face.ts` の `SOURCES` で、`m1` と `m2` の位置を入れ替える。
**確かめたら必ず元へ戻す。**

Run: `pnpm --dir apps/web test src/ui/tile-face.snapshot.test.ts`
Expected: FAIL（見本と一致しない）

戻してから Run: `pnpm --dir apps/web test src/ui/tile-face.snapshot.test.ts`
Expected: 1 passed

- [ ] **Step 4: コミット**

```bash
git add apps/web/src/ui/tile-face.snapshot.test.ts apps/web/src/ui/__snapshots__
git commit -m "test(web): 牌の化けを WebGL 抜きで捕まえる

37種と裏面の絵の印を見本として持つ。**絵そのものを見本にすると差分が読めず
履歴も膨らむ。**印だけなら入れ替わりと取り違えを検出できる。

牌を入れ替える変異で落ちることを確認済み。"
```

---

### Task 9: 光を整える

牌の面取りが光を拾わず、輪郭が出ていない。半球光を足し、既存の2灯を
**足すだけにせず再調整する。**

**Files:**
- Modify: `apps/web/src/scene/table.ts`

**Interfaces:**
- Consumes: なし

- [ ] **Step 1: 実装する**

`table.ts` の明かりを次のようにする。

```ts
    // **足すだけでは明るさが飽和する。**環境光を下げ、上からの明かりと
    // 卓面からの照り返しを半球光へ移す。
    this.#scene.add(new AmbientLight(0xffffff, 0.35));
    this.#scene.add(new HemisphereLight(0xfff6e0, 0x2f5d43, 0.85));
    const key = new DirectionalLight(0xffffff, 0.9);
```

`HemisphereLight` を `three` の import に足す。

- [ ] **Step 2: 型検査とビルドが通ることを確かめる**

Run: `pnpm --dir apps/web typecheck && pnpm --dir apps/web build`
Expected: エラー0件でビルド成功

Run: `pnpm --dir apps/web test`
Expected: 206 passed

- [ ] **Step 3: 絵を撮る（合否判定には使わない）**

```bash
OUT=$(mktemp -d)
cargo run -p server --bin serve &
SERVER=$!
trap 'kill $SERVER 2>/dev/null' EXIT
sleep 5
for v in 0 2; do
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless=new --disable-gpu --use-angle=swiftshader --enable-unsafe-swiftshader \
  --window-size=1280,720 --virtual-time-budget=8000 \
  --screenshot="$OUT/light-$v.png" "http://127.0.0.1:8080/preview.html?viewer=$v&still=1"
done
echo "$OUT"
```

Expected: 2枚の png が書き出され、`git status --porcelain` が空である。
**明るさの良し悪しは判定しない。**

- [ ] **Step 4: コミット**

```bash
git add apps/web/src/scene/table.ts
git commit -m "feat(web): 半球光を入れて牌の輪郭を出す

面取りが光を拾わず輪郭が出ていなかった。**足すだけでは飽和する**ので、
環境光を下げて上からの明かりと卓面からの照り返しを半球光へ移した。"
```

---

## Self-Review

**仕様の網羅:**

| 仕様の節 | 対応するタスク |
|---|---|
| 2. 順序（操作・情報を先に） | Task 3, 4 |
| 3. 素材の出所と許諾検査 | Task 5 |
| 3. 赤ドラ | Task 7 |
| 3. 2D と 3D の一致 | Task 6 |
| 4. 光 | Task 9 |
| 5. 牌姿一覧を WebGL 抜きで | Task 8 |
| 5. 撮影経路（1コマ描画） | Task 2 |

仕様が「やらない」とした粗さの塗り分け・接地の影・音・キャラは、タスクに無い。

**このウェーブに入っていないもの:** 3D 盤面の画素比較。Task 2 で撮影が速く
なった段階で改めて判断する。**壊れている経路の上に判定を積まない。**

**認められた妥協:**

- 牌姿一覧の見本は絵ではなく印である。**絵の見本は差分が読めず履歴が膨らむ。**
  絵としての良し悪しは人間が撮って見る
- 赤ドラは Task 6 の時点では通常の五と同じ絵になる。Task 7 で分かれる

**型の整合:** `tileFaceSvg(tile: Tile): string` と `tileBackSvg(): string` の形は
変えない。`createTileGeometry` `applyFaceUv` も変えない。呼び出し側（`face-atlas.ts`、
`board.ts`）に手を入れずに済む。

**確認済みの事実:** 34件のファイル名は Commons の API で実際に引いて確かめた
（`U+1F000 MJEastwind.svg` 〜 `U+1F021 MJ9bing.svg`）。`U+1F002_MJWestwind.svg` の
許諾が CC0・寸法 75x95 であること、`0401東風.svg` が CC BY-SA 4.0・140x200 で
あることも個別に確かめた。

## 人間に上げること

- 牌図に差し替えた牌が、麻雀牌として通るか
- 光を整えた卓の明るさが妥当か
- Task 3 の一覧のうち「直さない」とした項目の判断が妥当か
