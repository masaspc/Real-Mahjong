# Wave 3a: CPU 雀士 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ルールベースの CPU 雀士を作る。打牌と鳴きを決められるようにし、4人 CPU で半荘を通しで回せる土台にする。

**Architecture:** 純粋関数だけで組む。時刻も乱数も I/O も持たない。**CPU は自分の席から見える情報しか受け取らない。**呼び出し側が渡す `View` に他家の手牌を入れないことで、いかさまを構造で防ぐ。

**Tech Stack:** Rust 1.97.1 / edition 2021 / `protocol`・`mahjong-core` のみに依存（`mahjong-engine` には依存しない）

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
**作業規約:** `AGENTS.md`
**前提:** Wave 2f がマージ済みであること（Rust 全体で491件のテストが通ること）

## Global Constraints

- **編集してよいのは `crates/mahjong-ai/src/` の `discard.rs` / `call.rs` / `safety.rs` だけである**
- **`lib.rs` を編集しない。** Wave 0 で凍結済みである
- **`mahjong-engine` に依存しない。**山を持つ側を参照した瞬間、CPU が山を読めてしまう。`Cargo.toml` も変更しない
- `crates/protocol` と `crates/mahjong-core` は凍結済み。**編集も追加もしない**
- **時刻も乱数も使わない。** `Instant::now()` / `rand` を呼ばない。同じ `View` と同じ選択肢からは必ず同じ手を返す
- **和了の判定を書き直さない。**向聴数・待ち・役・符・鳴きの候補はすべて
  `mahjong-core` の公開 API を使う
- **牌の分類（自風・場風・三元牌など）は AI 側に書いてよい。**これは「その牌が
  役になるか」ではなく「鳴く価値があるか」という方針の判断であり、`mahjong-core`
  に対応する公開 API も無い。和了の判定とは別物として扱う
