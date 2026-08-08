# Wave 0: 足場と protocol 凍結 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** monorepo の足場を作り、全エージェントが依存する契約（イベント型・コマンド型・演出カタログ・期待値テーブル）を凍結する。

**Architecture:** Cargo workspace + pnpm workspace。`crates/protocol` に型・視界フィルタ・演出カタログを集約し、ts-rs で TypeScript 型を自動生成する。他クレートはモジュール木のみ宣言した空の状態にし、Wave 1 の各エージェントが自分のファイルの中身だけを書けるようにする。

**Tech Stack:** Rust (stable, edition 2021) / serde (JSON) / ts-rs / proptest / Node 26 + pnpm 11 / Vite + TypeScript

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`

## Global Constraints

- Rust edition は **2021** を使う。edition 2024 の新機能は本計画では一切使わない。複数の AI エージェントが並行実装するため、予測可能性を優先する
- ワイヤフォーマットは **JSON**。バイナリ形式は使わない（メッセージ頻度は毎秒数件程度で、デバッグ容易性が上回る）
- `crates/protocol` のパッケージ名は `protocol`
- 依存クレートのバージョンは手書きせず、必ず `cargo add` で解決する
- `lib.rs` / `mod.rs` は Task 11 で確定させ、**Wave 1 のエージェントは編集しない**
- 牌の記法は `123m456p789s1234567z` 形式。赤ドラは `0m` / `0p` / `0s`。字牌は 1z=東, 2z=南, 3z=西, 4z=北, 5z=白, 6z=發, 7z=中
- 牌のエンコードは u8 で 0..=36。0..=33 が通常牌（0-8=1m-9m, 9-17=1p-9p, 18-26=1s-9s, 27-33=東南西北白發中）、34=赤5m, 35=赤5p, 36=赤5s
- ルール既定値（雀魂 金の間）: 25000点持ち / 30000点返し / ウマ +15,+5,-5,-15 / 赤ドラ計3枚 / 喰いタンあり / ダブロンあり / 形式テンパイあり / ノーテン罰符3000 / 流し満貫あり / 責任払いあり / 切り上げ満貫なし / 飛びあり
- 演出時間（ms）: Draw=250, Discard=350, Pon=700, Chi=700, Kan=1100, RiichiDeclare=1800, DoraReveal=800
- 時間定数（ms）: 基準思考時間=5000, 溜め時間バンク=20000, 通信猶予=500, 反応ウィンドウ最低待機=350（打牌演出と同じ値に揃える）

## 仕様からの精緻化（実装時に確定させた点）

設計仕様に対し、実装上ここだけ形を変える。理由を添えて記録する。

1. **`Riichi` イベントから `tile` を落とす。** 仕様 5.3 では `Riichi { seat, tile, step }` だったが、リーチ宣言牌は直後の `Discard` イベントが必ず運ぶ。両方に持たせると真実が2箇所になり、不整合を作れてしまう。`Riichi { seat, step }` とする
2. **`seed_commit` / `seed` を `[u8;32]` ではなく hex 文字列にする。** JSON では 32要素の配列より hex 文字列の方が短く、TypeScript 側の扱いも素直なため
3. **`project()` を server ではなく protocol に置く。** 仕様 5.4 の視界フィルタは (Event, Seat) のみに依存する純粋関数であり、状態を必要としない。契約と同じクレートに置くことで、情報漏洩テストが Wave 0 の時点から存在できる

---

### Task 1: Rust ツールチェインと workspace 足場

**Files:**
- Create: `rust-toolchain.toml`
- Create: `Cargo.toml`
- Create: `.gitignore`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: なし
- Produces: `cargo build` / `cargo test` / `cargo clippy` / `cargo fmt --check` が通る空の workspace

- [ ] **Step 1: rustup と stable ツールチェインを導入する**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --component rustfmt clippy
source "$HOME/.cargo/env"
rustc --version
```

期待: `rustc 1.XX.Y (...)` が表示される。以降の手順でこの `1.XX.Y` を使う。

- [ ] **Step 2: ツールチェインを固定する**

`rustc --version` が出した実際のバージョンを `<VERSION>` に入れて `rust-toolchain.toml` を作る。バージョンを推測して書かないこと。全エージェントが同一ツールチェインを使うためのファイルである。

```toml
[toolchain]
channel = "<VERSION>"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: workspace のルート Cargo.toml を作る**

```toml
[workspace]
resolver = "2"
members = ["crates/protocol"]

[workspace.package]
edition = "2021"
license = "MIT"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
```

- [ ] **Step 4: .gitignore を作る**

```gitignore
/target
node_modules
dist
.DS_Store
```

- [ ] **Step 5: protocol クレートを作る**

```bash
mkdir -p crates/protocol/src
```

`crates/protocol/Cargo.toml`:

```toml
[package]
name = "protocol"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[lints]
workspace = true
```

`crates/protocol/src/lib.rs` は空ファイルで作る（中身は Task 11 で確定させる）。

- [ ] **Step 6: ビルドが通ることを確認する**

Run: `cargo build && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: 3つとも成功

- [ ] **Step 7: CI を作る**

`.github/workflows/ci.yml`:

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:

jobs:
  rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test --workspace
```

- [ ] **Step 8: コミット**

```bash
git add rust-toolchain.toml Cargo.toml .gitignore .github crates/protocol
git commit -m "chore: Cargo workspace と CI の足場を追加"
```

---

### Task 2: 牌の型（Tile / TileKind / Suit）

**Files:**
- Create: `crates/protocol/src/tile.rs`
- Modify: `crates/protocol/src/lib.rs`
- Modify: `crates/protocol/Cargo.toml`

**Interfaces:**
- Consumes: Task 1 の workspace
- Produces:
  - `pub struct Tile(u8)` — `Tile::from_encoded(u8) -> Option<Tile>`, `encoded(self) -> u8`, `kind(self) -> TileKind`, `is_red(self) -> bool`
  - `pub struct TileKind(u8)` — `TileKind::COUNT: usize = 34`, `from_index(u8) -> Option<TileKind>`, `index(self) -> u8`, `suit(self) -> Suit`, `number(self) -> Option<u8>`, `is_honor(self) -> bool`, `is_terminal(self) -> bool`, `is_terminal_or_honor(self) -> bool`
  - `pub enum Suit { Man, Pin, Sou, Honor }`

- [ ] **Step 1: serde を追加する**

```bash
cargo add --package protocol serde --features derive
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/protocol/src/tile.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_classifies_suits_and_numbers() {
        let m1 = TileKind::from_index(0).unwrap();
        assert_eq!(m1.suit(), Suit::Man);
        assert_eq!(m1.number(), Some(1));
        assert!(m1.is_terminal());

        let p5 = TileKind::from_index(13).unwrap();
        assert_eq!(p5.suit(), Suit::Pin);
        assert_eq!(p5.number(), Some(5));
        assert!(!p5.is_terminal_or_honor());

        let s9 = TileKind::from_index(26).unwrap();
        assert_eq!(s9.suit(), Suit::Sou);
        assert_eq!(s9.number(), Some(9));
        assert!(s9.is_terminal());

        let chun = TileKind::from_index(33).unwrap();
        assert_eq!(chun.suit(), Suit::Honor);
        assert_eq!(chun.number(), None);
        assert!(chun.is_honor());
        assert!(chun.is_terminal_or_honor());

        assert_eq!(TileKind::from_index(34), None);
    }

    #[test]
    fn red_five_maps_back_to_its_kind() {
        let red_m = Tile::from_encoded(34).unwrap();
        assert!(red_m.is_red());
        assert_eq!(red_m.kind().index(), 4);

        let red_p = Tile::from_encoded(35).unwrap();
        assert_eq!(red_p.kind().index(), 13);

        let red_s = Tile::from_encoded(36).unwrap();
        assert_eq!(red_s.kind().index(), 22);

        let plain = Tile::from_encoded(4).unwrap();
        assert!(!plain.is_red());
        assert_eq!(plain.kind(), red_m.kind());

        assert_eq!(Tile::from_encoded(37), None);
    }

    #[test]
    fn tile_serializes_as_a_bare_number() {
        let json = serde_json::to_string(&Tile::from_encoded(34).unwrap()).unwrap();
        assert_eq!(json, "34");
        let back: Tile = serde_json::from_str("34").unwrap();
        assert_eq!(back.encoded(), 34);
    }
}
```

- [ ] **Step 3: テストが失敗することを確認する**

```bash
cargo add --package protocol --dev serde_json
```

`crates/protocol/src/lib.rs` に `pub mod tile;` を追記してから:

Run: `cargo test --package protocol`
Expected: コンパイルエラー（`Tile` / `TileKind` / `Suit` が未定義）

- [ ] **Step 4: 最小の実装を書く**

`crates/protocol/src/tile.rs` の先頭に置く:

```rust
use serde::{Deserialize, Serialize};

/// 牌の種類（赤ドラを区別しない34種）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TileKind(u8);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Suit {
    Man,
    Pin,
    Sou,
    Honor,
}

impl TileKind {
    pub const COUNT: usize = 34;

    pub fn from_index(index: u8) -> Option<Self> {
        (index < Self::COUNT as u8).then_some(TileKind(index))
    }

    pub fn index(self) -> u8 {
        self.0
    }

    pub fn suit(self) -> Suit {
        match self.0 {
            0..=8 => Suit::Man,
            9..=17 => Suit::Pin,
            18..=26 => Suit::Sou,
            _ => Suit::Honor,
        }
    }

    /// 数牌なら 1..=9、字牌なら None。
    pub fn number(self) -> Option<u8> {
        (self.0 < 27).then(|| self.0 % 9 + 1)
    }

    pub fn is_honor(self) -> bool {
        self.0 >= 27
    }

    pub fn is_terminal(self) -> bool {
        matches!(self.number(), Some(1) | Some(9))
    }

    pub fn is_terminal_or_honor(self) -> bool {
        self.is_honor() || self.is_terminal()
    }
}

/// 場に存在する1枚の牌。赤ドラを区別する37値のエンコードを持つ。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tile(u8);

const RED_5M: u8 = 34;
const RED_5P: u8 = 35;
const RED_5S: u8 = 36;

impl Tile {
    pub const ENCODED_COUNT: usize = 37;

    pub fn from_encoded(encoded: u8) -> Option<Self> {
        (encoded < Self::ENCODED_COUNT as u8).then_some(Tile(encoded))
    }

    pub fn from_kind(kind: TileKind) -> Self {
        Tile(kind.index())
    }

    pub fn encoded(self) -> u8 {
        self.0
    }

    pub fn is_red(self) -> bool {
        self.0 >= RED_5M
    }

    pub fn kind(self) -> TileKind {
        match self.0 {
            RED_5M => TileKind(4),
            RED_5P => TileKind(13),
            RED_5S => TileKind(22),
            other => TileKind(other),
        }
    }
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test --package protocol`
Expected: 3テストすべて PASS

- [ ] **Step 6: コミット**

```bash
git add crates/protocol Cargo.toml Cargo.lock
git commit -m "feat(protocol): 牌の型 Tile / TileKind / Suit を追加"
```

---

### Task 3: 牌譜記法のパーサ

全エージェントのテストがこの記法を使う。ここが曖昧だと Wave 1 の4本すべてに波及するため、Wave 0 で固定する。

**Files:**
- Create: `crates/protocol/src/notation.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: Task 2 の `Tile` / `TileKind` / `Suit`
- Produces:
  - `pub fn parse_hand(s: &str) -> Result<Vec<Tile>, NotationError>`
  - `pub fn parse_tile(s: &str) -> Result<Tile, NotationError>` — 1枚だけを解釈し、2枚以上なら `NotationError::ExpectedSingleTile`
  - `pub fn to_notation(tiles: &[Tile]) -> String`
  - `pub enum NotationError { UnexpectedChar(char), EmptyRun(char), TrailingDigits, HonorOutOfRange(u8), ExpectedSingleTile }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/protocol/src/notation.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_suit() {
        let tiles = parse_hand("19m19p19s17z").unwrap();
        let encoded: Vec<u8> = tiles.iter().map(|t| t.encoded()).collect();
        assert_eq!(encoded, vec![0, 8, 9, 17, 18, 26, 27, 33]);
    }

    #[test]
    fn parses_red_fives() {
        let tiles = parse_hand("0m0p0s").unwrap();
        let encoded: Vec<u8> = tiles.iter().map(|t| t.encoded()).collect();
        assert_eq!(encoded, vec![34, 35, 36]);
        assert!(tiles.iter().all(|t| t.is_red()));
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(parse_hand("8z"), Err(NotationError::HonorOutOfRange(8)));
        assert_eq!(parse_hand("0z"), Err(NotationError::HonorOutOfRange(0)));
        assert_eq!(parse_hand("123"), Err(NotationError::TrailingDigits));
        assert_eq!(parse_hand("m"), Err(NotationError::EmptyRun('m')));
        assert_eq!(parse_hand("1x"), Err(NotationError::UnexpectedChar('x')));
    }

    #[test]
    fn parse_tile_requires_exactly_one() {
        assert_eq!(parse_tile("5p").unwrap().encoded(), 13);
        assert_eq!(parse_tile("55p"), Err(NotationError::ExpectedSingleTile));
        assert_eq!(parse_tile(""), Err(NotationError::ExpectedSingleTile));
    }

    /// 手牌は多重集合であり、to_notation は正規順へ並べ替える。
    /// したがって往復で保存されるのは牌の集まりであって、並び順ではない。
    #[test]
    fn round_trips_through_notation() {
        for input in ["123456789m", "0m5m", "1112223334445z", "19m19p19s1234567z"] {
            let mut tiles = parse_hand(input).unwrap();
            let rendered = to_notation(&tiles);
            let mut reparsed = parse_hand(&rendered).unwrap();
            tiles.sort();
            reparsed.sort();
            assert_eq!(tiles, reparsed, "input={input} rendered={rendered}");
        }
    }

    /// to_notation は入力の並び順によらず同じ文字列を返す。
    #[test]
    fn notation_is_canonical_regardless_of_input_order() {
        let a = parse_hand("0m5m").unwrap();
        let b = parse_hand("5m0m").unwrap();
        assert_eq!(to_notation(&a), to_notation(&b));
        assert_eq!(to_notation(&a), "50m");
    }

    #[test]
    fn notation_groups_by_suit_in_canonical_order() {
        let tiles = parse_hand("1z9s1p3m").unwrap();
        assert_eq!(to_notation(&tiles), "3m1p9s1z");
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

`crates/protocol/src/lib.rs` に `pub mod notation;` を追記してから:

Run: `cargo test --package protocol notation`
Expected: コンパイルエラー（`parse_hand` などが未定義）

- [ ] **Step 3: 最小の実装を書く**

`crates/protocol/src/notation.rs` の先頭に置く:

```rust
use crate::tile::{Suit, Tile};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotationError {
    UnexpectedChar(char),
    EmptyRun(char),
    TrailingDigits,
    HonorOutOfRange(u8),
    ExpectedSingleTile,
}

impl std::fmt::Display for NotationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotationError::UnexpectedChar(c) => write!(f, "予期しない文字 '{c}'"),
            NotationError::EmptyRun(c) => write!(f, "'{c}' の前に数字がありません"),
            NotationError::TrailingDigits => write!(f, "末尾の数字に対応する色がありません"),
            NotationError::HonorOutOfRange(n) => write!(f, "字牌は 1z..7z のみ有効です（{n}z）"),
            NotationError::ExpectedSingleTile => write!(f, "牌はちょうど1枚である必要があります"),
        }
    }
}

