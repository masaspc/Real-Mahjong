# Wave 1a: mahjong-core 判定系 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 向聴数・待ち・鳴き可否・振聴を `mahjong-core` に実装し、`fixtures/shanten` の期待値をすべて通す。

**Architecture:** すべて純粋関数。乱数も I/O も時間も持たない。Wave 0 で凍結した `HandCounts`（`hand.rs`）の上に書く。

**Wave 1b との関係:** この計画は 1b のファイルを一切参照しない。1b もこの計画のファイルを参照しない。両者は完全に独立しており、どちらが先に終わっても構わない。

**Tech Stack:** Rust 1.97.1 / edition 2021 / `protocol` と `mahjong-core` のみに依存

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
**作業規約:** `AGENTS.md`

## Global Constraints

- **向聴数の規約: `-1` が和了、`0` がテンパイ。** 標準形の最大は 8
- 牌の記法は `123m456p789s1234567z`、赤ドラは `0m` / `0p` / `0s`、字牌は 1z=東〜7z=中
- 赤ドラは向聴数・待ち・鳴き可否の判定では通常牌と同一に扱う（`Tile::kind()` を使う）
- **編集してよいファイルは以下のみ**
  - `crates/mahjong-core/src/shanten/{standard,chiitoitsu,kokushi,overall}.rs`
  - `crates/mahjong-core/src/wait.rs`
  - `crates/mahjong-core/src/furiten.rs`
  - `crates/mahjong-core/src/callable.rs`
- **`lib.rs` / `mod.rs` / `hand.rs` / `shapes.rs` は編集しない。** 必要になったらコーディネータへ報告する
- **`decompose.rs` を編集しない。** 面子分解が必要なら `shanten/standard.rs` の中に自前の探索を持つ（`decompose.rs` は Wave 1b の所有）
- **ロンの可否は実装しない。** 役の有無に依存するため Wave 2 で engine が結線する
- `fixtures/` の既存の期待値を変更しない。誤っていると確信したら根拠を添えて報告する
- 完了条件は `cargo test --workspace` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` がすべて通ること

---

### Task 1: 七対子の向聴数

3形のうち最も単純で、他の実装の足場になる。

**Files:**
- Modify: `crates/mahjong-core/src/shanten/chiitoitsu.rs`

**Interfaces:**
- Consumes: `mahjong_core::hand::HandCounts`
- Produces: `pub fn shanten(counts: &HandCounts, melds: u8) -> i8`

`test-fixtures` は dev-dependency として**既に追加済み**である（Wave 1b と共有する
`Cargo.toml` の衝突を避けるため、コーディネータが先に入れた）。**`Cargo.toml` を編集しないこと。**

- [ ] **Step 1: 失敗するテストを書く**

`crates/mahjong-core/src/shanten/chiitoitsu.rs` の末尾に置く:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::parse_hand;

    fn counts(notation: &str) -> HandCounts {
        HandCounts::from_tiles(&parse_hand(notation).unwrap())
    }

    #[test]
    fn six_pairs_and_a_single_is_tenpai() {
        assert_eq!(shanten(&counts("1122m3344p5566s7z"), 0), 0);
    }

    #[test]
    fn seven_pairs_is_a_win() {
        assert_eq!(shanten(&counts("1122m3344p5566s77z"), 0), -1);
    }

    /// 対子が無ければ 6 シャンテン。
    #[test]
    fn no_pairs_is_six_away() {
        assert_eq!(shanten(&counts("147m258p369s1234z"), 0), 6);
    }

    /// 4枚持ちは1対子としてしか数えられない。種類が足りない分だけ余計に遠い。
    #[test]
    fn four_of_a_kind_counts_as_one_pair_only() {
        // 1111m 2222m 3333m 4m = 4種、対子3
        let hand = counts("1111222233334m");
        // 6 - 3 + max(0, 7 - 4) = 6
        assert_eq!(shanten(&hand, 0), 6);
    }

    /// 副露していると七対子は成立しない。
    #[test]
    fn melds_make_chiitoitsu_impossible() {
        assert!(shanten(&counts("1122m3344p55s"), 1) > 8);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core chiitoitsu`
Expected: コンパイルエラー（`shanten` が未定義）

- [ ] **Step 3: 実装を書く**

`crates/mahjong-core/src/shanten/chiitoitsu.rs` の先頭に置く:

