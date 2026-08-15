import type { ClientEvent } from "../protocol/ClientEvent";
import type { Seat } from "../protocol/Seat";
import type { Tile } from "../protocol/Tile";
import { easeOutCubic } from "../timeline/timeline";
import type { Placement } from "./placement";
import type { Vec3 } from "./layout";

/**
 * どの牌がどこへ動くかを、**イベントから**決める。
 *
 * **配置の見た目で突き合わせてはならない。**他家の手牌は伏せ牌で
 * `encoded` が 0 なので、見た目で対応を取ると同じ点数の解が複数でき、
 * フレームごとに違う牌が選ばれて手牌がちらつく。席とイベントから一意に
 * 決まる規則にする。
 *
 * Three.js には触らない。**ここが純粋でないと、動きを目で見るしか
 * 確かめられなくなる。**
 */

export type Pose = {
  position: Vec3;
  rotationX: number;
  rotationY: number;
};

export type Motion = {
  /** 代理のメッシュを引き当てる鍵。 */
  id: string;
  fromKey: string;
  toKey: string;
  encoded: Tile;
  faceUp: boolean;
  from: Pose;
  to: Pose;
  /** 弧の高さ。0 なら直線。 */
  lift: number;
};

/** ツモは軽く持ち上げる。真横に滑らせると起点が分からない。 */
const LIFT_DRAW = 0.8;
/** 打牌は河へ放るので、ツモより高く上げる。 */
const LIFT_DISCARD = 1.2;

function poseOf(placement: Placement): Pose {
  return {
    position: placement.position,
    rotationX: placement.rotationX,
    rotationY: placement.rotationY,
  };
}

function motion(
  from: Placement,
  to: Placement,
  lift: number,
): Motion {
  return {
    id: `${from.key}->${to.key}`,
    fromKey: from.key,
    toKey: to.key,
    // 着地の側の見た目を使う。伏せから表になる牌は着いた姿で見せる。
    encoded: to.encoded,
    faceUp: to.faceUp,
    from: poseOf(from),
    to: poseOf(to),
    lift,
  };
}

function byKey(placements: Placement[]): Map<string, Placement> {
  const out = new Map<string, Placement>();
  for (const placement of placements) {
    if (!out.has(placement.key)) {
      out.set(placement.key, placement);
    }
  }
  return out;
}

function samePose(a: Placement, b: Placement): boolean {
  return (
    a.position.x === b.position.x &&
    a.position.y === b.position.y &&
    a.position.z === b.position.z &&
    a.rotationX === b.rotationX &&
    a.rotationY === b.rotationY
  );
}

/**
 * 消えた山の牌。**番号が最も小さいものである。**
 *
 * `slot = WALL_SLOTS - wallRemaining + index` なので、残りが減るほど
 * 開始の番号が上がる。`wall-135` は常に残るため、**最も大きいものを
 * 選ぶと前後どちらにも在る牌を掴んで動きが1つも出ない。**
 */
function consumedWall(
  before: Map<string, Placement>,
  after: Map<string, Placement>,
): Placement | null {
  let found: Placement | null = null;
  let smallest = Number.POSITIVE_INFINITY;
  for (const [key, placement] of before) {
    if (!key.startsWith("wall-") || after.has(key)) {
      continue;
    }
    const slot = Number(key.slice("wall-".length));
    if (Number.isFinite(slot) && slot < smallest) {
      smallest = slot;
      found = placement;
    }
  }
  return found;
}

/** 新しくできた河の牌。 */
function newRiverTile(
  before: Map<string, Placement>,
  after: Map<string, Placement>,
  seat: Seat,
): Placement | null {
  const prefix = `river-${seat}-`;
  for (const [key, placement] of after) {
    if (key.startsWith(prefix) && !before.has(key)) {
      return placement;
    }
  }
  return null;
}