- 完了条件は `cargo test --workspace` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` がすべて通ること

## CPU に何を見せるか

**`View` に入れてよいのは、その席が実際に見える情報だけである。**他家の手牌も
山の中身も入れない。呼び出し側（Wave 3b の卓 Actor）がここを守れば、
CPU がいかさまをする道は構造的に無くなる。

```rust
/// CPU が見てよい情報。**他家の手牌と山の中身は入れない。**
pub struct View {
    pub seat: Seat,
    pub seat_wind: Wind,
    pub round_wind: Wind,
    /// 自分の手牌。打牌の判断ではツモ牌を含む。
    pub hand: Vec<Tile>,
    /// **自分の副露と暗槓だけ。**他家の副露は入れない。
    pub melds: Vec<Meld>,
    /// 4席の捨て牌。鳴かれた牌も見えているので含める。
    pub rivers: [Vec<Tile>; 4],
    /// リーチが成立している席。
    pub riichi: [bool; 4],
    /// 開いているドラ表示牌。裏ドラは入れない。
    pub dora_indicators: Vec<Tile>,
    pub wall_remaining: u8,
    pub scores: [i32; 4],
}
```

`View` は `discard.rs` に置く。`call.rs` と `safety.rs` は
`use crate::discard::View;` で取り込む。**`lib.rs` を編集できないので、
新しいモジュールは作れない。**

## コーディネータが確定させたルール

v1 の CPU は「大きく負けない」ことを狙う。最善手を探さない。

| 項目 | 決定 |
|---|---|
| 和了 | 提示されたら必ず取る。ツモもロンも見送らない |
| 打牌の基本方針 | 向聴数が最も下がる牌を切る。同じなら安全度の高い牌を切る |
| 安全度 | リーチしている席の河にある牌は「通っている」とみなす。リーチが無ければ全部同じ扱い |
| 安全度も同じ場合 | 提示された順で先にあるものを切る。字牌を優先するような細工はしない。**表に書いて実装しない規則を作らない** |
| リーチ | テンパイして門前なら必ず宣言する。待ちの良し悪しは見ない |
| 鳴き | **役が見込めるときだけ鳴く。**鳴く牌が役牌（自風・場風・三元牌）であるか、手牌と自分の副露に幺九牌が1枚も無く鳴く牌も中張牌であること |
| チー | 鳴かない。v1 では手が安くなりやすく、判断も難しい |
| 槓 | しない。ドラが増えて他家に有利になる場面を判断できない |
| 九種九牌 | 提示されたら宣言する |
| 同点の選択 | 牌の並び順で先にあるものを選ぶ。**乱数を使わない。**同じ局面からは必ず同じ手が出る |

---

## タスクの依存関係

```
1 安全度と打牌 → 2 鳴きの判断
```

直列である。鳴くかどうかの判断が、鳴いたあとに切る牌の評価を使う。

---

### Task 1: 安全度と打牌

**Files:**
- Modify: `crates/mahjong-ai/src/safety.rs`
- Modify: `crates/mahjong-ai/src/discard.rs`

**Interfaces:**
- Produces:
  - `discard.rs`: `pub struct View { .. }`、`pub fn choose(view: &View, options: &[ActionOption]) -> Command`
  - `safety.rs`: `pub fn is_safe_against_riichi(view: &View, tile: Tile) -> bool`
- Consumes: `mahjong_core::shanten::overall::shanten`、`mahjong_core::hand::HandCounts`

**`choose` は `RequestAction.options` をそのまま受け取り、`Command` を返す。**
選択肢に無い手を返さない。和了と九種九牌が提示されていれば、打牌より先に取る。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::meld::MeldKind;
    use protocol::notation::{parse_hand, parse_tile};

    fn view_with(hand: &str) -> View {
        View {
            seat: Seat::new(0),
            seat_wind: Wind::East,
            round_wind: Wind::East,
            hand: parse_hand(hand).expect("正しい記法"),
            melds: Vec::new(),
            rivers: std::array::from_fn(|_| Vec::new()),
            riichi: [false; 4],
            dora_indicators: Vec::new(),
            wall_remaining: 70,
            scores: [25_000; 4],
        }
    }

    fn discard_option(hand: &str) -> ActionOption {
        ActionOption::Discard {
            allowed: parse_hand(hand).expect("正しい記法"),
            riichi_allowed: Vec::new(),
        }
    }

    /// 和了が提示されていれば必ず取る。
    #[test]
    fn a_win_is_always_taken() {
        let view = view_with("234567m23478p22s6p");
        let options = vec![discard_option("6p"), ActionOption::Tsumo];
        assert_eq!(choose(&view, &options), Command::Tsumo);
    }

    /// 九種九牌が提示されていれば宣言する。
    #[test]
    fn nine_terminals_is_declared() {
        let view = view_with("19m19p19s12345677z");
        let options = vec![discard_option("1z"), ActionOption::Kyuushu];
        assert_eq!(choose(&view, &options), Command::Kyuushu);
    }

    /// 和了は九種九牌より優先する。
    #[test]
    fn a_win_beats_an_abortive_draw() {
        let view = view_with("19m19p19s12345677z");
        let options = vec![
            discard_option("1z"),
            ActionOption::Kyuushu,
            ActionOption::Tsumo,
        ];
        assert_eq!(choose(&view, &options), Command::Tsumo);
    }

    /// 向聴数が最も下がる牌を切る。
    ///
    /// 234567m 23478p 22s に 9m を引いた形。9m は完全に浮いており、
    /// 切っても向聴数が変わらない。他の牌を切ると向聴数が増える。
    #[test]
    fn the_floating_tile_is_discarded() {
        let view = view_with("234567m23478p22s9m");
        let options = vec![discard_option("234567m23478p22s9m")];
        assert_eq!(
            choose(&view, &options),
            Command::Discard {
                tile: parse_tile("9m").expect("正しい記法"),
                riichi: false,
            }
        );
    }

    /// テンパイして門前ならリーチする。
    #[test]
    fn a_closed_tenpai_declares_riichi() {
        let view = view_with("234567m23478p22s9m");
        let options = vec![ActionOption::Discard {
            allowed: parse_hand("234567m23478p22s9m").expect("正しい記法"),
            riichi_allowed: parse_hand("9m").expect("正しい記法"),
        }];
        assert_eq!(
            choose(&view, &options),
            Command::Discard {
                tile: parse_tile("9m").expect("正しい記法"),
                riichi: true,
            }
        );
    }

    /// リーチできる牌が複数あっても、向聴数の判断は変わらない。
    #[test]
    fn riichi_follows_the_same_choice_as_a_plain_discard() {
        let view = view_with("234567m23478p22s9m");
        let plain = vec![discard_option("234567m23478p22s9m")];
        let with_riichi = vec![ActionOption::Discard {
            allowed: parse_hand("234567m23478p22s9m").expect("正しい記法"),
            riichi_allowed: parse_hand("234567m23478p22s9m").expect("正しい記法"),
        }];
        let Command::Discard { tile: a, .. } = choose(&view, &plain) else {
            panic!("打牌でない");
        };
        let Command::Discard { tile: b, riichi } = choose(&view, &with_riichi) else {
            panic!("打牌でない");
        };
        assert_eq!(a, b);
        assert!(riichi);
    }

    /// 鳴いていればリーチできないので、宣言もしない。
    #[test]
    fn an_open_hand_never_declares_riichi() {
        let mut view = view_with("234567m78p22s9m");
        view.melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("444p").expect("正しい記法"),
            from: Some(Seat::new(1)),
            called_tile: Some(parse_tile("4p").expect("正しい記法")),
        });
        // 提示側が riichi_allowed を空で出す。CPU はそれに従う。
        let options = vec![discard_option("234567m78p22s9m")];
        let Command::Discard { riichi, .. } = choose(&view, &options) else {
            panic!("打牌でない");
        };
        assert!(!riichi);
    }

    /// 同じ局面からは必ず同じ手が出る。
    #[test]
    fn the_same_view_always_gives_the_same_command() {
        let view = view_with("234567m23478p22s9m");
        let options = vec![discard_option("234567m23478p22s9m")];
        assert_eq!(choose(&view, &options), choose(&view, &options));
    }

    /// 選択肢に無い牌は返さない。
    #[test]
    fn only_offered_tiles_are_chosen() {
        let view = view_with("234567m23478p22s9m");
        let allowed = parse_hand("9m22s").expect("正しい記法");
        let options = vec![ActionOption::Discard {
            allowed: allowed.clone(),
            riichi_allowed: Vec::new(),
        }];
        let Command::Discard { tile, .. } = choose(&view, &options) else {
            panic!("打牌でない");
        };
        assert!(allowed.contains(&tile), "提示外の牌を選んだ: {tile:?}");
    }

    /// 向聴数が同じなら、リーチ者に通っている牌を選ぶ。
    ///
    /// 国士の形なので 1m を切っても 9m を切っても向聴数は変わらない。
    /// そこで安全度が効く。
    #[test]
    fn a_safe_tile_wins_a_tie() {
        let mut view = view_with("119m19p19s1234567z");
        // 席1がリーチしていて、その河に 1m がある。9m は通っていない。
        view.riichi[1] = true;
        view.rivers[1].push(parse_tile("1m").expect("正しい記法"));
        // どちらを切っても向聴数は変わらない浮き牌同士にする。
        let options = vec![discard_option("1m9m")];
        assert_eq!(
            choose(&view, &options),
            Command::Discard {
                tile: parse_tile("1m").expect("正しい記法"),
                riichi: false,
            }
        );
    }
}
```

