import {
  AmbientLight,
  BufferGeometry,
  DirectionalLight,
  Mesh,
  MeshStandardMaterial,
  PerspectiveCamera,
  PlaneGeometry,
  Scene,
  Texture,
  WebGLRenderer,
} from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";

import { drawPlaceholderAtlas } from "./atlas";
import { TILE, discardPosition, handPosition, wallPosition } from "./layout";
import {
  applyFaceUv,
  createTileGeometry,
  createTileMaterial,
} from "./tile-geometry";
import { TilePool } from "./tile-pool";

/** 固定俯瞰カメラで卓と牌を描くシーン。 */
export class TableScene {
  readonly #renderer: WebGLRenderer;
  readonly #scene = new Scene();
  readonly #camera: PerspectiveCamera;
  readonly #pool = new TilePool();
  readonly #atlas: Texture;
  readonly #meshes = new Map<number, Mesh>();
  #wall?: Mesh;

  constructor(canvas: HTMLCanvasElement) {
    this.#renderer = new WebGLRenderer({ canvas, antialias: true });
    this.#renderer.setPixelRatio(Math.min(devicePixelRatio, 2));

    this.#camera = new PerspectiveCamera(38, 16 / 9, 0.1, 100);
    this.#camera.position.set(0, 14, 13);
    this.#camera.lookAt(0, 0, 1);

    this.#atlas = new Texture(drawPlaceholderAtlas());
    this.#atlas.needsUpdate = true;

    this.#scene.add(new AmbientLight(0xffffff, 0.7));
    const key = new DirectionalLight(0xffffff, 0.9);
    key.position.set(4, 12, 6);
    this.#scene.add(key);

    const felt = new Mesh(
      new PlaneGeometry(20, 20),
      new MeshStandardMaterial({ color: 0x14532d, roughness: 0.95 }),
    );
    felt.rotation.x = -Math.PI / 2;
    this.#scene.add(felt);
  }

  /** 4人分の手牌と河、バッチした山を仮に並べる。 */
  showDemoHand(): void {
    for (let seat = 0; seat < 4; seat += 1) {
      for (let i = 0; i < 13; i += 1) {
        this.#place(seat === 0 ? i * 2 : 0, handPosition(seat, i, 13), seat === 0);
      }
      for (let i = 0; i < 8; i += 1) {
        this.#place((i * 3) % 34, discardPosition(seat, i), true);
      }
    }
    this.#buildWall();
  }

  #buildWall(): void {
    const parts: BufferGeometry[] = [];
    for (let seat = 0; seat < 4; seat += 1) {
      for (let i = 0; i < 34; i += 1) {
        const position = wallPosition(seat, i);
        const geometry = createTileGeometry();
        applyFaceUv(geometry, 0, false);
        geometry.rotateX(-Math.PI / 2);
        geometry.translate(position.x, position.y, position.z);
        parts.push(geometry);
      }
    }
    const merged = mergeGeometries(parts, false);
    for (const part of parts) {
      part.dispose();
    }
    if (merged === null) {
      throw new Error("山のジオメトリをまとめられなかった");
    }
    this.#wall = new Mesh(merged, createTileMaterial(this.#atlas));
    this.#scene.add(this.#wall);
  }

  #place(
    encoded: number,
    position: { x: number; y: number; z: number },
    faceUp: boolean,
  ): void {
    const tile = this.#pool.acquire(encoded);
    tile.position = position;
    tile.faceUp = faceUp;

    const geometry = createTileGeometry();
    applyFaceUv(geometry, encoded, faceUp);
    const mesh = new Mesh(geometry, createTileMaterial(this.#atlas));
    mesh.position.set(position.x, position.y, position.z);
    mesh.rotation.x = position.y > TILE.depth ? -Math.PI / 12 : -Math.PI / 2;
    this.#scene.add(mesh);
    this.#meshes.set(tile.id, mesh);
  }

  resize(width: number, height: number): void {
    this.#camera.aspect = width / height;
    this.#camera.updateProjectionMatrix();
    this.#renderer.setSize(width, height, false);
  }

  render(): void {
    this.#renderer.render(this.#scene, this.#camera);
  }

  dispose(): void {
    for (const mesh of this.#meshes.values()) {
      mesh.geometry.dispose();
      if (Array.isArray(mesh.material)) {
        for (const material of mesh.material) material.dispose();
      } else {
        mesh.material.dispose();
      }
    }
    this.#meshes.clear();
    this.#wall?.geometry.dispose();
    if (this.#wall !== undefined) {
      if (Array.isArray(this.#wall.material)) {
        for (const material of this.#wall.material) material.dispose();
      } else {
        this.#wall.material.dispose();
      }
    }
    this.#atlas.dispose();
    this.#renderer.dispose();
  }
}
