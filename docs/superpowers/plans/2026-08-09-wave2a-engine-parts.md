# Wave 2a: mahjong-engine の部品 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 局の進行に必要な部品を、進行そのものとは切り離して作る。山・反応ウィンドウ・局の状態・時間計算・不変条件の5つを、それぞれ単体で検証できる形にする。

**Architecture:** すべて決定的。乱数はシードから、時間は引数から受け取る。進行のステートマシン（`round.rs` / `match_flow.rs`）は**この計画では触らない**。Wave 2b が担当する。

**Tech Stack:** Rust 1.97.1 / edition 2021 / `protocol` と `mahjong-core` に依存、`sha2` を追加

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
**作業規約:** `AGENTS.md`

## Global Constraints

- **編集してよいのは次のみ**
  - `crates/mahjong-engine/src/wall.rs`
  - `crates/mahjong-engine/src/reaction.rs`
  - `crates/mahjong-engine/src/state.rs`
  - `crates/mahjong-engine/src/timing.rs`（新規作成。`state.rs` から `#[path]` で読み込む）
  - `crates/mahjong-engine/src/invariant.rs`
  - `crates/mahjong-engine/Cargo.toml`（`sha2` の追加のみ）
- **`lib.rs` / `round.rs` / `match_flow.rs` を編集しない。** 後者2つは Wave 2b の所有である
- `crates/protocol` と `crates/mahjong-core` は凍結済み。**編集も追加もしない。**足りなければ実装を止めて報告する
- **乱数を直接使わない。** `rand` を足さず、シードから決定的に生成する
- **時刻を直接読まない。** `Instant::now()` を呼ばず、`now_ms: u64` を引数で受け取る
- `Ruleset` に存在する値をハードコードしない。**`Ruleset` に無いルール定数**（リーチ棒の1000点など）は、このクレート内に名前付き定数として置き、根拠をコメントに書く
- 完了条件は `cargo test --workspace` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` がすべて通ること

## `lib.rs` を編集せずに新しいモジュールを足す

`lib.rs` は Wave 0 で凍結済みで `timing` を宣言していない。`state.rs` の先頭に次を書いて読み込む。

```rust
#[path = "timing.rs"]
mod timing;
pub use timing::{charge_bank, deadline_for, lead_in_of, remaining_for_event};
```

これで `crate::state::deadline_for` として参照できる。`lib.rs` に手を入れる必要はない。

## タスクの依存関係

```
1 wall ─────┐
2 reaction ─┼─→ 5 invariant
3 timing ───┤
4 state ────┘
```

Task 1〜4 は互いに独立で、**並行して実装できる**。Task 5 は 1・2・4 の型を使う。

---

### Task 1: 決定的な山

**Files:**
- Modify: `crates/mahjong-engine/src/wall.rs`
- Modify: `crates/mahjong-engine/Cargo.toml`

**Interfaces:**
- Produces:
  - `pub struct Seed([u8; 32])` — `new(bytes)`, `from_hex(&str) -> Option<Seed>`, `to_hex() -> String`, `commitment() -> String`
  - `pub struct Wall` — `new(&Seed, &Ruleset)`, `draw() -> Option<Tile>`, `draw_replacement() -> Option<Tile>`, `reveal_dora() -> Option<Tile>`, `live_remaining() -> u8`, `dora_indicators() -> &[Tile]`, `ura_indicators() -> &[Tile]`, `all_tiles() -> impl Iterator<Item = Tile>`
  - `tiles_in_wall() -> impl Iterator<Item = Tile>` — **まだ引かれていない牌だけ**を返す。不変条件の検査に使う
  - 検証用: `dora_positions() -> Vec<usize>`, `ura_positions() -> Vec<usize>`, `replacement_positions() -> Vec<usize>`

**`all_tiles()` と `tiles_in_wall()` を混同しない。** 前者は並びの検証用に
136枚すべてを返す。後者は「いま山に残っている牌」で、生牌の未取得分と
嶺上の未取得分の合計である。牌の総数を数えるときは必ず後者を使う。

**王牌の配置を固定する。** 嶺上を引くと生牌の末尾が1枚減るが、**ドラ表示牌の位置は動かさない**。動かすと既に開示した裏ドラと重複する。

136枚のうち生牌は `0..122`、王牌は `122..136` とし、次のように割り当てる。

| 位置 | 用途 |
|---|---|
| `122, 124, 126, 128, 130` | ドラ表示牌（5枚） |
| `123, 125, 127, 129, 131` | 裏ドラ表示牌（5枚） |
| `132, 133, 134, 135` | 嶺上牌（4枚） |

- [ ] **Step 1: sha2 を追加する**

```bash
cargo add --package mahjong-engine sha2
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/mahjong-engine/src/wall.rs` の末尾に置く。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::tile::TileKind;
    use std::collections::HashSet;

    fn rules() -> Ruleset {
        Ruleset::kin_no_ma(MatchLength::Hanchan)
    }

    fn seed(byte: u8) -> Seed {
        Seed::from_hex(&format!("{byte:02x}").repeat(32)).expect("hex")
    }

    #[test]
    fn a_wall_holds_every_tile_exactly_four_times() {
        let wall = Wall::new(&seed(1), &rules());
        let mut counts = [0u8; TileKind::COUNT];
        for tile in wall.all_tiles() {
            counts[tile.kind().index() as usize] += 1;
        }
        assert!(counts.iter().all(|c| *c == 4), "34種が4枚ずつでない");
    }

    #[test]
    fn a_wall_holds_exactly_one_hundred_and_thirty_six_tiles() {
        assert_eq!(Wall::new(&seed(1), &rules()).all_tiles().count(), 136);
    }

    #[test]
    fn exactly_three_tiles_are_red() {
        let wall = Wall::new(&seed(2), &rules());
        let mut reds: Vec<u8> = wall
            .all_tiles()
            .filter(|t| t.is_red())
            .map(|t| t.kind().index())
            .collect();
        reds.sort();
        assert_eq!(reds, vec![4, 13, 22], "赤は 5m/5p/5s の各1枚");
    }

    #[test]
    fn the_same_seed_always_produces_the_same_wall() {
        let a: Vec<u8> = Wall::new(&seed(7), &rules())
            .all_tiles()
            .map(|t| t.encoded())
            .collect();
        let b: Vec<u8> = Wall::new(&seed(7), &rules())
            .all_tiles()
            .map(|t| t.encoded())
            .collect();
        assert_eq!(a, b);
    }

    #[test]
    fn different_seeds_produce_different_walls() {
        let a: Vec<u8> = Wall::new(&seed(1), &rules())
            .all_tiles()
            .map(|t| t.encoded())
            .collect();
        let b: Vec<u8> = Wall::new(&seed(2), &rules())
            .all_tiles()
            .map(|t| t.encoded())
            .collect();
        assert_ne!(a, b);
    }

    /// シャッフル方式を変えると過去の牌譜が再現できなくなる。
    /// 固定シードに対する並びのハッシュを凍結し、変更を検出する。
    #[test]
    fn a_fixed_seed_matches_its_golden_vector() {
        use sha2::{Digest, Sha256};
        let encoded: Vec<u8> = Wall::new(&seed(0xAB), &rules())
            .all_tiles()
            .map(|t| t.encoded())
            .collect();
        assert_eq!(encoded.len(), 136);
        let digest: String = Sha256::digest(&encoded)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        // この値は計画作成時に splitmix64 と Fisher-Yates を実際に回して求めた。
        // **変更してはならない。**変わったらシャッフル方式が変わったということであり、
        // 過去の牌譜が再現できなくなる。
        assert_eq!(
            digest,
            "7b0d5f31b3ded153eeb6a5e7e06f041c14cbb4403d1c8f278b6bd37c912b43c4"
        );
    }

    #[test]
    fn one_hundred_and_twenty_two_tiles_can_be_drawn() {
        let mut wall = Wall::new(&seed(3), &rules());
        assert_eq!(wall.live_remaining(), 122);
        let mut drawn = 0;
        while wall.draw().is_some() {
            drawn += 1;
        }
        assert_eq!(drawn, 122);
        assert_eq!(wall.live_remaining(), 0);
    }

    #[test]
    fn a_replacement_draw_shortens_the_live_wall() {
        let mut wall = Wall::new(&seed(4), &rules());
        let before = wall.live_remaining();
        assert!(wall.draw_replacement().is_some());
        assert_eq!(wall.live_remaining(), before - 1);
    }

    #[test]
    fn only_four_replacements_are_available() {
        let mut wall = Wall::new(&seed(5), &rules());
        for _ in 0..4 {
            assert!(wall.draw_replacement().is_some());
        }
        assert!(wall.draw_replacement().is_none());
    }

    #[test]
    fn dora_starts_with_one_and_can_reveal_up_to_five() {
        let mut wall = Wall::new(&seed(6), &rules());
        assert_eq!(wall.dora_indicators().len(), 1);
        assert_eq!(wall.ura_indicators().len(), 1);
        for _ in 0..4 {
            assert!(wall.reveal_dora().is_some());
        }
        assert_eq!(wall.dora_indicators().len(), 5);
        assert_eq!(wall.ura_indicators().len(), 5);
        assert!(wall.reveal_dora().is_none());
    }

    /// 嶺上を引いてもドラ表示牌の位置は動かない。
    /// 動かすと既に開示した裏ドラと重複する。
    #[test]
    fn the_dead_wall_positions_never_overlap() {
        let mut wall = Wall::new(&seed(6), &rules());
        for _ in 0..4 {
            wall.draw_replacement();
            wall.reveal_dora();
        }

        let mut positions = Vec::new();
        positions.extend(wall.dora_positions());
        positions.extend(wall.ura_positions());
        positions.extend(wall.replacement_positions());

        let unique: HashSet<usize> = positions.iter().copied().collect();
        assert_eq!(unique.len(), positions.len(), "位置が重複した: {positions:?}");
        assert_eq!(unique.len(), 14, "王牌はちょうど14枚");
        assert!(unique.iter().all(|p| (122..136).contains(p)), "王牌の範囲外");
    }

    /// 嶺上を引いて山から出るのは、引いたその1枚だけである。
    /// 生牌の末尾はツモれなくなるが山には残るので、2枚減ってはいけない。
    #[test]
    fn a_replacement_draw_only_moves_one_tile_out_of_the_wall() {
        let mut wall = Wall::new(&seed(11), &rules());
        let before = wall.tiles_in_wall().count();
        assert!(wall.draw_replacement().is_some());
        assert_eq!(
            wall.tiles_in_wall().count(),
            before - 1,
            "嶺上で引いた1枚だけが山から出る"
        );
    }

    /// 配牌前の山は136枚。
    #[test]
    fn a_fresh_wall_holds_every_tile() {
        assert_eq!(Wall::new(&seed(12), &rules()).tiles_in_wall().count(), 136);
    }

    /// 嶺上を引く前後でドラ表示牌そのものが変わらない。
    #[test]
    fn a_replacement_draw_does_not_change_the_revealed_dora() {
        let mut wall = Wall::new(&seed(8), &rules());
        let before: Vec<u8> = wall.dora_indicators().iter().map(|t| t.encoded()).collect();
        wall.draw_replacement();
        let after: Vec<u8> = wall.dora_indicators().iter().map(|t| t.encoded()).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn a_seed_commits_to_itself() {
        let s = seed(9);
        assert_eq!(s.commitment().len(), 64, "SHA-256 の hex は64文字");
        assert_eq!(
            Seed::from_hex(&s.to_hex()).unwrap().commitment(),
            s.commitment()
        );
        assert_ne!(seed(10).commitment(), s.commitment());
    }

    #[test]
    fn malformed_hex_is_rejected() {
        assert!(Seed::from_hex("00").is_none(), "長さが足りない");
        assert!(Seed::from_hex(&"zz".repeat(32)).is_none(), "16進でない");
    }
}
```