impl std::error::Error for NotationError {}

pub fn parse_hand(input: &str) -> Result<Vec<Tile>, NotationError> {
    let mut out = Vec::new();
    let mut pending: Vec<u8> = Vec::new();

    for ch in input.chars() {
        match ch {
            '0'..='9' => pending.push(ch as u8 - b'0'),
            'm' | 'p' | 's' | 'z' => {
                if pending.is_empty() {
                    return Err(NotationError::EmptyRun(ch));
                }
                for digit in pending.drain(..) {
                    out.push(tile_from_digit(digit, ch)?);
                }
            }
            other => return Err(NotationError::UnexpectedChar(other)),
        }
    }

    if pending.is_empty() {
        Ok(out)
    } else {
        Err(NotationError::TrailingDigits)
    }
}

pub fn parse_tile(input: &str) -> Result<Tile, NotationError> {
    let tiles = parse_hand(input)?;
    match tiles.as_slice() {
        [only] => Ok(*only),
        _ => Err(NotationError::ExpectedSingleTile),
    }
}

fn tile_from_digit(digit: u8, suit: char) -> Result<Tile, NotationError> {
    if suit == 'z' {
        return match digit {
            1..=7 => Ok(Tile::from_encoded(27 + digit - 1).expect("字牌は範囲内")),
            other => Err(NotationError::HonorOutOfRange(other)),
        };
    }

    let (base, red) = match suit {
        'm' => (0u8, 34u8),
        'p' => (9, 35),
        _ => (18, 36),
    };

    match digit {
        0 => Ok(Tile::from_encoded(red).expect("赤ドラは範囲内")),
        1..=9 => Ok(Tile::from_encoded(base + digit - 1).expect("数牌は範囲内")),
        other => Err(NotationError::HonorOutOfRange(other)),
    }
}

pub fn to_notation(tiles: &[Tile]) -> String {
    let mut sorted = tiles.to_vec();
    sorted.sort_by_key(|t| (t.kind().index(), t.is_red()));

    let mut groups: [Vec<u8>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for tile in sorted {
        let kind = tile.kind();
        let (group, digit) = match kind.suit() {
            Suit::Man => (0usize, digit_for(&tile)),
            Suit::Pin => (1, digit_for(&tile)),
            Suit::Sou => (2, digit_for(&tile)),
            Suit::Honor => (3, kind.index() - 26),
        };
        groups[group].push(digit);
    }

    const SUIT_CHARS: [char; 4] = ['m', 'p', 's', 'z'];
    let mut out = String::new();
    for (index, group) in groups.iter().enumerate() {
        if group.is_empty() {
            continue;
        }
        for digit in group {
            out.push((b'0' + digit) as char);
        }
        out.push(SUIT_CHARS[index]);
    }
    out
}

fn digit_for(tile: &Tile) -> u8 {
    if tile.is_red() {
        0
    } else {
        tile.kind().number().expect("数牌には番号がある")
    }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package protocol notation`
Expected: 6テストすべて PASS

- [ ] **Step 5: コミット**

```bash
git add crates/protocol/src
git commit -m "feat(protocol): 牌譜記法のパーサと整形を追加"
```

---

### Task 4: 席・風・局・ルールセット

**Files:**
- Create: `crates/protocol/src/seat.rs`
- Create: `crates/protocol/src/ruleset.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: なし
- Produces:
  - `pub struct Seat(u8)` — `Seat::new(u8) -> Seat`（4以上は panic）, `index(self) -> usize`, `next(self) -> Seat`, `ALL: [Seat; 4]`
  - `pub enum Wind { East, South, West, North }`
  - `pub struct Round { pub wind: Wind, pub number: u8 }`
  - `pub struct Ruleset { ... }` — `Ruleset::kin_no_ma(MatchLength) -> Ruleset`
  - `pub enum MatchLength { Tonpuu, Hanchan }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/protocol/src/ruleset.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kin_no_ma_matches_the_spec_defaults() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);
        assert_eq!(rules.length, MatchLength::Hanchan);
        assert_eq!(rules.start_score, 25_000);
        assert_eq!(rules.return_score, 30_000);
        assert_eq!(rules.uma, [15, 5, -5, -15]);
        assert_eq!(rules.red_dora_count, 3);
        assert!(rules.kuitan);
        assert!(rules.double_ron);
        assert!(rules.formal_tenpai);
        assert_eq!(rules.noten_penalty, 3_000);
        assert!(rules.nagashi_mangan);
        assert!(rules.liability);
        assert!(!rules.round_up_mangan);
        assert!(rules.busted_ends_match);
    }

    #[test]
    fn timing_constants_match_the_spec() {
        let rules = Ruleset::kin_no_ma(MatchLength::Tonpuu);
        assert_eq!(rules.base_think_ms, 5_000);
        assert_eq!(rules.think_bank_ms, 20_000);
        assert_eq!(rules.network_grace_ms, 500);
        assert_eq!(rules.min_reaction_window_ms, 350);
    }

    #[test]
    fn uma_sums_to_zero() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);
        assert_eq!(rules.uma.iter().sum::<i32>(), 0);
    }
}
```

`crates/protocol/src/seat.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_wraps_around_the_table() {
        assert_eq!(Seat::new(0).next(), Seat::new(1));
        assert_eq!(Seat::new(3).next(), Seat::new(0));
    }

    #[test]
    fn all_lists_every_seat_once() {
        let indices: Vec<usize> = Seat::ALL.iter().map(|s| s.index()).collect();
        assert_eq!(indices, vec![0, 1, 2, 3]);
    }

    #[test]
    #[should_panic]
    fn rejects_out_of_range_seats() {
        let _ = Seat::new(4);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

`crates/protocol/src/lib.rs` に `pub mod ruleset;` と `pub mod seat;` を追記してから:

Run: `cargo test --package protocol`
Expected: コンパイルエラー

- [ ] **Step 3: seat.rs の実装を書く**

```rust
use serde::{Deserialize, Serialize};

/// 卓上の絶対的な席位置（0..=3）。自風は局と席から導出する。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Seat(u8);

impl Seat {
    pub const ALL: [Seat; 4] = [Seat(0), Seat(1), Seat(2), Seat(3)];

    pub fn new(index: u8) -> Self {
        assert!(index < 4, "席は 0..=3 のみ有効（{index} が渡された）");
        Seat(index)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// 下家（次にツモる席）。
    pub fn next(self) -> Seat {
        Seat((self.0 + 1) % 4)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Wind {
    East,
    South,
    West,
    North,
}

/// 東1局なら `Round { wind: Wind::East, number: 1 }`。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Round {
    pub wind: Wind,
    pub number: u8,
}
```

- [ ] **Step 4: ruleset.rs の実装を書く**

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MatchLength {
    Tonpuu,
    Hanchan,
}

/// 対局のルール設定。値の変更がコード変更にならないよう、すべてデータとして持つ。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Ruleset {
    pub length: MatchLength,
    pub start_score: i32,
    pub return_score: i32,
    pub uma: [i32; 4],
    pub red_dora_count: u8,
    pub kuitan: bool,
    pub double_ron: bool,
    pub formal_tenpai: bool,
    pub noten_penalty: i32,
    pub nagashi_mangan: bool,
    pub liability: bool,
    pub round_up_mangan: bool,
    pub busted_ends_match: bool,
    pub base_think_ms: u32,
    pub think_bank_ms: u32,
    pub network_grace_ms: u32,
    /// 打牌から次のツモまでの最短時間。鳴ける者の有無で間の長さが変わると
    /// それ自体が情報になるため、常にこの時間だけ待つ。打牌演出と同じ値に揃える。
    pub min_reaction_window_ms: u32,
}

impl Ruleset {
    /// 雀魂「金の間」準拠の既定値。
    pub fn kin_no_ma(length: MatchLength) -> Self {
        Ruleset {
            length,
            start_score: 25_000,
            return_score: 30_000,
            uma: [15, 5, -5, -15],
            red_dora_count: 3,
            kuitan: true,
            double_ron: true,
            formal_tenpai: true,
            noten_penalty: 3_000,
            nagashi_mangan: true,
            liability: true,
            round_up_mangan: false,
            busted_ends_match: true,
            base_think_ms: 5_000,
            think_bank_ms: 20_000,
            network_grace_ms: 500,
            min_reaction_window_ms: 350,
        }
    }
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test --package protocol`
Expected: 全テスト PASS

- [ ] **Step 6: コミット**

```bash
git add crates/protocol/src
git commit -m "feat(protocol): 席・風・局とルールセット既定値を追加"
```

---

### Task 5: 面子と役の識別子

`YakuId` は採点エンジン（Wave 1b）とクライアントの役表示（Wave 1d）の双方が参照する。ここで全役を列挙し凍結する。

**Files:**
- Create: `crates/protocol/src/meld.rs`
- Create: `crates/protocol/src/yaku.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: Task 2 の `Tile`、Task 4 の `Seat`
- Produces:
  - `pub enum MeldKind { Chi, Pon, Ankan, Minkan, Kakan }`
  - `pub struct Meld { pub kind: MeldKind, pub tiles: Vec<Tile>, pub from: Option<Seat>, pub called_tile: Option<Tile> }` — `is_concealed(&self) -> bool`
  - `pub enum YakuId { ... }` — `is_yakuman(self) -> bool`

- [ ] **Step 1: 失敗するテストを書く**

`crates/protocol/src/yaku.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yakuman_classification_is_correct() {
        assert!(YakuId::KokushiMusou.is_yakuman());
        assert!(YakuId::Suuankou.is_yakuman());
        assert!(YakuId::ChuurenPoutou.is_yakuman());
        assert!(!YakuId::Riichi.is_yakuman());
        assert!(!YakuId::Chinitsu.is_yakuman());
        assert!(!YakuId::Dora.is_yakuman());
    }

    #[test]
    fn dora_kinds_are_not_true_yaku() {
        assert!(YakuId::Dora.is_dora());
        assert!(YakuId::AkaDora.is_dora());
        assert!(YakuId::UraDora.is_dora());
        assert!(!YakuId::Tanyao.is_dora());
    }

    #[test]
    fn serializes_as_a_stable_snake_case_string() {
        let json = serde_json::to_string(&YakuId::MenzenTsumo).unwrap();
        assert_eq!(json, "\"menzen_tsumo\"");
        let back: YakuId = serde_json::from_str("\"kokushi_musou_13\"").unwrap();
        assert_eq!(back, YakuId::KokushiMusou13);
    }
}
```

`crates/protocol/src/meld.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation::parse_hand;

    #[test]
    fn ankan_is_concealed_and_others_are_not() {
        let ankan = Meld {
            kind: MeldKind::Ankan,
            tiles: parse_hand("1111m").unwrap(),
            from: None,
            called_tile: None,
        };
        assert!(ankan.is_concealed());

        let pon = Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("111m").unwrap(),
            from: Some(crate::seat::Seat::new(2)),
            called_tile: parse_hand("1m").unwrap().first().copied(),
        };
        assert!(!pon.is_concealed());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

`crates/protocol/src/lib.rs` に `pub mod meld;` と `pub mod yaku;` を追記してから:

Run: `cargo test --package protocol`
Expected: コンパイルエラー

- [ ] **Step 3: meld.rs の実装を書く**

```rust
use serde::{Deserialize, Serialize};

use crate::seat::Seat;
use crate::tile::Tile;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeldKind {
    Chi,
    Pon,
    Ankan,
    Minkan,
    Kakan,
}

/// 副露あるいは暗槓による固定された面子。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Meld {
    pub kind: MeldKind,
    pub tiles: Vec<Tile>,
    /// 鳴いた相手。暗槓は None。
    pub from: Option<Seat>,
    /// 鳴きの対象になった牌。暗槓は None。
    pub called_tile: Option<Tile>,
}

impl Meld {
    /// 門前を崩さない面子かどうか（暗槓のみ真）。
    pub fn is_concealed(&self) -> bool {
        self.kind == MeldKind::Ankan
    }
}
```

- [ ] **Step 4: yaku.rs の実装を書く**

```rust
use serde::{Deserialize, Serialize};

/// 成立しうる役とドラの識別子。表示順・翻数は採点側が決める。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum YakuId {
    // 1翻
    MenzenTsumo,
    Riichi,
    Ippatsu,
    Chankan,
    RinshanKaihou,
    HaiteiRaoyue,
    HouteiRaoyui,
    Pinfu,
    Tanyao,
    Iipeiko,
    YakuhaiHaku,
    YakuhaiHatsu,
    YakuhaiChun,
    YakuhaiRoundWind,
    YakuhaiSeatWind,
    // 2翻
    DoubleRiichi,
    Chiitoitsu,
    Toitoi,
    Sanankou,
    SanshokuDoukou,
    Sankantsu,
    Shousangen,
    Honroutou,
    SanshokuDoujun,
    Ittsu,
    Chanta,
    // 3翻
    Ryanpeikou,
    Junchan,
    Honitsu,
    // 6翻
    Chinitsu,
    // 役満
    Tenhou,
    Chiihou,
    KokushiMusou,
    // 既定の変換では kokushi_musou13 になる。契約として読みやすい形を明示する。
    #[serde(rename = "kokushi_musou_13")]
    KokushiMusou13,
    Suuankou,
    SuuankouTanki,
    Daisangen,
    Shousuushii,
    Daisuushii,
    Tsuuiisou,
    Ryuuiisou,
    Chinroutou,
    ChuurenPoutou,
    #[serde(rename = "chuuren_poutou_9")]
    ChuurenPoutou9,
    Suukantsu,
    // ドラ（役ではないが同じ枠で表示される）
    Dora,
    AkaDora,
    UraDora,
}

impl YakuId {
    pub fn is_yakuman(self) -> bool {
        use YakuId::*;
        matches!(
            self,
            Tenhou
                | Chiihou
                | KokushiMusou
                | KokushiMusou13
                | Suuankou
                | SuuankouTanki
                | Daisangen
                | Shousuushii
                | Daisuushii
                | Tsuuiisou
                | Ryuuiisou
                | Chinroutou
                | ChuurenPoutou
                | ChuurenPoutou9
                | Suukantsu
        )
    }

    /// ドラ枠（役の有無判定には数えない）かどうか。
    pub fn is_dora(self) -> bool {
        matches!(self, YakuId::Dora | YakuId::AkaDora | YakuId::UraDora)
    }
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test --package protocol`
Expected: 全テスト PASS

- [ ] **Step 6: コミット**

```bash
git add crates/protocol/src
git commit -m "feat(protocol): 面子の型と役識別子の一覧を追加"
```

---

### Task 6: コマンドと選択肢の型

**Files:**
- Create: `crates/protocol/src/command.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: Task 2 の `Tile` / `TileKind`
- Produces:
  - `pub enum Command { Discard { tile, riichi }, CallResponse { response }, Ankan { kind }, Kakan { tile }, Tsumo, Kyuushu }`
  - `pub enum CallResponse { Pass, Chi { tiles: [Tile; 2] }, Pon { tiles: [Tile; 2] }, Kan, Ron }`
  - `pub enum KanCandidate { Ankan { kind: TileKind }, Kakan { tile: Tile }, Minkan }`
  - `pub enum ActionOption { Discard { allowed: Vec<Tile>, riichi_allowed: Vec<Tile> }, Chi { candidates: Vec<[Tile; 2]> }, Pon { candidates: Vec<[Tile; 2]> }, Kan { candidates: Vec<KanCandidate> }, Ron, Tsumo, Kyuushu, Pass }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/protocol/src/command.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation::{parse_hand, parse_tile};

    #[test]
    fn command_round_trips_through_json() {
        let command = Command::Discard {
            tile: parse_tile("0p").unwrap(),
            riichi: true,
        };
        let json = serde_json::to_string(&command).unwrap();
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(command, back);
    }

    #[test]
    fn chi_names_the_two_tiles_from_hand() {
        let tiles = parse_hand("34p").unwrap();
        let response = CallResponse::Chi {
            tiles: [tiles[0], tiles[1]],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("chi"), "json={json}");
        let back: CallResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, back);
    }

    #[test]
    fn pass_is_representable() {
        let json = serde_json::to_string(&CallResponse::Pass).unwrap();
        let back: CallResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, CallResponse::Pass);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

`crates/protocol/src/lib.rs` に `pub mod command;` を追記してから:

Run: `cargo test --package protocol command`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
use serde::{Deserialize, Serialize};

use crate::tile::{Tile, TileKind};

/// クライアントがサーバへ送れる操作の全て。これ以外は受け付けない。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Discard {
        tile: Tile,
        riichi: bool,
    },
    /// window_id は RequestAction が示したものをそのまま返す。
    /// これがないと、遅延した応答や再送を別のウィンドウへ適用してしまう。
    CallResponse {
        window_id: u32,
        response: CallResponse,
    },
    Ankan { kind: TileKind },
    Kakan { tile: Tile },
    Tsumo,
    Kyuushu,
}

