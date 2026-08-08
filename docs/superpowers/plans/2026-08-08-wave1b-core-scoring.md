# Wave 1b: mahjong-core 点数系 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 和了形の分解・役判定・符計算・点数計算を `mahjong-core` に実装し、`fixtures/scoring` の期待値をすべて通す。

**Architecture:** すべて純粋関数。Wave 0 で凍結した `HandCounts`（`hand.rs`）と `Block` / `WaitShape` / `Decomposition`（`shapes.rs`）の上に書く。同じ手が複数通りに分解できる場合は**すべての分解を評価して最も高い点数を採る**。

**Tech Stack:** Rust 1.97.1 / edition 2021 / `protocol` と `mahjong-core` のみに依存

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
**作業規約:** `AGENTS.md`

## Global Constraints

- 牌の記法は `123m456p789s1234567z`、赤ドラは `0m` / `0p` / `0s`、字牌は 1z=東〜7z=中
- **編集してよいファイルは以下のみ**
  - `crates/mahjong-core/src/decompose.rs`
  - `crates/mahjong-core/src/yaku_check/{standard,yakuman}.rs`
  - `crates/mahjong-core/src/fu.rs`
  - `crates/mahjong-core/src/score.rs`
- **`lib.rs` / `mod.rs` / `hand.rs` / `shapes.rs` は編集しない。** 必要になったらコーディネータへ報告する
- **`shanten/` / `wait.rs` / `furiten.rs` / `callable.rs` を編集しない**（Wave 1a の所有）。
  それらを**参照もしない**。この計画は Wave 1a の成果物に一切依存しないため、
  どちらが先に終わっても構わない
- **待ち形（両面・嵌張・辺張など）の判定はこの計画が持つ。** 待ち形は手をどう
  分解したかに依存するため、分解を持つ `decompose.rs` の中で決める
