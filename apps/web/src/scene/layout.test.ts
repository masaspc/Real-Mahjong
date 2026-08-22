import { describe, expect, it } from "vitest";
import {
  HAND_Z,
  TILE,
  discardPosition,
  handPosition,
  relativeSeat,
  seatRotation,
  wallPosition,
} from "./layout";

describe("relativeSeat", () => {
  it("puts the viewer at zero", () => {
    expect(relativeSeat(2, 2)).toBe(0);
    expect(relativeSeat(3, 2)).toBe(1);
    expect(relativeSeat(0, 2)).toBe(2);
    expect(relativeSeat(1, 2)).toBe(3);
  });
});

describe("seatRotation", () => {
  it("turns a quarter circle per seat", () => {
    expect(seatRotation(0)).toBeCloseTo(0);
    expect(seatRotation(1)).toBeCloseTo(Math.PI / 2);
    expect(seatRotation(2)).toBeCloseTo(Math.PI);
    expect(seatRotation(3)).toBeCloseTo((3 * Math.PI) / 2);
  });
});

describe("handPosition", () => {
  it("lays the viewer's hand along +x, centred on the origin", () => {
    const size = 13;
    const first = handPosition(0, 0, size);
    const last = handPosition(0, size - 1, size);
    expect(first.z).toBeCloseTo(last.z);
    expect(last.x - first.x).toBeCloseTo(TILE.width * (size - 1));
    expect(first.x + last.x).toBeCloseTo(0);
  });

  it("keeps tiles resting on the table", () => {
    expect(handPosition(0, 0, 13).y).toBeGreaterThan(0);
  });

  it("rotates the whole hand for other seats", () => {
    const mine = handPosition(0, 0, 13);
    const across = handPosition(2, 0, 13);
    expect(across.x).toBeCloseTo(-mine.x);
    expect(across.z).toBeCloseTo(-mine.z);
  });
});

describe("discardPosition", () => {
  it("fills six per row before starting the next", () => {
    const a = discardPosition(0, 0);
    const f = discardPosition(0, 5);
    const g = discardPosition(0, 6);
    expect(a.z).toBeCloseTo(f.z);
    expect(g.z).not.toBeCloseTo(a.z);
    expect(g.x).toBeCloseTo(a.x);
  });

  it("段は埋まるたびに自分の側へ降りてくる", () => {
    // **以前は逆だった。**切るたびに新しい牌が卓の中心へ遠ざかり、
    // いちばん見たい直前の数枚が他家の河と混ざる位置に来ていた。
    const row0 = discardPosition(0, 0);
    const row1 = discardPosition(0, 6);
    const row2 = discardPosition(0, 12);
    expect(row1.z).toBeGreaterThan(row0.z);
    expect(row2.z).toBeGreaterThan(row1.z);
  });

  it("最後の段が手牌へ食い込まない", () => {
    // 手牌は立っているので厚みの半分だけ場所を取る。寝かせた河は
    // 高さの半分。ここが重なると、自分の河と手牌が刺さって見える。
    const last = discardPosition(0, 23);
    expect(last.z + TILE.height / 2).toBeLessThan(HAND_Z - TILE.depth / 2);
  });

  it("1段目が卓の中心を越えない", () => {
    const first = discardPosition(0, 0);
    expect(first.z - TILE.height / 2).toBeGreaterThan(0);
  });
});

describe("wallPosition", () => {
  it("stacks two tiles per position", () => {
    const lower = wallPosition(0, 0);
    const upper = wallPosition(0, 1);
    expect(upper.x).toBeCloseTo(lower.x);
    expect(upper.z).toBeCloseTo(lower.z);
    expect(upper.y).toBeGreaterThan(lower.y);
  });

  it("advances along the wall every two tiles", () => {
    const first = wallPosition(0, 0);
    const second = wallPosition(0, 2);
    expect(second.x).not.toBeCloseTo(first.x);
  });
});

describe("河が互いに重ならない", () => {
  it("4段目まで置いても、対面の河と重ならない", () => {
    // **寝かせた牌は z 方向に 1.35 を占める。**4段目が z=0.15 に
    // 来ると、対面の 4段目（z=-0.15）と重なる。
    const half = TILE.height / 2;
    const mine = discardPosition(0, 23);
    const across = discardPosition(2, 23);
    expect(Math.abs(mine.z - across.z)).toBeGreaterThan(TILE.height);
    expect(mine.z - half).toBeGreaterThan(0);
    expect(across.z + half).toBeLessThan(0);
  });

  it("1段目が山へ食い込まない", () => {
    const river = discardPosition(0, 0);
    const wall = wallPosition(0, 0);
    expect(river.z + TILE.height / 2).toBeLessThan(wall.z - TILE.height / 2);
  });
});
