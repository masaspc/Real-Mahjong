# Wave 2a: mahjong-engine 局の進行 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 配牌から終局までの局を進行させ、イベント列を生成するエンジンを作る。CPU 相当の単純な打ち手で対局を最後まで回せる状態にする。

**Architecture:** 乱数はシードとして、時間は `now_ms` として**外から注入する**。エンジン自体は決定的で、同じシードと同じ入力からは必ず同じイベント列が出る。これがリプレイ検証と不具合再現を可能にする。

**Tech Stack:** Rust 1.97.1 / edition 2021 / `protocol` と `mahjong-core` にのみ依存

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
**作業規約:** `AGENTS.md`（計画を書く側の検算義務を含む）

## Global Constraints

- **編集してよいのは `crates/mahjong-engine/src/` 配下のみ**（`lib.rs` を除く）
- `crates/protocol` と `crates/mahjong-core` は凍結済み。**編集も追加もしない。**足りなければ実装を止めて報告する
- **乱数を直接使わない。** `rand` などの依存を足さず、シードから決定的に生成する
- **時刻を直接読まない。** `Instant::now()` を呼ばず、`now_ms: u64` を引数で受け取る
- 局の進行に関わる定数は `Ruleset` から読む。ハードコードしない
- 完了条件は `cargo test --workspace` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` がすべて通ること

## 仕様のうち、ここで実装される決定

第6章で定義した時間モデルが、初めて実際に動くコードになる。

| 決定 | 出典 | 実装先 |
|---|---|---|
| 反応ウィンドウの早期確定は「以上」 | 6.4 | `reaction.rs` |
| 非ロンの同順位競合は発生しない（不変条件） | 6.4 | `reaction.rs` |
| 打牌から次のツモまで必ず 350ms 待つ | 6.4 | `reaction.rs` |
| `lead_in` = 前回その席へ要求を送ってから今回まで | 6.2.1 | `round.rs` |
| バンクは基準時間を超えた分だけ引く | 6.2.2 | `round.rs` |
| `window_id` は対局を通して単調増加 | 5.3.1 | `round.rs` |
| シードは局開始時に永続化できる形で持つ | 8.3 | `wall.rs` |

---

### Task 1: 決定的な山

**Files:**
- Modify: `crates/mahjong-engine/src/wall.rs`

**Interfaces:**
- Consumes: `protocol::tile::Tile`、`protocol::ruleset::Ruleset`
- Produces:
  - `pub struct Seed([u8; 32])` — `Seed::from_hex(&str) -> Option<Seed>`, `to_hex(&self) -> String`, `commitment(&self) -> String`
  - `pub struct Wall` — `Wall::new(seed: &Seed, rules: &Ruleset) -> Wall`
  - `Wall::draw(&mut self) -> Option<Tile>`、`draw_replacement(&mut self) -> Option<Tile>`
  - `Wall::reveal_dora(&mut self) -> Option<Tile>`、`ura_indicators(&self) -> &[Tile]`
  - `Wall::live_remaining(&self) -> u8`、`dora_indicators(&self) -> &[Tile]`

**山の構成:** 136枚。うち 5m/5p/5s の各1枚を赤ドラに置き換える（`rules.red_dora_count` が3のとき）。王牌14枚のうち、ドラ表示牌とその裏を5組ずつ持つ。ツモれる牌は 136 − 14 = 122枚。

**乱数:** `rand` を足さず、シードから決定的に生成する。実装は splitmix64 とする。バージョン間で挙動が変わらないことが重要である。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::tile::TileKind;

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
        assert!(
            counts.iter().all(|c| *c == 4),
            "34種が4枚ずつでない: {counts:?}"
        );
    }

    #[test]
    fn exactly_three_tiles_are_red() {
        let wall = Wall::new(&seed(2), &rules());
        let reds: Vec<u8> = wall
            .all_tiles()
            .filter(|t| t.is_red())
            .map(|t| t.kind().index())
            .collect();
        assert_eq!(reds.len(), 3);
        // 5m=4, 5p=13, 5s=22
        let mut sorted = reds.clone();
        sorted.sort();
        assert_eq!(sorted, vec![4, 13, 22]);
    }

    #[test]
    fn the_same_seed_always_produces_the_same_wall() {
        let a = Wall::new(&seed(7), &rules());
        let b = Wall::new(&seed(7), &rules());
        let first: Vec<u8> = a.all_tiles().map(|t| t.encoded()).collect();
        let second: Vec<u8> = b.all_tiles().map(|t| t.encoded()).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn different_seeds_produce_different_walls() {
        let a = Wall::new(&seed(1), &rules());
        let b = Wall::new(&seed(2), &rules());
        let first: Vec<u8> = a.all_tiles().map(|t| t.encoded()).collect();
        let second: Vec<u8> = b.all_tiles().map(|t| t.encoded()).collect();
        assert_ne!(first, second);
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

    /// 嶺上牌を引くと、その分だけツモれる牌が減る。
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
    fn dora_starts_with_one_indicator_and_can_reveal_up_to_five() {
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

    /// 開示前にハッシュを配り、開示後に一致を検算できる。
    #[test]
    fn a_seed_commits_to_itself() {
        let s = seed(9);
        let commitment = s.commitment();
        assert_eq!(commitment.len(), 64, "SHA-256 の hex は64文字");
        assert_eq!(Seed::from_hex(&s.to_hex()).unwrap().commitment(), commitment);
        assert_ne!(seed(10).commitment(), commitment);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine wall`