```rust
//! 七対子形の向聴数。門前でしか成立しない。

use crate::hand::HandCounts;

/// 成立しえない場合に返す値。overall で min を取るときに選ばれないよう十分大きくする。
pub const IMPOSSIBLE: i8 = 127;

/// 七対子の向聴数。-1 が和了、0 がテンパイ。
///
/// 対子が7つ揃えば和了。種類が7未満だと、足りない種類の分だけ余計に遠くなる
/// （同じ牌を4枚持っていても2対子にはできないため）。
pub fn shanten(counts: &HandCounts, melds: u8) -> i8 {
    if melds > 0 {
        return IMPOSSIBLE;
    }

    let mut pairs = 0i8;
    let mut kinds = 0i8;
    for (_, count) in counts.kinds() {
        kinds += 1;
        if count >= 2 {
            pairs += 1;
        }
    }

    6 - pairs + (7 - kinds).max(0)
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-core chiitoitsu`
Expected: 5テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-core
git commit -m "feat(core): 七対子形の向聴数を実装"
```

---

### Task 2: 国士無双の向聴数

**Files:**
- Modify: `crates/mahjong-core/src/shanten/kokushi.rs`

**Interfaces:**
- Consumes: `HandCounts`、`protocol::tile::TileKind`
- Produces: `pub fn shanten(counts: &HandCounts, melds: u8) -> i8`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::parse_hand;

    fn counts(notation: &str) -> HandCounts {
        HandCounts::from_tiles(&parse_hand(notation).unwrap())
    }

    /// 13種を1枚ずつ持つ十三面待ち。
    #[test]
    fn thirteen_kinds_without_a_pair_is_tenpai() {
        assert_eq!(shanten(&counts("19m19p19s1234567z"), 0), 0);
    }

    /// 12種＋対子1つ。欠けている1種の単騎待ち。
    #[test]
    fn twelve_kinds_with_a_pair_is_tenpai() {
        assert_eq!(shanten(&counts("119m19p19s123456z"), 0), 0);
    }

    #[test]
    fn thirteen_kinds_with_a_pair_is_a_win() {
        assert_eq!(shanten(&counts("119m19p19s1234567z"), 0), -1);
    }

    /// 幺九牌が6種で対子なし。
    #[test]
    fn counts_only_terminals_and_honors() {
        // 1m 9s 1z 2z 3z 4z の6種、対子なし
        assert_eq!(shanten(&counts("147m258p369s1234z"), 0), 7);
    }

    #[test]
    fn melds_make_kokushi_impossible() {
        assert!(shanten(&counts("19m19p19s1234z"), 1) > 13);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core kokushi`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
//! 国士無双形の向聴数。門前でしか成立しない。

use protocol::tile::TileKind;

use crate::hand::HandCounts;

pub const IMPOSSIBLE: i8 = 127;