- `fixtures/` の既存の期待値を変更しない。誤っていると確信したら根拠（符の内訳）を添えて報告する
- 完了条件は `cargo test --workspace` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` がすべて通ること

## 型の置き場

Rust は同一クレート内でモジュールを相互に参照できる。迷わないよう置き場を先に決める。

| 型 | 置き場 |
|---|---|
| `WinForm` | `decompose.rs` |
| `WinType` / `HandContext` / `Payment` / `ScoreResult` | `score.rs` |

`yaku_check::*` と `fu` は `crate::score::HandContext` を参照する。

## ルール表（雀魂 金の間準拠）

実装はこの表に従う。値は `fixtures/scoring` の10ケースと突き合わせて検算済みである。

### 符

| 要素 | 符 |
|---|---|
| 副底 | 20 |
| 門前ロン | +10 |
| ツモ | +2（平和形では付けない） |
| 順子 | 0 |
| 明刻（中張／幺九） | 2 / 4 |
| 暗刻（中張／幺九） | 4 / 8 |
| 明槓（中張／幺九） | 8 / 16 |
| 暗槓（中張／幺九） | 16 / 32 |
| 雀頭が役牌（場風・自風・三元牌） | +2 |
| 待ちが嵌張・辺張・単騎 | +2（両面・双碰は0） |

**連風牌の雀頭は 2符**とする（ダブ東の雀頭も2符）。流派により4符とするものがあるが、本仕様では2符で確定する。

最後に**10符単位へ切り上げる**。

特例が3つある。

- 平和ツモ … 20符固定（ツモの+2も付けない）
- 七対子 … 25符固定（切り上げもしない）
- 副露していて符が一切付かない形（喰い平和） … 30符とする

### 点数

```
基本点 = 符 × 2^(2 + 翻)
```

翻数による上限を次のとおり適用する。**切り上げ満貫は無し**なので、4翻以下は
基本点が2000を超えたときだけ満貫になる（30符4翻＝1920 は満貫にならず 7700）。

| 翻 | 基本点 |
|---|---|
| 13以上（役満） | 8000 ×（役満の重なり数） |
| 11〜12（三倍満） | 6000 |
| 8〜10（倍満） | 4000 |
| 6〜7（跳満） | 3000 |
| 5（満貫） | 2000 |
| 4以下 | `min(符 × 2^(2+翻), 2000)` |

支払いは次のとおりで、**各支払いを個別に100点単位へ切り上げる**。

| 和了 | 支払い |
|---|---|
| 子のロン | 放銃者が `基本点 × 4` |
| 親のロン | 放銃者が `基本点 × 6` |
| 子のツモ | 親が `基本点 × 2`、子が各 `基本点` |
| 親のツモ | 子が各 `基本点 × 2` |

---

### Task 1: 和了形の分解

役判定・符計算のすべてがこの出力を入力にする。ここが最初の土台になる。

**Files:**
- Modify: `crates/mahjong-core/src/decompose.rs`

**Interfaces:**
- Consumes: `protocol::tile::{Tile, TileKind}`、`protocol::meld::Meld`、`crate::hand::HandCounts`、`crate::shapes::{Block, Decomposition, WaitShape}`
- Produces:
  - `pub enum WinForm { Standard(Decomposition), Chiitoitsu { pairs: Vec<TileKind> }, Kokushi { pair: TileKind, thirteen_wait: bool } }`
  - `pub fn decompose(hand: &[Tile], melds: &[Meld], win_tile: Tile) -> Vec<WinForm>`
  - `pub fn wait_shape_of(blocks: &[Block], pair: TileKind, win_tile: TileKind) -> WaitShape`

`hand` は**和了牌を含まない**手の内の牌。`decompose` は内部で和了牌を加えてから分解する。
和了形でなければ空を返す。同じ手が複数通りに分解できる場合はすべて返し、
どれを採るかは `score.rs` が決める（点数が最大になるものを選ぶ）。

`test-fixtures` は dev-dependency として**既に追加済み**である（Wave 1a と共有する
`Cargo.toml` の衝突を避けるため、コーディネータが先に入れた）。**`Cargo.toml` を編集しないこと。**

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::{parse_hand, parse_tile};

    fn forms(concealed: &str, win: &str) -> Vec<WinForm> {
        decompose(
            &parse_hand(concealed).unwrap(),
            &[],
            parse_tile(win).unwrap(),
        )
    }

    #[test]
    fn a_standard_hand_decomposes_into_four_sets_and_a_pair() {
        let found = forms("234567m23478p22s", "6p");
        assert_eq!(found.len(), 1);
        let WinForm::Standard(d) = &found[0] else {
            panic!("標準形ではない");
        };
        assert_eq!(d.blocks.len(), 4);
        assert_eq!(d.pair, parse_tile("2s").unwrap().kind());
        assert_eq!(d.wait, WaitShape::Ryanmen);
    }

    #[test]
    fn seven_pairs_is_recognised() {
        let found = forms("1122m3344p5566s7z", "7z");
        assert!(found
            .iter()
            .any(|f| matches!(f, WinForm::Chiitoitsu { pairs } if pairs.len() == 7)));
    }

    #[test]
    fn kokushi_tanki_is_recognised_and_not_a_thirteen_wait() {
        let found = forms("119m19p19s123456z", "7z");
        let kokushi = found
            .iter()
            .find_map(|f| match f {
                WinForm::Kokushi {
                    pair,
                    thirteen_wait,
                } => Some((*pair, *thirteen_wait)),
                _ => None,
            })
            .expect("国士が見つからない");
        assert_eq!(kokushi.0, parse_tile("1m").unwrap().kind());
        assert!(!kokushi.1, "単騎待ちなので十三面ではない");
    }

    #[test]
    fn kokushi_thirteen_wait_is_flagged() {
        let found = forms("19m19p19s1234567z", "1m");
        let thirteen = found.iter().any(
            |f| matches!(f, WinForm::Kokushi { thirteen_wait, .. } if *thirteen_wait),
        );
        assert!(thirteen, "十三面待ちとして検出されるべき");
    }

    /// 同じ手が複数通りに分解できる場合は全部返す。
    /// 111222333m は「111 222 333」とも「123 123 123」とも読める。
    #[test]
    fn ambiguous_hands_yield_every_decomposition() {
        let found = forms("111222333m456p4s", "4s");
        let standards: Vec<_> = found
            .iter()
            .filter_map(|f| match f {
                WinForm::Standard(d) => Some(d),
                _ => None,
            })
            .collect();
        assert!(
            standards.len() >= 2,
            "複数の分解が返るべき。実際は {}",
            standards.len()
        );
    }

    #[test]
    fn a_hand_that_is_not_complete_yields_nothing() {
        assert!(forms("147m258p369s1234z", "1m").is_empty());
    }

    #[test]
    fn identifies_each_wait_shape() {
        let kind = |n: &str| parse_tile(n).unwrap().kind();
        let run = |n: &str| Block::Run(kind(n));

        // 678p の 6p → 残る 78p は両面
        assert_eq!(
            wait_shape_of(&[run("6p")], kind("2s"), kind("6p")),
            WaitShape::Ryanmen
        );
        // 123m の 3m → 残る 12m は辺張
        assert_eq!(
            wait_shape_of(&[run("1m")], kind("5s"), kind("3m")),
            WaitShape::Penchan
        );
        // 789m の 7m → 残る 89m は両面（辺張ではない）
        assert_eq!(
            wait_shape_of(&[run("7m")], kind("5s"), kind("7m")),
            WaitShape::Ryanmen
        );
        // 123p の 2p → 残る 13p は嵌張
        assert_eq!(
            wait_shape_of(&[run("1p")], kind("5s"), kind("2p")),
            WaitShape::Kanchan
        );
        // 和了牌が刻子の一部 → シャンポン
        assert_eq!(
            wait_shape_of(&[Block::Triplet(kind("1p"))], kind("5s"), kind("1p")),
            WaitShape::Shanpon
        );
        // 和了牌が雀頭 → 単騎
        assert_eq!(
            wait_shape_of(&[run("1p")], kind("5s"), kind("5s")),
            WaitShape::Tanki
        );
    }

    #[test]
    fn melds_are_carried_into_the_decomposition() {
        use protocol::meld::{Meld, MeldKind};
        use protocol::seat::Seat;

        let melds = vec![
            Meld {
                kind: MeldKind::Pon,
                tiles: parse_hand("222m").unwrap(),
                from: Some(Seat::new(2)),
                called_tile: Some(parse_tile("2m").unwrap()),
            },
            Meld {
                kind: MeldKind::Chi,
                tiles: parse_hand("345p").unwrap(),
                from: Some(Seat::new(3)),
                called_tile: Some(parse_tile("3p").unwrap()),
            },
        ];
        let found = decompose(
            &parse_hand("88p34678s").unwrap(),
            &melds,
            parse_tile("5s").unwrap(),
        );
        let WinForm::Standard(d) = &found[0] else {
            panic!("標準形ではない");
        };
        assert_eq!(d.melds.len(), 2);
        // 手の内で作った面子は2つ（345s と 678s）、雀頭は 8p
        assert_eq!(d.blocks.len(), 2);
        assert_eq!(d.pair, parse_tile("8p").unwrap().kind());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core decompose`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
//! 和了形の分解。役判定と符計算はすべてこの出力を入力にする。
//!
//! 同じ手が複数通りに分解できる場合はすべて返す。どれを採るかは
//! 点数が最大になるものを選ぶ `score.rs` の責務である。

use protocol::meld::Meld;
use protocol::tile::{Tile, TileKind};

use crate::hand::HandCounts;
use crate::shapes::{Block, Decomposition, WaitShape};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WinForm {
    Standard(Decomposition),
    Chiitoitsu { pairs: Vec<TileKind> },
    Kokushi { pair: TileKind, thirteen_wait: bool },
}