Expected: コンパイルエラー

- [ ] **Step 3: ハッシュの依存を足す**

コミットメントには暗号学的ハッシュが要る。自作しない。

```bash
cargo add --package mahjong-engine sha2
```

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
        let digest = Sha256::digest(self.0);
        digest.iter().map(|b| format!("{b:02x}")).collect()
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

/// 王牌の枚数。うち嶺上牌が4枚、ドラ表示とその裏が5組ずつ。
const DEAD_WALL: usize = 14;
const MAX_REPLACEMENTS: usize = 4;
const MAX_DORA: usize = 5;

pub struct Wall {
    tiles: Vec<Tile>,
    /// 次にツモる位置。
    next: usize,
    /// 生牌の終わり（ここから先が王牌）。嶺上を引くたび1つ手前へ下がる。
    live_end: usize,
    replacements_taken: usize,
    dora: Vec<Tile>,
    ura: Vec<Tile>,
}

impl Wall {
    pub fn new(seed: &Seed, rules: &Ruleset) -> Self {
        let mut tiles = Vec::with_capacity(136);
        for index in 0..TileKind::COUNT as u8 {
            for _ in 0..4 {
                tiles.push(Tile::from_kind(TileKind::from_index(index).expect("範囲内")));
            }
        }

        // 赤ドラ。5m/5p/5s を1枚ずつ置き換える。
        if rules.red_dora_count > 0 {
            for (kind_index, red_encoded) in [(4u8, 34u8), (13, 35), (22, 36)] {
                let position = tiles
                    .iter()
                    .position(|t| t.kind().index() == kind_index && !t.is_red())
                    .expect("該当牌がある");
                tiles[position] = Tile::from_encoded(red_encoded).expect("赤ドラは範囲内");
            }
        }

        let mut rng = Rng::from_seed(seed);
        for i in (1..tiles.len()).rev() {
            let j = rng.below(i + 1);
            tiles.swap(i, j);
        }

        let live_end = tiles.len() - DEAD_WALL;
        let mut wall = Wall {
            tiles,
            next: 0,
            live_end,
            replacements_taken: 0,
            dora: Vec::new(),
            ura: Vec::new(),
        };
        wall.push_dora();
        wall
    }