/// 反応ウィンドウへの応答。使う手牌が曖昧になりうる鳴きは牌を明示する。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CallResponse {
    Pass,
    Chi { tiles: [Tile; 2] },
    Pon { tiles: [Tile; 2] },
    Kan,
    Ron,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KanCandidate {
    Ankan { kind: TileKind },
    Kakan { tile: Tile },
    Minkan,
}

/// サーバが提示する、その席がいま取れる操作。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionOption {
    Discard {
        allowed: Vec<Tile>,
        riichi_allowed: Vec<Tile>,
    },
    Chi {
        candidates: Vec<[Tile; 2]>,
    },
    Pon {
        candidates: Vec<[Tile; 2]>,
    },
    Kan {
        candidates: Vec<KanCandidate>,
    },
    Ron,
    Tsumo,
    Kyuushu,
    Pass,
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package protocol command`
Expected: 3テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/protocol/src
git commit -m "feat(protocol): コマンドと選択肢の型を追加"
```

---

### Task 7: サーバ側イベント（真実）の型

**Files:**
- Create: `crates/protocol/src/event.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: Task 2 `Tile`、Task 4 `Seat` / `Round` / `Ruleset`、Task 5 `Meld` / `MeldKind` / `YakuId`、Task 6 `ActionOption`
- Produces:
  - `pub struct PlayerId(pub u64)`
  - `pub struct EventEnvelope { pub seq: u32, pub event: Event }`
  - `pub enum Event { ... }`（下記の全variant）
  - `pub enum DrawSource { Wall, DeadWall }`
  - `pub enum DiscardManner { Tedashi, Tsumogiri }`
  - `pub enum RiichiStep { Declare, Accepted }`
  - `pub struct AgariResult { pub seat, pub from: Option<Seat>, pub hand: Vec<Tile>, pub melds: Vec<Meld>, pub win_tile: Tile, pub yaku: Vec<(YakuId, u8)>, pub fu: u8, pub han: u8, pub score: i32, pub ura_indicators: Vec<Tile> }`
  - `pub enum RyuukyokuKind { Exhaustive, NineTerminals, FourRiichi, FourWinds, FourKans, ThreeRons }`
  - `pub enum NextRound { Next { round, dealer, honba, riichi_sticks }, MatchOver }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/protocol/src/event.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation::{parse_hand, parse_tile};
    use crate::ruleset::{MatchLength, Ruleset};
    use crate::seat::Wind;

    #[test]
    fn envelope_round_trips_through_json() {
        let envelope = EventEnvelope {
            seq: 42,
            event: Event::Discard {
                seat: Seat::new(1),
                tile: parse_tile("3p").unwrap(),
                manner: DiscardManner::Tedashi,
            },
        };
        let json = serde_json::to_string(&envelope).unwrap();
        let back: EventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(envelope, back);
    }

    #[test]
    fn deal_carries_every_seat_hand() {
        let hands = [
            parse_hand("1112223334445m").unwrap(),
            parse_hand("1112223334445p").unwrap(),
            parse_hand("1112223334445s").unwrap(),
            parse_hand("1112223334445z").unwrap(),
        ];
        for hand in &hands {
            assert_eq!(hand.len(), 13);
        }
        let event = Event::Deal {
            hands: hands.clone(),
            dora_indicator: parse_tile("7z").unwrap(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn round_start_carries_a_hex_seed_commitment() {
        let event = Event::RoundStart {
            round: Round {
                wind: Wind::East,
                number: 1,
            },
            dealer: Seat::new(0),
            honba: 0,
            riichi_sticks: 0,
            scores: [25_000; 4],
            seed_commit: "00".repeat(32),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(&"00".repeat(32)), "json={json}");
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn match_start_carries_the_ruleset() {
        let event = Event::MatchStart {
            players: [PlayerId(1), PlayerId(2), PlayerId(3), PlayerId(4)],
            rules: Ruleset::kin_no_ma(MatchLength::Hanchan),
        };
        let back: Event = serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
        assert_eq!(event, back);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

`crates/protocol/src/lib.rs` に `pub mod event;` を追記してから:

Run: `cargo test --package protocol event`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
use serde::{Deserialize, Serialize};

use crate::command::ActionOption;
use crate::meld::{Meld, MeldKind};
use crate::ruleset::Ruleset;
use crate::seat::{Round, Seat};
use crate::tile::Tile;
use crate::yaku::YakuId;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlayerId(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawSource {
    Wall,
    DeadWall,
}

/// 手出しかツモ切りか。演出と河の表示の双方が必要とするため、
/// 差分からの逆算ではなくイベント自身が持つ。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscardManner {
    Tedashi,
    Tsumogiri,
}

/// 宣言（牌を横に倒す）と成立（棒が出て1000点減る）を分ける。
/// 宣言牌そのものは直後の Discard イベントが運ぶ。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiichiStep {
    Declare,
    Accepted,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RyuukyokuKind {
    Exhaustive,
    NineTerminals,
    FourRiichi,
    FourWinds,
    FourKans,
    ThreeRons,
}

/// 責任払い（パオ）。大三元・大四喜の確定牌を鳴かせた者が負う。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Liability {
    pub seat: Seat,
    pub yaku: YakuId,
    pub mode: LiabilityMode,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiabilityMode {
    /// ツモ和了。責任者が全額を負担する。
    Full,
    /// ロン和了。責任者と放銃者で折半する。
    Split,
}

/// 点棒移動の内訳。最終差分だけでは、ダブロン時に供託を誰が取ったか、
/// 本場をどちらに付けたかを牌譜から復元できないため分けて持つ。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SettlementEntry {
    pub seat: Seat,
    /// 符と翻から決まる素点。
    pub base: i32,
    pub honba: i32,
    pub riichi_sticks: i32,
    /// 責任払いによる肩代わり分。
    pub liability: i32,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Settlement {
    /// 各席の最終増減。entries の合計と一致しなければならない。
    pub delta: [i32; 4],
    pub entries: Vec<SettlementEntry>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct AgariResult {
    pub seat: Seat,
    /// ロンなら放銃者、ツモなら None。
    pub from: Option<Seat>,
    pub hand: Vec<Tile>,
    pub melds: Vec<Meld>,
    pub win_tile: Tile,
    pub yaku: Vec<(YakuId, u8)>,
    pub fu: u8,
    pub han: u8,
    /// 供託と積み棒を含まない素点。
    pub score: i32,
    /// 責任払いが成立した場合のみ Some。
    pub liability: Option<Liability>,
    /// リーチ和了があった場合のみ Some。空配列との使い分けに頼らない。
    pub ura_indicators: Option<Vec<Tile>>,
}

/// 局と本場の進み方が決まった理由。エンジンの判断を牌譜から監査するために残す。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationReason {
    DealerWin,
    DealerTenpai,
    DealerLoss,
    AbortiveDraw,
    NagashiMangan,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NextRound {
    Next {
        round: Round,
        dealer: Seat,
        honba: u8,
        riichi_sticks: u8,
    },
    MatchOver,
}

/// サーバが持つ真実。クライアントへはそのまま出さず、必ず project() を通す。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    MatchStart {
        players: [PlayerId; 4],
        rules: Ruleset,
    },
    RoundStart {
        round: Round,
        dealer: Seat,
        honba: u8,
        riichi_sticks: u8,
        scores: [i32; 4],
        /// 山のシードのハッシュ（hex）。局終了後に SeedReveal で開示する。
        seed_commit: String,
    },
    Deal {
        hands: [Vec<Tile>; 4],
        dora_indicator: Tile,
    },
    Draw {
        seat: Seat,
        tile: Tile,
        source: DrawSource,
        wall_remaining: u8,
    },
    Discard {
        seat: Seat,
        tile: Tile,
        manner: DiscardManner,
    },
    Riichi {
        seat: Seat,
        step: RiichiStep,
    },
    Call {
        seat: Seat,
        from: Seat,
        kind: MeldKind,
        tiles: Vec<Tile>,
    },
    /// 加槓・暗槓の宣言。成立（Call）とは別イベントにすることで、
    /// この間に槍槓の反応ウィンドウを開ける。
    KanDeclared {
        seat: Seat,
        kind: MeldKind,
        tile: Tile,
    },
    DoraReveal {
        indicator: Tile,
    },
    /// 反応ウィンドウでの見逃し。同巡内フリテンとリーチ後の制約に必要なため、
    /// コマンドではなくサーバ側イベントとして牌譜に残す。
    ActionPassed {
        seat: Seat,
        window_id: u32,
        declined: Vec<ActionOption>,
    },
    Agari {
        results: Vec<AgariResult>,
        settlement: Settlement,
    },
    Ryuukyoku {
        kind: RyuukyokuKind,
        /// 九種九牌などの宣言者。荒牌平局では None。
        initiator: Option<Seat>,
        tenpai: [bool; 4],
        /// 公開資格のある席の手牌のみ。射影側でも再検査する。
        revealed_hands: Vec<(Seat, Vec<Tile>)>,
        nagashi_winners: Vec<Seat>,
        settlement: Settlement,
    },
    RoundEnd {
        scores: [i32; 4],
        next: NextRound,
        reason: ContinuationReason,
    },
    MatchEnd {
        final_scores: [i32; 4],
        placements: [u8; 4],
    },
    RequestAction {
        seat: Seat,
        /// どの打牌・宣言に対する要求か。遅延した応答や再送の取り違えを防ぐ。
        window_id: u32,
        options: Vec<ActionOption>,
        deadline_ms: u32,
    },
    /// 半荘終了後にまとめて開示する。局ごとに出すと、その局の他家手牌を
    /// 遡って復元できてしまい、同じ半荘の中で不公平が生じる。
    SeedReveal {
        seeds: Vec<String>,
    },
}

/// 牌譜と再接続のための連番付きイベント。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub seq: u32,
    pub event: Event,
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package protocol event`
Expected: 4テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/protocol/src
git commit -m "feat(protocol): サーバ側イベントの型を追加"
```

---

### Task 8: 視界フィルタと配信イベント

不正対策の全体重がここにかかる。`ClientEvent` を `Event` と**別の型**にすることで、隠すべき情報を運ぶ場所が型として存在しない状態を作る。

**Files:**
- Create: `crates/protocol/src/client_event.rs`
- Create: `crates/protocol/src/project.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: Task 7 の全型
- Produces:
  - `pub enum ClientEvent { ... }`
  - `pub fn project(event: &Event, viewer: Seat) -> Option<ClientEvent>` — その席に配信すべきものが無ければ `None`
  - `pub fn project_envelope(envelope: &EventEnvelope, viewer: Seat) -> Option<ClientEventEnvelope>`
  - `pub struct ClientEventEnvelope { pub seq: u32, pub event: ClientEvent }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/protocol/src/project.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_event::ClientEvent;
    use crate::event::{DiscardManner, DrawSource, Event, PlayerId};
    use crate::notation::{parse_hand, parse_tile};
    use crate::ruleset::{MatchLength, Ruleset};
    use crate::seat::Seat;

    /// 各席の手牌を色で完全に分離しておく。漏れた場合にどの席から漏れたか判る。
    fn disjoint_hands() -> [Vec<crate::tile::Tile>; 4] {
        [
            parse_hand("1112223334445m").unwrap(),
            parse_hand("1112223334445p").unwrap(),
            parse_hand("1112223334445s").unwrap(),
            parse_hand("1112223334445z").unwrap(),
        ]
    }

    #[test]
    fn deal_reveals_only_the_viewers_hand() {
        let hands = disjoint_hands();
        let event = Event::Deal {
            hands: hands.clone(),
            dora_indicator: parse_tile("7z").unwrap(),
        };

        let projected = project(&event, Seat::new(0)).expect("配牌は全席に配信される");
        let ClientEvent::Deal {
            your_hand,
            hand_sizes,
            dora_indicator,
        } = projected
        else {
            panic!("Deal が別の variant に射影された");
        };

        assert_eq!(your_hand, hands[0]);
        assert_eq!(hand_sizes, [13, 13, 13, 13]);
        assert_eq!(dora_indicator, parse_tile("7z").unwrap());
    }

    #[test]
    fn deal_json_contains_no_other_seat_tiles() {
        let hands = disjoint_hands();
        let event = Event::Deal {
            hands,
            dora_indicator: parse_tile("7z").unwrap(),
        };

        let projected = project(&event, Seat::new(0)).unwrap();
        let json = serde_json::to_string(&projected).unwrap();
        let back: ClientEvent = serde_json::from_str(&json).unwrap();

        let ClientEvent::Deal { your_hand, .. } = back else {
            panic!("Deal ではない");
        };
        // 自席は萬子のみ。他席の牌が混ざっていれば必ずここで落ちる。
        assert!(
            your_hand
                .iter()
                .all(|t| t.kind().suit() == crate::tile::Suit::Man),
            "自席以外の牌が混入した: {}",
            crate::notation::to_notation(&your_hand)
        );
    }

    #[test]
    fn draw_hides_the_tile_from_other_seats() {
        let event = Event::Draw {
            seat: Seat::new(1),
            tile: parse_tile("5z").unwrap(),
            source: DrawSource::Wall,
            wall_remaining: 69,
        };

        let own = project(&event, Seat::new(1)).unwrap();
        assert!(
            matches!(own, ClientEvent::Draw { tile: Some(_), .. }),
            "自分のツモ牌は見える"
        );

        for viewer in [0u8, 2, 3] {
            let other = project(&event, Seat::new(viewer)).unwrap();
            let ClientEvent::Draw {
                tile,
                wall_remaining,
                ..
            } = other
            else {
                panic!("Draw ではない");
            };
            assert_eq!(tile, None, "席{viewer} にツモ牌が漏れた");
            assert_eq!(wall_remaining, 69, "山の残り枚数は公開情報");
        }
    }

    #[test]
    fn discard_is_public_to_everyone() {
        let event = Event::Discard {
            seat: Seat::new(2),
            tile: parse_tile("3p").unwrap(),
            manner: DiscardManner::Tedashi,
        };

        for viewer in 0u8..4 {
            let projected = project(&event, Seat::new(viewer)).unwrap();
            let ClientEvent::Discard { tile, manner, .. } = projected else {
                panic!("Discard ではない");
            };
            assert_eq!(tile, parse_tile("3p").unwrap());
            assert_eq!(manner, DiscardManner::Tedashi);
        }
    }

    #[test]
    fn request_action_reaches_only_its_own_seat() {
        let event = Event::RequestAction {
            seat: Seat::new(3),
            window_id: 1,
            options: vec![],
            deadline_ms: 5_500,
        };

        assert!(project(&event, Seat::new(3)).is_some());
        for viewer in [0u8, 1, 2] {
            assert!(
                project(&event, Seat::new(viewer)).is_none(),
                "席{viewer} に他家への行動要求が漏れた"
            );
        }
    }

    #[test]
    fn match_start_tells_each_viewer_their_own_seat() {
        let event = Event::MatchStart {
            players: [PlayerId(10), PlayerId(11), PlayerId(12), PlayerId(13)],
            rules: Ruleset::kin_no_ma(MatchLength::Hanchan),
        };

        for viewer in 0u8..4 {
            let projected = project(&event, Seat::new(viewer)).unwrap();
            let ClientEvent::MatchStart { you, .. } = projected else {
                panic!("MatchStart ではない");
            };
            assert_eq!(you, Seat::new(viewer));
        }
    }

    fn empty_settlement() -> crate::event::Settlement {
        crate::event::Settlement {
            delta: [0; 4],
            entries: vec![],
        }
    }

    /// 生成側が誤ってノーテン者の手牌を入れても、射影で落ちなければならない。
    /// ここは型では守れないため、負例で守る。
    #[test]
    fn exhaustive_draw_reveals_only_tenpai_hands() {
        let hands = disjoint_hands();
        let event = Event::Ryuukyoku {
            kind: crate::event::RyuukyokuKind::Exhaustive,
            initiator: None,
            tenpai: [true, false, false, true],
            // 生成側のバグを模して全席分を入れる。
            revealed_hands: (0u8..4)
                .map(|i| (Seat::new(i), hands[i as usize].clone()))
                .collect(),
            nagashi_winners: vec![],
            settlement: empty_settlement(),
        };

        let projected = project(&event, Seat::new(0)).unwrap();
        let ClientEvent::Ryuukyoku { revealed_hands, .. } = projected else {
            panic!("Ryuukyoku ではない");
        };

        let revealed: Vec<usize> = revealed_hands.iter().map(|(s, _)| s.index()).collect();
        assert_eq!(revealed, vec![0, 3], "ノーテン者の手牌が公開された");
    }

    #[test]
    fn nagashi_winner_reveals_even_when_noten() {
        let hands = disjoint_hands();
        let event = Event::Ryuukyoku {
            kind: crate::event::RyuukyokuKind::Exhaustive,
            initiator: None,
            tenpai: [false, false, false, false],
            revealed_hands: vec![(Seat::new(2), hands[2].clone())],
            nagashi_winners: vec![Seat::new(2)],
            settlement: empty_settlement(),
        };

        let projected = project(&event, Seat::new(0)).unwrap();
        let ClientEvent::Ryuukyoku { revealed_hands, .. } = projected else {
            panic!("Ryuukyoku ではない");
        };
        assert_eq!(revealed_hands.len(), 1);
        assert_eq!(revealed_hands[0].0, Seat::new(2));
    }

    #[test]
    fn abortive_draws_reveal_nothing_but_nine_terminals_shows_the_declarer() {
        let hands = disjoint_hands();
        let all: Vec<(Seat, Vec<crate::tile::Tile>)> = (0u8..4)
            .map(|i| (Seat::new(i), hands[i as usize].clone()))
            .collect();

        for kind in [
            crate::event::RyuukyokuKind::FourRiichi,
            crate::event::RyuukyokuKind::FourWinds,
            crate::event::RyuukyokuKind::FourKans,
            crate::event::RyuukyokuKind::ThreeRons,
        ] {
            let event = Event::Ryuukyoku {
                kind,
                initiator: Some(Seat::new(1)),
                tenpai: [true; 4],
                revealed_hands: all.clone(),
                nagashi_winners: vec![],
                settlement: empty_settlement(),
            };
            let projected = project(&event, Seat::new(0)).unwrap();
            let ClientEvent::Ryuukyoku { revealed_hands, .. } = projected else {
                panic!("Ryuukyoku ではない");
            };
            assert!(revealed_hands.is_empty(), "{kind:?} で手牌が公開された");
        }

        let event = Event::Ryuukyoku {
            kind: crate::event::RyuukyokuKind::NineTerminals,
            initiator: Some(Seat::new(1)),
            tenpai: [true; 4],
            revealed_hands: all,
            nagashi_winners: vec![],
            settlement: empty_settlement(),
        };
        let projected = project(&event, Seat::new(0)).unwrap();
        let ClientEvent::Ryuukyoku { revealed_hands, .. } = projected else {
            panic!("Ryuukyoku ではない");
        };
        assert_eq!(revealed_hands.len(), 1, "九種九牌は宣言者のみ公開する");
        assert_eq!(revealed_hands[0].0, Seat::new(1));
    }

    #[test]
    fn request_action_carries_the_window_id() {
        let event = Event::RequestAction {
            seat: Seat::new(2),
            window_id: 77,
            options: vec![],
            deadline_ms: 5_850,
        };
        let projected = project(&event, Seat::new(2)).unwrap();
        let ClientEvent::RequestAction { window_id, .. } = projected else {
            panic!("RequestAction ではない");
        };
        assert_eq!(window_id, 77);
    }

    /// 見逃した具体的な選択肢は、他家に配ると待ちの手掛かりになる。
    #[test]
    fn action_passed_does_not_carry_the_declined_options() {
        let event = Event::ActionPassed {
            seat: Seat::new(1),
            window_id: 3,
            declined: vec![crate::command::ActionOption::Ron],
        };
        let projected = project(&event, Seat::new(0)).unwrap();
        assert!(matches!(
            projected,
            ClientEvent::ActionPassed {
                window_id: 3,
                ..
            }
        ));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

`crates/protocol/src/lib.rs` に `pub mod client_event;` と `pub mod project;` を追記してから:

Run: `cargo test --package protocol project`
Expected: コンパイルエラー

- [ ] **Step 3: client_event.rs の実装を書く**

`Event` と対応するが、隠すべき情報を持つ場所が構造として存在しない点に注意する。`Deal` に他席の手牌を入れるフィールドは無く、`Draw` の牌は `Option` である。

```rust
use serde::{Deserialize, Serialize};

use crate::command::ActionOption;
use crate::event::{
    AgariResult, ContinuationReason, DiscardManner, DrawSource, NextRound, PlayerId, RiichiStep,
    RyuukyokuKind, Settlement,
};
use crate::meld::MeldKind;
use crate::ruleset::Ruleset;
use crate::seat::{Round, Seat};
use crate::tile::Tile;

/// 1つの席へ配信されるイベント。Event から project() で作る以外の経路を作らない。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    MatchStart {
        players: [PlayerId; 4],
        rules: Ruleset,
        you: Seat,
    },
    RoundStart {
        round: Round,
        dealer: Seat,
        honba: u8,
        riichi_sticks: u8,
        scores: [i32; 4],
        seed_commit: String,
    },
    Deal {
        your_hand: Vec<Tile>,
        hand_sizes: [u8; 4],
        dora_indicator: Tile,
    },
    Draw {
        seat: Seat,
        /// 自席のツモのみ Some。
        tile: Option<Tile>,
        source: DrawSource,
        wall_remaining: u8,
    },
    Discard {
        seat: Seat,
        tile: Tile,
        manner: DiscardManner,
    },
    Riichi {
        seat: Seat,
        step: RiichiStep,
    },
    Call {
        seat: Seat,
        from: Seat,
        kind: MeldKind,
        tiles: Vec<Tile>,
    },
    KanDeclared {
        seat: Seat,
        kind: MeldKind,
        tile: Tile,
    },
    DoraReveal {
        indicator: Tile,
    },
    ActionPassed {
        seat: Seat,
        window_id: u32,
    },
    Agari {
        results: Vec<AgariResult>,
        settlement: Settlement,
    },
    Ryuukyoku {
        kind: RyuukyokuKind,
        initiator: Option<Seat>,
        tenpai: [bool; 4],
        /// 射影側で公開資格を再判定した結果のみが入る。
        revealed_hands: Vec<(Seat, Vec<Tile>)>,
        nagashi_winners: Vec<Seat>,
        settlement: Settlement,
    },
    RoundEnd {
        scores: [i32; 4],
        next: NextRound,
        reason: ContinuationReason,
    },
    MatchEnd {
        final_scores: [i32; 4],
        placements: [u8; 4],
    },
    /// 自席宛のみ配信されるため seat を持たない。
    RequestAction {
        window_id: u32,
        options: Vec<ActionOption>,
        deadline_ms: u32,
    },
    SeedReveal {
        seeds: Vec<String>,
    },
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ClientEventEnvelope {
    pub seq: u32,
    pub event: ClientEvent,
}
```

- [ ] **Step 4: project.rs の実装を書く**

```rust
use crate::client_event::{ClientEvent, ClientEventEnvelope};
use crate::event::{Event, EventEnvelope, RyuukyokuKind};
use crate::seat::Seat;

/// サーバの真実を、その席が見てよい形へ落とす。
/// クライアントへ出るバイト列は必ずこの関数を通す。
pub fn project(event: &Event, viewer: Seat) -> Option<ClientEvent> {
    let projected = match event {
        Event::MatchStart { players, rules } => ClientEvent::MatchStart {
            players: *players,
            rules: *rules,
            you: viewer,
        },
        Event::RoundStart {
            round,
            dealer,
            honba,
            riichi_sticks,
            scores,
            seed_commit,
        } => ClientEvent::RoundStart {
            round: *round,
            dealer: *dealer,
            honba: *honba,
            riichi_sticks: *riichi_sticks,
            scores: *scores,
            seed_commit: seed_commit.clone(),
        },
        Event::Deal {
            hands,
            dora_indicator,
        } => {
            let mut hand_sizes = [0u8; 4];
            for (index, hand) in hands.iter().enumerate() {
                hand_sizes[index] = hand.len() as u8;
            }
            ClientEvent::Deal {
                your_hand: hands[viewer.index()].clone(),
                hand_sizes,
                dora_indicator: *dora_indicator,
            }
        }
        Event::Draw {
            seat,
            tile,
            source,
            wall_remaining,
        } => ClientEvent::Draw {
            seat: *seat,
            tile: (*seat == viewer).then_some(*tile),
            source: *source,
            wall_remaining: *wall_remaining,
        },
        Event::Discard {
            seat,
            tile,
            manner,
        } => ClientEvent::Discard {
            seat: *seat,
            tile: *tile,
            manner: *manner,
        },
        Event::Riichi { seat, step } => ClientEvent::Riichi {
            seat: *seat,
            step: *step,
        },
        Event::Call {
            seat,
            from,
            kind,
            tiles,
        } => ClientEvent::Call {
            seat: *seat,
            from: *from,
            kind: *kind,
            tiles: tiles.clone(),
        },
        Event::KanDeclared { seat, kind, tile } => ClientEvent::KanDeclared {
            seat: *seat,
            kind: *kind,
            tile: *tile,
        },
        Event::DoraReveal { indicator } => ClientEvent::DoraReveal {
            indicator: *indicator,
        },
        Event::ActionPassed {
            seat, window_id, ..
        } => ClientEvent::ActionPassed {
            seat: *seat,
            window_id: *window_id,
        },
        Event::Agari {
            results,
            settlement,
        } => ClientEvent::Agari {
            results: results.clone(),
            settlement: settlement.clone(),
        },
        Event::Ryuukyoku {
            kind,
            initiator,
            tenpai,
            revealed_hands,
            nagashi_winners,
            settlement,
        } => ClientEvent::Ryuukyoku {
            kind: *kind,
            initiator: *initiator,
            tenpai: *tenpai,
            // 生成側を信用せず、公開資格をここで再判定する。
            revealed_hands: revealed_hands
                .iter()
                .filter(|(seat, _)| {
                    may_reveal_hand(*kind, *initiator, tenpai, nagashi_winners, *seat)
                })
                .cloned()
                .collect(),
            nagashi_winners: nagashi_winners.clone(),
            settlement: settlement.clone(),
        },
        Event::RoundEnd {
            scores,
            next,
            reason,
        } => ClientEvent::RoundEnd {
            scores: *scores,
            next: *next,
            reason: *reason,
        },
        Event::MatchEnd {
            final_scores,
            placements,
        } => ClientEvent::MatchEnd {
            final_scores: *final_scores,
            placements: *placements,
        },
        Event::RequestAction {
            seat,
            window_id,
            options,
            deadline_ms,
        } => {
            if *seat != viewer {
                return None;
            }
            ClientEvent::RequestAction {
                window_id: *window_id,
                options: options.clone(),
                deadline_ms: *deadline_ms,
            }
        }
        Event::SeedReveal { seeds } => ClientEvent::SeedReveal {
            seeds: seeds.clone(),
        },
    };

    Some(projected)
}

/// 流局時にその席の手牌を公開してよいか。
///
/// 荒牌平局ではテンパイ者と流し満貫成立者のみ。九種九牌は宣言者のみ。
/// それ以外の途中流局では誰の手牌も公開しない。
fn may_reveal_hand(
    kind: RyuukyokuKind,
    initiator: Option<Seat>,
    tenpai: &[bool; 4],
    nagashi_winners: &[Seat],
    seat: Seat,
) -> bool {
    match kind {
        RyuukyokuKind::Exhaustive => tenpai[seat.index()] || nagashi_winners.contains(&seat),
        RyuukyokuKind::NineTerminals => initiator == Some(seat),
        RyuukyokuKind::FourRiichi
        | RyuukyokuKind::FourWinds
        | RyuukyokuKind::FourKans
        | RyuukyokuKind::ThreeRons => false,
    }
}

pub fn project_envelope(
    envelope: &EventEnvelope,
    viewer: Seat,
) -> Option<ClientEventEnvelope> {
    project(&envelope.event, viewer).map(|event| ClientEventEnvelope {
        seq: envelope.seq,
        event,
    })
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test --package protocol project`
Expected: 6テスト PASS

- [ ] **Step 6: コミット**

```bash
git add crates/protocol/src
git commit -m "feat(protocol): 視界フィルタと配信イベントの型を追加"
```

---

### Task 9: 演出カタログ

サーバはこの表を使って思考時間の締切に演出時間を足す。クライアントは同じ表の通りに再生する。両者が同じ定数を見ることが要件そのものである。

**Files:**
- Create: `crates/protocol/src/effect.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Consumes: Task 7 `Event`、Task 4 `Ruleset`
- Produces:
  - `pub enum EffectKind { Draw, Discard, Pon, Chi, Kan, RiichiDeclare, DoraReveal }`
  - `pub const fn effect_duration_ms(kind: EffectKind) -> u32`
  - `pub fn effect_of(event: &Event) -> Option<EffectKind>`
  - `pub fn lead_in_ms(events: &[Event]) -> u32`
  - `pub fn action_deadline_ms(rules: &Ruleset, bank_remaining_ms: u32, lead_in_ms: u32) -> u32`

- [ ] **Step 1: 失敗するテストを書く**

`crates/protocol/src/effect.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{DiscardManner, Event, RiichiStep};
    use crate::meld::MeldKind;
    use crate::notation::parse_tile;
    use crate::ruleset::{MatchLength, Ruleset};
    use crate::seat::Seat;

    #[test]
    fn catalog_matches_the_spec() {
        assert_eq!(effect_duration_ms(EffectKind::Draw), 250);
        assert_eq!(effect_duration_ms(EffectKind::Discard), 350);
        assert_eq!(effect_duration_ms(EffectKind::Pon), 700);
        assert_eq!(effect_duration_ms(EffectKind::Chi), 700);
        assert_eq!(effect_duration_ms(EffectKind::Kan), 1_100);
        assert_eq!(effect_duration_ms(EffectKind::RiichiDeclare), 1_800);
        assert_eq!(effect_duration_ms(EffectKind::DoraReveal), 800);
    }

    #[test]
    fn maps_events_to_their_effects() {
        let discard = Event::Discard {
            seat: Seat::new(0),
            tile: parse_tile("1m").unwrap(),
            manner: DiscardManner::Tsumogiri,
        };
        assert_eq!(effect_of(&discard), Some(EffectKind::Discard));

        let declare = Event::Riichi {
            seat: Seat::new(0),
            step: RiichiStep::Declare,
        };
        assert_eq!(effect_of(&declare), Some(EffectKind::RiichiDeclare));

        // 成立側は点棒の移動のみで、局の進行を止める演出を持たない。
        let accepted = Event::Riichi {
            seat: Seat::new(0),
            step: RiichiStep::Accepted,
        };
        assert_eq!(effect_of(&accepted), None);

        let pon = Event::Call {
            seat: Seat::new(1),
            from: Seat::new(0),
            kind: MeldKind::Pon,
            tiles: vec![parse_tile("1m").unwrap()],
        };
        assert_eq!(effect_of(&pon), Some(EffectKind::Pon));

        let ankan = Event::Call {
            seat: Seat::new(1),
            from: Seat::new(1),
            kind: MeldKind::Ankan,
            tiles: vec![parse_tile("1m").unwrap()],
        };
        assert_eq!(effect_of(&ankan), Some(EffectKind::Kan));
    }

    #[test]
    fn lead_in_sums_only_events_that_have_effects() {
        let events = vec![
            Event::Riichi {
                seat: Seat::new(0),
                step: RiichiStep::Declare,
            },
            Event::Discard {
                seat: Seat::new(0),
                tile: parse_tile("1m").unwrap(),
                manner: DiscardManner::Tedashi,
            },
            Event::Riichi {
                seat: Seat::new(0),
                step: RiichiStep::Accepted,
            },
        ];
        assert_eq!(lead_in_ms(&events), 1_800 + 350);
    }

    #[test]
    fn deadline_adds_the_lead_in_so_effects_do_not_eat_think_time() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);

        let without_effects = action_deadline_ms(&rules, 20_000, 0);
        assert_eq!(without_effects, 5_000 + 20_000 + 500);

        // 直前にリーチ演出が入った分だけ締切が後ろへずれる。
        let after_riichi = action_deadline_ms(&rules, 20_000, 1_800);
        assert_eq!(after_riichi, without_effects + 1_800);
    }

    #[test]
    fn deadline_shrinks_as_the_bank_is_spent() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);
        assert_eq!(action_deadline_ms(&rules, 0, 0), 5_000 + 500);
    }

    /// 最低待機が打牌演出より短いと「次の行動が直前の演出完了より論理的に先行する」
    /// 状態が生まれる。両者の対応を検査で固定しておく。
    #[test]
    fn minimum_reaction_wait_covers_the_discard_effect() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);
        assert!(
            rules.min_reaction_window_ms >= effect_duration_ms(EffectKind::Discard),
            "最低待機{}ms が打牌演出{}ms より短い",
            rules.min_reaction_window_ms,
            effect_duration_ms(EffectKind::Discard)
        );
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