/// 和了牌を含まない手の内の牌から、成立している和了形をすべて挙げる。
/// 和了形でなければ空を返す。
pub fn decompose(hand: &[Tile], melds: &[Meld], win_tile: Tile) -> Vec<WinForm> {
    let mut counts = HandCounts::from_tiles(hand);
    counts.add(win_tile.kind());

    let mut out = Vec::new();

    // 標準形。副露していても成立する。
    for (blocks, pair) in standard_decompositions(&counts, melds.len()) {
        let wait = wait_shape_of(&blocks, pair, win_tile.kind());
        out.push(WinForm::Standard(Decomposition {
            blocks,
            pair,
            melds: melds.to_vec(),
            wait,
        }));
    }

    // 七対子と国士は門前のみ。
    if melds.is_empty() {
        if let Some(pairs) = chiitoitsu_pairs(&counts) {
            out.push(WinForm::Chiitoitsu { pairs });
        }
        if let Some((pair, thirteen_wait)) = kokushi_shape(&counts, win_tile.kind()) {
            out.push(WinForm::Kokushi {
                pair,
                thirteen_wait,
            });
        }
    }

    out
}

/// 4面子1雀頭に分ける全通り。副露分は面子数に数える。
fn standard_decompositions(
    counts: &HandCounts,
    called: usize,
) -> Vec<(Vec<Block>, TileKind)> {
    let needed_sets = 4 - called;
    let mut out = Vec::new();

    for index in 0..TileKind::COUNT as u8 {
        let pair_kind = TileKind::from_index(index).expect("範囲内");
        if counts.get(pair_kind) < 2 {
            continue;
        }
        let mut work = *counts;
        work.remove(pair_kind);
        work.remove(pair_kind);

        let mut blocks = Vec::new();
        collect_sets(&mut work, 0, needed_sets, &mut blocks, &mut |found| {
            out.push((found.to_vec(), pair_kind));
        });
    }

    out.sort();
    out.dedup();
    out
}

/// `remaining` 個の面子をちょうど取り出す全通りを列挙する。
fn collect_sets(
    counts: &mut HandCounts,
    index: u8,
    remaining: usize,
    blocks: &mut Vec<Block>,
    found: &mut impl FnMut(&[Block]),
) {
    if remaining == 0 {
        if counts.total() == 0 {
            found(blocks);
        }
        return;
    }
    if index >= TileKind::COUNT as u8 {
        return;
    }

    let kind = TileKind::from_index(index).expect("範囲内");
    if counts.get(kind) == 0 {
        collect_sets(counts, index + 1, remaining, blocks, found);
        return;
    }

    // 刻子
    if counts.get(kind) >= 3 {
        for _ in 0..3 {
            counts.remove(kind);
        }
        blocks.push(Block::Triplet(kind));
        collect_sets(counts, index, remaining - 1, blocks, found);
        blocks.pop();
        for _ in 0..3 {
            counts.add(kind);
        }
    }

    // 順子
    if let Some(run) = run_from(kind) {
        if run.iter().all(|k| counts.get(*k) >= 1) {
            for k in &run {
                counts.remove(*k);
            }
            blocks.push(Block::Run(kind));
            collect_sets(counts, index, remaining - 1, blocks, found);
            blocks.pop();
            for k in &run {
                counts.add(*k);
            }
        }
    }
}

fn chiitoitsu_pairs(counts: &HandCounts) -> Option<Vec<TileKind>> {
    let mut pairs = Vec::new();
    for (kind, count) in counts.kinds() {
        if count != 2 {
            return None;
        }
        pairs.push(kind);
    }
    (pairs.len() == 7).then_some(pairs)
}

/// 国士無双の形。対子になっている幺九牌と、十三面待ちだったかを返す。
fn kokushi_shape(counts: &HandCounts, win_kind: TileKind) -> Option<(TileKind, bool)> {
    let mut pair = None;
    let mut kinds = 0;
    for (kind, count) in counts.kinds() {
        if !kind.is_terminal_or_honor() {
            return None;
        }
        kinds += 1;
        match count {
            1 => {}
            2 if pair.is_none() => pair = Some(kind),
            _ => return None,
        }
    }
    if kinds != 13 {
        return None;
    }
    let pair = pair?;
    // 和了牌が対子を作ったなら、和了前は13種すべてを1枚ずつ持っていた
    // ことになる（＝十三面待ち）。対子が既にあったなら、欠けていた1種を
    // 待っていた単騎である。
    Some((pair, pair == win_kind))
}

fn run_from(kind: TileKind) -> Option<[TileKind; 3]> {
    let number = kind.number()?;
    if number > 7 {
        return None;
    }
    Some([
        kind,
        TileKind::from_index(kind.index() + 1)?,
        TileKind::from_index(kind.index() + 2)?,
    ])
}