- [ ] **Step 3: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine wall`
Expected: コンパイルエラー

- [ ] **Step 4: 実装を書く**

```rust
//! 山の生成と管理。
//!
//! 乱数はシードから決定的に作る。`rand` を使わないのは、クレートの版が
//! 変わっても同じシードから同じ山が出ることを保証するためである。
//! これが崩れると牌譜の再現とシードコミットメントの検算が壊れる。

use protocol::ruleset::Ruleset;
use protocol::tile::{Tile, TileKind};
use sha2::{Digest, Sha256};

/// 山を決める32バイトの種。局開始時に永続化し、対局終了後に開示する。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seed([u8; 32]);

impl Seed {
    pub fn new(bytes: [u8; 32]) -> Self {
        Seed(bytes)
    }

    pub fn from_hex(hex: &str) -> Option<Self> {
        if hex.len() != 64 {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (index, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let text = std::str::from_utf8(chunk).ok()?;
            bytes[index] = u8::from_str_radix(text, 16).ok()?;
        }
        Some(Seed(bytes))
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// 局開始時に配るハッシュ。開示後にプレイヤーがこれと照合する。
    pub fn commitment(&self) -> String {
        Sha256::digest(self.0)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

/// splitmix64。実装が短く、版によって挙動が変わらない。
struct Rng(u64);

impl Rng {
    fn from_seed(seed: &Seed) -> Self {
        let mut state = 0u64;
        for (index, byte) in seed.0.iter().enumerate() {
            state ^= (*byte as u64) << ((index % 8) * 8);
            state = state.rotate_left(7);
        }
        Rng(state | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

const TOTAL: usize = 136;
/// 生牌の終わり。ここから先が王牌14枚。
const DEAD_WALL_START: usize = 122;
const MAX_REPLACEMENTS: usize = 4;
const MAX_DORA: usize = 5;

pub struct Wall {
    tiles: Vec<Tile>,
    /// 次にツモる位置。
    next: usize,
    /// ツモれる牌の終わり。嶺上を引くたび1つ手前へ下がる。
    live_end: usize,
    replacements_taken: usize,
    /// 開示済みのドラ表示牌の枚数。1〜5。
    dora_revealed: usize,
    /// ドラ表示牌5枚。位置は固定なので生成時に確定する。
    dora: Vec<Tile>,
    /// 裏ドラ表示牌5枚。
    ura: Vec<Tile>,
}

/// ドラ表示牌の位置。`live_end` に依存させない。
fn dora_position(index: usize) -> usize {
    DEAD_WALL_START + index * 2
}

fn ura_position(index: usize) -> usize {
    DEAD_WALL_START + index * 2 + 1
}

fn replacement_position(index: usize) -> usize {
    DEAD_WALL_START + MAX_DORA * 2 + index
}

impl Wall {
    pub fn new(seed: &Seed, rules: &Ruleset) -> Self {
        let mut tiles = Vec::with_capacity(TOTAL);
        for index in 0..TileKind::COUNT as u8 {
            for _ in 0..4 {
                tiles.push(Tile::from_kind(TileKind::from_index(index).expect("範囲内")));
            }
        }

        // 赤ドラ。5m/5p/5s の順に、Ruleset が指定した枚数だけ置き換える。
        // 候補は3つしかないため、それより大きい値を指定しても3枚で頭打ちになる。
        let reds: [(u8, u8); 3] = [(4, 34), (13, 35), (22, 36)];
        for (kind_index, red_encoded) in reds.iter().copied().take(rules.red_dora_count as usize) {
            let position = tiles
                .iter()
                .position(|t| t.kind().index() == kind_index && !t.is_red())
                .expect("該当牌がある");
            tiles[position] = Tile::from_encoded(red_encoded).expect("赤ドラは範囲内");
        }

        let mut rng = Rng::from_seed(seed);
        for i in (1..tiles.len()).rev() {
            let j = rng.below(i + 1);
            tiles.swap(i, j);
        }

        let dora = (0..MAX_DORA).map(|i| tiles[dora_position(i)]).collect();
        let ura = (0..MAX_DORA).map(|i| tiles[ura_position(i)]).collect();

        Wall {
            tiles,
            next: 0,
            live_end: DEAD_WALL_START,
            replacements_taken: 0,
            dora_revealed: 1,
            dora,
            ura,
        }
    }

    /// 並びの検証用。**136枚すべて**を返す。牌の総数を数えるのに使わない。
    pub fn all_tiles(&self) -> impl Iterator<Item = Tile> + '_ {
        self.tiles.iter().copied()
    }

    /// いま山に残っている牌。まだ誰の手にも渡っていない牌すべて。
    ///
    /// **`live_end` ではなく `DEAD_WALL_START` まで数える。** 嶺上を引くと
    /// `live_end` が下がるが、そのとき生牌の末尾はツモれなくなるだけで
    /// 山からは消えない（王牌へ組み込まれる）。`live_end` で切ると
    /// その1枚を数え落とし、牌の総数が135枚になる。
    pub fn tiles_in_wall(&self) -> impl Iterator<Item = Tile> + '_ {
        let live = self.tiles[self.next..DEAD_WALL_START].iter().copied();
        let dead = (0..MAX_DORA * 2)
            .map(|i| self.tiles[DEAD_WALL_START + i])
            .chain(
                (self.replacements_taken..MAX_REPLACEMENTS)
                    .map(|i| self.tiles[replacement_position(i)]),
            );
        live.chain(dead)
    }

    pub fn live_remaining(&self) -> u8 {
        (self.live_end - self.next) as u8
    }

    pub fn draw(&mut self) -> Option<Tile> {
        if self.next >= self.live_end {
            return None;
        }
        let tile = self.tiles[self.next];
        self.next += 1;
        Some(tile)
    }

    /// 嶺上牌。引くたびに生牌の末尾が1枚減る。
    pub fn draw_replacement(&mut self) -> Option<Tile> {
        // 生牌を引き切っていたら live_end を下げられない。
        // 下げると live_remaining() が桁溢れする。
        if self.replacements_taken >= MAX_REPLACEMENTS || self.live_end == self.next {
            return None;
        }
        let tile = self.tiles[replacement_position(self.replacements_taken)];
        self.replacements_taken += 1;
        self.live_end -= 1;
        Some(tile)
    }

    pub fn reveal_dora(&mut self) -> Option<Tile> {
        if self.dora_revealed >= MAX_DORA {
            return None;
        }
        self.dora_revealed += 1;
        self.dora.get(self.dora_revealed - 1).copied()
    }

    pub fn dora_indicators(&self) -> &[Tile] {
        &self.dora[..self.dora_revealed]
    }

    pub fn ura_indicators(&self) -> &[Tile] {
        &self.ura[..self.dora_revealed]
    }

    pub fn dora_positions(&self) -> Vec<usize> {
        (0..self.dora_revealed).map(dora_position).collect()
    }

    pub fn ura_positions(&self) -> Vec<usize> {
        (0..self.dora_revealed).map(ura_position).collect()
    }

    pub fn replacement_positions(&self) -> Vec<usize> {
        (0..MAX_REPLACEMENTS).map(replacement_position).collect()
    }
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine wall`
Expected: 16テスト PASS

`a_fixed_seed_matches_its_golden_vector` の期待値は計画作成時に算出済みである。
**落ちたらテストを直すのではなく、実装が計画どおりでないことを疑う。**

- [ ] **Step 6: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): シードから決定的に作る山を実装"
```

---

### Task 2: 反応ウィンドウの解決

**Files:**
- Modify: `crates/mahjong-engine/src/reaction.rs`

**Interfaces:**
- Produces:
  - `pub enum Priority { Pass, Chi, Pon, Ron }`（宣言順が優先度。`PartialOrd, Ord` を導出）
  - `pub enum WindowKind { Discard, Chankan }`
  - `pub struct ReactionWindow` — `open(id, kind, from, tile, candidates, opened_at_ms, deadline_ms)`, `respond(seat, response) -> Result<(), Rejection>`, `resolve(now_ms, min_wait_ms) -> Outcome`, `id()`, `kind()`, `non_ron_ties() -> Vec<Seat>`
  - `pub enum Outcome { Pending, Ron(Vec<Seat>), Call { seat: Seat, response: CallResponse }, PassAll }`
  - `pub enum Rejection { NotACandidate, AlreadyResponded, NotOffered, IsTheDiscarder }`

**明槓はポンと同順位である**（仕様 6.4 の「ロン > ポン / 明カン > チー」）。
`CallResponse::Kan` と `ActionOption::Kan` は `Priority::Pon` へ写す。
`Priority` に `Kan` を作らないのは、同順位のものを別の値にすると
比較のたびに読み替えが要り、取り違えの元になるためである。

**解決の規則（仕様 6.4）:**

1. `now_ms < opened_at_ms + min_wait_ms` なら、全員が答えていても `Pending`
2. 確定している最高優先度を `best` とする
3. 未応答者のうち `best` **以上**を出せる者がいれば `Pending`（締切前のみ）
4. それ以外は確定。ロンが最高優先なら**ロンした全員**を席順で返す
5. 締切を過ぎた未応答はパスとして扱う

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::command::{ActionOption, CallResponse};
    use protocol::notation::parse_tile;
    use protocol::seat::Seat;

    const MIN_WAIT: u32 = 350;

    fn ron_only() -> Vec<ActionOption> {
        vec![ActionOption::Ron]
    }

    fn pon_only() -> Vec<ActionOption> {
        vec![ActionOption::Pon { candidates: vec![] }]
    }

    fn chi_only() -> Vec<ActionOption> {
        vec![ActionOption::Chi { candidates: vec![] }]
    }

    fn window(candidates: [Vec<ActionOption>; 4]) -> ReactionWindow {
        ReactionWindow::open(
            1,
            WindowKind::Discard,
            Seat::new(0),
            parse_tile("3p").unwrap(),
            candidates,
            0,
            5_000,
        )
    }

    fn pon_response() -> CallResponse {
        CallResponse::Pon {
            tiles: [parse_tile("3p").unwrap(); 2],
        }
    }

    fn chi_response() -> CallResponse {
        CallResponse::Chi {
            tiles: [parse_tile("2p").unwrap(), parse_tile("4p").unwrap()],
        }
    }

    /// 全員が答えても最低待機の前は確定しない。
    /// 鳴ける者がいない局面と間の長さを揃え、情報を漏らさないため。
    #[test]
    fn nothing_resolves_before_the_minimum_wait() {
        let mut w = window([vec![], vec![], chi_only(), vec![]]);
        w.respond(Seat::new(2), CallResponse::Pass).unwrap();
        assert_eq!(w.resolve(349, MIN_WAIT), Outcome::Pending);
        assert_eq!(w.resolve(350, MIN_WAIT), Outcome::PassAll);
    }

    #[test]
    fn a_window_with_no_candidates_passes_after_the_wait() {
        let w = window([vec![], vec![], vec![], vec![]]);
        assert_eq!(w.resolve(349, MIN_WAIT), Outcome::Pending);
        assert_eq!(w.resolve(350, MIN_WAIT), Outcome::PassAll);
    }

    /// ポンが確定すれば、チーしか出せない未応答者は待たない。
    #[test]
    fn a_pon_resolves_without_waiting_for_a_chi_candidate() {
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(1), pon_response()).unwrap();
        match w.resolve(400, MIN_WAIT) {
            Outcome::Call { seat, .. } => assert_eq!(seat, Seat::new(1)),
            other => panic!("ポンで確定するはず: {other:?}"),
        }
    }

    /// チーが答えても、ポンできる未応答者がいれば待つ。
    #[test]
    fn a_chi_waits_for_a_pending_pon_candidate() {
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(3), chi_response()).unwrap();
        assert_eq!(w.resolve(400, MIN_WAIT), Outcome::Pending);
    }

    /// ポンがパスすれば、チーが確定する。
    #[test]
    fn a_chi_resolves_once_the_pon_candidate_passes() {
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(3), chi_response()).unwrap();
        w.respond(Seat::new(1), CallResponse::Pass).unwrap();
        match w.resolve(400, MIN_WAIT) {
            Outcome::Call { seat, .. } => assert_eq!(seat, Seat::new(3)),
            other => panic!("チーで確定するはず: {other:?}"),
        }
    }

    /// ロンが1つ確定しても、ロン可能な未応答者がいれば待つ。
    /// ここを「より上」にするとダブロンが原理的に成立しなくなる。
    #[test]
    fn a_ron_waits_for_other_ron_candidates() {
        let mut w = window([vec![], ron_only(), ron_only(), vec![]]);
        w.respond(Seat::new(1), CallResponse::Ron).unwrap();
        assert_eq!(w.resolve(400, MIN_WAIT), Outcome::Pending);

        w.respond(Seat::new(2), CallResponse::Ron).unwrap();
        assert_eq!(
            w.resolve(400, MIN_WAIT),
            Outcome::Ron(vec![Seat::new(1), Seat::new(2)])
        );
    }

    /// もう一方がパスすれば、単独のロンで確定する。
    #[test]
    fn a_single_ron_resolves_once_the_other_candidate_passes() {
        let mut w = window([vec![], ron_only(), ron_only(), vec![]]);
        w.respond(Seat::new(1), CallResponse::Ron).unwrap();
        w.respond(Seat::new(2), CallResponse::Pass).unwrap();
        assert_eq!(w.resolve(400, MIN_WAIT), Outcome::Ron(vec![Seat::new(1)]));
    }

    /// 3人がロンすれば全員を返す。三家和にするかは呼び出し側が決める。
    #[test]
    fn three_rons_are_all_reported() {
        let mut w = window([vec![], ron_only(), ron_only(), ron_only()]);
        for seat in [1u8, 2, 3] {
            w.respond(Seat::new(seat), CallResponse::Ron).unwrap();
        }
        assert_eq!(
            w.resolve(400, MIN_WAIT),
            Outcome::Ron(vec![Seat::new(1), Seat::new(2), Seat::new(3)])
        );
    }

    /// ロンは席順で返す。ダブロンの供託と本場の割り当てが決定的になる。
    #[test]
    fn rons_are_reported_in_seat_order() {
        let mut w = window([vec![], ron_only(), ron_only(), ron_only()]);
        for seat in [3u8, 1, 2] {
            w.respond(Seat::new(seat), CallResponse::Ron).unwrap();
        }
        assert_eq!(
            w.resolve(400, MIN_WAIT),
            Outcome::Ron(vec![Seat::new(1), Seat::new(2), Seat::new(3)])
        );
    }

    #[test]
    fn the_deadline_turns_silence_into_a_pass() {
        let w = window([vec![], pon_only(), vec![], vec![]]);
        assert_eq!(w.resolve(400, MIN_WAIT), Outcome::Pending);
        assert_eq!(w.resolve(5_001, MIN_WAIT), Outcome::PassAll);
    }

    /// 締切を過ぎても、既に答えた鳴きは有効である。
    #[test]
    fn the_deadline_keeps_an_answer_that_already_arrived() {
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(1), pon_response()).unwrap();
        match w.resolve(5_001, MIN_WAIT) {
            Outcome::Call { seat, .. } => assert_eq!(seat, Seat::new(1)),
            other => panic!("ポンが残るはず: {other:?}"),
        }
    }

    #[test]
    fn a_seat_without_candidates_cannot_respond() {
        let mut w = window([vec![], pon_only(), vec![], vec![]]);
        assert!(matches!(
            w.respond(Seat::new(2), CallResponse::Ron),
            Err(Rejection::NotACandidate)
        ));
    }

    #[test]
    fn the_discarder_cannot_respond_to_their_own_discard() {
        let mut w = window([ron_only(), vec![], vec![], vec![]]);
        assert!(matches!(
            w.respond(Seat::new(0), CallResponse::Ron),
            Err(Rejection::IsTheDiscarder)
        ));
    }

    #[test]
    fn responding_twice_is_rejected() {
        let mut w = window([vec![], pon_only(), vec![], vec![]]);
        w.respond(Seat::new(1), CallResponse::Pass).unwrap();
        assert!(matches!(
            w.respond(Seat::new(1), CallResponse::Pass),
            Err(Rejection::AlreadyResponded)
        ));
    }

    #[test]
    fn a_response_outside_the_offered_options_is_rejected() {
        let mut w = window([vec![], chi_only(), vec![], vec![]]);
        assert!(matches!(
            w.respond(Seat::new(1), CallResponse::Ron),
            Err(Rejection::NotOffered)
        ));
    }

    /// パスは候補を持つ席なら常に許す。
    #[test]
    fn passing_is_always_allowed_for_a_candidate() {
        let mut w = window([vec![], chi_only(), vec![], vec![]]);
        assert!(w.respond(Seat::new(1), CallResponse::Pass).is_ok());
    }

    /// 槍槓のウィンドウはロンだけを受け付ける。
    #[test]
    fn a_chankan_window_only_offers_ron() {
        let w = ReactionWindow::open(
            2,
            WindowKind::Chankan,
            Seat::new(0),
            parse_tile("5s").unwrap(),
            [vec![], ron_only(), vec![], vec![]],
            0,
            5_000,
        );
        assert_eq!(w.kind(), WindowKind::Chankan);
    }

    /// 明槓はポンと同順位。チーしか出せない席を待たずに確定する。
    #[test]
    fn a_minkan_has_the_same_priority_as_a_pon() {
        let kan_only = vec![ActionOption::Kan { candidates: vec![] }];
        let mut w = window([vec![], kan_only, vec![], chi_only()]);
        w.respond(Seat::new(1), CallResponse::Kan).unwrap();
        match w.resolve(400, MIN_WAIT) {
            Outcome::Call { seat, .. } => assert_eq!(seat, Seat::new(1)),
            other => panic!("明槓で確定するはず: {other:?}"),
        }
    }

    /// チーが答えても、明槓できる未応答者がいれば待つ。
    #[test]
    fn a_chi_waits_for_a_pending_minkan_candidate() {
        let kan_only = vec![ActionOption::Kan { candidates: vec![] }];
        let mut w = window([vec![], kan_only, vec![], chi_only()]);
        w.respond(Seat::new(3), chi_response()).unwrap();
        assert_eq!(w.resolve(400, MIN_WAIT), Outcome::Pending);
    }

    /// 槍槓のウィンドウはロン以外を受け付けない。
    ///
    /// **候補にロンしか無いから拒否される、では検査になっていない。**
    /// ポンも載せて `open` へ渡し、それが落とされた結果として
    /// ポンの応答が通らないことを見る。
    #[test]
    fn a_chankan_window_drops_non_ron_candidates() {
        let mut offered = ron_only();
        offered.push(ActionOption::Pon { candidates: vec![] });
        let mut w = ReactionWindow::open(
            2,
            WindowKind::Chankan,
            Seat::new(0),
            parse_tile("5s").unwrap(),
            [vec![], offered, vec![], vec![]],
            0,
            5_000,
        );
        assert!(
            matches!(
                w.respond(Seat::new(1), pon_response()),
                Err(Rejection::NotOffered)
            ),
            "槍槓でポンを受理した"
        );
        assert!(w.respond(Seat::new(1), CallResponse::Ron).is_ok());
    }

    /// 打牌のウィンドウなら同じ候補でポンが通る。
    /// 上のテストが「候補が無いから落ちた」のではないことを示す。
    #[test]
    fn the_same_options_allow_a_pon_on_a_discard_window() {
        let mut offered = ron_only();
        offered.push(ActionOption::Pon { candidates: vec![] });
        let mut w = window([vec![], offered, vec![], vec![]]);
        assert!(w.respond(Seat::new(1), pon_response()).is_ok());
    }

    /// 同順位が3件並んでも検出できる。最初に見たものとだけ比べると
    /// チー→ポン→ポン の順で2つ目のポンを見落とす。
    #[test]
    fn non_ron_ties_are_detected_regardless_of_order() {
        let mut w = window([vec![], pon_only(), pon_only(), chi_only()]);
        w.respond(Seat::new(3), chi_response()).unwrap();
        w.respond(Seat::new(1), pon_response()).unwrap();
        w.respond(Seat::new(2), pon_response()).unwrap();
        let ties = w.non_ron_ties();
        assert_eq!(ties.len(), 2, "ポンの競合2件を検出するはず: {ties:?}");
        assert!(ties.contains(&Seat::new(1)) && ties.contains(&Seat::new(2)));
    }

    /// 非ロンの同順位は牌の枚数上ありえない。
    /// 2人がポンするには各自2枚＋捨て牌1枚で5枚必要だが、牌は1種4枚しかない。
    /// ポンと明槓の競合も6枚必要で成立しない。席順ロジックは書かず検査で守る。
    #[test]
    fn non_ron_ties_never_occur() {
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(1), pon_response()).unwrap();
        w.respond(Seat::new(3), chi_response()).unwrap();
        assert!(w.non_ron_ties().is_empty(), "同順位の非ロン競合が現れた");
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine reaction`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

**`respond` は応答の「種類」が提示済みかだけを見る。** 具体的にどの牌を使うかの
妥当性は進行側（Wave 2b）が確かめる。ここまで見ると責務が二重になり、
テストも候補の中身に引きずられる。そのためテストでは `candidates: vec![]` で
種類だけを表現している。

```rust
//! 打牌と槓宣言に対する反応の受付。
//!
//! 早期確定の条件は「現在の最高優先度**以上**を出せる未応答者がいなければ確定」。
//! 「より上」にするとダブロンが原理的に成立しなくなる（仕様 6.4）。

use protocol::command::{ActionOption, CallResponse};
use protocol::seat::Seat;
use protocol::tile::Tile;

/// 応答の優先度。宣言順がそのまま順序になる。
/// **明槓はポンと同順位**なので、専用の値を作らず Pon へ写す。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Priority {
    Pass,
    Chi,
    Pon,
    Ron,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowKind {
    /// 打牌への反応。チー・ポン・明槓・ロンを受け付ける。
    Discard,
    /// 槓宣言への反応。ロンだけを受け付ける。
    Chankan,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rejection {
    /// その席に候補を提示していない。
    NotACandidate,
    AlreadyResponded,
    /// 提示していない種類の応答。
    NotOffered,
    /// 打牌者自身は応答できない。
    IsTheDiscarder,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    Pending,
    /// ロンした席を席順で返す。3人なら三家和として呼び出し側が流局にする。
    Ron(Vec<Seat>),
    Call { seat: Seat, response: CallResponse },
    PassAll,
}

fn priority_of_response(response: &CallResponse) -> Priority {
    match response {
        CallResponse::Pass => Priority::Pass,
        CallResponse::Chi { .. } => Priority::Chi,
        // 明槓はポンと同順位。
        CallResponse::Pon { .. } | CallResponse::Kan => Priority::Pon,
        CallResponse::Ron => Priority::Ron,
    }
}

fn priority_of_option(option: &ActionOption) -> Priority {
    match option {
        ActionOption::Chi { .. } => Priority::Chi,
        ActionOption::Pon { .. } | ActionOption::Kan { .. } => Priority::Pon,
        ActionOption::Ron => Priority::Ron,
        _ => Priority::Pass,
    }
}

pub struct ReactionWindow {
    id: u32,
    kind: WindowKind,
    from: Seat,
    tile: Tile,
    candidates: [Vec<ActionOption>; 4],
    responses: [Option<CallResponse>; 4],
    opened_at_ms: u64,
    deadline_ms: u64,
}

impl ReactionWindow {
    pub fn open(
        id: u32,
        kind: WindowKind,
        from: Seat,
        tile: Tile,
        candidates: [Vec<ActionOption>; 4],
        opened_at_ms: u64,
        deadline_ms: u64,
    ) -> Self {
        // 槍槓のウィンドウはロンしか受け付けない。
        // debug_assert で呼び出し側の契約にすると、release では素通りし、
        // debug ではテストが検証したい経路より先に落ちる。
        // **ここで落として不変条件を構造で保証する。**
        let candidates = if kind == WindowKind::Chankan {
            candidates.map(|options| {
                options
                    .into_iter()
                    .filter(|o| matches!(o, ActionOption::Ron))
                    .collect()
            })
        } else {
            candidates
        };

        ReactionWindow {
            id,
            kind,
            from,
            tile,
            candidates,
            responses: [None, None, None, None],
            opened_at_ms,
            deadline_ms,
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn kind(&self) -> WindowKind {
        self.kind
    }

    pub fn tile(&self) -> Tile {
        self.tile
    }

    pub fn from(&self) -> Seat {
        self.from
    }

    pub fn respond(&mut self, seat: Seat, response: CallResponse) -> Result<(), Rejection> {
        if seat == self.from {
            return Err(Rejection::IsTheDiscarder);
        }
        let offered = &self.candidates[seat.index()];
        if offered.is_empty() {
            return Err(Rejection::NotACandidate);
        }
        if self.responses[seat.index()].is_some() {
            return Err(Rejection::AlreadyResponded);
        }
        // パスは候補を持つ席なら常に許す。
        // 槍槓でロン以外が弾かれるのは、open が候補から落としているためである。
        if response != CallResponse::Pass {
            let wanted = priority_of_response(&response);
            let offered_here = offered.iter().any(|o| priority_of_option(o) == wanted);
            if !offered_here {
                return Err(Rejection::NotOffered);
            }
        }
        self.responses[seat.index()] = Some(response);
        Ok(())
    }

    /// 状態を変えずに現在の結論を返す。同じ入力からは同じ答えが出る。
    pub fn resolve(&self, now_ms: u64, min_wait_ms: u32) -> Outcome {
        // 全員が答えていても最低待機の前は確定しない。
        // 鳴ける者がいない局面と間の長さを揃え、情報を漏らさないため。
        if now_ms < self.opened_at_ms + min_wait_ms as u64 {
            return Outcome::Pending;
        }
        let expired = now_ms > self.deadline_ms;

        let best = Seat::ALL
            .iter()
            .filter_map(|s| self.responses[s.index()].as_ref())
            .map(priority_of_response)
            .max()
            .unwrap_or(Priority::Pass);

        if !expired {
            // 未応答者が best 以上を出せるなら待つ。「より上」ではない。
            let someone_could_match = Seat::ALL.iter().any(|s| {
                self.responses[s.index()].is_none()
                    && self.candidates[s.index()]
                        .iter()
                        .any(|o| priority_of_option(o) >= best)
            });
            if someone_could_match {
                return Outcome::Pending;
            }
        }

        if best == Priority::Ron {
            let rons: Vec<Seat> = Seat::ALL
                .iter()
                .copied()
                .filter(|s| self.responses[s.index()] == Some(CallResponse::Ron))
                .collect();
            return Outcome::Ron(rons);
        }

        for seat in Seat::ALL {
            if let Some(response) = self.responses[seat.index()] {
                if priority_of_response(&response) == best && best != Priority::Pass {
                    return Outcome::Call { seat, response };
                }
            }
        }
        Outcome::PassAll
    }

    /// ロン以外で同じ優先度の応答が2つ以上ある席。
    ///
    /// 牌は1種4枚しかないため起こりえない。起きたら呼び出し側が落とす。
    /// **優先度ごとに数える。**最初に見たものとだけ比べると、
    /// チー→ポン→ポン の順で来たときに2つ目のポンを見落とす。
    pub fn non_ron_ties(&self) -> Vec<Seat> {
        let mut ties = Vec::new();
        for target in [Priority::Chi, Priority::Pon] {
            let matching: Vec<Seat> = Seat::ALL
                .iter()
                .copied()
                .filter(|s| {
                    self.responses[s.index()]
                        .as_ref()
                        .map(priority_of_response)
                        == Some(target)
                })
                .collect();
            if matching.len() >= 2 {
                ties.extend(matching);
            }
        }
        ties
    }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine reaction`
Expected: 23テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 反応ウィンドウの早期確定を実装"
```

---

### Task 3: 時間の計算

**Files:**
- Create: `crates/mahjong-engine/src/timing.rs`

このファイルは Task 4 の `state.rs` から `#[path]` で読み込む。**単体では
モジュール木に載らないため、テストは `state.rs` 経由で走る。**

**Interfaces:**
- Produces:
  - `pub fn lead_in_of(events: &[Event]) -> u32`
  - `pub fn deadline_for(rules: &Ruleset, now_ms: u64, bank_remaining_ms: u32, lead_in_ms: u32) -> u64`
  - `pub fn charge_bank(rules: &Ruleset, bank_remaining_ms: u32, elapsed_ms: u64, lead_in_ms: u32) -> u32`
  - `pub fn remaining_for_event(absolute_deadline: u64, now_ms: u64) -> u32`

**課金式（仕様 6.2.2）:**

```
思考に使った時間 = max(0, 実時間 − lead_in − 通信猶予)
引き落とし       = max(0, 思考に使った時間 − 基準思考時間)
```

**通信猶予を引くのを忘れない。** 引かないと、基準時間内に答えても
500ms がバンクから減る。

- [ ] **Step 1: 失敗するテストを書く**

`crates/mahjong-engine/src/timing.rs` の末尾に置く。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::event::{DiscardManner, DrawSource, Event};
    use protocol::meld::MeldKind;
    use protocol::notation::parse_tile;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::Seat;

    fn rules() -> Ruleset {
        Ruleset::kin_no_ma(MatchLength::Hanchan)
    }

    /// 槓の演出は宣言が持ち、成立は0。
    /// KanDeclared 1100 + Call 0 + DoraReveal 800 + Draw 250 + Discard 350 = 2500
    #[test]
    fn lead_in_counts_a_kan_animation_once() {
        let events = vec![
            Event::KanDeclared {
                seat: Seat::new(1),
                kind: MeldKind::Kakan,
                tile: parse_tile("5s").unwrap(),
            },
            Event::Call {
                seat: Seat::new(1),
                from: Seat::new(1),
                kind: MeldKind::Kakan,
                tiles: vec![parse_tile("5s").unwrap()],
            },
            Event::DoraReveal {
                indicator: parse_tile("1z").unwrap(),
            },
            Event::Draw {
                seat: Seat::new(1),
                tile: parse_tile("2m").unwrap(),
                source: DrawSource::DeadWall,
                wall_remaining: 60,
            },
            Event::Discard {
                seat: Seat::new(1),
                tile: parse_tile("1m").unwrap(),
                manner: DiscardManner::Tsumogiri,
            },
        ];
        assert_eq!(lead_in_of(&events), 2_500);
    }

    #[test]
    fn an_empty_event_list_has_no_lead_in() {
        assert_eq!(lead_in_of(&[]), 0);
    }

    #[test]
    fn the_deadline_pushes_back_by_the_lead_in() {
        let plain = deadline_for(&rules(), 10_000, 20_000, 0);
        assert_eq!(plain, 10_000 + 5_000 + 20_000 + 500);
        assert_eq!(deadline_for(&rules(), 10_000, 20_000, 1_800), plain + 1_800);
    }

    #[test]
    fn an_empty_bank_leaves_only_the_base_time() {
        assert_eq!(deadline_for(&rules(), 0, 0, 0), 5_500);
    }

    #[test]
    fn answering_within_the_base_time_costs_nothing() {
        assert_eq!(charge_bank(&rules(), 20_000, 4_000, 0), 20_000);
        assert_eq!(charge_bank(&rules(), 20_000, 5_000, 0), 20_000);
    }

    /// 通信猶予はバンクから引かない。基準時間ちょうど＋猶予でも減らない。
    #[test]
    fn the_network_grace_is_not_charged() {
        assert_eq!(charge_bank(&rules(), 20_000, 5_500, 0), 20_000);
        assert_eq!(charge_bank(&rules(), 20_000, 5_501, 0), 19_999);
    }

    /// 演出を見ていた時間も課金しない。
    /// 実時間 8000 − 演出 1800 − 猶予 500 = 思考 5700。超過は 700。
    #[test]
    fn the_lead_in_is_not_charged() {
        assert_eq!(charge_bank(&rules(), 20_000, 8_000, 1_800), 19_300);
    }

    /// 実時間 8000 − 猶予 500 = 思考 7500。超過は 2500。
    #[test]
    fn overtime_comes_out_of_the_bank() {
        assert_eq!(charge_bank(&rules(), 20_000, 8_000, 0), 17_500);
    }

    #[test]
    fn the_bank_never_goes_below_zero() {
        assert_eq!(charge_bank(&rules(), 1_000, 30_000, 0), 0);
    }

    /// イベントへ載せる残り時間は要求発行時点からの相対値。
    #[test]
    fn the_event_carries_a_relative_deadline() {
        assert_eq!(remaining_for_event(35_500, 10_000), 25_500);
        assert_eq!(remaining_for_event(10_000, 10_000), 0);
        assert_eq!(remaining_for_event(9_000, 10_000), 0, "既に過ぎていたら0");
    }
}
```

- [ ] **Step 2: 実装を書く**

```rust
//! 締切と溜め時間バンクの計算。
//!
//! 仕様 6.2.1 と 6.2.2 をそのまま関数にしたもの。状態を持たない。

use protocol::effect::{effect_duration_ms, effect_of};
use protocol::event::Event;
use protocol::ruleset::Ruleset;

/// 直前に配信した一連のイベントの演出時間の合計。
///
/// 呼び出し側は「前回その席へ RequestAction を送ってから今回まで」に
/// 絞ったイベント列を渡す。区間の切り出しは進行側（Wave 2b）の責務である。
pub fn lead_in_of(events: &[Event]) -> u32 {
    events
        .iter()
        .filter_map(effect_of)
        .map(effect_duration_ms)
        .sum()
}

/// 行動要求の絶対締切。演出で思考時間が削られないよう lead_in を加算する。
pub fn deadline_for(
    rules: &Ruleset,
    now_ms: u64,
    bank_remaining_ms: u32,
    lead_in_ms: u32,
) -> u64 {
    now_ms
        + rules.base_think_ms as u64
        + bank_remaining_ms as u64
        + rules.network_grace_ms as u64
        + lead_in_ms as u64
}

/// 応答を受け取ったあとのバンク残量。
///
/// **演出を見ていた時間と通信の遅れは課金しない。** 課金すると 6.2 で
/// 締切を後ろへずらした意味が失われる。
pub fn charge_bank(
    rules: &Ruleset,
    bank_remaining_ms: u32,
    elapsed_ms: u64,
    lead_in_ms: u32,
) -> u32 {
    let excluded = lead_in_ms as u64 + rules.network_grace_ms as u64;
    let thinking = elapsed_ms.saturating_sub(excluded);
    let overtime = thinking.saturating_sub(rules.base_think_ms as u64);
    bank_remaining_ms.saturating_sub(overtime.min(u32::MAX as u64) as u32)
}

/// イベントへ載せる残り時間。要求発行時点からの相対値で、既に過ぎていたら 0。
pub fn remaining_for_event(absolute_deadline: u64, now_ms: u64) -> u32 {
    let remaining = absolute_deadline.saturating_sub(now_ms);
    debug_assert!(
        remaining <= u32::MAX as u64,
        "締切が u32 に収まらない: {remaining}"
    );
    remaining.min(u32::MAX as u64) as u32
}
```

- [ ] **Step 3: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine timing`（Task 4 完了後に走る）
Expected: 10テスト PASS

- [ ] **Step 4: コミット**

```bash
git commit -m "feat(engine): 締切とバンクの計算を実装"
```

---

### Task 4: 局の状態

**Files:**
- Modify: `crates/mahjong-engine/src/state.rs`

**先頭に `timing` の読み込みを書く。**

```rust
#[path = "timing.rs"]
mod timing;
pub use timing::{charge_bank, deadline_for, lead_in_of, remaining_for_event};
```

**Interfaces:**
- Produces:
  - `pub struct Discarded { pub tile: Tile, pub manner: DiscardManner, pub called_by: Option<Seat>, pub riichi_declaration: bool }`
  - `pub struct RiichiState { pub step: RiichiStep, pub declared_at_turn: u32, pub ippatsu: bool, pub double: bool }`
  - `pub struct PendingKan { pub seat: Seat, pub kind: MeldKind, pub tile: Tile }`
  - `pub struct SeatState { ... }`
  - `pub struct RoundState { ... }`
  - `RoundState::new(rules, round, dealer, honba, riichi_sticks, scores, seed) -> Self`
  - `RoundState::seat(&self, Seat) -> &SeatState` / `seat_mut`
  - `RoundState::hand_counts(&self, Seat) -> HandCounts`
  - `RoundState::is_menzen(&self, Seat) -> bool`
  - `RoundState::seat_wind(&self, Seat) -> Wind`
  - `RoundState::begin_turn(&mut self, Seat)` — 同巡内フリテンを解除する
  - `RoundState::hand_context(&self, Seat, win_type: WinType) -> HandContext`

**`HandContext` を組み立てるのに必要な状態を、ここで漏れなく持つ。**

| `HandContext` の項目 | 由来 |
|---|---|
| `win_type` | 引数で受け取る |
| `seat_wind` | `seat_wind(seat)` |
| `round_wind` | `round.wind` |
| `riichi` | `seat.riichi` が `Some` かつ `step == Accepted` |
| `double_riichi` | 同上かつ `double == true` |
| `ippatsu` | `seat.riichi` の `ippatsu` |
| `rinshan` | ツモ和了 かつ `last_draw == Some((その席, DeadWall))` |
| `chankan` | `pending_kan.is_some()` かつ ロン和了 |
| `haitei` | `wall.live_remaining() == 0` かつ ツモ和了 |
| `houtei` | `wall.live_remaining() == 0` かつ ロン和了 |
| `tenhou` | **ツモ和了** かつ 親 かつ `draw_count[seat] == 1` かつ `!any_call_made` |
| `chiihou` | **ツモ和了** かつ 子 かつ `draw_count[seat] == 1` かつ `!any_call_made` |
| `dora_indicators` | `wall.dora_indicators()` |
| `ura_indicators` | リーチ成立済みなら `wall.ura_indicators()`、でなければ空 |

**保持する状態:**

```rust
pub struct SeatState {
    pub hand: Vec<Tile>,
    pub melds: Vec<Meld>,
    pub river: Vec<Discarded>,
    pub riichi: Option<RiichiState>,
    pub think_bank_ms: u32,
    /// 同巡内フリテン。自分のツモで解除される。
    pub passed_this_turn: Vec<TileKind>,
    /// リーチ後にロンを見逃した待ち。**局の終わりまで解除されない。**
    pub permanent_furiten: Vec<TileKind>,
    /// 自分の捨て牌がすべて幺九牌で、一度も鳴かれていないか（流し満貫）。
    pub nagashi_alive: bool,
}

pub struct RoundState {
    pub rules: Ruleset,
    pub round: Round,
    pub dealer: Seat,
    pub honba: u8,
    pub riichi_sticks: u8,
    pub scores: [i32; 4],
    pub wall: Wall,
    pub seats: [SeatState; 4],
    /// 直前のツモを引いた席と、その出どころ。嶺上開花の判定に使う。
    /// 席を持たせるのは、誰のツモだったかを取り違えないためである。
    pub last_draw: Option<(Seat, DrawSource)>,
    /// 各席が何回ツモしたか。天和・地和の判定に使う。
    pub draw_count: [u32; 4],
    /// 局を通して誰か1人でも鳴いたか。
    pub any_call_made: bool,
    /// 槍槓の受付中かどうか。
    pub pending_kan: Option<PendingKan>,
    /// 席ごとの確定した槓の数。四開槓の判定に使う。
    pub kan_count: [u8; 4],
    /// 1巡目に切られた風牌。四風連打の判定に使う。
    pub first_turn_winds: Vec<TileKind>,
}
```

**リーチ棒は1000点。** `Ruleset` にこの値は無いため、このクレート内に
名前付き定数として置く。

```rust
/// リーチ宣言時に供託する点数。リーチ麻雀では普遍の値であり、
/// Ruleset に設定項目として存在しない。
pub const RIICHI_STICK: i32 = 1_000;
```

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::wall::Seed;
    use protocol::notation::parse_hand;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::{Round, Seat, Wind};
    use protocol::tile::TileKind;

    fn fresh() -> RoundState {
        RoundState::new(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            Round {
                wind: Wind::East,
                number: 1,
            },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &Seed::from_hex(&"11".repeat(32)).unwrap(),
        )
    }

    #[test]
    fn every_seat_starts_with_thirteen_tiles() {
        let state = fresh();
        for seat in Seat::ALL {
            assert_eq!(state.seat(seat).hand.len(), 13);
        }
    }

    /// 122 − 13×4 = 70
    #[test]
    fn the_wall_loses_exactly_the_dealt_tiles() {
        assert_eq!(fresh().wall.live_remaining(), 70);
    }

    #[test]
    fn seat_winds_follow_the_dealer() {
        let state = fresh();
        assert_eq!(state.seat_wind(Seat::new(0)), Wind::East);
        assert_eq!(state.seat_wind(Seat::new(1)), Wind::South);
        assert_eq!(state.seat_wind(Seat::new(2)), Wind::West);
        assert_eq!(state.seat_wind(Seat::new(3)), Wind::North);
    }

    /// 親が席2なら、席2が東で席3が南。
    #[test]
    fn seat_winds_rotate_with_a_different_dealer() {
        let state = RoundState::new(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            Round {
                wind: Wind::East,
                number: 3,
            },
            Seat::new(2),
            0,
            0,
            [25_000; 4],
            &Seed::from_hex(&"22".repeat(32)).unwrap(),
        );
        assert_eq!(state.seat_wind(Seat::new(2)), Wind::East);
        assert_eq!(state.seat_wind(Seat::new(3)), Wind::South);
        assert_eq!(state.seat_wind(Seat::new(0)), Wind::West);
    }

    #[test]
    fn a_hand_with_no_melds_is_menzen() {
        assert!(fresh().is_menzen(Seat::new(0)));
    }

    #[test]
    fn every_seat_starts_with_a_full_think_bank() {
        let state = fresh();
        for seat in Seat::ALL {
            assert_eq!(state.seat(seat).think_bank_ms, 20_000);
        }
    }

    #[test]
    fn hand_counts_ignore_red_fives() {
        let mut state = fresh();
        state.seat_mut(Seat::new(0)).hand = parse_hand("0p5p").unwrap();
        let counts = state.hand_counts(Seat::new(0));
        assert_eq!(counts.get(TileKind::from_index(13).unwrap()), 2);
    }

    /// リーチ後の見逃しは局の終わりまで解除されない。
    /// 同巡内フリテンだけが自分のツモで消える。
    #[test]
    fn permanent_furiten_survives_the_next_draw() {
        let mut state = fresh();
        let seat = Seat::new(0);
        let kind = protocol::notation::parse_tile("3p").unwrap().kind();
        state.seat_mut(seat).passed_this_turn.push(kind);
        state.seat_mut(seat).permanent_furiten.push(kind);

        state.begin_turn(seat);
        assert!(
            state.seat(seat).passed_this_turn.is_empty(),
            "同巡内は解除される"
        );
        assert_eq!(
            state.seat(seat).permanent_furiten,
            vec![kind],
            "リーチ後の見逃しは残る"
        );
    }

    #[test]
    fn a_fresh_round_has_no_calls_and_no_kans() {
        let state = fresh();
        assert!(!state.any_call_made);
        assert_eq!(state.kan_count, [0; 4]);
        assert!(state.pending_kan.is_none());
        assert!(state.first_turn_winds.is_empty());
    }

    #[test]
    fn every_seat_starts_eligible_for_nagashi() {
        let state = fresh();
        for seat in Seat::ALL {
            assert!(state.seat(seat).nagashi_alive);
        }
    }

    /// 状況役がすべて偽の文脈を作れる。
    #[test]
    fn a_plain_hand_context_has_no_situational_yaku() {
        let state = fresh();
        let ctx = state.hand_context(Seat::new(1), WinType::Ron);
        assert!(!ctx.riichi);
        assert!(!ctx.ippatsu);
        assert!(!ctx.rinshan);
        assert!(!ctx.chankan);
        assert!(!ctx.haitei);
        assert!(!ctx.houtei);
        assert!(!ctx.tenhou);
        assert!(!ctx.chiihou);
        assert_eq!(ctx.seat_wind, Wind::South);
        assert_eq!(ctx.round_wind, Wind::East);
    }

    /// 親の第一ツモは天和の条件を満たす。
    #[test]
    fn the_dealers_first_draw_qualifies_for_tenhou() {
        let mut state = fresh();
        state.draw_count[0] = 1;
        let ctx = state.hand_context(Seat::new(0), WinType::Tsumo);
        assert!(ctx.tenhou);
        assert!(!ctx.chiihou);
    }

    /// 天和・地和はツモ和了に限る。第一巡のロンでは立たない。
    #[test]
    fn tenhou_and_chiihou_require_a_tsumo() {
        let mut state = fresh();
        state.draw_count[0] = 1;
        state.draw_count[1] = 1;
        assert!(!state.hand_context(Seat::new(0), WinType::Ron).tenhou);
        assert!(!state.hand_context(Seat::new(1), WinType::Ron).chiihou);
    }

    /// 子の第一ツモは地和。
    #[test]
    fn a_non_dealers_first_draw_qualifies_for_chiihou() {
        let mut state = fresh();
        state.draw_count[1] = 1;
        let ctx = state.hand_context(Seat::new(1), WinType::Tsumo);
        assert!(ctx.chiihou);
        assert!(!ctx.tenhou);
    }

    /// 誰かが鳴いていれば天和・地和は成立しない。
    #[test]
    fn a_call_disqualifies_tenhou_and_chiihou() {
        let mut state = fresh();
        state.draw_count[0] = 1;
        state.any_call_made = true;
        assert!(!state.hand_context(Seat::new(0), WinType::Tsumo).tenhou);
    }

    /// 嶺上からのツモは rinshan が立つ。
    #[test]
    fn a_dead_wall_draw_sets_rinshan() {
        let mut state = fresh();
        state.last_draw = Some((Seat::new(1), DrawSource::DeadWall));
        assert!(state.hand_context(Seat::new(1), WinType::Tsumo).rinshan);
        // ロンでは立たない
        assert!(!state.hand_context(Seat::new(1), WinType::Ron).rinshan);
        // 別の席のツモでは立たない
        assert!(!state.hand_context(Seat::new(2), WinType::Tsumo).rinshan);
    }

    /// 槍槓の受付中のロンは chankan が立つ。
    #[test]
    fn a_pending_kan_sets_chankan_on_a_ron() {
        let mut state = fresh();
        state.pending_kan = Some(PendingKan {
            seat: Seat::new(0),
            kind: protocol::meld::MeldKind::Kakan,
            tile: protocol::notation::parse_tile("5s").unwrap(),
        });
        assert!(state.hand_context(Seat::new(1), WinType::Ron).chankan);
        assert!(!state.hand_context(Seat::new(1), WinType::Tsumo).chankan);
    }

    /// 裏ドラはリーチが成立している席にだけ渡す。
    #[test]
    fn ura_indicators_are_only_given_to_a_riichi_seat() {
        let mut state = fresh();
        assert!(state
            .hand_context(Seat::new(1), WinType::Ron)
            .ura_indicators
            .is_empty());

        state.seat_mut(Seat::new(1)).riichi = Some(RiichiState {
            step: protocol::event::RiichiStep::Accepted,
            declared_at_turn: 3,
            ippatsu: false,
            double: false,
        });
        assert!(!state
            .hand_context(Seat::new(1), WinType::Ron)
            .ura_indicators
            .is_empty());
    }

    /// 宣言しただけで成立していないリーチは、役にもならず裏ドラも見られない。
    #[test]
    fn a_declared_but_unaccepted_riichi_is_not_yet_a_yaku() {
        let mut state = fresh();
        state.seat_mut(Seat::new(1)).riichi = Some(RiichiState {
            step: protocol::event::RiichiStep::Declare,
            declared_at_turn: 3,
            ippatsu: true,
            double: false,
        });
        let ctx = state.hand_context(Seat::new(1), WinType::Ron);
        assert!(!ctx.riichi);
        assert!(ctx.ura_indicators.is_empty());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine state`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

要点は3つ。

**1. 配牌は13枚ずつ。** 親の第一ツモは進行側（Wave 2b）が引く。`new` の時点では
全員13枚で、山は 122 − 52 = 70枚である。

```rust
//! 局の状態。進行のステートマシンは持たず、状態と導出だけを担う。
//!
//! 状況役の判定を進行側へ散らさないよう、HandContext の組み立てはここに集約する。

#[path = "timing.rs"]
mod timing;
pub use timing::{charge_bank, deadline_for, lead_in_of, remaining_for_event};

use mahjong_core::hand::HandCounts;
use mahjong_core::score::{HandContext, WinType};
use protocol::event::{DiscardManner, DrawSource, RiichiStep};
use protocol::meld::{Meld, MeldKind};
use protocol::ruleset::Ruleset;
use protocol::seat::{Round, Seat, Wind};
use protocol::tile::{Tile, TileKind};

use crate::wall::{Seed, Wall};

/// リーチ宣言時に供託する点数。リーチ麻雀では普遍の値であり、
/// Ruleset に設定項目として存在しない。
pub const RIICHI_STICK: i32 = 1_000;

/// 河に捨てられた1枚。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Discarded {
    pub tile: Tile,
    pub manner: DiscardManner,
    /// 鳴かれた場合、鳴いた席。**牌の総数を数えるときはこれが Some のものを除く**
    /// （鳴いた者の melds に入っているため）。
    pub called_by: Option<Seat>,
    /// リーチ宣言牌かどうか。横向きに置く演出と、四家立直の判定に使う。
    pub riichi_declaration: bool,
}

/// リーチの状態。宣言と成立を分ける。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RiichiState {
    /// `Declare` は宣言しただけ。`Accepted` で初めて役になり供託が出る。
    pub step: RiichiStep,
    pub declared_at_turn: u32,
    pub ippatsu: bool,
    pub double: bool,
}

/// 槍槓の受付中である槓。成立するまでここに置く。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PendingKan {
    pub seat: Seat,
    pub kind: MeldKind,
    pub tile: Tile,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeatState {
    pub hand: Vec<Tile>,
    pub melds: Vec<Meld>,
    pub river: Vec<Discarded>,
    pub riichi: Option<RiichiState>,
    pub think_bank_ms: u32,
    /// 同巡内フリテン。自分のツモで解除される。
    pub passed_this_turn: Vec<TileKind>,
    /// リーチ後にロンを見逃した待ち。**局の終わりまで解除されない。**
    pub permanent_furiten: Vec<TileKind>,
    /// 自分の捨て牌がすべて幺九牌で、一度も鳴かれていないか（流し満貫）。
    pub nagashi_alive: bool,
}

pub struct RoundState {
    pub rules: Ruleset,
    pub round: Round,
    pub dealer: Seat,
    pub honba: u8,
    pub riichi_sticks: u8,
    pub scores: [i32; 4],
    pub wall: Wall,
    pub seats: [SeatState; 4],
    /// 直前のツモを引いた席と、その出どころ。嶺上開花の判定に使う。
    /// 席を持たせるのは、誰のツモだったかを取り違えないためである。
    pub last_draw: Option<(Seat, DrawSource)>,
    /// 各席が何回ツモしたか。天和・地和の判定に使う。
    pub draw_count: [u32; 4],
    /// 局を通して誰か1人でも鳴いたか。
    pub any_call_made: bool,
    /// 槍槓の受付中かどうか。
    pub pending_kan: Option<PendingKan>,
    /// 席ごとの確定した槓の数。四開槓の判定に使う。
    pub kan_count: [u8; 4],
    /// 1巡目に切られた風牌。四風連打の判定に使う。
    pub first_turn_winds: Vec<TileKind>,
}

impl RoundState {
    pub fn new(
        rules: Ruleset,
        round: Round,
        dealer: Seat,
        honba: u8,
        riichi_sticks: u8,
        scores: [i32; 4],
        seed: &Seed,
    ) -> Self {
        let mut wall = Wall::new(seed, &rules);
        let bank = rules.think_bank_ms;

        let seats = std::array::from_fn(|_| {
            let hand = (0..13)
                .map(|_| wall.draw().expect("配牌の分は必ずある"))
                .collect();
            SeatState {
                hand,
                melds: Vec::new(),
                river: Vec::new(),
                riichi: None,
                think_bank_ms: bank,
                passed_this_turn: Vec::new(),
                permanent_furiten: Vec::new(),
                nagashi_alive: true,
            }
        });

        RoundState {
            rules,
            round,
            dealer,
            honba,
            riichi_sticks,
            scores,
            wall,
            seats,
            last_draw: None,
            draw_count: [0; 4],
            any_call_made: false,
            pending_kan: None,
            kan_count: [0; 4],
            first_turn_winds: Vec::new(),
        }
    }

    pub fn seat(&self, seat: Seat) -> &SeatState {
        &self.seats[seat.index()]
    }

    pub fn seat_mut(&mut self, seat: Seat) -> &mut SeatState {
        &mut self.seats[seat.index()]
    }

    pub fn hand_counts(&self, seat: Seat) -> HandCounts {
        HandCounts::from_tiles(&self.seat(seat).hand)
    }

    /// 暗槓は門前を崩さない。
    pub fn is_menzen(&self, seat: Seat) -> bool {
        self.seat(seat).melds.iter().all(|m| m.is_concealed())
    }

    /// **2. 自風は親からの距離で決まる。** 親が東、その下家が南。
    pub fn seat_wind(&self, seat: Seat) -> Wind {
        let offset = (seat.index() + 4 - self.dealer.index()) % 4;
        match offset {
            0 => Wind::East,
            1 => Wind::South,
            2 => Wind::West,
            _ => Wind::North,
        }
    }

    /// **3. `hand_context` は表のとおりに組み立てる。**
    /// 状況役の判定を進行側へ散らさず、ここに集約する。
    pub fn hand_context(&self, seat: Seat, win_type: WinType) -> HandContext {
        let is_tsumo = win_type == WinType::Tsumo;
        let riichi = self
            .seat(seat)
            .riichi
            .as_ref()
            .filter(|r| r.step == RiichiStep::Accepted);
        let exhausted = self.wall.live_remaining() == 0;
        let first_draw_untouched =
            self.draw_count[seat.index()] == 1 && !self.any_call_made;

        HandContext {
            win_type,
            seat_wind: self.seat_wind(seat),
            round_wind: self.round.wind,
            riichi: riichi.is_some(),
            double_riichi: riichi.map(|r| r.double).unwrap_or(false),
            ippatsu: riichi.map(|r| r.ippatsu).unwrap_or(false),
            rinshan: is_tsumo && self.last_draw == Some((seat, DrawSource::DeadWall)),
            chankan: !is_tsumo && self.pending_kan.is_some(),
            haitei: is_tsumo && exhausted,
            houtei: !is_tsumo && exhausted,
            tenhou: is_tsumo && seat == self.dealer && first_draw_untouched,
            chiihou: is_tsumo && seat != self.dealer && first_draw_untouched,
            dora_indicators: self.wall.dora_indicators().to_vec(),
            // 裏ドラはリーチが成立している席にだけ渡す。
            ura_indicators: if riichi.is_some() {
                self.wall.ura_indicators().to_vec()
            } else {
                Vec::new()
            },
        }
    }

    /// 自分のツモ番の始まり。**同巡内フリテンだけを消す。**
    /// 永続フリテン（リーチ後の見逃し）には触らない。
    pub fn begin_turn(&mut self, seat: Seat) {
        self.seats[seat.index()].passed_this_turn.clear();
    }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine state`
Expected: `state` 自身の19テストに加え、`state::timing` の10テストも走り計29テスト PASS

- [ ] **Step 5: コミット**

```bash
git commit -m "feat(engine): 局の状態と HandContext の組み立てを実装"
```

---

### Task 5: 不変条件

**Files:**
- Modify: `crates/mahjong-engine/src/invariant.rs`

**Interfaces:**
- Produces:
  - `pub fn assert_tiles_conserved(state: &RoundState)`
  - `pub fn assert_scores_conserved(before: &[i32; 4], after: &[i32; 4], sticks_delta: i32)`
  - `pub fn assert_no_simultaneous_non_ron(window: &ReactionWindow)`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::RoundState;
    use crate::wall::Seed;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::{Round, Seat, Wind};

    fn fresh() -> RoundState {
        RoundState::new(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            Round {
                wind: Wind::East,
                number: 1,
            },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &Seed::from_hex(&"33".repeat(32)).unwrap(),
        )
    }

    #[test]
    fn a_fresh_round_conserves_every_tile() {
        assert_tiles_conserved(&fresh());
    }

    /// 嶺上を引いた後も136枚のまま。
    /// tiles_in_wall が live_end で切っていると135枚になって落ちる。
    #[test]
    fn a_replacement_draw_keeps_the_count() {
        let mut state = fresh();
        let tile = state.wall.draw_replacement().expect("嶺上がある");
        state.seat_mut(Seat::new(0)).hand.push(tile);
        assert_tiles_conserved(&state);
    }

    #[test]
    #[should_panic(expected = "136")]
    fn a_missing_tile_is_caught() {
        let mut state = fresh();
        state.seat_mut(Seat::new(0)).hand.pop();
        assert_tiles_conserved(&state);
    }

    #[test]
    #[should_panic(expected = "136")]
    fn a_duplicated_tile_is_caught() {
        let mut state = fresh();
        let extra = state.seat(Seat::new(0)).hand[0];
        state.seat_mut(Seat::new(0)).hand.push(extra);
        assert_tiles_conserved(&state);
    }

    /// 点棒は卓の中を移動するだけ。供託の増減を含めて合計は変わらない。
    #[test]
    fn scores_and_sticks_balance() {
        assert_scores_conserved(&[25_000; 4], &[24_000, 26_000, 25_000, 25_000], 0);
        // リーチ棒が1本出た局面。手元から1000減り、供託が1000増える。
        assert_scores_conserved(&[25_000; 4], &[24_000, 25_000, 25_000, 25_000], 1_000);
        // 供託を回収した局面。
        assert_scores_conserved(&[25_000; 4], &[26_000, 25_000, 25_000, 25_000], -1_000);
    }

    #[test]
    #[should_panic(expected = "点棒")]
    fn a_score_leak_is_caught() {
        assert_scores_conserved(&[25_000; 4], &[24_000, 25_000, 25_000, 25_000], 0);
    }

    #[test]
    #[should_panic(expected = "点棒")]
    fn a_score_creation_is_caught() {
        assert_scores_conserved(&[25_000; 4], &[26_000, 25_000, 25_000, 25_000], 0);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine invariant`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
//! エンジンの不変条件。破れていれば即座に落とす。
//!
//! 麻雀は牌と点棒が増えも減りもしない閉じた系である。
//! 進行のどこかで壊れたら、その場で気づけるようにする。

use crate::reaction::ReactionWindow;
use crate::state::RoundState;
use protocol::seat::Seat;

/// 牌はちょうど136枚。**牌は1箇所にだけ属する。**
pub fn assert_tiles_conserved(state: &RoundState) {
    let mut total = 0usize;
    for seat in Seat::ALL {
        let s = state.seat(seat);
        total += s.hand.len();
        total += s.melds.iter().map(|m| m.tiles.len()).sum::<usize>();
        // 鳴かれた牌は鳴いた者の melds に入っている。両方数えると二重計上。
        total += s.river.iter().filter(|d| d.called_by.is_none()).count();
    }
    // all_tiles() は136枚すべてを返すので使わない。
    total += state.wall.tiles_in_wall().count();

    assert_eq!(total, 136, "牌の総数が136でない: {total}");
}

/// 点棒は卓の中を移動するだけ。供託の増減を含めて合計は変わらない。
///
/// `sticks_delta` は供託の増加量。リーチ棒が1本出れば +1000、
/// 回収されれば -1000 である。
pub fn assert_scores_conserved(before: &[i32; 4], after: &[i32; 4], sticks_delta: i32) {
    let before_total: i32 = before.iter().sum();
    let after_total: i32 = after.iter().sum();
    assert_eq!(
        after_total + sticks_delta,
        before_total,
        "点棒の合計が変わった: {before_total} → {after_total}（供託 {sticks_delta:+}）"
    );
}

/// 非ロンの同順位が同時に成立していないこと。
///
/// 牌は1種4枚しかないため、2人が同じ牌をポンすることも、ポンと明槓が
/// 競合することも起こりえない（仕様 6.4）。席順で解決するロジックを
/// 書かない代わりに、発生しないことをここで主張する。
pub fn assert_no_simultaneous_non_ron(window: &ReactionWindow) {
    let ties = window.non_ron_ties();
    assert!(ties.is_empty(), "非ロンの同順位が同時に成立した: {ties:?}");
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine invariant`
Expected: 7テスト PASS

- [ ] **Step 5: コミット**

**集計規則を先に決める。牌はちょうど1箇所に属する。**

| 場所 | 数える |
|---|---|
| 手牌 `SeatState::hand` | すべて |
| 副露 `SeatState::melds` の `tiles` | すべて |
| 河 `SeatState::river` | **`called_by.is_none()` のものだけ** |
| 山 `Wall::tiles_in_wall()` | まだ引かれていない生牌＋未取得の嶺上牌 |

**鳴かれた捨て牌を河から除く**のが要点である。鳴かれた牌は鳴いた者の
`Meld::tiles` にも入っているため、両方数えると二重計上になる。

同様に **`Wall::all_tiles()` を使わない。** あれは136枚すべてを返すので、
既に配った牌まで数えてしまう。必ず `tiles_in_wall()` を使う。

配牌直後で検算すると、手牌 13×4 = 52、河 0、副露 0、山 70 + 王牌 14 = 84。
合計 136 で一致する。

panic メッセージに「136」を含める（テストが `expected` で照合するため）。

`assert_scores_conserved` は `after の合計 + sticks_delta == before の合計` を見る。
panic メッセージに「点棒」を含める。

Run: `cargo test --package mahjong-engine invariant`
Expected: 7テスト PASS

```bash
git commit -m "feat(engine): 不変条件の検査を実装"
```

---

## Wave 2a 完了の判定

- [ ] `cargo test --workspace` が通る
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] 山の golden vector が固定されている（実際のハッシュが埋まっている）
- [ ] 王牌14枚の位置が重複しない
- [ ] `HandContext` の全14項目を組み立てられる
- [ ] `round.rs` / `match_flow.rs` / `lib.rs` を編集していない
- [ ] `protocol` と `mahjong-core` を編集していない
- [ ] `rand` を依存に足していない（`sha2` のみ）
- [ ] `Instant::now()` を呼んでいない

## Wave 2b（局と半荘の進行）へ渡すもの

この計画が作る部品を、Wave 2b が組み合わせて進行させる。

| 部品 | Wave 2b での使われ方 |
|---|---|
| `Wall` | 局開始時に生成。ツモ・嶺上・新ドラ |
| `ReactionWindow` | 打牌後と槓宣言後に開く |
| `RoundState` | 進行の全状態。`hand_context` で採点へ渡す |
| `timing::*` | 要求ごとに締切を出し、応答ごとにバンクを引く |
| `invariant::*` | 全イベントの後で検査する |
| `RIICHI_STICK` | リーチ成立時の供託 |
