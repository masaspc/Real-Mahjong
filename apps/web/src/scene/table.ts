import {
  AmbientLight,
  DirectionalLight,
  Mesh,
  MeshStandardMaterial,
  PerspectiveCamera,
  PlaneGeometry,
  Raycaster,
  Scene,
  SRGBColorSpace,
  Texture,
  Vector2,
  WebGLRenderer,
} from "three";

import type { Tile } from "../protocol/Tile";
import { drawPlaceholderAtlas } from "./atlas";
import { paintAtlas } from "./face-atlas";
import {
  TABLE_SIZE,
  discardPosition,
  handPosition,
  seatRotation,
  wallPosition,
} from "./layout";
import { pickFrom, type Placement } from "./placement";
import { poseAt, reconcileMoving, type Motion } from "./motion";
import {
  applyFaceUv,
  createTileGeometry,
  createTileMaterial,
} from "./tile-geometry";
import { TilePool, type PooledTile } from "./tile-pool";

type TileMesh = {
  mesh: Mesh;
  tile: PooledTile;
};

/** 固定俯瞰カメラで卓と牌を描くシーン。 */
export class TableScene {
  readonly #canvas: HTMLCanvasElement;
  readonly #renderer: WebGLRenderer;
  readonly #scene = new Scene();
  readonly #camera: PerspectiveCamera;
  readonly #pool = new TilePool();
  readonly #atlas: Texture;
  readonly #meshes = new Map<string, TileMesh>();
  /**
   * 移動中の代理。**`#meshes` とは別に持つ。**
   *
   * `#place` は「渡された配置に無い鍵のメッシュを消す」ので、同じ入れ物へ
   * 入れると次のフレームで代理が消える。鍵は `Motion` の id。
   */
  readonly #moving = new Map<string, TileMesh>();
  readonly #raycaster = new Raycaster();
  #placements: Placement[] = [];

  constructor(canvas: HTMLCanvasElement) {
    this.#canvas = canvas;
    this.#renderer = new WebGLRenderer({ canvas, antialias: true });
    this.#renderer.setPixelRatio(Math.min(devicePixelRatio, 2));

    this.#camera = new PerspectiveCamera(38, 16 / 9, 0.1, 100);
    this.#camera.position.set(0, 31, 22);
    this.#camera.lookAt(0, 0, 1);

    // **牌面の解像度はここで決まる。**1,024 では 8x5 のセル1つが 128px
    // しかなく、漢数字がにじむ。2,048 にすると 256px になる。
    const atlasCanvas = drawPlaceholderAtlas(2048);
    this.#atlas = new Texture(atlasCanvas);
    // canvas をそのまま貼ると色空間が付かず、暗く沈む。
    this.#atlas.colorSpace = SRGBColorSpace;
    // **牌は寝かせて見るので、異方性フィルタが最も効く。**無いと河と副露の
    // 文字が斜めから見たときに潰れる。
    this.#atlas.anisotropy = this.#renderer.capabilities.getMaxAnisotropy();
    this.#atlas.needsUpdate = true;
    void paintAtlas(atlasCanvas)
      .then(() => {
        this.#atlas.needsUpdate = true;
      })
      .catch((error: unknown) => {
        console.error("牌面アトラスを作れなかった", error);
      });

    // 色空間を正すと中間調が沈むので、明かりを少し足して戻す。
    this.#scene.add(new AmbientLight(0xffffff, 1.05));
    const key = new DirectionalLight(0xffffff, 1.15);
    key.position.set(4, 12, 6);
    this.#scene.add(key);

    const felt = new Mesh(
      new PlaneGeometry(TABLE_SIZE, TABLE_SIZE),
      new MeshStandardMaterial({ color: 0x14532d, roughness: 0.95 }),
    );
    felt.rotation.x = -Math.PI / 2;
    this.#scene.add(felt);
  }

  /** main.ts へ結線しない、従来の確認用デモ。 */
  showDemoHand(): void {
    const placements: Placement[] = [];
    for (let seat = 0; seat < 4; seat += 1) {
      for (let index = 0; index < 13; index += 1) {
        placements.push({
          key: `demo-hand-${seat}-${index}`,
          kind: "hand",
          seat: seat as 0 | 1 | 2 | 3,
          encoded: seat === 0 ? (index * 2) % 34 : 0,
          position: handPosition(seat, index, 13),
          rotationX: 0,
          rotationY: seatRotation(seat),
          faceUp: seat === 0,
          pickable: false,
        });
      }
      for (let index = 0; index < 8; index += 1) {
        placements.push({
          key: `demo-river-${seat}-${index}`,
          kind: "river",
          seat: seat as 0 | 1 | 2 | 3,
          encoded: (index * 3) % 34,
          position: discardPosition(seat, index),
          rotationX: -Math.PI / 2,
          rotationY: seatRotation(seat),
          faceUp: true,
          pickable: false,
        });
      }
      for (let index = 0; index < 34; index += 1) {
        placements.push({
          key: `demo-wall-${seat}-${index}`,
          kind: "wall",
          seat: seat as 0 | 1 | 2 | 3,
          encoded: 0,
          position: wallPosition(seat, index),
          rotationX: -Math.PI / 2,
          rotationY: seatRotation(seat),
          faceUp: false,
          pickable: false,
        });
      }
    }
    this.sync(placements);
  }

  /**
   * 動きが無いときの入口。**代理をすべて捨ててから置く。**
   *
   * 動きが無いということは、空中に牌があってはならないということである。
   */
  sync(placements: Placement[]): void {
    this.#dropMoving();
    this.#place(placements);
  }