    /// 検証とテスト用。山の並びをそのまま見る。
    pub fn all_tiles(&self) -> impl Iterator<Item = Tile> + '_ {
        self.tiles.iter().copied()
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
        if self.replacements_taken >= MAX_REPLACEMENTS {
            return None;
        }
        let position = self.tiles.len() - 1 - self.replacements_taken;
        self.replacements_taken += 1;
        self.live_end -= 1;
        Some(self.tiles[position])
    }

    pub fn reveal_dora(&mut self) -> Option<Tile> {
        if self.dora.len() >= MAX_DORA {
            return None;
        }
        self.push_dora();
        self.dora.last().copied()
    }

    fn push_dora(&mut self) {
        let index = self.dora.len();
        // 王牌の先頭側からドラ表示とその裏を交互に取る。
        let dora_at = self.live_end + index * 2;
        let ura_at = dora_at + 1;
        self.dora.push(self.tiles[dora_at]);
        self.ura.push(self.tiles[ura_at]);
    }

    pub fn dora_indicators(&self) -> &[Tile] {
        &self.dora
    }

    pub fn ura_indicators(&self) -> &[Tile] {
        &self.ura
    }
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine wall`
Expected: 9テスト PASS

`push_dora` の添字が王牌の範囲を超えていないかに注意する。嶺上を4回引くと
`live_end` が4つ下がるため、ドラ表示の位置も一緒に動く。テストが落ちる場合、
王牌の先頭を `live_end` からではなく固定位置から取る形へ直す。

- [ ] **Step 6: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): シードから決定的に作る山を実装"
```

---

### Task 2: 反応ウィンドウの解決

**仕様 6.4 が実際に動くコードになる箇所である。** 早期確定の「以上」判定と、非ロン同順位が発生しないことの検査をここで固定する。

**Files:**
- Modify: `crates/mahjong-engine/src/reaction.rs`

**Interfaces:**
- Consumes: `protocol::command::{ActionOption, CallResponse}`、`protocol::seat::Seat`
- Produces:
  - `pub enum Priority { Pass, Chi, Pon, Ron }`（`Ord` を導出）
  - `pub struct ReactionWindow` — `open(id, from, discarded, candidates, now_ms, deadline_ms)`
  - `ReactionWindow::respond(&mut self, seat, response) -> Result<(), Rejection>`
  - `ReactionWindow::resolve(&self, now_ms: u64, min_wait_ms: u32) -> Outcome`
  - `pub enum Outcome { Pending, Ron(Vec<Seat>), Call { seat: Seat, response: CallResponse }, PassAll }`

**解決の規則（仕様 6.4）:**