`crates/protocol/src/lib.rs` に `pub mod effect;` を追記してから:

Run: `cargo test --package protocol effect`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
use serde::{Deserialize, Serialize};

use crate::event::{Event, RiichiStep};
use crate::meld::MeldKind;
use crate::ruleset::Ruleset;

/// 局の進行を止める演出の種類。サーバとクライアントが同じ表を見る。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    Draw,
    Discard,
    Pon,
    Chi,
    Kan,
    RiichiDeclare,
    DoraReveal,
}

pub const fn effect_duration_ms(kind: EffectKind) -> u32 {
    match kind {
        EffectKind::Draw => 250,
        EffectKind::Discard => 350,
        EffectKind::Pon => 700,
        EffectKind::Chi => 700,
        EffectKind::Kan => 1_100,
        EffectKind::RiichiDeclare => 1_800,
        EffectKind::DoraReveal => 800,
    }
}

/// そのイベントが進行を止める演出を伴うか。伴わないものは None。
pub fn effect_of(event: &Event) -> Option<EffectKind> {
    match event {
        Event::Draw { .. } => Some(EffectKind::Draw),
        Event::Discard { .. } => Some(EffectKind::Discard),
        Event::DoraReveal { .. } => Some(EffectKind::DoraReveal),
        Event::Riichi { step, .. } => match step {
            RiichiStep::Declare => Some(EffectKind::RiichiDeclare),
            RiichiStep::Accepted => None,
        },
        Event::Call { kind, .. } => match kind {
            MeldKind::Chi => Some(EffectKind::Chi),
            MeldKind::Pon => Some(EffectKind::Pon),
            MeldKind::Ankan | MeldKind::Minkan | MeldKind::Kakan => Some(EffectKind::Kan),
        },
        _ => None,
    }
}

