import {
  AmbientLight,
  DirectionalLight,
  Mesh,
  MeshStandardMaterial,
  PerspectiveCamera,
  PlaneGeometry,
  Raycaster,
  Scene,
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
  readonly #raycaster = new Raycaster();
  #placements: Placement[] = [];

  constructor(canvas: HTMLCanvasElement) {
    this.#canvas = canvas;
    this.#renderer = new WebGLRenderer({ canvas, antialias: true });
    this.#renderer.setPixelRatio(Math.min(devicePixelRatio, 2));

    this.#camera = new PerspectiveCamera(38, 16 / 9, 0.1, 100);
    this.#camera.position.set(0, 31, 22);
    this.#camera.lookAt(0, 0, 1);

    const atlasCanvas = drawPlaceholderAtlas();
    this.#atlas = new Texture(atlasCanvas);
    this.#atlas.needsUpdate = true;
    void paintAtlas(atlasCanvas)
      .then(() => {
        this.#atlas.needsUpdate = true;
      })
      .catch((error: unknown) => {
        console.error("牌面アトラスを作れなかった", error);
      });

    this.#scene.add(new AmbientLight(0xffffff, 0.7));
    const key = new DirectionalLight(0xffffff, 0.9);
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

  /** 現在の配置とメッシュを鍵で同期する。 */
  sync(placements: Placement[]): void {
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
    for (const [key, entry] of this.#meshes) this.#remove(key, entry);
    this.#atlas.dispose();
    this.#renderer.dispose();
  }

  #remove(key: string, entry: TileMesh): void {
    this.#scene.remove(entry.mesh);
    entry.mesh.geometry.dispose();
    if (Array.isArray(entry.mesh.material)) {
      for (const material of entry.mesh.material) material.dispose();
    } else {
      entry.mesh.material.dispose();
    }
    this.#pool.release(entry.tile);
    this.#meshes.delete(key);
  }
}