`safety.rs` のテスト。

```rust
#[cfg(test)]
mod tests {
    // `View` と `Seat` は親モジュールが取り込んでいる。二重に書かない。
    use super::*;
    use protocol::notation::parse_tile;
    use protocol::seat::Wind;

    fn view() -> View {
        View {
            seat: Seat::new(0),
            seat_wind: Wind::East,
            round_wind: Wind::East,
            hand: Vec::new(),
            melds: Vec::new(),
            rivers: std::array::from_fn(|_| Vec::new()),
            riichi: [false; 4],
            dora_indicators: Vec::new(),
            wall_remaining: 70,
            scores: [25_000; 4],
        }
    }

    /// 誰もリーチしていなければ、どの牌も安全とみなす。
    #[test]
    fn everything_is_safe_when_nobody_declared() {
        let view = view();
        assert!(is_safe_against_riichi(
            &view,
            parse_tile("1m").expect("正しい記法")
        ));
    }

    /// リーチ者の河にある牌は通っている。
    #[test]
    fn a_tile_in_the_river_is_safe() {
        let mut view = view();
        view.riichi[2] = true;
        view.rivers[2].push(parse_tile("5p").expect("正しい記法"));
        assert!(is_safe_against_riichi(
            &view,
            parse_tile("5p").expect("正しい記法")
        ));
    }

    /// リーチ者の河に無い牌は通っていない。
    #[test]
    fn a_tile_outside_the_river_is_not_safe() {
        let mut view = view();
        view.riichi[2] = true;
        view.rivers[2].push(parse_tile("5p").expect("正しい記法"));
        assert!(!is_safe_against_riichi(
            &view,
            parse_tile("6p").expect("正しい記法")
        ));
    }

    /// リーチしていない席の河は関係ない。
    #[test]
    fn a_river_without_riichi_does_not_make_a_tile_safe() {
        let mut view = view();
        view.riichi[2] = true;
        view.rivers[1].push(parse_tile("5p").expect("正しい記法"));
        assert!(!is_safe_against_riichi(
            &view,
            parse_tile("5p").expect("正しい記法")
        ));
    }

    /// 複数のリーチには、全員に通っている牌だけが安全である。
    #[test]
    fn every_declarer_must_have_seen_the_tile() {
        let mut view = view();
        view.riichi[1] = true;
        view.riichi[2] = true;
        view.rivers[1].push(parse_tile("5p").expect("正しい記法"));
        assert!(
            !is_safe_against_riichi(&view, parse_tile("5p").expect("正しい記法")),
            "席2には通っていない"
        );
        view.rivers[2].push(parse_tile("5p").expect("正しい記法"));
        assert!(is_safe_against_riichi(
            &view,
            parse_tile("5p").expect("正しい記法")
        ));
    }

    /// 赤5と通常の5は同じ牌として扱う。安全度は種類で決まる。
    #[test]
    fn a_red_five_is_as_safe_as_a_normal_five() {
        let mut view = view();
        view.riichi[1] = true;
        view.rivers[1].push(parse_tile("5p").expect("正しい記法"));
        assert!(is_safe_against_riichi(
            &view,
            parse_tile("0p").expect("正しい記法")
        ));
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-ai`
Expected: コンパイルエラー（`View` も `choose` も未定義）