/// その分解のもとで、和了牌がどの役割だったかを答える。
///
/// 待ち形は「どう分解したか」に依存する。同じ手でも分解が違えば待ち形が
/// 変わりうるため、分解と同じ場所に置く。
///
/// 判定の順序に意味がある。雀頭と一致するなら単騎、刻子の一部なら双碰、
/// 順子の一部なら位置で両面／嵌張／辺張を分ける。
pub fn wait_shape_of(blocks: &[Block], pair: TileKind, win_tile: TileKind) -> WaitShape {
    if win_tile == pair {
        return WaitShape::Tanki;
    }

    for block in blocks {
        match *block {
            Block::Triplet(kind) | Block::Pair(kind) if kind == win_tile => {
                return WaitShape::Shanpon;
            }
            Block::Run(start) => {
                let (Some(number), Some(win_number)) = (start.number(), win_tile.number()) else {
                    continue;
                };
                if start.suit() != win_tile.suit() {
                    continue;
                }
                // 順子は start, start+1, start+2。
                match win_number.checked_sub(number) {
                    // 和了牌が真ん中 → 嵌張
                    Some(1) => return WaitShape::Kanchan,
                    // 和了牌が最小。789 の 7 も 123 の 1 も、残り2枚は連続しており両面。
                    Some(0) => return WaitShape::Ryanmen,
                    // 和了牌が最大。123 の 3 は 12 に対する辺張。それ以外は両面。
                    Some(2) => {
                        return if number == 1 {
                            WaitShape::Penchan
                        } else {
                            WaitShape::Ryanmen
                        };
                    }
                    _ => continue,
                }
            }
            _ => continue,
        }
    }

    // ここへ来るのは分解と和了牌が食い違っている場合。呼び出し側のバグ。
    WaitShape::Tanki
}
```

`Block` に `Ord` が無くて `sort` が通らない場合は、`blocks` を
`Vec<u8>`（`Block` を種類と番号で符号化したもの）に直してから重複を除く。
`shapes.rs` は編集しないこと。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-core decompose`
Expected: 7テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-core
git commit -m "feat(core): 和了形の分解を実装"
```

---

### Task 2: 役満の判定

通常役より先に実装する。役満が成立していれば通常役を数える必要がなく、
点数計算の分岐が単純になるためである。

**Files:**
- Modify: `crates/mahjong-core/src/yaku_check/yakuman.rs`
- Modify: `crates/mahjong-core/src/score.rs`（`HandContext` / `WinType` の定義のみ）

**Interfaces:**
- Consumes: `WinForm`、`crate::score::HandContext`
- Produces: `pub fn detect(form: &WinForm, context: &HandContext) -> Vec<YakuId>`

**判定表:**

| 役 | 条件 |
|---|---|
| `KokushiMusou` | `WinForm::Kokushi` かつ `thirteen_wait == false` |
| `KokushiMusou13` | `WinForm::Kokushi` かつ `thirteen_wait == true` |
| `Suuankou` | 暗刻4つ。ロンで和了牌が刻子の一部になった場合、その刻子は明刻扱い |
| `SuuankouTanki` | 暗刻4つ かつ 待ちが単騎 |
| `Daisangen` | 白・發・中がすべて刻子または槓子 |
| `Shousuushii` | 風牌のうち3種が刻子、残り1種が雀頭 |
| `Daisuushii` | 風牌4種すべてが刻子または槓子 |
| `Tsuuiisou` | 全ての牌が字牌 |
| `Ryuuiisou` | 全ての牌が 2s/3s/4s/6s/8s/發 のいずれか |
| `Chinroutou` | 全ての牌が 1 か 9 の数牌 |
| `ChuurenPoutou` | 門前・清一色で `1112345678999` ＋ 任意の1枚。和了牌が9面待ちでない |
| `ChuurenPoutou9` | 同上で、和了前の形がちょうど `1112345678999` |
| `Suukantsu` | 槓子4つ |
| `Tenhou` | 親の配牌和了（`context.tenhou`） |
| `Chiihou` | 子の第一ツモ和了（`context.chiihou`） |

- [ ] **Step 1: score.rs に文脈の型を置く**

役判定と符計算の双方がこれを参照する。中身の計算は Task 5 で書く。

```rust
//! 点数計算。符と翻から支払いを決める。
//!
//! `WinType` / `HandContext` / `Payment` / `ScoreResult` はここで定義し、
//! `yaku_check` と `fu` から参照する。

use protocol::seat::Wind;
use protocol::tile::Tile;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinType {
    Tsumo,
    Ron,
}

/// 役と符の判定に必要な、手牌の外側の情報。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HandContext {
    pub win_type: WinType,
    pub seat_wind: Wind,
    pub round_wind: Wind,
    pub riichi: bool,
    pub double_riichi: bool,
    pub ippatsu: bool,
    pub rinshan: bool,
    pub chankan: bool,
    pub haitei: bool,
    pub houtei: bool,
    pub tenhou: bool,
    pub chiihou: bool,
    pub dora_indicators: Vec<Tile>,
    pub ura_indicators: Vec<Tile>,
}

impl HandContext {
    /// 立直していない・一発でない等、状況役がすべて無い文脈。テストの土台に使う。
    pub fn plain(win_type: WinType, seat_wind: Wind, round_wind: Wind) -> Self {
        HandContext {
            win_type,
            seat_wind,
            round_wind,
            riichi: false,
            double_riichi: false,
            ippatsu: false,
            rinshan: false,
            chankan: false,
            haitei: false,
            houtei: false,
            tenhou: false,
            chiihou: false,
            dora_indicators: Vec::new(),
            ura_indicators: Vec::new(),
        }
    }
}
```

- [ ] **Step 2: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::decompose::decompose;
    use crate::score::{HandContext, WinType};
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::seat::Wind;

    fn detect_for(concealed: &str, win: &str, win_type: WinType) -> Vec<YakuId> {
        let forms = decompose(
            &parse_hand(concealed).unwrap(),
            &[],
            parse_tile(win).unwrap(),
        );
        let context = HandContext::plain(win_type, Wind::South, Wind::East);
        forms
            .iter()
            .flat_map(|f| detect(f, &context))
            .collect()
    }

    #[test]
    fn kokushi_tanki_is_single_yakuman() {
        let found = detect_for("119m19p19s123456z", "7z", WinType::Ron);
        assert!(found.contains(&YakuId::KokushiMusou));
        assert!(!found.contains(&YakuId::KokushiMusou13));
    }

    #[test]
    fn kokushi_thirteen_wait_is_double_yakuman() {
        let found = detect_for("19m19p19s1234567z", "1m", WinType::Ron);
        assert!(found.contains(&YakuId::KokushiMusou13));
    }

    #[test]
    fn daisangen_needs_all_three_dragons() {
        let found = detect_for("555z666z777z123m1p", "1p", WinType::Ron);
        assert!(found.contains(&YakuId::Daisangen));
    }

    #[test]
    fn tsuuiisou_needs_every_tile_to_be_an_honor() {
        let found = detect_for("111z222z333z444z5z", "5z", WinType::Ron);
        assert!(found.contains(&YakuId::Tsuuiisou));
    }

    #[test]
    fn ryuuiisou_accepts_only_green_tiles() {
        let found = detect_for("234s234s666s888s6z", "6z", WinType::Ron);
        assert!(found.contains(&YakuId::Ryuuiisou));

        // 5s は緑一色に使えない
        let not_green = detect_for("345s345s666s888s6z", "6z", WinType::Ron);
        assert!(!not_green.contains(&YakuId::Ryuuiisou));
    }

    #[test]
    fn suuankou_requires_four_concealed_triplets() {
        // ツモなら4暗刻
        let found = detect_for("111m222m333m444m5p", "5p", WinType::Tsumo);
        assert!(found.contains(&YakuId::Suuankou));
        assert!(found.contains(&YakuId::SuuankouTanki));
    }

    /// ロンで和了牌が刻子を作った場合、その刻子は暗刻にならない。
    #[test]
    fn ron_completing_a_triplet_breaks_suuankou() {
        let found = detect_for("111m222m333m44m55p", "4m", WinType::Ron);
        assert!(!found.contains(&YakuId::Suuankou));
    }

    #[test]
    fn a_normal_hand_has_no_yakuman() {
        let found = detect_for("234567m23478p22s", "6p", WinType::Tsumo);
        assert!(found.is_empty());
    }
}
```