/** その席の手牌のうち、添字が最大のもの。他家のツモ牌はここに来る。 */
function lastHandTile(
  placements: Map<string, Placement>,
  seat: Seat,
): Placement | null {
  const prefix = `hand-${seat}-`;
  let found: Placement | null = null;
  let largest = -1;
  for (const [key, placement] of placements) {
    if (!key.startsWith(prefix)) {
      continue;
    }
    const index = Number(key.slice(prefix.length));
    if (Number.isFinite(index) && index > largest) {
      largest = index;
      found = placement;
    }
  }
  return found;
}

function handTiles(
  placements: Map<string, Placement>,
  seat: Seat,
): Placement[] {
  const prefix = `hand-${seat}-`;
  return [...placements.values()]
    .filter((placement) => placement.key.startsWith(prefix))
    .sort(
      (a, b) =>
        Number(a.key.slice(prefix.length)) - Number(b.key.slice(prefix.length)),
    );
}

export function motionsFor(
  before: Placement[],
  after: Placement[],
  event: ClientEvent,
  viewer: Seat,
): Motion[] {
  const was = byKey(before);
  const now = byKey(after);
  const out: Motion[] = [];
  /** 第1段で使い切った鍵。下の段では扱わない。 */
  const usedFrom = new Set<string>();
  const usedTo = new Set<string>();

  const add = (from: Placement | null, to: Placement | null, lift: number): void => {
    if (from === null || to === null) {
      return;
    }
    out.push(motion(from, to, lift));
    usedFrom.add(from.key);
    usedTo.add(to.key);
  };

  // ---- 第1段。領域をまたぐ牌をイベントから決める。
  if (event.type === "draw") {
    const seat = event.seat;
    const mine = seat === viewer;
    const wall = consumedWall(was, now);
    if (mine) {
      add(wall, now.get(`drawn-${seat}`) ?? null, LIFT_DRAW);
      // **すでにツモ牌があるときは、それが手牌へ吸収される。**
      // 暗槓のあとの嶺上ツモで実際に通る。鍵が変わるので下の段では拾えない。
      const previous = was.get(`drawn-${seat}`);
      if (previous !== undefined) {
        add(previous, absorbedSlot(was, now, seat, previous.encoded, usedTo), 0);
      }
    } else {
      add(wall, lastHandTile(now, seat), LIFT_DRAW);
    }
  } else if (event.type === "discard") {
    const seat = event.seat;
    const mine = seat === viewer;
    const landing = newRiverTile(was, now, seat);
    if (!mine) {
      add(lastHandTile(was, seat), landing, LIFT_DISCARD);
    } else if (event.manner === "tsumogiri") {
      add(was.get(`drawn-${seat}`) ?? null, landing, LIFT_DISCARD);
    } else {
      const source = handTiles(was, seat).find(
        (placement) => placement.encoded === event.tile,
      );
      add(source ?? null, landing, LIFT_DISCARD);
      // **手出しでは牌が2枚動く。**切った牌が河へ行くと同時に、
      // ツモ牌が手牌へ吸収される。
      const previous = was.get(`drawn-${seat}`);
      if (previous !== undefined) {
        add(previous, absorbedSlot(was, now, seat, previous.encoded, usedTo), 0);
      }
    }
  }

  // ---- 第2段。自分の手牌の中の並び替え。
  //
  // **同じ `encoded` の n 個目どうしを対応させる。**手牌はツモのたびに
  // 並べ替わるので、鍵で対応させると中身が入れ替わった牌が滑って見える。
  const mineBefore = handTiles(was, viewer).filter(
    (placement) => !usedFrom.has(placement.key),
  );
  const mineAfter = handTiles(now, viewer).filter(
    (placement) => !usedTo.has(placement.key),
  );
  const takenAfter = new Set<string>();
  for (const from of mineBefore) {
    const to = mineAfter.find(
      (candidate) =>
        candidate.encoded === from.encoded && !takenAfter.has(candidate.key),
    );
    if (to === undefined) {
      continue;
    }
    takenAfter.add(to.key);
    usedFrom.add(from.key);
    usedTo.add(to.key);
    if (!samePose(from, to)) {
      out.push(motion(from, to, 0));
    }
  }

  // ---- 第3段。鍵が前後の両方にあり、姿勢が変わったもの。
  //
  // **この段が無いと Goal を達成できない。**手牌も河も列を中央揃えする
  // ため、枚数が1枚変わると残りの牌がすべて半スロットずれる。
  for (const [key, to] of now) {
    if (usedTo.has(key) || key.startsWith(`hand-${viewer}-`)) {
      continue;
    }
    const from = was.get(key);
    if (from === undefined || samePose(from, to)) {
      continue;
    }
    out.push(motion(from, to, 0));
  }

  return out;
}