- [ ] **Step 3: 実装を書く**

`safety.rs`。

```rust
//! 打牌の安全度。v1 は「リーチ者の河にあるか」だけを見る。
//!
//! 筋や壁は見ない。読み違えて放銃するより、通っている牌を切るほうが
//! 分かりやすく、CPU の狙い（大きく負けない）にも合う。

use crate::discard::View;
use protocol::seat::Seat;
use protocol::tile::Tile;

/// リーチしている全員の河に、その牌があるか。
///
/// **誰もリーチしていなければ真を返す。**危険を測る相手がいない。
/// 赤5と通常の5は同じ種類なので、`TileKind` で比べる。
pub fn is_safe_against_riichi(view: &View, tile: Tile) -> bool {
    Seat::ALL.iter().all(|seat| {
        if !view.riichi[seat.index()] {
            return true;
        }
        view.rivers[seat.index()]
            .iter()
            .any(|d| d.kind() == tile.kind())
    })
}
```

`discard.rs`。

```rust
//! 打牌の選択。向聴数を第一に見て、同じなら安全度で選ぶ。

use crate::safety::is_safe_against_riichi;
use mahjong_core::hand::HandCounts;
use mahjong_core::shanten::overall;
use protocol::command::{ActionOption, Command};
use protocol::meld::Meld;
use protocol::seat::{Seat, Wind};
use protocol::tile::Tile;

/// CPU が見てよい情報。**他家の手牌と山の中身は入れない。**
///
/// 呼び出し側がここを守れば、CPU がいかさまをする道は構造的に無くなる。
pub struct View {
    pub seat: Seat,
    pub seat_wind: Wind,
    pub round_wind: Wind,
    /// 自分の手牌。打牌の判断ではツモ牌を含む。
    pub hand: Vec<Tile>,
    /// **自分の副露と暗槓だけ。**他家の副露は入れない。
    pub melds: Vec<Meld>,
    /// 4席の捨て牌。鳴かれた牌も見えているので含める。
    pub rivers: [Vec<Tile>; 4],
    /// リーチが成立している席。
    pub riichi: [bool; 4],
    /// 開いているドラ表示牌。**裏ドラは入れない。**
    pub dora_indicators: Vec<Tile>,
    pub wall_remaining: u8,
    pub scores: [i32; 4],
}

/// 提示された選択肢から1つ選ぶ。
///
/// **提示に無い手は返さない。**和了が出ていれば必ず取る。
pub fn choose(view: &View, options: &[ActionOption]) -> Command {
    if options.iter().any(|o| matches!(o, ActionOption::Tsumo)) {
        return Command::Tsumo;
    }
    if options.iter().any(|o| matches!(o, ActionOption::Kyuushu)) {
        return Command::Kyuushu;
    }

    let (allowed, riichi_allowed) = options
        .iter()
        .find_map(|o| match o {
            ActionOption::Discard {
                allowed,
                riichi_allowed,
            } => Some((allowed.clone(), riichi_allowed.clone())),
            _ => None,
        })
        .expect("手番には必ず打牌の選択肢がある");

    let tile = best_discard(view, &allowed);
    Command::Discard {
        tile,
        // 宣言できるなら必ずする。待ちの良し悪しは見ない。
        riichi: riichi_allowed.contains(&tile),
    }
}

/// 切る牌を選ぶ。
///
/// 第一に向聴数、第二に安全度、第三に提示された順で決める。
/// **乱数を使わない。**同じ入力からは必ず同じ牌が出る。
fn best_discard(view: &View, allowed: &[Tile]) -> Tile {
    let melds = view.melds.len() as u8;
    allowed
        .iter()
        .copied()
        .enumerate()
        .min_by_key(|(index, tile)| {
            let rest = without(&view.hand, *tile);
            let shanten = overall::shanten(&HandCounts::from_tiles(&rest), melds);
            // 安全な牌を先にしたいので、危険なら 1 を足す。
            let risk = u8::from(!is_safe_against_riichi(view, *tile));
            (shanten, risk, *index)
        })
        .map(|(_, tile)| tile)
        .expect("提示された牌が1つ以上ある")
}

/// 手牌から1枚だけ取り除く。
fn without(hand: &[Tile], tile: Tile) -> Vec<Tile> {
    let mut out = hand.to_vec();
    if let Some(position) = out.iter().position(|t| *t == tile) {
        out.remove(position);
    }
    out
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-ai`
Expected: 16テスト PASS（`discard` 10件 + `safety` 6件）

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-ai
git commit -m "feat(ai): 安全度と打牌の選択を実装"
```

---

### Task 2: 鳴きの判断

**Files:**
- Modify: `crates/mahjong-ai/src/call.rs`

**Interfaces:**
- Produces: `pub fn respond(view: &View, options: &[ActionOption]) -> CallResponse`
- Consumes: `crate::discard::View`

**役が見込めるときだけ鳴く。**鳴いて役が無ければ和了れない。v1 では
判断できる形を2つに絞る。

| 鳴く条件 | 理由 |
|---|---|
| ロンが提示された | 和了は必ず取る |
| 役牌（自風・場風・三元牌）のポン | 鳴いても1翻が確定する |
| 断幺九が見込めるポン | **手牌にも自分の副露にも**幺九牌が1枚も無く、鳴く牌も中張牌であること。副露を見落とすと、幺九牌を含む副露があるのに断幺九を当てにしてしまう |

チーと槓はしない。

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::command::KanCandidate;
    use protocol::meld::{Meld, MeldKind};
    use protocol::notation::{parse_hand, parse_tile};
    use protocol::seat::Seat;

    /// 東場の東家。自風も場風も東になる。
    fn view_with(hand: &str) -> View {
        view_in(Wind::East, Wind::East, hand)
    }

    fn view_in(seat_wind: Wind, round_wind: Wind, hand: &str) -> View {
        View {
            seat: Seat::new(0),
            seat_wind,
            round_wind,
            hand: parse_hand(hand).expect("正しい記法"),
            melds: Vec::new(),
            rivers: std::array::from_fn(|_| Vec::new()),
            riichi: [false; 4],
            dora_indicators: Vec::new(),
            wall_remaining: 70,
            scores: [25_000; 4],
        }
    }

    fn pon_of(notation: &str) -> ActionOption {
        let tiles = parse_hand(notation).expect("正しい記法");
        ActionOption::Pon {
            candidates: vec![[tiles[0], tiles[1]]],
        }
    }

    fn chi_of(notation: &str) -> ActionOption {
        let tiles = parse_hand(notation).expect("正しい記法");
        ActionOption::Chi {
            candidates: vec![[tiles[0], tiles[1]]],
        }
    }

    /// ロンが提示されたら必ず取る。
    #[test]
    fn a_ron_is_always_taken() {
        let view = view_with("234567m23478p22s");
        let options = vec![ActionOption::Ron, pon_of("22s")];
        assert_eq!(respond(&view, &options), CallResponse::Ron);
    }

    /// 場風のポンは鳴く。1翻が確定する。
    #[test]
    fn the_round_wind_is_ponned() {
        let view = view_with("234567m234p11z99s");
        let options = vec![pon_of("11z")];
        let tiles = parse_hand("11z").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }

    /// 三元牌のポンは鳴く。
    #[test]
    fn a_dragon_is_ponned() {
        let view = view_with("234567m234p55z99s");
        let options = vec![pon_of("55z")];
        let tiles = parse_hand("55z").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }

    /// 役にならない風牌のポンは見送る。
    #[test]
    fn a_guest_wind_is_not_ponned() {
        // 東場の東家にとって、西（3z）は自風でも場風でもない。
        let view = view_with("234567m234p33z99s");
        let options = vec![pon_of("33z")];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    /// 幺九牌が手に無ければ、断幺九が見込めるので鳴く。
    #[test]
    fn a_hand_without_terminals_pons_for_tanyao() {
        let view = view_with("234567m234p55p22s");
        let options = vec![pon_of("55p")];
        let tiles = parse_hand("55p").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }

    /// 幺九牌が手にあれば、断幺九にならないので見送る。
    #[test]
    fn a_hand_with_a_terminal_does_not_pon_for_tanyao() {
        let view = view_with("134567m234p55p22s");
        let options = vec![pon_of("55p")];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    /// 幺九牌そのもののポンは断幺九にならない。
    #[test]
    fn a_terminal_pon_is_never_taken_for_tanyao() {
        let view = view_with("234567m234p11p22s");
        let options = vec![pon_of("11p")];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    /// チーはしない。
    #[test]
    fn a_chi_is_never_taken() {
        let view = view_with("234567m234p56p22s");
        let options = vec![chi_of("56p")];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    /// 槓はしない。
    #[test]
    fn a_kan_is_never_taken() {
        let view = view_with("234567m234p555p2s");
        let options = vec![ActionOption::Kan {
            candidates: vec![KanCandidate::Minkan],
        }];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    /// 鳴ける形が無ければ見送る。
    #[test]
    fn nothing_offered_means_pass() {
        let view = view_with("234567m23478p22s");
        assert_eq!(respond(&view, &[]), CallResponse::Pass);
    }

    /// 幺九牌を含む副露があれば、手が中張牌だけでも断幺九にならない。
    ///
    /// **手牌だけを見ると見落とす。**副露も数える。
    #[test]
    fn a_meld_with_a_terminal_blocks_the_tanyao_pon() {
        // 副露1つぶん短い10枚。すべて中張牌で、ポンの対象 5p を2枚持つ。
        let mut view = view_with("234567m55p22s");
        view.melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("111m").expect("正しい記法"),
            from: Some(Seat::new(1)),
            called_tile: Some(parse_tile("1m").expect("正しい記法")),
        });
        let options = vec![pon_of("55p")];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    /// 中張牌だけの副露なら断幺九は生きている。
    #[test]
    fn a_clean_meld_keeps_the_tanyao_pon() {
        let mut view = view_with("234567m55p22s");
        view.melds.push(Meld {
            kind: MeldKind::Pon,
            tiles: parse_hand("333m").expect("正しい記法"),
            from: Some(Seat::new(1)),
            called_tile: Some(parse_tile("3m").expect("正しい記法")),
        });
        let options = vec![pon_of("55p")];
        let tiles = parse_hand("55p").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }

    /// 自風だけでも鳴く。東場の南家にとって南は自風である。
    #[test]
    fn the_seat_wind_alone_is_enough() {
        let view = view_in(Wind::South, Wind::East, "234567m234p22z99s");
        let options = vec![pon_of("22z")];
        let tiles = parse_hand("22z").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }

    /// 場風だけでも鳴く。東場の南家にとって東は場風である。
    #[test]
    fn the_round_wind_alone_is_enough() {
        let view = view_in(Wind::South, Wind::East, "234567m234p11z99s");
        let options = vec![pon_of("11z")];
        let tiles = parse_hand("11z").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }

    /// どちらでもない風は鳴かない。東場の南家にとって西は客風である。
    #[test]
    fn a_wind_that_is_neither_is_passed() {
        let view = view_in(Wind::South, Wind::East, "234567m234p33z99s");
        let options = vec![pon_of("33z")];
        assert_eq!(respond(&view, &options), CallResponse::Pass);
    }

    /// 同じ局面からは必ず同じ答えが出る。
    #[test]
    fn the_same_view_always_gives_the_same_response() {
        let view = view_with("234567m234p11z99s");
        let options = vec![pon_of("11z")];
        assert_eq!(respond(&view, &options), respond(&view, &options));
    }

    /// 役牌のポンは断幺九の判断より優先する。
    ///
    /// 幺九牌が手にあっても、役牌なら鳴く。
    #[test]
    fn a_value_tile_is_ponned_even_with_terminals() {
        let view = view_with("134567m234p11z99s");
        let options = vec![pon_of("11z")];
        let tiles = parse_hand("11z").expect("正しい記法");
        assert_eq!(
            respond(&view, &options),
            CallResponse::Pon {
                tiles: [tiles[0], tiles[1]]
            }
        );
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test --package mahjong-ai call`
Expected: コンパイルエラー（`respond` が未定義）