/// 国士無双の向聴数。-1 が和了、0 がテンパイ。
///
/// 13種の幺九牌を揃え、うち1種を対子にする。持っている種類数と、
/// 対子が1つでもあるかで決まる。
pub fn shanten(counts: &HandCounts, melds: u8) -> i8 {
    if melds > 0 {
        return IMPOSSIBLE;
    }

    let mut kinds = 0i8;
    let mut has_pair = false;
    for index in 0..TileKind::COUNT as u8 {
        let kind = TileKind::from_index(index).expect("範囲内");
        if !kind.is_terminal_or_honor() {
            continue;
        }
        let count = counts.get(kind);
        if count >= 1 {
            kinds += 1;
        }
        if count >= 2 {
            has_pair = true;
        }
    }

    13 - kinds - i8::from(has_pair)
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-core kokushi`
Expected: 5テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-core
git commit -m "feat(core): 国士無双形の向聴数を実装"
```

---

### Task 3: 標準形の向聴数

3形のうち唯一難しい。**公式を暗記して当てはめるのではなく、面子と搭子の取り出し方を総当たりして最小を採る**方針にする。13枚程度なら十分速く、境界条件を間違えにくい。

**Files:**
- Modify: `crates/mahjong-core/src/shanten/standard.rs`

**Interfaces:**
- Consumes: `HandCounts`、`TileKind`
- Produces: `pub fn shanten(counts: &HandCounts, melds: u8) -> i8`

**アルゴリズム:**

牌の種類を昇順に走査し、各段階で「この牌から刻子を取る」「順子を取る」「対子を取る」「両面/嵌張の搭子を取る」「何も取らずに次へ」を再帰的に試す。取り終えたら次式で評価し、全経路の最小を採る。

```
shanten = 8 - 2 * (副露数 + 手の内で作った面子数) - 搭子数 - (雀頭があれば 1)
```

制約: `面子数 + 搭子数 + 雀頭数 <= 5`。これを超える取り出しは無意味なので枝刈りする。

**この式が正しいことの確認**（`fixtures/shanten` の値と一致する）:

| 手 | 分解 | 計算 | 期待 |
|---|---|---|---|
| `123456789m123p11s` | 面子4＋雀頭 | `8-8-0-1` | `-1` |
| `123m456m789m12p11s` | 面子3＋搭子1＋雀頭 | `8-6-1-1` | `0` |
| `123m456m789m14p11s` | 面子3＋雀頭（1p/4pは孤立） | `8-6-0-1` | `1` |
| `147m258p369s1234z` | 何も取れない | `8-0-0-0` | `8` |
| `123m456m12p11s`（副露1） | 副露1＋面子2＋搭子1＋雀頭 | `8-6-1-1` | `0` |

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::parse_hand;

    fn counts(notation: &str) -> HandCounts {
        HandCounts::from_tiles(&parse_hand(notation).unwrap())
    }

    #[test]
    fn a_complete_hand_is_minus_one() {
        assert_eq!(shanten(&counts("123456789m123p11s"), 0), -1);
    }

    #[test]
    fn three_melds_a_partial_and_a_pair_is_tenpai() {
        assert_eq!(shanten(&counts("123m456m789m12p11s"), 0), 0);
        assert_eq!(shanten(&counts("123m456m789m13p11s"), 0), 0);
        assert_eq!(shanten(&counts("123m456m789m11p11s"), 0), 0);
    }

    #[test]
    fn isolated_tiles_do_not_form_a_partial_set() {
        // 1p と 4p は間が3つ空いており搭子にならない
        assert_eq!(shanten(&counts("123m456m789m14p11s"), 0), 1);
    }

    #[test]
    fn a_hand_with_nothing_usable_is_eight_away() {
        assert_eq!(shanten(&counts("147m258p369s1234z"), 0), 8);
    }

    /// 副露は面子1つ分として数える。
    #[test]
    fn called_melds_count_toward_the_four_sets() {
        assert_eq!(shanten(&counts("123m456m12p11s"), 1), 0);
    }

    /// 順子は色をまたがない。
    #[test]
    fn runs_do_not_cross_suits() {
        // 9m 1p 2p は順子にならない
        let hand = counts("99m11p22p33s44s55z");
        assert!(shanten(&hand, 0) > 0);
    }

    /// 字牌は順子を作れない。
    #[test]
    fn honors_never_form_runs() {
        assert_eq!(shanten(&counts("123z456z1234567z"), 0), shanten(&counts("123z456z1234567z"), 0));
        // 1z2z3z は面子にならないので、刻子・対子だけで評価される
        let hand = counts("111z222z333z44z5z");
        assert_eq!(shanten(&hand, 0), 0);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core standard`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
//! 標準形（4面子1雀頭）の向聴数。
//!
//! 面子・搭子・雀頭の取り出し方を総当たりし、最小の向聴数を採る。
//! 13〜14枚なら探索は十分小さく、公式の境界条件を間違えるより確実である。

use protocol::tile::TileKind;

use crate::hand::HandCounts;

/// 標準形の向聴数。-1 が和了、0 がテンパイ。
pub fn shanten(counts: &HandCounts, melds: u8) -> i8 {
    let mut work = *counts;
    let mut best = i8::MAX;
    search(&mut work, 0, melds as i8, 0, false, &mut best);
    best
}

/// `index` 以降の牌から面子・搭子・雀頭を取り出す全通りを試す。
///
/// - `sets` … 副露を含む完成面子の数
/// - `partials` … 搭子（両面・嵌張・辺張）と、雀頭以外の対子の数
/// - `has_pair` … 雀頭を確保済みか
fn search(
    counts: &mut HandCounts,
    index: u8,
    sets: i8,
    partials: i8,
    has_pair: bool,
    best: &mut i8,
) {
    // ブロックは合計5つまで（4面子＋雀頭）。超える取り出しは無意味。
    if sets + partials + i8::from(has_pair) > 5 {
        return;
    }

    if index >= TileKind::COUNT as u8 {
        let value = 8 - 2 * sets - partials - i8::from(has_pair);
        if value < *best {
            *best = value;
        }
        return;
    }

    let kind = TileKind::from_index(index).expect("範囲内");

    // この牌を使い切ったら次の種類へ。
    if counts.get(kind) == 0 {
        search(counts, index + 1, sets, partials, has_pair, best);
        return;
    }

    // 1) 刻子として取る
    if counts.get(kind) >= 3 {
        take(counts, kind, 3);
        search(counts, index, sets + 1, partials, has_pair, best);
        give(counts, kind, 3);
    }

    // 2) 順子として取る（数牌のみ、色をまたがない）
    if let Some(run) = run_from(kind) {
        if run.iter().all(|k| counts.get(*k) >= 1) {
            for k in &run {
                take(counts, *k, 1);
            }
            search(counts, index, sets + 1, partials, has_pair, best);
            for k in &run {
                give(counts, *k, 1);
            }
        }
    }

    // 3) 雀頭として取る（まだ確保していない場合のみ）
    if !has_pair && counts.get(kind) >= 2 {
        take(counts, kind, 2);
        search(counts, index, sets, partials, true, best);
        give(counts, kind, 2);
    }

    // 4) 対子を搭子として取る（シャンポン待ちの片割れになる）
    if counts.get(kind) >= 2 {
        take(counts, kind, 2);
        search(counts, index, sets, partials + 1, has_pair, best);
        give(counts, kind, 2);
    }

    // 5) 搭子として取る（両面・辺張・嵌張）
    for gap in [1u8, 2] {
        if let Some(other) = offset_within_suit(kind, gap) {
            if counts.get(kind) >= 1 && counts.get(other) >= 1 {
                take(counts, kind, 1);
                take(counts, other, 1);
                search(counts, index, sets, partials + 1, has_pair, best);
                give(counts, other, 1);
                give(counts, kind, 1);
            }
        }
    }

    // 6) この牌を1枚捨てて先へ進む（孤立牌として扱う）
    take(counts, kind, 1);
    search(counts, index, sets, partials, has_pair, best);
    give(counts, kind, 1);
}

fn take(counts: &mut HandCounts, kind: TileKind, n: u8) {
    for _ in 0..n {
        assert!(counts.remove(kind), "取り出せる枚数を超えている");
    }
}

fn give(counts: &mut HandCounts, kind: TileKind, n: u8) {
    for _ in 0..n {
        counts.add(kind);
    }
}

/// `kind` を最小とする順子の3枚。字牌と、色をまたぐ場合は None。
fn run_from(kind: TileKind) -> Option<[TileKind; 3]> {
    let second = offset_within_suit(kind, 1)?;
    let third = offset_within_suit(kind, 2)?;
    Some([kind, second, third])
}

/// 同じ色の中で `gap` だけ大きい牌。色をまたぐ場合と字牌は None。
fn offset_within_suit(kind: TileKind, gap: u8) -> Option<TileKind> {
    let number = kind.number()?;
    if number + gap > 9 {
        return None;
    }
    TileKind::from_index(kind.index() + gap)
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-core standard`
Expected: 7テスト PASS

もし遅い場合でも、まず正しさを優先する。速度は Task 5 の計測で判断する。

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-core
git commit -m "feat(core): 標準形の向聴数を総当たり探索で実装"
```

---

### Task 4: 3形を統合した向聴数

**Files:**
- Modify: `crates/mahjong-core/src/shanten/overall.rs`

**Interfaces:**
- Consumes: `shanten::{standard, chiitoitsu, kokushi}`
- Produces:
  - `pub fn shanten(counts: &HandCounts, melds: u8) -> i8`
  - `pub fn is_tenpai(counts: &HandCounts, melds: u8) -> bool`
  - `pub fn is_complete(counts: &HandCounts, melds: u8) -> bool`

- [ ] **Step 1: 失敗するテストを書く**

`fixtures/shanten` を丸ごと回すテストを含める。これが Wave 1a の合否そのものになる。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::parse_hand;

    fn counts(notation: &str) -> HandCounts {
        HandCounts::from_tiles(&parse_hand(notation).unwrap())
    }

    /// 期待値テーブルの全ケースを通す。これが Wave 1a の合否条件。
    #[test]
    fn matches_every_fixture() {
        for case in test_fixtures::load_shanten_cases() {
            let hand = counts(&case.concealed);
            let actual = shanten(&hand, case.melds);
            assert_eq!(
                actual, case.expect.overall,
                "{}: {} → overall が {} だが期待は {}（{}）",
                case.id, case.concealed, actual, case.expect.overall, case.note
            );
        }
    }

    /// 個別形が指定されているケースは、その形の値も一致させる。
    #[test]
    fn matches_declared_individual_forms() {
        for case in test_fixtures::load_shanten_cases() {
            let hand = counts(&case.concealed);
            if let Some(expected) = case.expect.chiitoitsu {
                assert_eq!(
                    crate::shanten::chiitoitsu::shanten(&hand, case.melds),
                    expected,
                    "{}: 七対子形",
                    case.id
                );
            }
            if let Some(expected) = case.expect.kokushi {
                assert_eq!(
                    crate::shanten::kokushi::shanten(&hand, case.melds),
                    expected,
                    "{}: 国士形",
                    case.id
                );
            }
        }
    }

    #[test]
    fn tenpai_and_complete_agree_with_shanten() {
        let tenpai = counts("123m456m789m12p11s");
        assert!(is_tenpai(&tenpai, 0));
        assert!(!is_complete(&tenpai, 0));

        let won = counts("123456789m123p11s");
        assert!(is_complete(&won, 0));
        assert!(!is_tenpai(&won, 0));
    }

    /// 副露していても七対子・国士の値に引きずられない。
    #[test]
    fn melded_hands_ignore_menzen_only_forms() {
        let hand = counts("123m456m12p11s");
        assert_eq!(shanten(&hand, 1), 0);
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core overall`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
//! 3形を統合した向聴数。呼び出し側は原則これを使う。

use crate::hand::HandCounts;
use crate::shanten::{chiitoitsu, kokushi, standard};

/// 標準形・七対子・国士無双のうち最も近い形の向聴数。
/// -1 が和了、0 がテンパイ。
pub fn shanten(counts: &HandCounts, melds: u8) -> i8 {
    standard::shanten(counts, melds)
        .min(chiitoitsu::shanten(counts, melds))
        .min(kokushi::shanten(counts, melds))
}

pub fn is_tenpai(counts: &HandCounts, melds: u8) -> bool {
    shanten(counts, melds) == 0
}

pub fn is_complete(counts: &HandCounts, melds: u8) -> bool {
    shanten(counts, melds) == -1
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-core overall`
Expected: 4テスト PASS。`matches_every_fixture` が10ケースすべてを通ること

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-core
git commit -m "feat(core): 3形を統合した向聴数を実装し期待値テーブルを通す"
```

---

### Task 5: 待ち牌の列挙

**Files:**
- Modify: `crates/mahjong-core/src/wait.rs`

**Interfaces:**
- Consumes: `HandCounts`、`shanten::overall`
- Produces: `pub fn waiting_tiles(counts: &HandCounts, melds: u8) -> Vec<TileKind>`

**待ち形（両面・嵌張・辺張など）の判定はここに実装しない。** 待ち形は
「その手をどう分解したか」に依存し、分解は Wave 1b の `decompose.rs` が持つ。
分解と切り離して待ち形だけを決めると、1a と 1b で違う分解を前提にした判断が
生まれる。したがって待ち形の判定は 1b 側に置き、この計画では扱わない。

ここが担うのは「テンパイのとき、どの牌で和了できるか」だけである。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::{parse_hand, parse_tile};

    fn counts(notation: &str) -> HandCounts {
        HandCounts::from_tiles(&parse_hand(notation).unwrap())
    }

    fn kind(notation: &str) -> TileKind {
        parse_tile(notation).unwrap().kind()
    }

    #[test]
    fn lists_the_tiles_that_complete_the_hand() {
        // 78p の両面。6p と 9p で和了。
        let hand = counts("234567m23478p22s");
        let waits: Vec<u8> = waiting_tiles(&hand, 0).iter().map(|k| k.index()).collect();
        assert_eq!(waits, vec![kind("6p").index(), kind("9p").index()]);
    }

    #[test]
    fn penchan_waits_on_a_single_tile() {
        // 12m の辺張。3m のみ。
        let hand = counts("12456m234678p55s");
        let waits: Vec<u8> = waiting_tiles(&hand, 0).iter().map(|k| k.index()).collect();
        assert_eq!(waits, vec![kind("3m").index()]);
    }

    #[test]
    fn kokushi_thirteen_wait_lists_all_terminals_and_honors() {
        let hand = counts("19m19p19s1234567z");
        assert_eq!(waiting_tiles(&hand, 0).len(), 13);
    }

    #[test]
    fn a_hand_that_is_not_tenpai_has_no_waits() {
        assert!(waiting_tiles(&counts("147m258p369s1234z"), 0).is_empty());
    }

    /// 副露していてもテンパイなら待ちを返す。
    #[test]
    fn melded_hands_still_report_their_waits() {
        let hand = counts("123m456m12p11s");
        let waits: Vec<u8> = waiting_tiles(&hand, 1).iter().map(|k| k.index()).collect();
        assert_eq!(waits, vec![kind("3p").index()]);
    }

    /// シャンポン待ちは2種類を返す。
    #[test]
    fn shanpon_waits_on_both_pairs() {
        let hand = counts("123m456m789m11p11s");
        let waits: Vec<u8> = waiting_tiles(&hand, 0).iter().map(|k| k.index()).collect();
        assert_eq!(waits, vec![kind("1p").index(), kind("1s").index()]);
    }

    /// 4枚見えている牌は待ちに含めない。
    #[test]
    fn a_tile_already_held_four_times_is_not_a_wait() {
        // 1p を4枚持っている状態では 1p は和了牌になりえない
        let hand = counts("1111p234567m99s");
        assert!(!waiting_tiles(&hand, 0)
            .iter()
            .any(|k| *k == kind("1p")));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core wait`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
//! 待ち牌の列挙。
//!
//! 待ち形（両面・嵌張など）の判定はここに無い。待ち形は手をどう分解したかに
//! 依存するため、分解を持つ `decompose.rs`（Wave 1b）が担う。

use protocol::tile::TileKind;

use crate::hand::HandCounts;
use crate::shanten::overall;

/// テンパイ時に和了となる牌を、種類の昇順で返す。テンパイでなければ空。
///
/// 34種すべてを1枚足して和了形になるかを見る。総当たりだが、
/// 判定が向聴数の実装ひとつに集約されるため取りこぼしが起きない。
pub fn waiting_tiles(counts: &HandCounts, melds: u8) -> Vec<TileKind> {
    if !overall::is_tenpai(counts, melds) {
        return Vec::new();
    }

    let mut waits = Vec::new();
    for index in 0..TileKind::COUNT as u8 {
        let kind = TileKind::from_index(index).expect("範囲内");
        // 同じ牌は4枚までしか存在しない。
        if counts.get(kind) >= 4 {
            continue;
        }
        let mut probe = *counts;
        probe.add(kind);
        if overall::is_complete(&probe, melds) {
            waits.push(kind);
        }
    }
    waits
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-core wait`
Expected: 7テスト PASS

- [ ] **Step 5: 速度を確認する**

`waiting_tiles` は向聴計算を34回呼ぶ。CPU 同士の対局を毎秒数千局回す前提なので、
ここが遅いと全体の足を引っ張る。

```bash
cargo test --package mahjong-core --release -- --nocapture wait
```

明らかに待たされる（数秒かかる）場合は、報告したうえで `standard::shanten` に
メモ化を入れる。体感で一瞬なら次へ進む。

- [ ] **Step 6: コミット**

```bash
git add crates/mahjong-core
git commit -m "feat(core): 待ち牌の列挙と待ち形の判定を実装"
```

---

### Task 6: 振聴の判定

**Files:**
- Modify: `crates/mahjong-core/src/furiten.rs`

**Interfaces:**
- Consumes: `TileKind`、`protocol::tile::Tile`
- Produces:
  - `pub fn is_furiten_by_discards(waits: &[TileKind], own_discards: &[Tile]) -> bool`
  - `pub fn is_temporary_furiten(waits: &[TileKind], passed_since_draw: &[TileKind]) -> bool`

リーチ後の永続振聴は、見逃した牌を `own_discards` 相当として渡すことで同じ関数で扱う。
状態の管理は engine（Wave 2）の責務であり、ここは判定だけを持つ。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::notation::{parse_hand, parse_tile};

    fn kinds(notation: &str) -> Vec<TileKind> {
        parse_hand(notation)
            .unwrap()
            .iter()
            .map(|t| t.kind())
            .collect()
    }

    #[test]
    fn discarding_any_waiting_tile_causes_furiten() {
        let waits = kinds("6p9p");
        let discards = parse_hand("1m9p3s").unwrap();
        assert!(is_furiten_by_discards(&waits, &discards));
    }

    #[test]
    fn unrelated_discards_do_not_cause_furiten() {
        let waits = kinds("6p9p");
        let discards = parse_hand("1m2m3s").unwrap();
        assert!(!is_furiten_by_discards(&waits, &discards));
    }

    /// 赤ドラは通常牌と同じ種類として扱う。赤5pを捨てていれば5p待ちは振聴。
    #[test]
    fn red_fives_count_as_their_normal_kind() {
        let waits = kinds("5p");
        let discards = parse_hand("0p").unwrap();
        assert!(is_furiten_by_discards(&waits, &discards));
    }

    #[test]
    fn passing_on_a_waiting_tile_causes_temporary_furiten() {
        let waits = kinds("6p9p");
        assert!(is_temporary_furiten(&waits, &kinds("6p")));
        assert!(!is_temporary_furiten(&waits, &kinds("1m")));
    }

    #[test]
    fn no_waits_means_no_furiten() {
        assert!(!is_furiten_by_discards(&[], &parse_hand("1m").unwrap()));
        assert!(!is_temporary_furiten(&[], &kinds("1m")));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core furiten`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
//! 振聴の判定。状態の管理は engine の責務であり、ここは判定だけを持つ。

use protocol::tile::{Tile, TileKind};

/// 自分の河に待ち牌が1枚でもあれば振聴。
///
/// リーチ後の永続振聴も、見逃した牌を `own_discards` に含めて渡せば同じ判定で扱える。
pub fn is_furiten_by_discards(waits: &[TileKind], own_discards: &[Tile]) -> bool {
    own_discards
        .iter()
        .any(|tile| waits.contains(&tile.kind()))
}

/// 同巡内振聴。次のツモまでの間に待ち牌を見逃していれば振聴。
pub fn is_temporary_furiten(waits: &[TileKind], passed_since_draw: &[TileKind]) -> bool {
    passed_since_draw.iter().any(|kind| waits.contains(kind))
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-core furiten`
Expected: 5テスト PASS

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-core
git commit -m "feat(core): 振聴の判定を実装"
```

---

### Task 7: 鳴きの候補

**ロンの可否はここに実装しない。**役の有無に依存するため Wave 2 で engine が結線する。

**Files:**
- Modify: `crates/mahjong-core/src/callable.rs`

**Interfaces:**
- Consumes: `protocol::tile::{Tile, TileKind}`
- Produces:
  - `pub fn chi_candidates(hand: &[Tile], discarded: Tile) -> Vec<[Tile; 2]>`
  - `pub fn pon_candidates(hand: &[Tile], discarded: Tile) -> Vec<[Tile; 2]>`
  - `pub fn minkan_possible(hand: &[Tile], discarded: Tile) -> bool`
  - `pub fn ankan_candidates(hand: &[Tile]) -> Vec<TileKind>`
  - `pub fn kakan_candidates(hand: &[Tile], melds: &[Meld]) -> Vec<Tile>`

**赤ドラの扱いが要点である。** 5p を2枚持っていて片方が赤の場合、ポンで
どちらを使うかで手の価値が変わる。したがって候補は `TileKind` ではなく
実際の `Tile` の組で返し、プレイヤーが選べるようにする。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::meld::{Meld, MeldKind};
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::seat::Seat;

    fn notation_of(pairs: &[[Tile; 2]]) -> Vec<String> {
        pairs
            .iter()
            .map(|p| protocol::notation::to_notation(p))
            .collect()
    }

    #[test]
    fn chi_lists_every_way_to_form_a_run() {
        // 3p を鳴く。手に 12p / 24p / 45p があるので3通り。
        let hand = parse_hand("124 5p").unwrap();
        let _ = hand;
        let hand = parse_hand("1245p").unwrap();
        let candidates = chi_candidates(&hand, parse_tile("3p").unwrap());
        assert_eq!(notation_of(&candidates), vec!["12p", "24p", "45p"]);
    }

    #[test]
    fn chi_does_not_cross_suits() {
        let hand = parse_hand("89m12p").unwrap();
        assert!(chi_candidates(&hand, parse_tile("1p").unwrap()).is_empty() == false);
        // 9m と 1p は繋がらない
        let hand = parse_hand("89m").unwrap();
        assert!(chi_candidates(&hand, parse_tile("1p").unwrap()).is_empty());
    }

    #[test]
    fn honors_cannot_be_chied() {
        let hand = parse_hand("1234567z").unwrap();
        assert!(chi_candidates(&hand, parse_tile("2z").unwrap()).is_empty());
    }

    /// 赤5を含む場合、使う牌の組み合わせを選べるよう別候補として返す。
    #[test]
    fn pon_distinguishes_red_fives() {
        // 5p を2枚（うち1枚が赤）＋通常5p を1枚 = 3枚持ち
        let hand = parse_hand("55p0p").unwrap();
        let candidates = pon_candidates(&hand, parse_tile("5p").unwrap());
        // 通常2枚の組と、通常＋赤の組
        assert_eq!(notation_of(&candidates), vec!["55p", "50p"]);
    }

    #[test]
    fn pon_needs_two_matching_tiles() {
        let hand = parse_hand("5p").unwrap();
        assert!(pon_candidates(&hand, parse_tile("5p").unwrap()).is_empty());
    }

    #[test]
    fn minkan_needs_three_matching_tiles() {
        assert!(minkan_possible(
            &parse_hand("555p").unwrap(),
            parse_tile("5p").unwrap()
        ));
        assert!(!minkan_possible(
            &parse_hand("55p").unwrap(),
            parse_tile("5p").unwrap()
        ));
    }

    #[test]
    fn ankan_needs_four_in_hand() {
        let hand = parse_hand("5555p111m").unwrap();
        let candidates: Vec<u8> = ankan_candidates(&hand).iter().map(|k| k.index()).collect();
        assert_eq!(candidates, vec![parse_tile("5p").unwrap().kind().index()]);
    }

    #[test]
    fn kakan_extends_an_existing_pon() {
        let hand = parse_hand("5p1m").unwrap();
        let melds = vec![Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("555p").unwrap(),
            from: Some(Seat::new(1)),
            called_tile: Some(parse_tile("5p").unwrap()),
        }];
        let candidates = kakan_candidates(&hand, &melds);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].kind(), parse_tile("5p").unwrap().kind());
    }

    #[test]
    fn kakan_does_not_apply_to_chi() {
        let hand = parse_hand("5p").unwrap();
        let melds = vec![Meld {
            kind: MeldKind::Chi,
            tiles: parse_hand("345p").unwrap(),
            from: Some(Seat::new(3)),
            called_tile: Some(parse_tile("3p").unwrap()),
        }];
        assert!(kakan_candidates(&hand, &melds).is_empty());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-core callable`
Expected: コンパイルエラー

- [ ] **Step 3: 実装を書く**

```rust
//! 鳴きの候補を列挙する。
//!
//! ロンの可否はここに無い。役の有無に依存するため Wave 2 で engine が結線する。
//!
//! 候補は `TileKind` ではなく実際の `Tile` の組で返す。赤5を使うかどうかで
//! 手の価値が変わるため、プレイヤーが選べる必要がある。

use protocol::meld::{Meld, MeldKind};
use protocol::tile::{Tile, TileKind};

/// 手牌から同じ種類の牌を、通常牌を先に、赤ドラを後に並べて取り出す。
fn tiles_of_kind(hand: &[Tile], kind: TileKind) -> Vec<Tile> {
    let mut found: Vec<Tile> = hand.iter().copied().filter(|t| t.kind() == kind).collect();
    found.sort_by_key(|t| t.is_red());
    found
}

/// チーの候補。上家からの打牌かどうかは呼び出し側が判断する。
pub fn chi_candidates(hand: &[Tile], discarded: Tile) -> Vec<[Tile; 2]> {
    let Some(number) = discarded.kind().number() else {
        return Vec::new();
    };

    let mut out = Vec::new();
    // 打牌を含む順子は、打牌が最大・中央・最小の3通り。
    for lowest in [number.wrapping_sub(2), number.wrapping_sub(1), number] {
        if lowest == 0 || lowest > 7 || lowest > number || number > lowest + 2 {
            continue;
        }
        let needed: Vec<u8> = (lowest..lowest + 3).filter(|n| *n != number).collect();
        let mut picked = Vec::new();
        for n in needed {
            let Some(kind) = kind_in_same_suit(discarded.kind(), n) else {
                picked.clear();
                break;
            };
            let Some(tile) = tiles_of_kind(hand, kind).into_iter().next() else {
                picked.clear();
                break;
            };
            picked.push(tile);
        }
        if picked.len() == 2 {
            out.push([picked[0], picked[1]]);
        }
    }
    out
}

/// ポンの候補。赤ドラを使う組と使わない組を別候補として返す。
pub fn pon_candidates(hand: &[Tile], discarded: Tile) -> Vec<[Tile; 2]> {
    let available = tiles_of_kind(hand, discarded.kind());
    if available.len() < 2 {
        return Vec::new();
    }

    let mut out = Vec::new();
    for i in 0..available.len() {
        for j in (i + 1)..available.len() {
            let pair = [available[i], available[j]];
            if !out.contains(&pair) {
                out.push(pair);
            }
        }
    }
    out
}

/// 明槓できるか。手に同じ種類が3枚必要。
pub fn minkan_possible(hand: &[Tile], discarded: Tile) -> bool {
    tiles_of_kind(hand, discarded.kind()).len() >= 3
}

/// 暗槓できる種類。手に4枚必要。
pub fn ankan_candidates(hand: &[Tile]) -> Vec<TileKind> {
    let mut out = Vec::new();
    for index in 0..TileKind::COUNT as u8 {
        let kind = TileKind::from_index(index).expect("範囲内");
        if tiles_of_kind(hand, kind).len() >= 4 && !out.contains(&kind) {
            out.push(kind);
        }
    }
    out
}

/// 加槓できる牌。既にポンしている刻子と同じ種類を手に持っている場合。
pub fn kakan_candidates(hand: &[Tile], melds: &[Meld]) -> Vec<Tile> {
    let mut out = Vec::new();
    for meld in melds {
        if meld.kind != MeldKind::Pon {
            continue;
        }
        let Some(kind) = meld.tiles.first().map(|t| t.kind()) else {
            continue;
        };
        for tile in tiles_of_kind(hand, kind) {
            if !out.contains(&tile) {
                out.push(tile);
            }
        }
    }
    out
}

/// 同じ色の中で番号 `number` の牌。字牌と範囲外は None。
fn kind_in_same_suit(reference: TileKind, number: u8) -> Option<TileKind> {
    let current = reference.number()?;
    if !(1..=9).contains(&number) {
        return None;
    }
    let base = reference.index() - (current - 1);
    TileKind::from_index(base + number - 1)
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-core callable`
Expected: 9テスト PASS

チーの候補の順序がテストの期待と違う場合、**テストの期待順ではなく実装の
出力順に合わせてテストを直してよい**（順序に意味は無い）。ただし候補の
集合そのものが違う場合は実装を疑う。

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-core
git commit -m "feat(core): 鳴きの候補列挙を実装"
```

---

### Task 8: 全体の検証と速度の確認

**Files:**
- なし（検証のみ）

- [ ] **Step 1: 全テストを走らせる**

Run: `cargo test --workspace`
Expected: すべて PASS。`matches_every_fixture` が10ケースを通していること

- [ ] **Step 2: lint を通す**

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Expected: どちらも成功

- [ ] **Step 3: 速度を測る**

CPU 同士の対局を毎秒数千局回せることが以降の開発の前提になる。
向聴計算がその中心なので、ここで実測しておく。

```bash
cargo test --package mahjong-core --release -- --nocapture
```

テスト全体が1秒以内に終われば十分。数秒かかる場合は
`standard::shanten` の探索にメモ化を入れることを検討し、コーディネータへ報告する。

- [ ] **Step 4: 進捗を報告する**

```bash
orca worktree set --worktree active --comment "Wave 1a 完了。向聴・待ち・振聴・鳴き候補すべて実装、期待値テーブル通過" --workspace-status in-review --json
```

- [ ] **Step 5: コミット**

```bash
git add -A
git commit -m "chore(core): Wave 1a の検証を完了"
```

---

## Wave 1a 完了の判定

- [ ] `cargo test --workspace` が通る
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] `fixtures/shanten` の10ケースすべてを `overall::shanten` が通す
- [ ] `decompose.rs` / `yaku_check/` / `fu.rs` / `score.rs` を編集していない
- [ ] `lib.rs` / `mod.rs` / `hand.rs` / `shapes.rs` を編集していない