/// 直前に配信した一連のイベントの演出時間の合計。
pub fn lead_in_ms(events: &[Event]) -> u32 {
    events
        .iter()
        .filter_map(effect_of)
        .map(effect_duration_ms)
        .sum()
}

/// 行動要求の締切。演出で思考時間が削られないよう lead_in を加算する。
pub fn action_deadline_ms(rules: &Ruleset, bank_remaining_ms: u32, lead_in_ms: u32) -> u32 {
    rules.base_think_ms + bank_remaining_ms + rules.network_grace_ms + lead_in_ms
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package protocol effect`
Expected: 5テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/protocol/src
git commit -m "feat(protocol): 演出カタログと締切計算を追加"
```

---

### Task 10: TypeScript 型の自動生成と apps/web 足場

**`Event` には TS 導出を付けない。** クライアント側の型に、サーバの真実を表現する手段が存在しない状態を作る。TS へ出すのは `ClientEvent` / `Command` とその依存型だけである。

**Files:**
- Create: `pnpm-workspace.yaml`
- Create: `package.json`
- Create: `apps/web/package.json`
- Create: `apps/web/tsconfig.json`
- Create: `apps/web/vite.config.ts`
- Create: `apps/web/index.html`
- Create: `apps/web/src/main.ts`
- Create: `apps/web/src/protocol/.gitkeep`
- Modify: `crates/protocol/src/{tile,seat,ruleset,meld,yaku,command,client_event}.rs`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: Task 2〜9 の全型
- Produces: `apps/web/src/protocol/*.ts`（生成物。手で編集しない）

- [ ] **Step 1: ts-rs を追加する**

```bash
cargo add --package protocol ts-rs
```

- [ ] **Step 2: 配信側の型に TS 導出を付ける**

`Tile` / `TileKind` / `Suit` / `Seat` / `Wind` / `Round` / `Ruleset` / `MatchLength` / `Meld` / `MeldKind` / `YakuId` / `Command` / `CallResponse` / `KanCandidate` / `ActionOption` / `PlayerId` / `DrawSource` / `DiscardManner` / `RiichiStep` / `AgariResult` / `RyuukyokuKind` / `NextRound` / `ClientEvent` / `ClientEventEnvelope` / `EffectKind` の各 `derive` に `TS` を足し、次の属性を付ける。

```rust
#[derive(/* 既存の derive はそのまま */, ts_rs::TS)]
#[ts(export, export_to = "../../apps/web/src/protocol/")]
```

`Event` と `EventEnvelope` には**付けない**。

- [ ] **Step 3: 生成が走ることを確認する**

Run: `cargo test --package protocol`
Expected: PASS し、`apps/web/src/protocol/` に `.ts` ファイル群が生成される

Run: `ls apps/web/src/protocol/`
Expected: `ClientEvent.ts`, `Command.ts`, `Tile.ts` などが存在する

Run: `ls apps/web/src/protocol/ | grep -c '^Event.ts$' || true`
Expected: `0`（サーバ側イベントは TS へ出ない）

- [ ] **Step 4: pnpm workspace と web 足場を作る**

`pnpm-workspace.yaml`:

```yaml
packages:
  - "apps/*"
```

`package.json`:

```json
{
  "name": "real-mahjong",
  "private": true,
  "scripts": {
    "typecheck": "pnpm --recursive typecheck",
    "build": "pnpm --recursive build"
  }
}
```

`apps/web/package.json`:

```json
{
  "name": "@real-mahjong/web",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "typecheck": "tsc --noEmit"
  }
}
```

`apps/web/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "noEmit": true,
    "skipLibCheck": true,
    "lib": ["ES2022", "DOM"],
    "types": []
  },
  "include": ["src"]
}
```

`apps/web/vite.config.ts`:

```ts
import { defineConfig } from "vite";

export default defineConfig({
  server: { port: 5173 },
});
```

`apps/web/index.html`:

```html
<!doctype html>
<meta charset="utf-8" />
<title>Real Mahjong</title>
<div id="app"></div>
<script type="module" src="/src/main.ts"></script>
```

- [ ] **Step 5: 生成型を実際に使うコードを書く**

生成された型が壊れたら `tsc` が落ちるようにしておく。`apps/web/src/main.ts`:

```ts
import type { ClientEvent } from "./protocol/ClientEvent";
import type { Command } from "./protocol/Command";

/** 生成された型が期待どおりの形であることを、コンパイル時に確かめるための足場。 */
export function describeEvent(event: ClientEvent): string {
  return event.type;
}

export function discardCommand(tile: number, riichi: boolean): Command {
  return { type: "discard", tile, riichi };
}

document.querySelector("#app")!.textContent = "Real Mahjong";
```

- [ ] **Step 6: 型検査が通ることを確認する**

```bash
pnpm install
pnpm --filter @real-mahjong/web add -D typescript vite
pnpm --filter @real-mahjong/web typecheck
```

Expected: エラーなし。もし `Command` の `tile` が `number` でない形に生成されていたら、`main.ts` を実際の生成結果に合わせて直す（生成物ではなく利用側を直す）。

- [ ] **Step 7: 生成物のずれを CI で検出する**

`.github/workflows/ci.yml` の `rust` ジョブの末尾に追記する。

```yaml
      - name: 生成された TS 型が最新か確認
        run: |
          cargo test --package protocol
          git diff --exit-code -- apps/web/src/protocol
```

同じファイルに web ジョブを足す。

```yaml
  web:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: "22"
      - uses: pnpm/action-setup@v4
        with:
          version: 9
      - run: pnpm install --frozen-lockfile
      - run: pnpm --filter @real-mahjong/web typecheck
```

- [ ] **Step 8: コミット**

```bash
git add -A
git commit -m "feat: TypeScript 型の自動生成と web の足場を追加"
```

---

### Task 11: 各クレートのモジュール木を宣言する

Wave 1 以降のエージェントが `lib.rs` を奪い合わないよう、モジュール宣言をここで確定させる。**以降 `lib.rs` と `mod.rs` は編集禁止**とし、各エージェントは自分のファイルの中身だけを書く。

**Files:**
- Create: `crates/mahjong-core/{Cargo.toml,src/lib.rs,src/*.rs}`
- Create: `crates/mahjong-engine/{Cargo.toml,src/lib.rs,src/*.rs}`
- Create: `crates/mahjong-ai/{Cargo.toml,src/lib.rs,src/*.rs}`
- Create: `crates/mahjong-wasm/{Cargo.toml,src/lib.rs}`
- Create: `crates/server/{Cargo.toml,src/lib.rs,src/*.rs}`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: Task 1 の workspace、Task 2〜9 の `protocol`
- Produces: 空だがコンパイルの通る5クレートと、確定したモジュール木

- [ ] **Step 1: workspace メンバーを追加する**

ルート `Cargo.toml` の `members` を差し替える。

```toml
members = [
  "crates/protocol",
  "crates/mahjong-core",
  "crates/mahjong-engine",
  "crates/mahjong-ai",
  "crates/mahjong-wasm",
  "crates/server",
]
```

- [ ] **Step 2: mahjong-core の骨格を作る**

```bash
mkdir -p crates/mahjong-core/src/shanten crates/mahjong-core/src/yaku_check
touch crates/mahjong-core/src/{hand,shapes,wait,furiten,callable,decompose,fu,score}.rs
touch crates/mahjong-core/src/shanten/{standard,chiitoitsu,kokushi}.rs
touch crates/mahjong-core/src/yaku_check/{standard,yakuman}.rs
```

`hand.rs` と `shapes.rs` は Task 12 で中身を書く。判定系（Wave 1a）と点数系（Wave 1b）が共有する語彙であり、**Wave 1 では編集禁止**とする。

`crates/mahjong-core/Cargo.toml`:

```toml
[package]
name = "mahjong-core"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
protocol = { path = "../protocol" }

[lints]
workspace = true
```

`crates/mahjong-core/src/lib.rs`（**編集禁止**）:

```rust
//! 純粋な麻雀判定。乱数・I/O・時間を一切持たない。
//!
//! このファイルはモジュール木の宣言のみを担う。Wave 1 の作業では編集しないこと。

pub mod callable;
pub mod decompose;
pub mod fu;
pub mod furiten;
pub mod hand;
pub mod score;
pub mod shanten;
pub mod shapes;
pub mod wait;
pub mod yaku_check;
```

### ファイル所有権

Wave 1 の各エージェントが編集してよいファイルを、着手前に確定させる。所有者のいないファイルを残さない。

| ファイル | 所有 | 備考 |
|---|---|---|
| `hand.rs` / `shapes.rs` | Wave 0 | 共有語彙。Wave 1 では編集しない |
| `shanten/*.rs` | Wave 1a | |
| `wait.rs` | Wave 1a | 型は `shapes.rs`、算出はここ |
| `furiten.rs` | Wave 1a | |
| `callable.rs` | Wave 1a | チー・ポン・槓の候補のみ |
| `decompose.rs` | Wave 1b | 1a は自前の探索を `shanten/standard.rs` に持ち、ここを編集しない |
| `yaku_check/*.rs` | Wave 1b | |
| `fu.rs` | Wave 1b | 待ち形は `wait.rs` の結果を消費し、判定を再実装しない |
| `score.rs` | Wave 1b | |

**ロンの可否はどちらにも置かない。**役の有無に依存するため、1a の鳴き候補と 1b の役判定が揃った Wave 2 で engine 側が結線する。単一ファイルを両者に編集させないための措置である。

`crates/mahjong-core/src/shanten/mod.rs`（**編集禁止**）:

```rust
//! 向聴数の計算。標準形・七対子・国士無双の3形を別ファイルに分ける。

pub mod chiitoitsu;
pub mod kokushi;
pub mod standard;
```

`crates/mahjong-core/src/yaku_check/mod.rs`（**編集禁止**）:

```rust
//! 役の判定。通常役と役満を別ファイルに分ける。

pub mod standard;
pub mod yakuman;
```

- [ ] **Step 3: mahjong-engine の骨格を作る**

```bash
mkdir -p crates/mahjong-engine/src
touch crates/mahjong-engine/src/{wall,state,round,reaction,match_flow,invariant}.rs
```

`crates/mahjong-engine/Cargo.toml`:

```toml
[package]
name = "mahjong-engine"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
protocol = { path = "../protocol" }
mahjong-core = { path = "../mahjong-core" }

[lints]
workspace = true
```

`crates/mahjong-engine/src/lib.rs`（**編集禁止**）:

```rust
//! 局と半荘の進行。シードを外から注入され、イベント列を生成する。
//! 時間も I/O も持たないため、同じシードと同じ入力からは必ず同じ結果になる。

pub mod invariant;
pub mod match_flow;
pub mod reaction;
pub mod round;
pub mod state;
pub mod wall;
```

- [ ] **Step 4: mahjong-ai / mahjong-wasm / server の骨格を作る**

```bash
mkdir -p crates/mahjong-ai/src crates/mahjong-wasm/src crates/server/src
touch crates/mahjong-ai/src/{safety,discard,call}.rs
touch crates/server/src/{table,session,matchmaking,persistence}.rs
```

`crates/mahjong-ai/Cargo.toml` は `protocol` と `mahjong-core` に依存する。`crates/server/Cargo.toml` は `protocol` と `mahjong-engine` と `mahjong-ai` に依存する。`crates/mahjong-wasm/Cargo.toml` は `protocol` と `mahjong-core` にのみ依存する。いずれも `[package]` は mahjong-core と同じ形にする。

`crates/mahjong-ai/src/lib.rs`（**編集禁止**）:

```rust
//! ルールベースの CPU 雀士。core の判定のみを使い、engine には依存しない。

pub mod call;
pub mod discard;
pub mod safety;
```

`crates/mahjong-wasm/src/lib.rs`（**編集禁止**）:

```rust
//! クライアント向けの WASM 境界。core の判定系のみを公開する。
//! engine（山を持つ側）をここから参照してはならない。
```

`crates/server/src/lib.rs`（**編集禁止**）:

```rust
//! 唯一 I/O と時間を持つ層。1卓 = 1 tokio task の Actor とする。

pub mod matchmaking;
pub mod persistence;
pub mod session;
pub mod table;
```

- [ ] **Step 5: 全体がビルドできることを確認する**

Run: `cargo build --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: 3つとも成功

- [ ] **Step 6: 依存の向きが設計どおりであることを確認する**

Run: `cargo tree --package mahjong-wasm --depth 1`
Expected: `protocol` と `mahjong-core` のみが並び、`mahjong-engine` が現れない

- [ ] **Step 7: コミット**

```bash
git add -A
git commit -m "chore: 全クレートのモジュール木を宣言"
```

---

### Task 12: 共有語彙（手牌表現と待ち形・分解結果）

判定系（Wave 1a）と点数系（Wave 1b）を素朴に分けると、同じファイルを取り合う。符計算は待ち形を必要とし、向聴計算と役判定はどちらも面子分解を必要とする。

解決は Wave 0 の原則の延長である。**両者が共有する語彙（型）をここで凍結し、それを使うアルゴリズムを Wave 1 で分担する。**型が先にあるため、1b は 1a の実装完了を待たずに `WaitShape` を直接組み立ててテストを書ける。

**Files:**
- Modify: `crates/mahjong-core/src/hand.rs`
- Modify: `crates/mahjong-core/src/shapes.rs`

**Interfaces:**
- Consumes: Task 2 `Tile` / `TileKind`、Task 3 `parse_hand`、Task 5 `Meld`
- Produces:
  - `pub struct HandCounts([u8; 34])` — `HandCounts::new() -> Self`, `from_tiles(&[Tile]) -> Self`, `get(TileKind) -> u8`, `add(TileKind)`, `remove(TileKind) -> bool`, `total() -> u8`, `kinds() -> impl Iterator<Item = (TileKind, u8)>`
  - `pub enum WaitShape { Ryanmen, Penchan, Kanchan, Shanpon, Tanki }`
  - `pub enum Block { Run(TileKind), Triplet(TileKind), Pair(TileKind) }` — `Run` は最小の牌で表す（`123m` なら `1m`）
  - `pub struct Decomposition { pub blocks: Vec<Block>, pub pair: TileKind, pub melds: Vec<Meld>, pub wait: WaitShape }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/mahjong-core/src/hand.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::parse_hand;
    use protocol::tile::TileKind;

    #[test]
    fn counts_tiles_by_kind_ignoring_red() {
        // 0p は赤5pであり、5p と同じ種類として数える。
        let counts = HandCounts::from_tiles(&parse_hand("55p0p").unwrap());
        assert_eq!(counts.get(TileKind::from_index(13).unwrap()), 3);
        assert_eq!(counts.total(), 3);
    }

    #[test]
    fn add_and_remove_round_trip() {
        let mut counts = HandCounts::new();
        let east = TileKind::from_index(27).unwrap();
        counts.add(east);
        counts.add(east);
        assert_eq!(counts.get(east), 2);
        assert!(counts.remove(east));
        assert_eq!(counts.get(east), 1);
        assert!(counts.remove(east));
        assert!(!counts.remove(east), "0枚からは取り除けない");
    }

    #[test]
    fn kinds_lists_only_present_tiles() {
        let counts = HandCounts::from_tiles(&parse_hand("111m9s").unwrap());
        let present: Vec<(u8, u8)> = counts.kinds().map(|(k, n)| (k.index(), n)).collect();
        assert_eq!(present, vec![(0, 3), (26, 1)]);
    }

    #[test]
    fn a_full_hand_totals_thirteen() {
        let counts = HandCounts::from_tiles(&parse_hand("123456789m123p11s").unwrap());
        assert_eq!(counts.total(), 14);
    }
}
```

`crates/mahjong-core/src/shapes.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::tile::TileKind;

    #[test]
    fn a_decomposition_names_its_blocks_and_wait() {
        let decomposition = Decomposition {
            blocks: vec![
                Block::Run(TileKind::from_index(0).unwrap()),
                Block::Run(TileKind::from_index(3).unwrap()),
                Block::Triplet(TileKind::from_index(27).unwrap()),
                Block::Run(TileKind::from_index(9).unwrap()),
            ],
            pair: TileKind::from_index(18).unwrap(),
            melds: vec![],
            wait: WaitShape::Ryanmen,
        };
        assert_eq!(decomposition.blocks.len(), 4);
        assert_eq!(decomposition.wait, WaitShape::Ryanmen);
    }

    #[test]
    fn wait_shapes_are_distinguishable() {
        let all = [
            WaitShape::Ryanmen,
            WaitShape::Penchan,
            WaitShape::Kanchan,
            WaitShape::Shanpon,
            WaitShape::Tanki,
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    /// 符計算はこの対応表に従う。値そのものは fu.rs（Wave 1b）が持つが、
    /// 「どの待ちが2符か」は語彙として固定しておく。
    #[test]
    fn only_penchan_kanchan_and_tanki_earn_fu() {
        assert!(!WaitShape::Ryanmen.earns_fu());
        assert!(!WaitShape::Shanpon.earns_fu());
        assert!(WaitShape::Penchan.earns_fu());
        assert!(WaitShape::Kanchan.earns_fu());
        assert!(WaitShape::Tanki.earns_fu());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core`
Expected: コンパイルエラー（`HandCounts` / `Decomposition` などが未定義）

- [ ] **Step 3: hand.rs の実装を書く**

```rust
//! 手牌の集計表現。判定系と点数系が共有するため Wave 0 で凍結する。

use protocol::tile::{Tile, TileKind};

/// 34種それぞれの枚数。赤ドラは対応する通常牌として数える。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HandCounts([u8; TileKind::COUNT]);

impl HandCounts {
    pub fn new() -> Self {
        HandCounts([0; TileKind::COUNT])
    }

    pub fn from_tiles(tiles: &[Tile]) -> Self {
        let mut counts = HandCounts::new();
        for tile in tiles {
            counts.add(tile.kind());
        }
        counts
    }

    pub fn get(&self, kind: TileKind) -> u8 {
        self.0[kind.index() as usize]
    }

    pub fn add(&mut self, kind: TileKind) {
        self.0[kind.index() as usize] += 1;
    }

    /// 1枚取り除く。0枚なら false を返して何もしない。
    pub fn remove(&mut self, kind: TileKind) -> bool {
        let slot = &mut self.0[kind.index() as usize];
        if *slot == 0 {
            false
        } else {
            *slot -= 1;
            true
        }
    }

    pub fn total(&self) -> u8 {
        self.0.iter().sum()
    }

    /// 1枚以上ある種類だけを、種類の昇順で返す。
    pub fn kinds(&self) -> impl Iterator<Item = (TileKind, u8)> + '_ {
        self.0.iter().enumerate().filter_map(|(index, &count)| {
            (count > 0).then(|| (TileKind::from_index(index as u8).expect("範囲内"), count))
        })
    }

    pub fn as_array(&self) -> &[u8; TileKind::COUNT] {
        &self.0
    }
}
```

- [ ] **Step 4: shapes.rs の実装を書く**

```rust
//! 和了形の語彙。待ち形は Wave 1a が算出し、Wave 1b が符計算で消費する。
//! 分解結果は Wave 1b が生成する。型を先に凍結することで両者が同時に着手できる。

use protocol::meld::Meld;
use protocol::tile::TileKind;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitShape {
    Ryanmen,
    Penchan,
    Kanchan,
    Shanpon,
    Tanki,
}

impl WaitShape {
    /// 符が付く待ちかどうか。両面と双碰は0符、それ以外は2符。
    pub fn earns_fu(self) -> bool {
        matches!(
            self,
            WaitShape::Penchan | WaitShape::Kanchan | WaitShape::Tanki
        )
    }
}

/// 面子ひとつ。順子は最小の牌で表す（123m なら 1m）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Block {
    Run(TileKind),
    Triplet(TileKind),
    Pair(TileKind),
}

/// 和了形をひととおりに分解した結果。同じ手が複数通りに分解できる場合、
/// どれを採るかは点数が最大になる方を選ぶ（score.rs の責務）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Decomposition {
    /// 副露を含まない、手の内で構成した面子。
    pub blocks: Vec<Block>,
    pub pair: TileKind,
    pub melds: Vec<Meld>,
    pub wait: WaitShape,
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test --package mahjong-core`
Expected: 7テスト PASS

- [ ] **Step 6: コミット**

```bash
git add crates/mahjong-core/src
git commit -m "feat(core): 判定系と点数系が共有する手牌表現と和了形の語彙を追加"
```

---

### Task 13: 期待値テーブル（採点）と読み込みクレート

これが「テストを仕様書にする」の実体である。Wave 1b の採点実装は、このテーブルを通すことが完了条件になる。**期待値はエージェントが勝手に変更してはならない**（変更が必要に見えたらコーディネータへ報告する）。

**Files:**
- Create: `crates/test-fixtures/{Cargo.toml,src/lib.rs}`
- Create: `fixtures/scoring/{basic,dealer,melded,yakuman,fu-coverage}.json`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: Task 5 `YakuId`
- Produces:
  - `pub fn load_scoring_cases() -> Vec<ScoringCase>`
  - `pub struct ScoringCase { pub id, pub note, pub concealed: String, pub melds: Vec<MeldSpec>, pub win_tile: String, pub win_type: WinType, pub context: Context, pub expect: Expect }`
  - `pub enum WinType { Tsumo, Ron }`
  - `pub struct MeldSpec { pub kind: String, pub tiles: String, pub from: u8, pub called_tile: Option<String> }`
  - `pub struct Context { pub seat_wind, pub round_wind: String, pub riichi, pub double_riichi, pub ippatsu, pub rinshan, pub chankan, pub haitei, pub houtei: bool, pub dora_indicators, pub ura_indicators: Vec<String> }`
  - `pub struct Expect { pub yaku: Vec<YakuExpect>, pub fu: u8, pub han: u8, pub payment: Payment }`
  - `pub struct YakuExpect { pub id: YakuId, pub han: u8 }`
  - `pub enum Payment { Ron { total: i32 }, TsumoDealer { from_each: i32 }, TsumoNonDealer { from_dealer: i32, from_each_non_dealer: i32 } }`

- [ ] **Step 1: 採点ケースを書く**

和了者は常に席0とする。`from` は放銃者の絶対席番号。

`fixtures/scoring/basic.json`:

```json
[
  {
    "id": "pinfu-tsumo-nondealer",
    "note": "平和ツモ。全牌が中張牌なので断幺九も複合して20符3翻。子のツモは 700/1300",
    "concealed": "234567m23478p22s",
    "melds": [],
    "win_tile": "6p",
    "win_type": "tsumo",
    "context": {
      "seat_wind": "south",
      "round_wind": "east",
      "riichi": false,
      "double_riichi": false,
      "ippatsu": false,
      "rinshan": false,
      "chankan": false,
      "haitei": false,
      "houtei": false,
      "dora_indicators": ["1z"],
      "ura_indicators": []
    },
    "expect": {
      "yaku": [
        { "id": "pinfu", "han": 1 },
        { "id": "menzen_tsumo", "han": 1 },
        { "id": "tanyao", "han": 1 }
      ],
      "fu": 20,
      "han": 3,
      "payment": { "type": "tsumo_non_dealer", "from_dealer": 1300, "from_each_non_dealer": 700 }
    }
  },
  {
    "id": "pinfu-riichi-ron-nondealer",
    "note": "立直＋平和＋断幺九。副底20＋門前ロン10で30符3翻＝3900。dealer-mangan-tsumo と同一の手牌であり、役の判定は一致していなければならない",
    "concealed": "234567m23467p55s",
    "melds": [],
    "win_tile": "8p",
    "win_type": "ron",
    "context": {
      "seat_wind": "south",
      "round_wind": "east",
      "riichi": true,
      "double_riichi": false,
      "ippatsu": false,
      "rinshan": false,
      "chankan": false,
      "haitei": false,
      "houtei": false,
      "dora_indicators": ["1z"],
      "ura_indicators": []
    },
    "expect": {
      "yaku": [
        { "id": "riichi", "han": 1 },
        { "id": "pinfu", "han": 1 },
        { "id": "tanyao", "han": 1 }
      ],
      "fu": 30,
      "han": 3,
      "payment": { "type": "ron", "total": 3900 }
    }
  },
  {
    "id": "riichi-only-penchan-ron",
    "note": "辺張待ちで符が付く例。20＋門前ロン10＋辺張2＝32→切り上げ40符1翻＝1300",
    "concealed": "12456m234678p55s",
    "melds": [],
    "win_tile": "3m",
    "win_type": "ron",
    "context": {
      "seat_wind": "south",
      "round_wind": "east",
      "riichi": true,
      "double_riichi": false,
      "ippatsu": false,
      "rinshan": false,
      "chankan": false,
      "haitei": false,
      "houtei": false,
      "dora_indicators": ["1z"],
      "ura_indicators": []
    },
    "expect": {
      "yaku": [{ "id": "riichi", "han": 1 }],
      "fu": 40,
      "han": 1,
      "payment": { "type": "ron", "total": 1300 }
    }
  },
  {
    "id": "chiitoitsu-riichi-tsumo",
    "note": "七対子は25符固定。25符4翻＝1600/3200",
    "concealed": "1122m3344p5566s7z",
    "melds": [],
    "win_tile": "7z",
    "win_type": "tsumo",
    "context": {
      "seat_wind": "south",
      "round_wind": "east",
      "riichi": true,
      "double_riichi": false,
      "ippatsu": false,
      "rinshan": false,
      "chankan": false,
      "haitei": false,
      "houtei": false,
      "dora_indicators": ["1z"],
      "ura_indicators": []
    },
    "expect": {
      "yaku": [
        { "id": "riichi", "han": 1 },
        { "id": "chiitoitsu", "han": 2 },
        { "id": "menzen_tsumo", "han": 1 }
      ],
      "fu": 25,
      "han": 4,
      "payment": { "type": "tsumo_non_dealer", "from_dealer": 3200, "from_each_non_dealer": 1600 }
    }
  }
]
```

`fixtures/scoring/dealer.json`:

```json
[
  {
    "id": "dealer-mangan-tsumo",
    "note": "立直＋ツモ＋平和＋断幺九＋ドラ1で5翻。親の満貫ツモは4000オール",
    "concealed": "234567m23467p55s",
    "melds": [],
    "win_tile": "8p",
    "win_type": "tsumo",
    "context": {
      "seat_wind": "east",
      "round_wind": "east",
      "riichi": true,
      "double_riichi": false,
      "ippatsu": false,
      "rinshan": false,
      "chankan": false,
      "haitei": false,
      "houtei": false,
      "dora_indicators": ["4m"],
      "ura_indicators": []
    },
    "expect": {
      "yaku": [
        { "id": "riichi", "han": 1 },
        { "id": "menzen_tsumo", "han": 1 },
        { "id": "pinfu", "han": 1 },
        { "id": "tanyao", "han": 1 },
        { "id": "dora", "han": 1 }
      ],
      "fu": 20,
      "han": 5,
      "payment": { "type": "tsumo_dealer", "from_each": 4000 }
    }
  }
]
```

`fixtures/scoring/melded.json`:

```json
[
  {
    "id": "kuitan-pon-chi-ron",
    "note": "喰いタン。20＋明刻(中張)2＝22→切り上げ30符1翻＝1000。副露ありのロンに門前加符は付かない",
    "concealed": "88p34678s",
    "melds": [
      { "kind": "pon", "tiles": "222m", "from": 2, "called_tile": "2m" },
      { "kind": "chi", "tiles": "345p", "from": 3, "called_tile": "3p" }
    ],
    "win_tile": "5s",
    "win_type": "ron",
    "context": {
      "seat_wind": "south",
      "round_wind": "east",
      "riichi": false,
      "double_riichi": false,
      "ippatsu": false,
      "rinshan": false,
      "chankan": false,
      "haitei": false,
      "houtei": false,
      "dora_indicators": ["1z"],
      "ura_indicators": []
    },
    "expect": {
      "yaku": [{ "id": "tanyao", "han": 1 }],
      "fu": 30,
      "han": 1,
      "payment": { "type": "ron", "total": 1000 }
    }
  }
]
```

`fixtures/scoring/yakuman.json`:

```json
[
  {
    "id": "kokushi-tanki-ron",
    "note": "1mを重ねた単騎形。十三面ではないため単倍の役満32000",
    "concealed": "119m19p19s123456z",
    "melds": [],
    "win_tile": "7z",
    "win_type": "ron",
    "context": {
      "seat_wind": "south",
      "round_wind": "east",
      "riichi": false,
      "double_riichi": false,
      "ippatsu": false,
      "rinshan": false,
      "chankan": false,
      "haitei": false,
      "houtei": false,
      "dora_indicators": ["1p"],
      "ura_indicators": []
    },
    "expect": {
      "yaku": [{ "id": "kokushi_musou", "han": 13 }],
      "fu": 0,
      "han": 13,
      "payment": { "type": "ron", "total": 32000 }
    }
  }
]
```

- [ ] **Step 1b: 符の論点を直接固定するケースを書く**

上の4ファイルは役と支払いが主眼で、符の構成要素を個別に踏んでいない。暗刻・明刻・嵌張・単騎・役牌雀頭・非平和ツモ2符と、「切り上げ満貫なし」の境界を直接固定する。

`fixtures/scoring/fu-coverage.json`:

```json
[
  {
    "id": "ankou-kanchan-yakuhai-pair-tsumo",
    "note": "副底20＋幺九暗刻8＋嵌張2＋役牌雀頭2＋ツモ2＝34→40符。門前ツモのみ1翻で 400/700",
    "concealed": "111m456m789p13s55z",
    "melds": [],
    "win_tile": "2s",
    "win_type": "tsumo",
    "context": {
      "seat_wind": "south",
      "round_wind": "east",
      "riichi": false,
      "double_riichi": false,
      "ippatsu": false,
      "rinshan": false,
      "chankan": false,
      "haitei": false,
      "houtei": false,
      "dora_indicators": ["9z"],
      "ura_indicators": []
    },
    "expect": {
      "yaku": [{ "id": "menzen_tsumo", "han": 1 }],
      "fu": 40,
      "han": 1,
      "payment": { "type": "tsumo_non_dealer", "from_dealer": 700, "from_each_non_dealer": 400 }
    }
  },
  {
    "id": "toitoi-minkou-ankou-tanki-ron",
    "note": "副底20＋明刻2＋明刻2＋中張暗刻4＋幺九暗刻8＋役牌雀頭2＋単騎2＝40符。対々和2翻で子ロン2600",
    "concealed": "888999s7z",
    "melds": [
      { "kind": "pon", "tiles": "222m", "from": 1, "called_tile": "2m" },
      { "kind": "pon", "tiles": "555p", "from": 2, "called_tile": "5p" }
    ],
    "win_tile": "7z",
    "win_type": "ron",
    "context": {
      "seat_wind": "south",
      "round_wind": "east",
      "riichi": false,
      "double_riichi": false,
      "ippatsu": false,
      "rinshan": false,
      "chankan": false,
      "haitei": false,
      "houtei": false,
      "dora_indicators": ["9z"],
      "ura_indicators": []
    },
    "expect": {
      "yaku": [{ "id": "toitoi", "han": 2 }],
      "fu": 40,
      "han": 2,
      "payment": { "type": "ron", "total": 2600 }
    }
  },
  {
    "id": "no-round-up-mangan-30fu-4han",
    "note": "切り上げ満貫なしの境界。30符4翻は8000ではなく7700。pinfu-riichi-ron-nondealer とドラ表示牌だけが違う",
    "concealed": "234567m23467p55s",
    "melds": [],
    "win_tile": "8p",
    "win_type": "ron",
    "context": {
      "seat_wind": "south",
      "round_wind": "east",
      "riichi": true,
      "double_riichi": false,
      "ippatsu": false,
      "rinshan": false,
      "chankan": false,
      "haitei": false,
      "houtei": false,
      "dora_indicators": ["4m"],
      "ura_indicators": []
    },
    "expect": {
      "yaku": [
        { "id": "riichi", "han": 1 },
        { "id": "pinfu", "han": 1 },
        { "id": "tanyao", "han": 1 },
        { "id": "dora", "han": 1 }
      ],
      "fu": 30,
      "han": 4,
      "payment": { "type": "ron", "total": 7700 }
    }
  }
]
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/test-fixtures/src/lib.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_scoring_case_loads() {
        let cases = load_scoring_cases();
        assert!(cases.len() >= 10, "実際に読めたのは {} 件", cases.len());
    }

    /// 同じ手牌・同じ和了牌なら、役の集合も一致していなければならない。
    /// 断幺九の取りこぼしはこの検査で機械的に見つかる。
    #[test]
    fn identical_hands_declare_identical_yaku_sets() {
        use std::collections::BTreeSet;
        let mut by_hand: std::collections::HashMap<(String, String), Vec<&ScoringCase>> =
            std::collections::HashMap::new();
        let cases = load_scoring_cases();
        for case in &cases {
            if !case.melds.is_empty() {
                continue;
            }
            by_hand
                .entry((case.concealed.clone(), case.win_tile.clone()))
                .or_default()
                .push(case);
        }

        for ((hand, win), group) in by_hand {
            // 状況役（立直・ツモ・ドラ）は文脈で変わるため、手牌のみで決まる役に絞って比較する。
            let structural = |case: &ScoringCase| -> BTreeSet<YakuId> {
                case.expect
                    .yaku
                    .iter()
                    .map(|y| y.id)
                    .filter(|id| {
                        !id.is_dora()
                            && !matches!(
                                id,
                                YakuId::Riichi
                                    | YakuId::DoubleRiichi
                                    | YakuId::Ippatsu
                                    | YakuId::MenzenTsumo
                                    | YakuId::HaiteiRaoyue
                                    | YakuId::HouteiRaoyui
                                    | YakuId::RinshanKaihou
                                    | YakuId::Chankan
                            )
                    })
                    .collect()
            };
            let Some(first) = group.first() else { continue };
            let expected = structural(first);
            for case in &group {
                assert_eq!(
                    structural(case),
                    expected,
                    "{hand}+{win}: {} と {} で手牌由来の役が食い違う",
                    first.id,
                    case.id
                );
            }
        }
    }

    #[test]
    fn scoring_case_ids_are_unique() {
        let cases = load_scoring_cases();
        let mut seen = HashSet::new();
        for case in &cases {
            assert!(seen.insert(case.id.clone()), "id が重複している: {}", case.id);
        }
    }

    #[test]
    fn declared_han_equals_the_sum_of_listed_yaku() {
        for case in load_scoring_cases() {
            let sum: u32 = case.expect.yaku.iter().map(|y| y.han as u32).sum();
            assert_eq!(
                sum, case.expect.han as u32,
                "{}: 役の翻の合計({sum})と han({}) が食い違う",
                case.id, case.expect.han
            );
        }
    }

    #[test]
    fn tsumo_cases_use_a_tsumo_payment() {
        for case in load_scoring_cases() {
            let matches = match (&case.win_type, &case.expect.payment) {
                (WinType::Ron, Payment::Ron { .. }) => true,
                (WinType::Tsumo, Payment::TsumoDealer { .. }) => true,
                (WinType::Tsumo, Payment::TsumoNonDealer { .. }) => true,
                _ => false,
            };
            assert!(matches, "{}: 和了種別と支払い形式が食い違う", case.id);
        }
    }

    #[test]
    fn dealer_flag_agrees_with_the_payment_shape() {
        for case in load_scoring_cases() {
            let is_dealer = case.context.seat_wind == "east";
            if let Payment::TsumoDealer { .. } = case.expect.payment {
                assert!(is_dealer, "{}: 親ツモ支払いなのに自風が東でない", case.id);
            }
            if let Payment::TsumoNonDealer { .. } = case.expect.payment {
                assert!(!is_dealer, "{}: 子ツモ支払いなのに自風が東", case.id);
            }
        }
    }
}
```

- [ ] **Step 3: テストが失敗することを確認する**

Run: `cargo test --package test-fixtures`
Expected: パッケージが存在せず失敗する

- [ ] **Step 4: クレートを作る**

ルート `Cargo.toml` の `members` に `"crates/test-fixtures"` を足す。

`crates/test-fixtures/Cargo.toml`:

```toml
[package]
name = "test-fixtures"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
protocol = { path = "../protocol" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[lints]
workspace = true
```

- [ ] **Step 5: 読み込み実装を書く**

`crates/test-fixtures/src/lib.rs` の先頭に置く:

```rust
//! 期待値テーブルの読み込み。実装側ではなく仕様側の資産であり、
//! 期待値の変更はコーディネータの承認を要する。

use std::path::PathBuf;

use protocol::yaku::YakuId;
use serde::Deserialize;

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WinType {
    Tsumo,
    Ron,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub struct MeldSpec {
    pub kind: String,
    pub tiles: String,
    pub from: u8,
    pub called_tile: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub struct Context {
    pub seat_wind: String,
    pub round_wind: String,
    pub riichi: bool,
    pub double_riichi: bool,
    pub ippatsu: bool,
    pub rinshan: bool,
    pub chankan: bool,
    pub haitei: bool,
    pub houtei: bool,
    pub dora_indicators: Vec<String>,
    pub ura_indicators: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub struct YakuExpect {
    pub id: YakuId,
    pub han: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Payment {
    Ron {
        total: i32,
    },
    TsumoDealer {
        from_each: i32,
    },
    TsumoNonDealer {
        from_dealer: i32,
        from_each_non_dealer: i32,
    },
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub struct Expect {
    pub yaku: Vec<YakuExpect>,
    pub fu: u8,
    pub han: u8,
    pub payment: Payment,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub struct ScoringCase {
    pub id: String,
    pub note: String,
    /// 和了牌を含まない手牌。和了者は常に席0とする。
    pub concealed: String,
    pub melds: Vec<MeldSpec>,
    pub win_tile: String,
    pub win_type: WinType,
    pub context: Context,
    pub expect: Expect,
}

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
}

fn load_dir<T: serde::de::DeserializeOwned>(subdir: &str) -> Vec<T> {
    let dir = fixtures_root().join(subdir);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} を読めない: {e}", dir.display()))
        .map(|entry| entry.expect("ディレクトリ項目").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();

    let mut out = Vec::new();
    for path in paths {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} を読めない: {e}", path.display()));
        let batch: Vec<T> = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} の解釈に失敗: {e}", path.display()));
        out.extend(batch);
    }
    out
}

pub fn load_scoring_cases() -> Vec<ScoringCase> {
    load_dir("scoring")
}
```

- [ ] **Step 6: テストが通ることを確認する**

Run: `cargo test --package test-fixtures`
Expected: 5テスト PASS

- [ ] **Step 7: コミット**

```bash
git add -A
git commit -m "feat(fixtures): 採点の期待値テーブルと読み込みを追加"
```

---

### Task 14: 期待値テーブル（向聴数）

**Files:**
- Create: `fixtures/shanten/basic.json`
- Modify: `crates/test-fixtures/src/lib.rs`

**Interfaces:**
- Consumes: Task 13 の `load_dir`
- Produces:
  - `pub fn load_shanten_cases() -> Vec<ShantenCase>`
  - `pub struct ShantenCase { pub id: String, pub note: String, pub concealed: String, pub melds: u8, pub expect: ShantenExpect }`
  - `pub struct ShantenExpect { pub overall: i8, pub chiitoitsu: Option<i8>, pub kokushi: Option<i8> }`

向聴数の規約: **-1 が和了、0 がテンパイ**。標準形の最大は 8。

- [ ] **Step 1: 向聴数ケースを書く**

`fixtures/shanten/basic.json`:

```json
[
  {
    "id": "complete-hand",
    "note": "和了形は -1",
    "concealed": "123456789m123p11s",
    "melds": 0,
    "expect": { "overall": -1, "chiitoitsu": null, "kokushi": null }
  },
  {
    "id": "tenpai-penchan",
    "note": "3面子＋辺張＋雀頭でテンパイ（12p の 3p 待ち）",
    "concealed": "123m456m789m12p11s",
    "melds": 0,
    "expect": { "overall": 0, "chiitoitsu": null, "kokushi": null }
  },
  {
    "id": "tenpai-kanchan",
    "note": "嵌張待ちでもテンパイ（2p待ち）",
    "concealed": "123m456m789m13p11s",
    "melds": 0,
    "expect": { "overall": 0, "chiitoitsu": null, "kokushi": null }
  },
  {
    "id": "tenpai-shanpon",
    "note": "3面子＋対子2つのシャンポン待ち",
    "concealed": "123m456m789m11p11s",
    "melds": 0,
    "expect": { "overall": 0, "chiitoitsu": null, "kokushi": null }
  },
  {
    "id": "one-shanten-isolated",
    "note": "14pは搭子にならないため1シャンテン",
    "concealed": "123m456m789m14p11s",
    "melds": 0,
    "expect": { "overall": 1, "chiitoitsu": null, "kokushi": null }
  },
  {
    "id": "chiitoitsu-tenpai",
    "note": "6対子＋7種でテンパイ。七対子形の向聴が最小になる",
    "concealed": "1122m3344p5566s7z",
    "melds": 0,
    "expect": { "overall": 0, "chiitoitsu": 0, "kokushi": null }
  },
  {
    "id": "kokushi-13-wait",
    "note": "13種すべてを1枚ずつ持つ十三面待ち",
    "concealed": "19m19p19s1234567z",
    "melds": 0,
    "expect": { "overall": 0, "chiitoitsu": null, "kokushi": 0 }
  },
  {
    "id": "kokushi-tanki-tenpai",
    "note": "12種＋1mの対子。7z単騎でテンパイ",
    "concealed": "119m19p19s123456z",
    "melds": 0,
    "expect": { "overall": 0, "chiitoitsu": null, "kokushi": 0 }
  },
  {
    "id": "worst-case-scattered",
    "note": "面子も搭子も対子も無い手。七対子形が最小で6シャンテン",
    "concealed": "147m258p369s1234z",
    "melds": 0,
    "expect": { "overall": 6, "chiitoitsu": 6, "kokushi": 7 }
  },
  {
    "id": "melded-tenpai",
    "note": "1副露して残り10枚。2面子＋辺張＋雀頭でテンパイ（12p の 3p 待ち）",
    "concealed": "123m456m12p11s",
    "melds": 1,
    "expect": { "overall": 0, "chiitoitsu": null, "kokushi": null }
  }
]
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/test-fixtures/src/lib.rs` の `mod tests` に足す:

```rust
    #[test]
    fn every_shanten_case_loads() {
        let cases = load_shanten_cases();
        assert!(cases.len() >= 10, "実際に読めたのは {} 件", cases.len());
    }

    #[test]
    fn shanten_values_are_in_range() {
        for case in load_shanten_cases() {
            assert!(
                (-1..=8).contains(&case.expect.overall),
                "{}: overall が範囲外({})",
                case.id,
                case.expect.overall
            );
        }
    }

    #[test]
    fn overall_never_exceeds_a_declared_form() {
        for case in load_shanten_cases() {
            for form in [case.expect.chiitoitsu, case.expect.kokushi]
                .into_iter()
                .flatten()
            {
                assert!(
                    case.expect.overall <= form,
                    "{}: overall({}) が個別形({form}) を上回っている",
                    case.id,
                    case.expect.overall
                );
            }
        }
    }

    #[test]
    fn hand_size_matches_the_declared_meld_count() {
        for case in load_shanten_cases() {
            let tiles = protocol::notation::parse_hand(&case.concealed)
                .unwrap_or_else(|e| panic!("{}: 記法が不正 {e}", case.id));
            let expected = 13 - (case.melds as usize) * 3;
            assert!(
                tiles.len() == expected || tiles.len() == expected + 1,
                "{}: 手牌が{}枚。副露{}なら{}枚か{}枚のはず",
                case.id,
                tiles.len(),
                case.melds,
                expected,
                expected + 1
            );
        }
    }
```

- [ ] **Step 3: テストが失敗することを確認する**

Run: `cargo test --package test-fixtures shanten`
Expected: `load_shanten_cases` が未定義でコンパイルエラー

- [ ] **Step 4: 実装を足す**

`crates/test-fixtures/src/lib.rs` に追記:

```rust
/// 向聴数の期待値。-1 が和了、0 がテンパイ。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub struct ShantenExpect {
    pub overall: i8,
    /// その形が論点になるケースでのみ指定する。
    pub chiitoitsu: Option<i8>,
    pub kokushi: Option<i8>,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub struct ShantenCase {
    pub id: String,
    pub note: String,
    pub concealed: String,
    /// 副露の数。手牌の枚数は 13 - melds*3（ツモ番なら +1）。
    pub melds: u8,
    pub expect: ShantenExpect,
}

pub fn load_shanten_cases() -> Vec<ShantenCase> {
    load_dir("shanten")
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test --package test-fixtures`
Expected: 9テスト PASS

- [ ] **Step 6: コミット**

```bash
git add -A
git commit -m "feat(fixtures): 向聴数の期待値テーブルを追加"
```

---

### Task 15: エージェント作業規約

Wave 1 以降、複数のエージェントが別々の worktree で同時に作業する。衝突と手戻りを防ぐ規約をリポジトリに置く。ファイル名を `AGENTS.md` にするのは、Codex と opencode がこの名前を自動で読むためである。

**Files:**
- Create: `AGENTS.md`

**Interfaces:**
- Consumes: Task 11 のモジュール木、Task 12 の共有語彙、Task 13/14 の期待値テーブル
- Produces: なし（ドキュメント）

- [ ] **Step 1: AGENTS.md を書く**

```markdown
# エージェント作業規約

このリポジトリは複数の AI エージェントが別々の worktree で並行実装する。
以下は衝突と手戻りを防ぐための取り決めである。

## 設計と計画

- 設計仕様: `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
- 実装計画: `docs/superpowers/plans/` 配下
- 仕様と食い違う実装が必要に見えたら、実装を進めずコーディネータへ報告する

## 触ってよい範囲

- 自分に割り当てられたクレートの、自分のファイルのみを編集する
- **`lib.rs` と `mod.rs` は編集しない。** モジュール木は Wave 0 で確定済みである。
  新しいファイルが必要になったらコーディネータへ報告する
- 他のクレートを編集しない。他クレートの API が足りない場合も、自分で足さずに報告する
- `crates/protocol` は凍結済みである。変更が必要に見えたら必ず報告する
- **`mahjong-core/src/hand.rs` と `shapes.rs` は共有語彙であり編集禁止。**
  判定系と点数系が同じ表現の上で書くための土台である

### mahjong-core のファイル所有権

| ファイル | 所有 |
|---|---|
| `hand.rs` / `shapes.rs` | Wave 0（編集禁止） |
| `shanten/*.rs` / `wait.rs` / `furiten.rs` / `callable.rs` | Wave 1a |
| `decompose.rs` / `yaku_check/*.rs` / `fu.rs` / `score.rs` | Wave 1b |

Wave 1a は面子分解が必要になっても `decompose.rs` を編集せず、自前の探索を
`shanten/standard.rs` の中に持つ。Wave 1b は待ち形の判定を再実装せず、
`wait.rs` の公開結果を消費する。

**ロンの可否はどちらにも実装しない。**役の有無に依存するため、Wave 2 で engine が結線する。

## 期待値テーブル

- `fixtures/` は仕様側の資産である
- **既存の期待値を変更してはならない。** 自分の実装が通らない場合、まず実装を疑う
- 期待値が誤っていると確信した場合は、根拠（採点の内訳）を添えて報告する
- ケースの追加は歓迎する。追加時は既存ファイルではなく新しい JSON ファイルを作る
  （同じファイルを複数エージェントが編集すると衝突するため）

## 完了条件

自分の作業単位は、次がすべて通って初めて完了である。

```bash
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

「実装したが未検証」の状態で完了を報告しない。

## 進捗の報告

意味のある区切りで worktree のコメントを更新する。

```bash
orca worktree set --worktree active --comment "向聴数の標準形を実装。七対子形に着手" --json
```

更新すべき区切り: 着手、主要な部品の完成、テスト通過、行き詰まり、完了。

## コミット

- テストが通った単位でコミットする
- コミットメッセージは日本語で、何をなぜ変えたかを書く
- 生成物（`apps/web/src/protocol/`）は手で編集せず、`cargo test --package protocol` で再生成する

## 麻雀ドメインの注意

- 向聴数の規約は **-1 が和了、0 がテンパイ**
- 牌の記法は `123m456p789s1234567z`、赤ドラは `0m` / `0p` / `0s`
- 字牌は 1z=東, 2z=南, 3z=西, 4z=北, 5z=白, 6z=發, 7z=中
- ルールは雀魂「金の間」準拠。判断に迷う点は `Ruleset` の値を見る。
  そこに無い挙動は仕様書を見る。どちらにも無ければ報告する
```

- [ ] **Step 2: 規約に書いた検証コマンドが実際に通ることを確認する**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: 3つとも成功

- [ ] **Step 3: コミット**

```bash
git add AGENTS.md
git commit -m "docs: エージェント作業規約を追加"
```

---

## Wave 0 完了の判定

以下がすべて満たされたとき Wave 0 は完了とし、Wave 1 の4本を並行で開始できる。

- [ ] `cargo test --workspace` が通る
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] `pnpm --filter @real-mahjong/web typecheck` が通る
- [ ] `apps/web/src/protocol/` に `ClientEvent.ts` があり、`Event.ts` が**無い**
- [ ] `cargo tree --package mahjong-wasm --depth 1` に `mahjong-engine` が現れない
- [ ] `fixtures/scoring` が10件以上、`fixtures/shanten` が10件以上読める
- [ ] 同一手牌のケース同士で、手牌由来の役の集合が一致している
- [ ] `Ryuukyoku` の射影がノーテン者の手牌を落とす（負例テストが通る）
- [ ] `AGENTS.md` が存在し、ファイル所有権の表を含む

## 次の計画

Wave 0 が完了したら、以下の4つを別々の計画書として書き、並行して実行する。

| 計画 | 範囲 | 所有ファイル | 想定エージェント |
|---|---|---|---|
| Wave 1a | 判定系（向聴数・待ち・鳴き可否・振聴） | `shanten/*` `wait.rs` `furiten.rs` `callable.rs` | Codex |
| Wave 1b | 点数系（和了形分解・役判定・符・点数） | `decompose.rs` `yaku_check/*` `fu.rs` `score.rs` | Codex |
| Wave 1c | 3D卓と牌の描画 | `apps/web/src/scene/` | Claude |
| Wave 1d | 演出タイムライン骨格 | `apps/web/src/timeline/` | Claude |

1a と 1b は Task 12 の共有語彙（`hand.rs` / `shapes.rs`）の上で書くため、
互いの実装完了を待たずに着手できる。1b は `WaitShape` を直接組み立てて
符計算のテストを書けばよく、1a の `wait.rs` が未完成でも進められる。

1c と 1d はどちらも `apps/web` を触るため、ディレクトリで分ける。
`apps/web/src/main.ts` の結線は Wave 1 完了後にコーディネータが行う。
