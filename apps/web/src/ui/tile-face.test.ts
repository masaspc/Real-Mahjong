import { describe, expect, it } from "vitest";

import { normalizeTileSvg, tileBackSvg, tileFaceSvg } from "./tile-face";

describe("牌の面を描く", () => {
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

  it("34種すべてに class=\"tile-face\" が付く", () => {
    // **取り込み素材はどれも class を持たない。**`board.css` の
    // `.tile-face { width: 100%; height: auto; ... }` はこのクラスが
    // 付いた要素にしか当たらない。付かないと 2D の盤面で実寸のまま
    // 描かれ、28px の牌の枠からはみ出す。
    for (let kind = 0; kind < 34; kind += 1) {
      expect(tileFaceSvg(kind)).toContain('class="tile-face"');
    }
  });

  it("34種すべてに viewBox が付く", () => {
    // **取り込み素材はどれも viewBox を持たない。**無いと width/height の
    // 実寸でそのまま描かれ、CSS の width:100%/height:auto が効かない。
    for (let kind = 0; kind < 34; kind += 1) {
      expect(tileFaceSvg(kind)).toMatch(/<svg[^>]*\bviewBox="/);
    }
  });

  it("裏向きの牌にも class と viewBox が付く", () => {
    const back = tileBackSvg();
    expect(back).toContain('class="tile-face"');
    expect(back).toMatch(/<svg[^>]*\bviewBox="/);
  });
});

describe("SVG の正規化", () => {
  it("既に class を持つ SVG には属性を足さず、値を混ぜる", () => {
    // **`class="a" class="tile-face"` を作ってはいけない。**属性が2つある
    // 不正な XML になり、ブラウザは後ろを黙って捨てる。寸法の指定だけが
    // 効かなくなるという、最も気付きにくい壊れ方をする。
    const out = normalizeTileSvg(
      '<svg xmlns="http://www.w3.org/2000/svg" class="a" width="10" height="20"></svg>',
    );
    expect(out.match(/\bclass="/g)).toHaveLength(1);
    expect(out).toContain('class="a tile-face"');
  });

  it("既に tile-face を持つ SVG は増やさない", () => {
    const once = normalizeTileSvg(
      '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="20"></svg>',
    );
    expect(normalizeTileSvg(once)).toBe(once);
    expect(once.match(/\bclass="/g)).toHaveLength(1);
  });

  it("取り込んだ34種と裏の class は1つだけ", () => {
    for (let kind = 0; kind < 34; kind += 1) {
      expect(tileFaceSvg(kind).match(/\bclass="/g)).toHaveLength(1);
    }
    expect(tileBackSvg().match(/\bclass="/g)).toHaveLength(1);
  });
});

describe("牌の色", () => {
  it("37種のどれにも黒の指定が残らない", () => {
    // **書き方が3通りある素材を、1通りだけ直して済ませない。**
    // `fill:#000000` だけを見ていたときは九筒（`stroke="#000"` の線画）
    // だけが黒いまま残り、筒子の中で1枚だけ色が違っていた。
    // 東南西北は黒だが `#1c1c1c` であり、素材の生の黒とは別物である。
    for (let tile = 0; tile <= 36; tile += 1) {
      const svg = tileFaceSvg(tile);
      expect(svg, `${tile} に生の黒が残っている`).not.toMatch(
        /#000000\b|#000"|:\s*#000\b/,
      );
    }
  });

  it("種類ごとに違う色で塗られる", () => {
    // 筒子は青、索子は緑、發は緑、中は赤。萬子は上下で分ける。
    expect(tileFaceSvg(9)).toContain("#1f4e9c");
    expect(tileFaceSvg(18)).toContain("#1a7a3c");
    expect(tileFaceSvg(32)).toContain("#1a7a3c");
    expect(tileFaceSvg(33)).toContain("#b3261e");
    expect(tileFaceSvg(0)).toContain("linearGradient");
  });

  it("九筒と白は線画なので stroke を塗る", () => {
    // この2件だけ `fill` を持たない。`fill` しか見ないと黒いまま残る。
    expect(tileFaceSvg(17)).toContain('stroke="#1f4e9c"');
    expect(tileFaceSvg(31)).toContain('stroke="#1f4e9c"');
  });

  it("赤ドラは種類ごとの色より赤が優先される", () => {
    // 0m/0p/0s。筒子の青や索子の緑が残ってはいけない。
    for (const tile of [34, 35, 36]) {
      const svg = tileFaceSvg(tile);
      expect(svg).toContain("#b3261e");
      expect(svg).not.toContain("#1f4e9c");
      expect(svg).not.toContain("#1a7a3c");
    }
  });
});