  /**
   * 動きがあるときの入口。
   *
   * **`sync` を呼んではならない。**呼ぶと毎フレーム自分の代理を捨てて
   * 作り直すことになり、動きが1フレームも続かない。
   */
  syncWithMotion(
    placements: Placement[],
    motions: Motion[],
    progress: number,
  ): void {
    const plan = reconcileMoving([...this.#moving.keys()], motions);
    for (const id of plan.drop) {
      this.#dropOne(id);
    }

    // 着地の鍵は確定ぶんから外す。**外さないと同じ牌が2つ見える。**
    const landing = new Set(motions.map((motion) => motion.toKey));
    this.#place(placements.filter((placement) => !landing.has(placement.key)));

    for (const motion of motions) {
      let entry = this.#moving.get(motion.id);
      if (entry === undefined) {
        const tile = this.#pool.acquire(motion.encoded);
        const mesh = new Mesh(createTileGeometry(), createTileMaterial(this.#atlas));
        this.#scene.add(mesh);
        entry = { mesh, tile };
        this.#moving.set(motion.id, entry);
      }
      // **使い回すのはメッシュだけで、中身ではない。**条件を付けずに毎回
      // 入れ直す。付けると、WebGL 抜きでは試せない場所に判断が生まれる。
      entry.tile.encoded = motion.encoded;
      entry.tile.faceUp = motion.faceUp;
      applyFaceUv(entry.mesh.geometry, motion.encoded, motion.faceUp);

      const pose = poseAt(motion, progress);
      entry.mesh.position.set(pose.position.x, pose.position.y, pose.position.z);
      entry.mesh.rotation.set(pose.rotationX, pose.rotationY, 0);
    }
  }

  #dropMoving(): void {
    for (const id of [...this.#moving.keys()]) {
      this.#dropOne(id);
    }
  }

  #dropOne(id: string): void {
    const entry = this.#moving.get(id);
    if (entry === undefined) {
      return;
    }
    // **`#remove` を使わない。**あれは `#meshes` からも消すので、代理の id と
    // 確定ぶんの鍵がたまたま一致したときに巻き添えで消える。
    this.#dispose(entry);
    this.#moving.delete(id);
  }

  #dispose(entry: TileMesh): void {
    this.#scene.remove(entry.mesh);
    entry.mesh.geometry.dispose();
    if (Array.isArray(entry.mesh.material)) {
      for (const material of entry.mesh.material) material.dispose();
    } else {
      entry.mesh.material.dispose();
    }
    this.#pool.release(entry.tile);
  }

  /** 確定した配置をメッシュへ写す。 */
  #place(placements: Placement[]): void {
    const unique = new Map<string, Placement>();
    for (const placement of placements) {
      if (!unique.has(placement.key)) unique.set(placement.key, placement);
    }

    for (const [key, entry] of this.#meshes) {
      if (!unique.has(key)) this.#remove(key, entry);
    }

    for (const [key, placement] of unique) {
      let entry = this.#meshes.get(key);
      if (entry === undefined) {
        const tile = this.#pool.acquire(placement.encoded);
        const geometry = createTileGeometry();
        const mesh = new Mesh(geometry, createTileMaterial(this.#atlas));
        mesh.userData["placementKey"] = key;
        this.#scene.add(mesh);
        entry = { mesh, tile };
        this.#meshes.set(key, entry);
      }

      entry.tile.encoded = placement.encoded;
      entry.tile.position = placement.position;
      entry.tile.rotationY = placement.rotationY;
      entry.tile.faceUp = placement.faceUp;
      applyFaceUv(entry.mesh.geometry, placement.encoded, placement.faceUp);
      entry.mesh.position.set(
        placement.position.x,
        placement.position.y,
        placement.position.z,
      );
      entry.mesh.rotation.set(placement.rotationX, placement.rotationY, 0);
    }

    this.#placements = [...unique.values()];
  }

  /** canvas 内の CSS ピクセル座標から、押された自分の牌を返す。 */
  pickHandTile(x: number, y: number): Tile | null {
    const rect = this.#canvas.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    const pointer = new Vector2(
      (x / rect.width) * 2 - 1,
      -(y / rect.height) * 2 + 1,
    );
    this.#raycaster.setFromCamera(pointer, this.#camera);
    // **移動中の代理は対象に入れない。**掴めてしまうと、見えている牌と
    // 返される牌が食い違う。終端で確定ぶんへ焼き込まれてから押せる。
    const hits = this.#raycaster.intersectObjects(
      [...this.#meshes.values()].map((entry) => entry.mesh),
      false,
    );
    return pickFrom(
      hits.flatMap((hit) => {
        const key = hit.object.userData["placementKey"];
        return typeof key === "string" ? [{ key, distance: hit.distance }] : [];
      }),
      this.#placements,
    );
  }

  resize(width: number, height: number): void {
    if (width <= 0 || height <= 0) return;
    this.#camera.aspect = width / height;
    this.#camera.updateProjectionMatrix();
    this.#renderer.setSize(width, height, false);
  }

  render(): void {
    this.#renderer.render(this.#scene, this.#camera);
  }

  dispose(): void {
    this.#dropMoving();
    for (const [key, entry] of this.#meshes) this.#remove(key, entry);
    this.#atlas.dispose();
    this.#renderer.dispose();
  }

  #remove(key: string, entry: TileMesh): void {
    this.#dispose(entry);
    this.#meshes.delete(key);
  }
}