- [ ] **Step 3: 実装を書く**

```rust
//! 鳴きの判断。**役が見込めるときだけ鳴く。**
//!
//! 鳴いて役が無ければ和了れない。v1 は判断できる形を2つに絞り、
//! チーと槓はしない。チーは手が安くなりやすく、槓はドラを増やして
//! 他家を利する場面があるが、その見極めができない。

use crate::discard::View;
use protocol::command::{ActionOption, CallResponse};
use protocol::seat::Wind;
use protocol::tile::{Tile, TileKind};

/// 提示された選択肢から1つ選ぶ。鳴かないときは `Pass` を返す。
pub fn respond(view: &View, options: &[ActionOption]) -> CallResponse {
    if options.iter().any(|o| matches!(o, ActionOption::Ron)) {
        return CallResponse::Ron;
    }
    for option in options {
        let ActionOption::Pon { candidates } = option else {
            continue;
        };
        let Some(tiles) = candidates.first() else {
            continue;
        };
        if worth_ponning(view, tiles[0]) {
            return CallResponse::Pon { tiles: *tiles };
        }
    }
    CallResponse::Pass
}

/// その牌をポンする価値があるか。
fn worth_ponning(view: &View, tile: Tile) -> bool {
    // 役牌なら1翻が確定する。幺九牌が手にあっても関係ない。
    if is_value_tile(view, tile.kind()) {
        return true;
    }
    // 断幺九。鳴く牌が中張牌で、**手にも副露にも**幺九牌が無いこと。
    // 副露を見ないと、幺九牌を含む副露があるのに断幺九を当てにしてしまう。
    if tile.kind().is_terminal_or_honor() {
        return false;
    }
    let hand_is_clean = view.hand.iter().all(|t| !t.kind().is_terminal_or_honor());
    let melds_are_clean = view
        .melds
        .iter()
        .flat_map(|m| m.tiles.iter())
        .all(|t| !t.kind().is_terminal_or_honor());
    hand_is_clean && melds_are_clean
}

/// 自風・場風・三元牌のいずれか。
fn is_value_tile(view: &View, kind: TileKind) -> bool {
    if is_dragon(kind) {
        return true;
    }
    wind_of(kind).is_some_and(|wind| wind == view.seat_wind || wind == view.round_wind)
}

/// 三元牌は 5z..7z。字牌は 27 から東南西北・白發中の順に並ぶ。
fn is_dragon(kind: TileKind) -> bool {
    (31..=33).contains(&kind.index())
}

/// 風牌なら、その風を返す。
fn wind_of(kind: TileKind) -> Option<Wind> {
    match kind.index() {
        27 => Some(Wind::East),
        28 => Some(Wind::South),
        29 => Some(Wind::West),
        30 => Some(Wind::North),
        _ => None,
    }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test --package mahjong-ai`
Expected: 17テスト PASS（クレート全体では33件）

- [ ] **Step 5: コミット**

```bash
git add crates/mahjong-ai
git commit -m "feat(ai): 鳴きの判断を実装"
```

---

## Wave 3a 完了の判定

- [ ] `cargo test --workspace` が通る（mahjong-ai 33テスト。discard 10 / safety 6 / call 17）
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] `mahjong-ai` が `mahjong-engine` に依存していない（`Cargo.toml` が無変更）
- [ ] `lib.rs` を編集していない
- [ ] 乱数も時刻も使っていない。同じ `View` と同じ選択肢からは必ず同じ手が出る
- [ ] `View` に他家の手牌も山の中身も入っていない

## Wave 3b へ渡すもの

| 部品 | 卓 Actor での使われ方 |
|---|---|
| `View` | 卓が自分の持つ `RoundState` から、その席が見てよい分だけを詰めて作る |
| `discard::choose` | 手番の CPU へ `RequestAction.options` を渡して打牌を決める |
| `call::respond` | 反応ウィンドウの CPU へ同じく渡して鳴きを決める |
| 決定性 | 同じ局面から同じ手が出るので、卓ごと再現できる |