/**
 * 吸収されたツモ牌が入った手牌の位置。
 *
 * 第2段の対応を取ったあとに**余る**位置である。ここでは、同じ `encoded` を
 * 持つ `after` の手牌のうち、`before` の手牌が使い切れずに残るものを選ぶ。
 */
function absorbedSlot(
  was: Map<string, Placement>,
  now: Map<string, Placement>,
  seat: Seat,
  encoded: Tile,
  usedTo: Set<string>,
): Placement | null {
  const beforeCount = handTiles(was, seat).filter(
    (placement) => placement.encoded === encoded,
  ).length;
  const candidates = handTiles(now, seat).filter(
    (placement) => placement.encoded === encoded && !usedTo.has(placement.key),
  );
  // `before` にあるぶんは第2段が引き受ける。余った1枚が吸収された牌である。
  return candidates[beforeCount] ?? candidates[candidates.length - 1] ?? null;
}

/** 角度の差を [-PI, PI] へ畳む。**畳まないと牌が大回りする。** */
function shortestDelta(from: number, to: number): number {
  let delta = (to - from) % (Math.PI * 2);
  if (delta > Math.PI) {
    delta -= Math.PI * 2;
  }
  if (delta < -Math.PI) {
    delta += Math.PI * 2;
  }
  return delta;
}

/**
 * 進捗 0..1 のときの姿勢。
 *
 * **描画側で計算し直さない。**二重に持つと、片方だけ直したときに静かに
 * ずれる。弧の高さも回転の向きもここが唯一の定義である。
 */
export function poseAt(motion: Motion, progress: number): Pose {
  const t = progress < 0 ? 0 : progress > 1 ? 1 : progress;
  const eased = easeOutCubic(t);
  const lerp = (from: number, to: number): number => from + (to - from) * eased;
  // 弧は両端で 0 になる。**ここが 0 でないと、置いた瞬間に牌が沈む。**
  const arc = motion.lift * 4 * t * (1 - t);
  return {
    position: {
      x: lerp(motion.from.position.x, motion.to.position.x),
      y: lerp(motion.from.position.y, motion.to.position.y) + arc,
      z: lerp(motion.from.position.z, motion.to.position.z),
    },
    rotationX:
      motion.from.rotationX +
      shortestDelta(motion.from.rotationX, motion.to.rotationX) * eased,
    rotationY:
      motion.from.rotationY +
      shortestDelta(motion.from.rotationY, motion.to.rotationY) * eased,
  };
}

/**
 * 代理のメッシュを、どれを残しどれを捨てるか。
 *
 * **判断を `TableScene` の中に置かない。**WebGL 抜きでは試せなくなる。
 */
export function reconcileMoving(
  currentIds: string[],
  motions: Motion[],
): { keep: string[]; drop: string[]; create: string[] } {
  const wanted = new Set(motions.map((m) => m.id));
  const held = new Set(currentIds);
  return {
    keep: currentIds.filter((id) => wanted.has(id)),
    drop: currentIds.filter((id) => !wanted.has(id)),
    create: [...wanted].filter((id) => !held.has(id)),
  };
}
