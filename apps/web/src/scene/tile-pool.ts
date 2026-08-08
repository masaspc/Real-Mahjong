import type { Vec3 } from "./layout";

/** 個別に動かせる牌。id は使い回されるメッシュ枠を表す。 */
export interface PooledTile {
  readonly id: number;
  encoded: number;
  position: Vec3;
  rotationY: number;
  faceUp: boolean;
}

/** 動く牌の個別メッシュ枠を貸し借りし、生成回数を抑える。 */
export class TilePool {
  #tiles: PooledTile[] = [];
  #free: number[] = [];
  #inUse = new Set<number>();

  get size(): number {
    return this.#tiles.length;
  }

  get inUse(): number {
    return this.#inUse.size;
  }

  get active(): PooledTile[] {
    return this.#tiles.filter((tile) => this.#inUse.has(tile.id));
  }

  acquire(encoded: number): PooledTile {
    const recycled = this.#free.pop();
    const tile =
      recycled === undefined ? this.#grow() : (this.#tiles[recycled] as PooledTile);

    tile.encoded = encoded;
    tile.position = { x: 0, y: 0, z: 0 };
    tile.rotationY = 0;
    tile.faceUp = true;
    this.#inUse.add(tile.id);
    return tile;
  }

  release(tile: PooledTile): void {
    if (!this.#inUse.delete(tile.id)) {
      throw new Error(`使用中でない牌を返却した（id=${tile.id}）`);
    }
    this.#free.push(tile.id);
  }

  #grow(): PooledTile {
    const tile: PooledTile = {
      id: this.#tiles.length,
      encoded: 0,
      position: { x: 0, y: 0, z: 0 },
      rotationY: 0,
      faceUp: true,
    };
    this.#tiles.push(tile);
    return tile;
  }
}