1. `now_ms < opened_at + min_wait_ms` なら、全員が答えていても `Pending`。鳴ける者がいない局面と間の長さを揃え、情報を漏らさない
2. 確定している最高優先度を `best` とする
3. 未応答者のうち、`best` **以上**を出せる者が1人でもいれば `Pending`（締切前の場合）
4. それ以外は確定。ロンが最高優先なら**ロンした全員**を返す。3人なら三家和として呼び出し側が流局にする
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

    fn window(candidates: [Vec<ActionOption>; 4]) -> ReactionWindow {
        ReactionWindow::open(
            1,
            Seat::new(0),
            parse_tile("3p").unwrap(),
            candidates,
            0,
            5_000,
        )
    }

    fn ron_only() -> Vec<ActionOption> {
        vec![ActionOption::Ron]
    }

    fn pon_only() -> Vec<ActionOption> {
        vec![ActionOption::Pon { candidates: vec![] }]
    }

    fn chi_only() -> Vec<ActionOption> {
        vec![ActionOption::Chi { candidates: vec![] }]
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

    /// ポンが確定すれば、チーしか出せない未応答者は待たない。
    #[test]
    fn a_pon_resolves_without_waiting_for_a_chi_candidate() {
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(1), CallResponse::Pon { tiles: [parse_tile("3p").unwrap(); 2] })
            .unwrap();
        match w.resolve(400, MIN_WAIT) {
            Outcome::Call { seat, .. } => assert_eq!(seat, Seat::new(1)),
            other => panic!("ポンで確定するはず: {other:?}"),
        }
    }

    /// チーが答えても、ポンできる未応答者がいれば待つ。
    #[test]
    fn a_chi_waits_for_a_pending_pon_candidate() {
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(3), CallResponse::Chi { tiles: [parse_tile("2p").unwrap(); 2] })
            .unwrap();
        assert_eq!(w.resolve(400, MIN_WAIT), Outcome::Pending);
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

    /// 3人がロンすれば三家和。呼び出し側が流局にする。
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

    /// 締切を過ぎた未応答はパス。
    #[test]
    fn the_deadline_turns_silence_into_a_pass() {
        let w = window([vec![], pon_only(), vec![], vec![]]);
        assert_eq!(w.resolve(400, MIN_WAIT), Outcome::Pending);
        assert_eq!(w.resolve(5_001, MIN_WAIT), Outcome::PassAll);
    }

    /// 鳴ける者がいなければ、待機の後すぐ通す。
    #[test]
    fn a_window_with_no_candidates_passes_after_the_wait() {
        let w = window([vec![], vec![], vec![], vec![]]);
        assert_eq!(w.resolve(349, MIN_WAIT), Outcome::Pending);
        assert_eq!(w.resolve(350, MIN_WAIT), Outcome::PassAll);
    }

    /// 候補を持たない席の応答は拒否する。
    #[test]
    fn a_seat_without_candidates_cannot_respond() {
        let mut w = window([vec![], pon_only(), vec![], vec![]]);
        assert!(w.respond(Seat::new(2), CallResponse::Ron).is_err());
    }

    /// 打牌者自身は応答できない。
    #[test]
    fn the_discarder_cannot_respond_to_their_own_discard() {
        let mut w = window([ron_only(), vec![], vec![], vec![]]);
        assert!(w.respond(Seat::new(0), CallResponse::Ron).is_err());
    }

    /// 二重応答は拒否する。
    #[test]
    fn responding_twice_is_rejected() {
        let mut w = window([vec![], pon_only(), vec![], vec![]]);
        w.respond(Seat::new(1), CallResponse::Pass).unwrap();
        assert!(w.respond(Seat::new(1), CallResponse::Pass).is_err());
    }

    /// 候補にない種類の応答は拒否する。
    #[test]
    fn a_response_outside_the_offered_options_is_rejected() {
        let mut w = window([vec![], chi_only(), vec![], vec![]]);
        assert!(w.respond(Seat::new(1), CallResponse::Ron).is_err());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine reaction`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

要点は3つ。

1. `resolve` は `&self` で、状態を変えない。同じ入力から同じ答えが出る純粋関数にする
2. 最低待機は「全員答えても待つ」。ここを飛ばすと間の長さから情報が漏れる
3. 未応答者が出しうる最高優先度は、その席に**提示した候補**から決まる

`Priority` の順序は `Pass < Chi < Pon < Ron`。`#[derive(PartialOrd, Ord)]` で
宣言順がそのまま順序になる。

**非ロンの同順位が同時に成立しないことを検査する。** 牌は1種4枚しかないため、
2人が同じ牌をポンすることも、ポンと明槓が競合することも起こりえない（仕様 6.4）。
席順で解決するロジックは書かず、`debug_assert!` で発生しないことを主張する。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine reaction`
Expected: 11テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 反応ウィンドウの早期確定を実装"
```

---

### Task 3: 局の状態

**Files:**
- Modify: `crates/mahjong-engine/src/state.rs`

**Interfaces:**
- Produces:
  - `pub struct Discarded { pub tile: Tile, pub manner: DiscardManner, pub called_by: Option<Seat> }`
  - `pub struct SeatState { pub hand: Vec<Tile>, pub melds: Vec<Meld>, pub river: Vec<Discarded>, pub riichi: Option<RiichiState>, pub think_bank_ms: u32, pub passed_this_turn: Vec<TileKind> }`
  - `pub struct RiichiState { pub declared_at: usize, pub ippatsu: bool, pub double: bool }`
  - `pub struct RoundState` — 上記4席分と山・局・親・本場・供託・点数を持つ
  - `RoundState::hand_counts(&self, seat) -> HandCounts`
  - `RoundState::is_menzen(&self, seat) -> bool`
  - `RoundState::seat_wind(&self, seat) -> Wind`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::parse_hand;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::{Round, Seat, Wind};

    fn fresh() -> RoundState {
        RoundState::new(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            Round { wind: Wind::East, number: 1 },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &crate::wall::Seed::from_hex(&"11".repeat(32)).unwrap(),
        )
    }

    #[test]
    fn every_seat_starts_with_thirteen_tiles() {
        let state = fresh();
        for seat in Seat::ALL {
            assert_eq!(state.seat(seat).hand.len(), 13);
        }
    }

    #[test]
    fn the_wall_loses_exactly_the_dealt_tiles() {
        let state = fresh();
        // 122 - 52 = 70
        assert_eq!(state.wall.live_remaining(), 70);
    }

    /// 東1局なら親が東、下家が南。
    #[test]
    fn seat_winds_follow_the_dealer() {
        let state = fresh();
        assert_eq!(state.seat_wind(Seat::new(0)), Wind::East);
        assert_eq!(state.seat_wind(Seat::new(1)), Wind::South);
        assert_eq!(state.seat_wind(Seat::new(2)), Wind::West);
        assert_eq!(state.seat_wind(Seat::new(3)), Wind::North);
    }

    #[test]
    fn a_hand_with_no_melds_is_menzen() {
        let state = fresh();
        assert!(state.is_menzen(Seat::new(0)));
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
        assert_eq!(counts.get(protocol::tile::TileKind::from_index(13).unwrap()), 2);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine state`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

`RoundState::new` で山を作り、各席へ13枚ずつ配る。親の第一ツモは
`round.rs` の進行側が行う（配牌の時点では13枚ずつ）。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine state`
Expected: 6テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 局の状態を実装"
```

---

### Task 4: 締切とバンクの計算

**仕様 6.2.1 と 6.2.2 がここで動く。**

**Files:**
- Modify: `crates/mahjong-engine/src/round.rs`（この Task では時間の計算のみ）

**Interfaces:**
- Consumes: `protocol::effect::{effect_of, effect_duration_ms}`、`protocol::ruleset::Ruleset`
- Produces:
  - `pub struct Timing { pub deadline_ms: u64, pub lead_in_ms: u32 }`
  - `pub fn lead_in_since_last_request(events: &[Event]) -> u32`
  - `pub fn deadline_for(rules: &Ruleset, now_ms: u64, bank_remaining_ms: u32, lead_in_ms: u32) -> u64`
  - `pub fn charge_bank(rules: &Ruleset, bank_remaining_ms: u32, elapsed_ms: u64, lead_in_ms: u32) -> u32`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod timing_tests {
    use super::*;
    use protocol::event::{DiscardManner, Event, RiichiStep};
    use protocol::notation::parse_tile;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::Seat;

    fn rules() -> Ruleset {
        Ruleset::kin_no_ma(MatchLength::Hanchan)
    }

    fn discard() -> Event {
        Event::Discard {
            seat: Seat::new(1),
            tile: parse_tile("1m").unwrap(),
            manner: DiscardManner::Tsumogiri,
        }
    }

    /// 連続カンの例（仕様 6.2.1）。
    /// KanDeclared 1100 + Call 1100 + DoraReveal 800 + Draw 250 + Discard 350
    #[test]
    fn lead_in_sums_the_whole_burst_since_the_last_request() {
        let events = vec![
            Event::KanDeclared {
                seat: Seat::new(1),
                kind: protocol::meld::MeldKind::Kakan,
                tile: parse_tile("5s").unwrap(),
            },
            Event::Call {
                seat: Seat::new(1),
                from: Seat::new(1),
                kind: protocol::meld::MeldKind::Kakan,
                tiles: vec![parse_tile("5s").unwrap()],
            },
            Event::DoraReveal {
                indicator: parse_tile("1z").unwrap(),
            },
            Event::Draw {
                seat: Seat::new(1),
                tile: parse_tile("2m").unwrap(),
                source: protocol::event::DrawSource::DeadWall,
                wall_remaining: 60,
            },
            discard(),
        ];
        assert_eq!(lead_in_since_last_request(&events), 3_600);
    }

    #[test]
    fn the_deadline_pushes_back_by_the_lead_in() {
        let plain = deadline_for(&rules(), 10_000, 20_000, 0);
        assert_eq!(plain, 10_000 + 5_000 + 20_000 + 500);

        let after_riichi = deadline_for(&rules(), 10_000, 20_000, 1_800);
        assert_eq!(after_riichi, plain + 1_800);
    }

    #[test]
    fn an_empty_bank_leaves_only_the_base_time() {
        assert_eq!(deadline_for(&rules(), 0, 0, 0), 5_000 + 500);
    }

    /// 基準時間の中で答えればバンクは減らない。
    #[test]
    fn answering_within_the_base_time_costs_nothing() {
        assert_eq!(charge_bank(&rules(), 20_000, 4_000, 0), 20_000);
        assert_eq!(charge_bank(&rules(), 20_000, 5_000, 0), 20_000);
    }

    /// 超えた分だけ引く。
    #[test]
    fn overtime_comes_out_of_the_bank() {
        assert_eq!(charge_bank(&rules(), 20_000, 8_000, 0), 17_000);
    }

    /// 演出を見ていた時間は課金しない。ここを引くと締切をずらした意味が消える。
    #[test]
    fn the_lead_in_is_not_charged() {
        // 実時間 8000ms のうち 1800ms は演出。思考は 6200ms で、超過は 1200ms。
        assert_eq!(charge_bank(&rules(), 20_000, 8_000, 1_800), 18_800);
    }

    #[test]
    fn the_bank_never_goes_below_zero() {
        assert_eq!(charge_bank(&rules(), 1_000, 30_000, 0), 0);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine timing`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

`lead_in_since_last_request` は、呼び出し側が「前回その席へ要求を送ってから
今回まで」に絞ったイベント列を渡す前提とする。この関数自体は合計するだけである。
区間の切り出しは `round.rs` の進行側が席ごとに持つ。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine timing`
Expected: 7テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 締切と溜め時間バンクの計算を実装"
```

---

### Task 5: 局の進行

**Files:**
- Modify: `crates/mahjong-engine/src/round.rs`

**Interfaces:**
- Produces:
  - `pub enum Phase { AwaitingAction { seat: Seat, window_id: u32 }, Reacting(ReactionWindow), Finished(RoundOutcome) }`
  - `pub enum RoundOutcome { Agari { .. }, Ryuukyoku { .. } }`
  - `pub struct RoundEngine` — `new(...) -> (Self, Vec<Event>)`, `apply(&mut self, seat, command, now_ms) -> Result<Vec<Event>, Rejection>`, `tick(&mut self, now_ms) -> Vec<Event>`, `phase(&self) -> &Phase`
  - `pub enum Rejection { NotYourTurn, IllegalCommand, UnknownWindow, AlreadyResponded }`

**進行の骨格:**

```
配牌 → 親ツモ → [打牌 → 反応ウィンドウ → 鳴きなら鳴いた者の打牌へ、
                  なければ次家ツモ] の繰り返し → 和了 or 流局
```

`window_id` は `RoundEngine` が持つカウンタから採る。**局をまたいでも
リセットしない**ため、`new` に開始値を渡す（仕様 5.3.1）。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod round_tests {
    use super::*;
    use protocol::event::Event;
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::{Round, Seat, Wind};

    fn start() -> (RoundEngine, Vec<Event>) {
        RoundEngine::new(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            Round { wind: Wind::East, number: 1 },
            Seat::new(0),
            0,
            0,
            [25_000; 4],
            &crate::wall::Seed::from_hex(&"22".repeat(32)).unwrap(),
            1,
        )
    }

    #[test]
    fn starting_a_round_deals_and_draws_for_the_dealer() {
        let (engine, events) = start();
        assert!(matches!(events[0], Event::RoundStart { .. }));
        assert!(events.iter().any(|e| matches!(e, Event::Deal { .. })));
        assert!(events.iter().any(|e| matches!(e, Event::Draw { .. })));
        assert!(matches!(
            engine.phase(),
            Phase::AwaitingAction { seat, .. } if *seat == Seat::new(0)
        ));
    }

    #[test]
    fn the_round_start_carries_the_seed_commitment() {
        let (_, events) = start();
        let Event::RoundStart { seed_commit, .. } = &events[0] else {
            panic!("RoundStart ではない");
        };
        assert_eq!(seed_commit.len(), 64);
    }

    /// 手番でない席のコマンドは拒否する。
    #[test]
    fn a_command_from_the_wrong_seat_is_rejected() {
        let (mut engine, _) = start();
        let discard = protocol::command::Command::Discard {
            tile: protocol::notation::parse_tile("1m").unwrap(),
            riichi: false,
        };
        assert!(matches!(
            engine.apply(Seat::new(2), discard, 1_000),
            Err(Rejection::NotYourTurn)
        ));
    }

    /// 手にない牌は切れない。
    #[test]
    fn discarding_a_tile_not_in_hand_is_rejected() {
        let (mut engine, _) = start();
        let held: Vec<u8> = engine
            .state()
            .seat(Seat::new(0))
            .hand
            .iter()
            .map(|t| t.encoded())
            .collect();
        let missing = (0u8..34).find(|e| !held.contains(e)).expect("見つかる");
        let command = protocol::command::Command::Discard {
            tile: protocol::tile::Tile::from_encoded(missing).unwrap(),
            riichi: false,
        };
        assert!(engine.apply(Seat::new(0), command, 1_000).is_err());
    }

    /// window_id は局をまたいでもリセットしない。
    #[test]
    fn window_ids_continue_across_rounds() {
        let (engine, _) = RoundEngine::new(
            Ruleset::kin_no_ma(MatchLength::Hanchan),
            Round { wind: Wind::East, number: 2 },
            Seat::new(1),
            0,
            0,
            [25_000; 4],
            &crate::wall::Seed::from_hex(&"33".repeat(32)).unwrap(),
            500,
        );
        assert!(engine.next_window_id() >= 500);
    }

    /// 牌の総数は局の間ずっと136枚のまま。
    #[test]
    fn the_tile_count_is_conserved_through_a_whole_round() {
        let (mut engine, _) = start();
        for step in 0..200u64 {
            if matches!(engine.phase(), Phase::Finished(_)) {
                break;
            }
            engine.drive_tsumogiri(step * 1_000);
            crate::invariant::assert_tiles_conserved(engine.state());
        }
    }
}
```

`drive_tsumogiri` は、テストから局を進めるための最小の打ち手である。
手番ならツモ切り、反応ウィンドウならパスを返す。CPU 雀士（Wave 3）とは別物で、
**エンジンを回して不変条件を検査するためだけに置く**。

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-engine round`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

`apply` は必ず合法手集合と照合してから状態を変える。照合に落ちたら
`Rejection` を返し、状態は一切変えない。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-engine round`
Expected: 6テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-engine
git commit -m "feat(engine): 局の進行を実装"
```

---

### Task 6: 不変条件

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

    #[test]
    fn a_fresh_round_conserves_every_tile() {
        let state = crate::state::RoundState::new(/* ... */);
        assert_tiles_conserved(&state);
    }

    #[test]
    #[should_panic]
    fn a_missing_tile_is_caught() {
        let mut state = crate::state::RoundState::new(/* ... */);
        state.seat_mut(protocol::seat::Seat::new(0)).hand.pop();
        assert_tiles_conserved(&state);
    }

    /// 点棒は卓の中を移動するだけ。供託の増減を含めて合計は変わらない。
    #[test]
    fn scores_and_sticks_balance() {
        assert_scores_conserved(&[25_000; 4], &[24_000, 26_000, 25_000, 25_000], 0);
        // リーチ棒が1本出た局面
        assert_scores_conserved(&[25_000; 4], &[24_000, 25_000, 25_000, 25_000], 1_000);
    }

    #[test]
    #[should_panic]
    fn a_score_leak_is_caught() {
        assert_scores_conserved(&[25_000; 4], &[24_000, 25_000, 25_000, 25_000], 0);
    }
}
```

- [ ] **Step 2〜5: 実装・検証・コミット**

Run: `cargo test --package mahjong-engine invariant`

```bash
git commit -m "feat(engine): 不変条件の検査を実装"
```

---

### Task 7: 半荘の進行

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**Interfaces:**
- Produces:
  - `pub struct MatchEngine` — `new(rules, players, seeds) -> Self`, `current_round(&mut self) -> &mut RoundEngine`, `finish_round(&mut self, outcome) -> Vec<Event>`
  - `pub fn next_round(rules, round, dealer, honba, sticks, scores, reason) -> NextRound`

**連荘の規則:**

| 局の結果 | 親 | 本場 | 理由 |
|---|---|---|---|
| 親の和了 | 続投 | +1 | `DealerWin` |
| 子の和了 | 流れる | 0 | `DealerLoss` |
| 流局・親テンパイ | 続投 | +1 | `DealerTenpai` |
| 流局・親ノーテン | 流れる | +1 | `DealerLoss` |
| 途中流局 | 続投 | +1 | `AbortiveDraw` |
| 流し満貫 | 成立者が親なら続投 | +1 | `NagashiMangan` |

**終局条件:** 東風戦は東4局終了、半荘戦は南4局終了。ただし誰かが0点未満なら即終局（飛び）。オーラスで親が和了しトップなら終了（続行しない）。

- [ ] **Step 1〜5: TDD で実装**

上表をそのままテストにする。

```bash
git commit -m "feat(engine): 半荘の進行と連荘規則を実装"
```

---

### Task 8: 通し対局と決定性の検証

**Files:**
- Create: `crates/mahjong-engine/tests/full_game.rs`

- [ ] **Step 1: テストを書く**

```rust
//! ツモ切りだけの打ち手で半荘を最後まで回す。
//!
//! 局の進行が詰まらないこと、不変条件が全イベントで保たれること、
//! 同じシードから同じイベント列が出ることを確かめる。

#[test]
fn a_whole_match_runs_to_completion() {
    // 東風戦をツモ切りで最後まで。無限ループにならないこと。
}

#[test]
fn the_same_seeds_produce_the_same_event_stream() {
    // 2回走らせて全イベントが一致すること。リプレイ検証の土台。
}

#[test]
fn scores_are_conserved_across_the_whole_match() {
    // 供託を含めた合計が常に 100000 のまま。
}

#[test]
fn a_thousand_matches_never_panic() {
    // シードを変えて1000半荘。不変条件が一度も破れないこと。
}
```

- [ ] **Step 2〜4: 実装・検証・コミット**

`a_thousand_matches_never_panic` が数分かかる場合は回数を減らし、
理由を添えて報告する。

```bash
git commit -m "test(engine): 通し対局と決定性を検証"
```

---

## Wave 2a 完了の判定

- [ ] `cargo test --workspace` が通る
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] ツモ切りの打ち手で半荘が最後まで回る
- [ ] 同じシードから同じイベント列が出る
- [ ] 牌136枚と点棒合計が全イベントで保たれる
- [ ] `protocol` と `mahjong-core` を編集していない
- [ ] `rand` を依存に足していない（`sha2` のみ追加してよい）
- [ ] `Instant::now()` を呼んでいない