- [ ] **Step 3: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core yakuman`
Expected: コンパイルエラー

- [ ] **Step 4: 実装を書く**

判定表のとおりに素直に書く。役満は成立条件が明確なので、凝った抽象化を入れず
1役1関数で並べたほうが読みやすく、間違いも見つけやすい。

暗刻の数え方だけ注意する。**ロンで和了牌が刻子を完成させた場合、その刻子は
明刻として数える。**四暗刻とシャンポン待ちのロンを取り違えるのはよくある誤りである。

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test --package mahjong-core yakuman`
Expected: 8テスト PASS

- [ ] **Step 6: コミット**

```bash
git add crates/mahjong-core
git commit -m "feat(core): 役満の判定を実装"
```

---

### Task 3: 通常役の判定

**Files:**
- Modify: `crates/mahjong-core/src/yaku_check/standard.rs`

**Interfaces:**
- Consumes: `WinForm`、`HandContext`
- Produces: `pub fn detect(form: &WinForm, context: &HandContext) -> Vec<(YakuId, u8)>`

翻数は**門前とそれ以外で変わる**役がある。返り値に翻数を含めるのはそのためである。

**判定表:**

| 役 | 翻（門前／副露） | 条件 |
|---|---|---|
| `MenzenTsumo` | 1／− | 門前 かつ ツモ |
| `Riichi` | 1／− | `context.riichi` |
| `DoubleRiichi` | 2／− | `context.double_riichi`（`Riichi` とは重複させない） |
| `Ippatsu` | 1／− | `context.ippatsu` |
| `Chankan` | 1／1 | `context.chankan` |
| `RinshanKaihou` | 1／1 | `context.rinshan` |
| `HaiteiRaoyue` | 1／1 | `context.haitei` かつ ツモ |
| `HouteiRaoyui` | 1／1 | `context.houtei` かつ ロン |
| `Pinfu` | 1／− | 門前・全て順子・雀頭が役牌でない・待ちが両面 |
| `Tanyao` | 1／1 | 全ての牌が 2〜8 の数牌 |
| `Iipeiko` | 1／− | 同じ順子が2組 |
| `Ryanpeikou` | 3／− | 同じ順子が2組×2種（`Iipeiko` とは重複させない） |
| `YakuhaiHaku` / `Hatsu` / `Chun` | 1／1 | 該当の三元牌が刻子または槓子 |
| `YakuhaiRoundWind` | 1／1 | 場風の刻子または槓子 |
| `YakuhaiSeatWind` | 1／1 | 自風の刻子または槓子 |
| `SanshokuDoujun` | 2／1 | 同じ番号の順子が3色 |
| `Ittsu` | 2／1 | 同色で 123・456・789 の3順子 |
| `Chanta` | 2／1 | 全ての面子と雀頭が幺九牌を含み、字牌を含む |
| `Junchan` | 3／2 | 全ての面子と雀頭が 1 か 9 を含み、字牌を含まない |
| `Honroutou` | 2／2 | 全ての牌が幺九牌（順子が無い） |
| `Toitoi` | 2／2 | 全ての面子が刻子または槓子 |
| `Sanankou` | 2／2 | 暗刻3つ（ロンの扱いは役満と同じ） |
| `SanshokuDoukou` | 2／2 | 同じ番号の刻子が3色 |
| `Sankantsu` | 2／2 | 槓子3つ |
| `Shousangen` | 2／2 | 三元牌のうち2種が刻子、1種が雀頭 |
| `Honitsu` | 3／2 | 1色の数牌＋字牌のみ |
| `Chinitsu` | 6／5 | 1色の数牌のみ |
| `Chiitoitsu` | 2／− | `WinForm::Chiitoitsu` |

**重複の除外**を忘れないこと。`Ryanpeikou` が成立したら `Iipeiko` は付けない。
`Junchan` が成立したら `Chanta` は付けない。`Chinitsu` が成立したら `Honitsu` は付けない。
`DoubleRiichi` が成立したら `Riichi` は付けない。

- [ ] **Step 1: 失敗するテストを書く**

期待値テーブルに現れる役を最優先で押さえる。特に**平和と断幺九の複合**は、
Codex のレビューで実際に取りこぼしが見つかった箇所である。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::decompose::decompose;
    use crate::score::{HandContext, WinType};
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::seat::Wind;

    fn detect_best(concealed: &str, win: &str, context: &HandContext) -> Vec<(YakuId, u8)> {
        let forms = decompose(
            &parse_hand(concealed).unwrap(),
            &[],
            parse_tile(win).unwrap(),
        );
        forms
            .iter()
            .map(|f| detect(f, context))
            .max_by_key(|list| list.iter().map(|(_, han)| *han as u32).sum::<u32>())
            .unwrap_or_default()
    }

    fn plain_tsumo() -> HandContext {
        HandContext::plain(WinType::Tsumo, Wind::South, Wind::East)
    }

    /// 全牌が中張牌の平和形は、平和と断幺九が両方成立する。
    /// 片方だけ検出する誤りが実際に起きたため、明示的に固定する。
    #[test]
    fn pinfu_and_tanyao_both_apply_to_an_all_simples_run_hand() {
        let found = detect_best("234567m23478p22s", "6p", &plain_tsumo());
        assert!(found.iter().any(|(y, _)| *y == YakuId::Pinfu));
        assert!(found.iter().any(|(y, _)| *y == YakuId::Tanyao));
        assert!(found.iter().any(|(y, _)| *y == YakuId::MenzenTsumo));
    }

    /// 辺張待ちでは平和が成立しない。
    #[test]
    fn penchan_wait_breaks_pinfu() {
        let found = detect_best("12456m234678p55s", "3m", &plain_tsumo());
        assert!(!found.iter().any(|(y, _)| *y == YakuId::Pinfu));
    }

    /// 1mを含むので断幺九は成立しない。
    #[test]
    fn a_terminal_breaks_tanyao() {
        let found = detect_best("12456m234678p55s", "3m", &plain_tsumo());
        assert!(!found.iter().any(|(y, _)| *y == YakuId::Tanyao));
    }

    /// 役牌の雀頭では平和にならない。
    #[test]
    fn a_yakuhai_pair_breaks_pinfu() {
        let found = detect_best("234567m23478p55z", "6p", &plain_tsumo());
        assert!(!found.iter().any(|(y, _)| *y == YakuId::Pinfu));
    }

    #[test]
    fn chiitoitsu_is_two_han() {
        let context = HandContext::plain(WinType::Tsumo, Wind::South, Wind::East);
        let found = detect_best("1122m3344p5566s7z", "7z", &context);
        assert!(found.contains(&(YakuId::Chiitoitsu, 2)));
    }

    #[test]
    fn ittsu_needs_all_three_runs_in_one_suit() {
        let found = detect_best("123456789m22p34s", "5s", &plain_tsumo());
        assert!(found.iter().any(|(y, _)| *y == YakuId::Ittsu));
    }

    #[test]
    fn sanshoku_needs_the_same_numbers_in_three_suits() {
        let found = detect_best("234m234p234s56m11z", "7m", &plain_tsumo());
        assert!(found.iter().any(|(y, _)| *y == YakuId::SanshokuDoujun));
    }

    #[test]
    fn ryanpeikou_supersedes_iipeiko() {
        let found = detect_best("112233m44556p11s", "6p", &plain_tsumo());
        assert!(found.iter().any(|(y, _)| *y == YakuId::Ryanpeikou));
        assert!(
            !found.iter().any(|(y, _)| *y == YakuId::Iipeiko),
            "二盃口が成立したら一盃口は付けない"
        );
    }

    #[test]
    fn junchan_supersedes_chanta() {
        let found = detect_best("123m789m123p789p1s", "1s", &plain_tsumo());
        assert!(found.iter().any(|(y, _)| *y == YakuId::Junchan));
        assert!(!found.iter().any(|(y, _)| *y == YakuId::Chanta));
    }

    #[test]
    fn chinitsu_supersedes_honitsu() {
        let found = detect_best("1112234567899m", "9m", &plain_tsumo());
        assert!(found.iter().any(|(y, _)| *y == YakuId::Chinitsu));
        assert!(!found.iter().any(|(y, _)| *y == YakuId::Honitsu));
    }

    #[test]
    fn seat_and_round_wind_stack_when_they_match() {
        let context = HandContext::plain(WinType::Tsumo, Wind::East, Wind::East);
        let found = detect_best("111z234m567m234p1s", "1s", &context);
        assert!(found.iter().any(|(y, _)| *y == YakuId::YakuhaiRoundWind));
        assert!(found.iter().any(|(y, _)| *y == YakuId::YakuhaiSeatWind));
    }

    #[test]
    fn double_riichi_supersedes_riichi() {
        let mut context = plain_tsumo();
        context.riichi = true;
        context.double_riichi = true;
        let found = detect_best("234567m23478p22s", "6p", &context);
        assert!(found.iter().any(|(y, _)| *y == YakuId::DoubleRiichi));
        assert!(!found.iter().any(|(y, _)| *y == YakuId::Riichi));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core yaku_check::standard`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

判定表のとおり、1役1関数で並べる。`detect` はそれらを順に呼び、
最後に重複の除外をかける。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-core yaku_check::standard`
Expected: 12テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-core
git commit -m "feat(core): 通常役の判定を実装"
```

---

### Task 4: 符計算

**Files:**
- Modify: `crates/mahjong-core/src/fu.rs`

**Interfaces:**
- Consumes: `WinForm`、`HandContext`、`crate::shapes::WaitShape`
- Produces: `pub fn fu_of(form: &WinForm, context: &HandContext, has_pinfu: bool) -> u8`

`has_pinfu` を引数で受け取るのは、平和ツモ20符固定の判定に役の成立可否が要るためである。
役判定を fu の中で再実装しない。

**待ち形の判定は `wait::wait_shape_of` の結果（`Decomposition.wait`）を使う。ここで再実装しない。**

- [ ] **Step 1: 失敗するテストを書く**

期待値テーブルの符をそのまま検査する。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::decompose::decompose;
    use crate::score::{HandContext, WinType};
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::seat::Wind;

    fn fu_for(concealed: &str, win: &str, context: &HandContext, pinfu: bool) -> u8 {
        let forms = decompose(
            &parse_hand(concealed).unwrap(),
            &[],
            parse_tile(win).unwrap(),
        );
        forms
            .iter()
            .map(|f| fu_of(f, context, pinfu))
            .max()
            .expect("和了形が無い")
    }

    #[test]
    fn pinfu_tsumo_is_exactly_twenty() {
        let context = HandContext::plain(WinType::Tsumo, Wind::South, Wind::East);
        assert_eq!(fu_for("234567m23478p22s", "6p", &context, true), 20);
    }

    #[test]
    fn menzen_ron_adds_ten() {
        let context = HandContext::plain(WinType::Ron, Wind::South, Wind::East);
        // 副底20＋門前ロン10＝30（平和形なので他に符は付かない）
        assert_eq!(fu_for("234567m23467p55s", "8p", &context, true), 30);
    }

    #[test]
    fn penchan_wait_adds_two_and_rounds_up() {
        let context = HandContext::plain(WinType::Ron, Wind::South, Wind::East);
        // 20＋門前ロン10＋辺張2＝32→40
        assert_eq!(fu_for("12456m234678p55s", "3m", &context, false), 40);
    }

    /// 幺九暗刻8＋嵌張2＋役牌雀頭2＋ツモ2＋副底20＝34→40
    #[test]
    fn terminal_concealed_triplet_yakuhai_pair_and_tsumo() {
        let context = HandContext::plain(WinType::Tsumo, Wind::South, Wind::East);
        assert_eq!(fu_for("111m456m789p13s55z", "2s", &context, false), 40);
    }

    #[test]
    fn chiitoitsu_is_exactly_twenty_five() {
        let context = HandContext::plain(WinType::Tsumo, Wind::South, Wind::East);
        assert_eq!(fu_for("1122m3344p5566s7z", "7z", &context, false), 25);
    }

    /// 副露していて符が一切付かない形は30符とする。
    #[test]
    fn open_hand_with_no_fu_is_thirty() {
        use protocol::meld::{Meld, MeldKind};
        use protocol::seat::Seat;

        let melds = vec![
            Meld {
                kind: MeldKind::Chi,
                tiles: parse_hand("234p").unwrap(),
                from: Some(Seat::new(3)),
                called_tile: Some(parse_tile("2p").unwrap()),
            },
            Meld {
                kind: MeldKind::Chi,
                tiles: parse_hand("345s").unwrap(),
                from: Some(Seat::new(3)),
                called_tile: Some(parse_tile("3s").unwrap()),
            },
        ];
        let context = HandContext::plain(WinType::Ron, Wind::South, Wind::East);
        let forms = decompose(
            &parse_hand("234m78p33s").unwrap(),
            &melds,
            parse_tile("6p").unwrap(),
        );
        let fu = forms.iter().map(|f| fu_of(f, &context, false)).max().unwrap();
        assert_eq!(fu, 30);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core fu`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

冒頭のルール表をそのままコードに落とす。要点は3つ。

1. 特例（平和ツモ20・七対子25・喰い平和30）を先に処理して早期に返す
2. 面子の符は「明暗」と「中張／幺九」の2軸で決まる。ロンで和了牌が完成させた
   刻子は明刻として数える
3. 最後に10符単位へ切り上げる

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-core fu`
Expected: 6テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-core
git commit -m "feat(core): 符計算を実装"
```

---

### Task 5: 点数計算と期待値テーブルの通し

**Files:**
- Modify: `crates/mahjong-core/src/score.rs`

**Interfaces:**
- Consumes: `decompose`、`yaku_check::{standard, yakuman}`、`fu`
- Produces:
  - `pub enum Payment { Ron { total: i32 }, TsumoDealer { from_each: i32 }, TsumoNonDealer { from_dealer: i32, from_each_non_dealer: i32 } }`
  - `pub struct ScoreResult { pub yaku: Vec<(YakuId, u8)>, pub fu: u8, pub han: u8, pub payment: Payment }`
  - `pub fn score(hand: &[Tile], melds: &[Meld], win_tile: Tile, context: &HandContext, rules: &Ruleset) -> Option<ScoreResult>`

`score` は**すべての分解を評価し、最も点数が高くなるものを採る**。和了形でなければ `None`。

- [ ] **Step 1: 失敗するテストを書く**

`fixtures/scoring` を丸ごと回すテストを含める。これが Wave 1b の合否そのものになる。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::meld::{Meld, MeldKind};
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::ruleset::{MatchLength, Ruleset};
    use protocol::seat::{Seat, Wind};

    fn wind_of(name: &str) -> Wind {
        match name {
            "east" => Wind::East,
            "south" => Wind::South,
            "west" => Wind::West,
            "north" => Wind::North,
            other => panic!("未知の風: {other}"),
        }
    }

    fn meld_of(spec: &test_fixtures::MeldSpec) -> Meld {
        let kind = match spec.kind.as_str() {
            "chi" => MeldKind::Chi,
            "pon" => MeldKind::Pon,
            "ankan" => MeldKind::Ankan,
            "minkan" => MeldKind::Minkan,
            "kakan" => MeldKind::Kakan,
            other => panic!("未知の副露: {other}"),
        };
        Meld {
            kind,
            tiles: parse_hand(&spec.tiles).unwrap(),
            from: Some(Seat::new(spec.from)),
            called_tile: spec.called_tile.as_ref().map(|t| parse_tile(t).unwrap()),
        }
    }

    /// 期待値テーブルの全ケースを通す。これが Wave 1b の合否条件。
    #[test]
    fn matches_every_scoring_fixture() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);

        for case in test_fixtures::load_scoring_cases() {
            let hand = parse_hand(&case.concealed).unwrap();
            let melds: Vec<Meld> = case.melds.iter().map(meld_of).collect();
            let win_tile = parse_tile(&case.win_tile).unwrap();

            let mut context = HandContext::plain(
                match case.win_type {
                    test_fixtures::WinType::Tsumo => WinType::Tsumo,
                    test_fixtures::WinType::Ron => WinType::Ron,
                },
                wind_of(&case.context.seat_wind),
                wind_of(&case.context.round_wind),
            );
            context.riichi = case.context.riichi;
            context.double_riichi = case.context.double_riichi;
            context.ippatsu = case.context.ippatsu;
            context.rinshan = case.context.rinshan;
            context.chankan = case.context.chankan;
            context.haitei = case.context.haitei;
            context.houtei = case.context.houtei;
            context.dora_indicators = case
                .context
                .dora_indicators
                .iter()
                .map(|t| parse_tile(t).unwrap())
                .collect();
            context.ura_indicators = case
                .context
                .ura_indicators
                .iter()
                .map(|t| parse_tile(t).unwrap())
                .collect();

            let result = score(&hand, &melds, win_tile, &context, &rules)
                .unwrap_or_else(|| panic!("{}: 和了形として認識されなかった", case.id));

            assert_eq!(
                result.fu, case.expect.fu,
                "{}: 符が {} だが期待は {}（{}）",
                case.id, result.fu, case.expect.fu, case.note
            );
            assert_eq!(
                result.han, case.expect.han,
                "{}: 翻が {} だが期待は {}（{}）",
                case.id, result.han, case.expect.han, case.note
            );

            let expected_payment = match case.expect.payment {
                test_fixtures::Payment::Ron { total } => Payment::Ron { total },
                test_fixtures::Payment::TsumoDealer { from_each } => {
                    Payment::TsumoDealer { from_each }
                }
                test_fixtures::Payment::TsumoNonDealer {
                    from_dealer,
                    from_each_non_dealer,
                } => Payment::TsumoNonDealer {
                    from_dealer,
                    from_each_non_dealer,
                },
            };
            assert_eq!(
                result.payment, expected_payment,
                "{}: 支払いが食い違う（{}）",
                case.id, case.note
            );

            let mut actual: Vec<_> = result.yaku.iter().map(|(y, h)| (*y, *h)).collect();
            actual.sort();
            let mut expected: Vec<_> =
                case.expect.yaku.iter().map(|y| (y.id, y.han)).collect();
            expected.sort();
            assert_eq!(actual, expected, "{}: 役の一覧が食い違う（{}）", case.id, case.note);
        }
    }

    #[test]
    fn base_points_cap_at_mangan_from_five_han() {
        assert_eq!(base_points(20, 5), 2000);
        assert_eq!(base_points(70, 4), 2000, "4翻でも基本点2000を超えたら満貫");
        assert_eq!(base_points(30, 4), 1920, "切り上げ満貫は無いので7700のまま");
    }

    #[test]
    fn payments_round_up_to_the_hundred() {
        assert_eq!(round_up_to_hundred(1280), 1300);
        assert_eq!(round_up_to_hundred(1300), 1300);
        assert_eq!(round_up_to_hundred(1), 100);
    }

    #[test]
    fn a_hand_that_is_not_complete_scores_nothing() {
        let rules = Ruleset::kin_no_ma(MatchLength::Hanchan);
        let context = HandContext::plain(WinType::Tsumo, Wind::South, Wind::East);
        assert!(score(
            &parse_hand("147m258p369s1234z").unwrap(),
            &[],
            parse_tile("1m").unwrap(),
            &context,
            &rules
        )
        .is_none());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core score`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

冒頭の点数表をそのままコードに落とす。`base_points` と `round_up_to_hundred` は
テストから直接呼ぶため `pub(crate)` 以上にしておく。

ドラの数え方に注意する。**ドラは役ではないので、ドラだけでは和了できない。**
役が1つも無ければ `None` を返す（役無しの和了は成立しない）。
赤ドラは `Tile::is_red()` で数え、`AkaDora` として別に計上する。
裏ドラは `context.riichi` が真のときだけ数える。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-core score`
Expected: 4テスト PASS。`matches_every_scoring_fixture` が10ケースすべてを通すこと

**フィクスチャが通らない場合、まず自分の実装を疑う。**期待値は Codex による
1件ずつの検算を経ている。それでも誤っていると確信したら、符の内訳を添えて
コーディネータへ報告する（自分で書き換えない）。

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-core
git commit -m "feat(core): 点数計算を実装し採点の期待値テーブルを通す"
```

---

### Task 6: 全体の検証

**Files:**
- なし（検証のみ）

- [ ] **Step 1: 全テストを走らせる**

Run: `cargo test --workspace`
Expected: すべて PASS

- [ ] **Step 2: lint を通す**

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

- [ ] **Step 3: 速度を測る**

```bash
cargo test --package mahjong-core --release
```

`decompose` は分解を全通り列挙するため、`shanten` より重い。テスト全体が
数秒を超える場合は報告する。CPU 同士の対局を毎秒数千局回す前提が崩れる。

- [ ] **Step 4: 進捗を報告する**

```bash
orca worktree set --worktree active --comment "Wave 1b 完了。分解・役・符・点数すべて実装、採点10件通過" --workspace-status in-review --json
```

---

## Wave 1b 完了の判定

- [ ] `cargo test --workspace` が通る
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] `fixtures/scoring` の10ケースすべてを `score` が通す（符・翻・支払い・役の一覧すべて）
- [ ] `shanten/` / `wait.rs` / `furiten.rs` / `callable.rs` を編集していない
- [ ] `lib.rs` / `mod.rs` / `hand.rs` / `shapes.rs` を編集していない
- [ ] 役が無い手に `None` を返す（ドラだけでは和了できない）
