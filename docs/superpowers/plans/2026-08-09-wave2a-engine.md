# Wave 2a: mahjong-engine 局の進行 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 配牌から終局までの局を進行させ、イベント列を生成するエンジンを作る。ツモ切りの打ち手で半荘を最後まで回せる状態にする。

**Architecture:** 乱数はシードとして、時間は `now_ms` として**外から注入する**。エンジン自体は決定的で、同じシード・同じコマンド列・同じ時刻列からは必ず同じイベント列が出る。

**Tech Stack:** Rust 1.97.1 / edition 2021 / `protocol` と `mahjong-core` に依存、`sha2` を追加

**設計仕様:** `docs/superpowers/specs/2026-08-08-real-mahjong-design.md`
**作業規約:** `AGENTS.md`

## Global Constraints

- **編集してよいのは次のみ**
  - `crates/mahjong-engine/src/` 配下（**ただし `lib.rs` を除く**）
  - `crates/mahjong-engine/Cargo.toml`（`sha2` の追加のみ）
  - `crates/mahjong-engine/tests/` 配下（新規作成してよい）
- `crates/protocol` と `crates/mahjong-core` は凍結済み。**編集も追加もしない。**足りなければ実装を止めて報告する
- **乱数を直接使わない。** `rand` などの依存を足さず、シードから決定的に生成する
- **時刻を直接読まない。** `Instant::now()` を呼ばず、`now_ms: u64` を引数で受け取る
- 局の進行に関わる定数は `Ruleset` から読む。ハードコードしない
- 完了条件は `cargo test --workspace` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` がすべて通ること

## 時刻と締切の表現

**エンジン内部は卓の開始を 0 とする絶対時刻（`u64`, ms）で扱う。**
`protocol::event::RequestAction.deadline_ms` は `u32` で、**要求発行時点からの残り時間**である。
エンジンが `Event` を組み立てるとき、絶対締切から現在時刻を引いて `u32` へ変換する。

```
deadline_ms（イベント） = (絶対締切 − now_ms) as u32
```

差が `u32::MAX` を超えることは設計上ありえない（最大でも 5000 + 20000 + 500 + lead_in）が、
変換は `try_into` で行い、溢れたら `debug_assert!` で落とす。

## 仕様のうち、ここで実装される決定

| 決定 | 出典 | 実装先 |
|---|---|---|
| 反応ウィンドウの早期確定は「以上」 | 6.4 | `reaction.rs` |
| 非ロンの同順位競合は発生しない（不変条件） | 6.4 | `reaction.rs` |
| 打牌から次のツモまで必ず 350ms 待つ | 6.4 | `reaction.rs` |
| `lead_in` = 前回その席へ要求を送ってから今回まで | 6.2.1 | `round.rs` |
| バンクは基準時間の超過分だけ引く（`lead_in` と通信猶予は課金しない） | 6.2.2 | `timing.rs` |
| `window_id` は対局を通して単調増加 | 5.3.1 | `match_flow.rs` が所有 |
| 槓の演出は宣言が持ち、成立は 0 | 6.1 | 既に `protocol` で確定 |
| シードは局開始時に永続化できる形で持つ | 8.3 | `wall.rs` |

## コーディネータが確定させたルール

仕様に明記が無く、実装者が推測すると割れる箇所を先に決める。いずれも雀魂「金の間」に合わせた。

| 項目 | 決定 |
|---|---|
| 四開槓 | 4つの槓が**2人以上に分かれた**場合のみ流局。1人で4つなら四槓子確定として続行する |
| 四開槓の判定時点 | 4つ目の槓の**打牌に対する反応が解決した後**。それまでに和了があれば和了を優先 |
| 暗槓への槍槓 | **国士無双のみ可**。それ以外の役では暗槓を槍槓できない |
| 西入 | 半荘戦で南4局終了時に誰も `return_score`（30000点）に達していなければ**西入**。西4局終了または誰かが到達した時点で終局 |
| アガリ止め | あり。オーラスで親が和了しトップなら続行しない |
| テンパイ止め | あり。オーラス流局で親がテンパイかつトップなら続行しない |
| 四風連打 | 1巡目に4人が同じ風牌を切ったら流局。鳴きが入っていたら成立しない |
| 四家立直 | 4人目のリーチが**成立**した時点で流局。宣言牌にロンがあればロンを優先 |

## タスクの依存関係

```
1 wall ──┬─→ 3 state ──┬─→ 5 round ──→ 6 kan/riichi ──→ 7 abortive ──→ 9 match ──→ 10 全体
         │              │      ↑
2 reaction ─────────────┘      │
         4 timing ─────────────┘
                        8 invariant ───────────────────────────────────┘
```

**Task 4 と Task 5 は同じ担当が直列で行う。**`timing.rs` を独立ファイルにしたのは、
`round.rs` との同時編集を避けるためである。

---

### Task 1: 決定的な山

**Files:**
- Modify: `crates/mahjong-engine/src/wall.rs`
- Modify: `crates/mahjong-engine/Cargo.toml`（`sha2` の追加のみ）

**Interfaces:**
- Produces:
  - `pub struct Seed([u8; 32])` — `new`, `from_hex`, `to_hex`, `commitment`
  - `pub struct Wall` — `new(seed, rules)`, `draw`, `draw_replacement`, `reveal_dora`, `live_remaining`, `dora_indicators`, `ura_indicators`, `all_tiles`

**王牌の配置を固定する。** これが前稿の欠陥だった。嶺上を引くと生牌の末尾が減るが、
**ドラ表示牌の位置は動かしてはならない**。動かすと既に開示した裏ドラと重複する。

136枚の並びを次のように固定する。生牌は `0..122`、王牌は `122..136`。

| 位置 | 用途 |
|---|---|
| `122, 124, 126, 128, 130` | ドラ表示牌（5枚） |
| `123, 125, 127, 129, 131` | 裏ドラ表示牌（5枚） |
| `132, 133, 134, 135` | 嶺上牌（4枚） |

嶺上を引くと `live_end` が1つ下がり、ツモれる牌が1枚減る。**ドラの添字は `122 + index*2` で固定**し、`live_end` に依存させない。

- [ ] **Step 1: sha2 を追加する**

```bash
cargo add --package mahjong-engine sha2
```

- [ ] **Step 2: 失敗するテストを書く**

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
        assert!(counts.iter().all(|c| *c == 4), "34種が4枚ずつでない");
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
        assert_eq!(reds, vec![4, 13, 22], "赤は5m/5p/5sの各1枚");
    }

    #[test]
    fn the_same_seed_always_produces_the_same_wall() {
        let a: Vec<u8> = Wall::new(&seed(7), &rules()).all_tiles().map(|t| t.encoded()).collect();
        let b: Vec<u8> = Wall::new(&seed(7), &rules()).all_tiles().map(|t| t.encoded()).collect();
        assert_eq!(a, b);
    }

    /// 実装を変えても過去の牌譜を再現できるよう、固定シードの並びを凍結する。
    /// この値が変わるならシャッフル方式の変更であり、牌譜互換が壊れる。
    #[test]
    fn a_fixed_seed_matches_its_golden_vector() {
        let wall = Wall::new(&seed(0xAB), &rules());
        let encoded: Vec<u8> = wall.all_tiles().map(|t| t.encoded()).collect();
        let digest = {
            use sha2::{Digest, Sha256};
            let d = Sha256::digest(&encoded);
            d.iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        // 初回実装時に実際の値を埋める。以後この値を変えてはならない。
        assert_eq!(digest.len(), 64);
        assert_eq!(encoded.len(), 136);
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

    /// **前稿の欠陥。** 嶺上を引いてもドラ表示牌の位置は動かない。
    /// 動かすと既に開示した裏ドラと重複する。
    #[test]
    fn revealing_dora_after_replacements_never_repeats_a_position() {
        let mut wall = Wall::new(&seed(6), &rules());
        let mut positions = Vec::new();
        positions.extend(wall.dora_positions());
        positions.extend(wall.ura_positions());

        for _ in 0..4 {
            wall.draw_replacement();
            wall.reveal_dora();
        }
        positions.clear();
        positions.extend(wall.dora_positions());
        positions.extend(wall.ura_positions());
        positions.extend(wall.replacement_positions());

        let unique: std::collections::HashSet<usize> = positions.iter().copied().collect();
        assert_eq!(
            unique.len(),
            positions.len(),
            "王牌の位置が重複した: {positions:?}"
        );
        assert_eq!(unique.len(), 14, "王牌は14枚ちょうど");
        assert!(unique.iter().all(|p| (122..136).contains(p)), "王牌の範囲外");
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
        assert!(wall.reveal_dora().is_none());
    }

    #[test]
    fn a_seed_commits_to_itself() {
        let s = seed(9);
        assert_eq!(s.commitment().len(), 64);
        assert_eq!(Seed::from_hex(&s.to_hex()).unwrap().commitment(), s.commitment());
        assert_ne!(seed(10).commitment(), s.commitment());
    }
}
```

`dora_positions` / `ura_positions` / `replacement_positions` は検証用に公開する。

- [ ] **Step 3〜5: 実装・検証・コミット**

Run: `cargo test --package mahjong-engine wall`
Expected: 10テスト PASS

`a_fixed_seed_matches_its_golden_vector` は初回実装で実際のハッシュを埋め、
以後**その値を変更してはならない**。変わったらシャッフル方式が変わったということであり、
過去の牌譜が再現できなくなる。

```bash
git commit -m "feat(engine): シードから決定的に作る山を実装"
```

---

### Task 2: 反応ウィンドウの解決

**Files:**
- Modify: `crates/mahjong-engine/src/reaction.rs`

**Interfaces:**
- Produces:
  - `pub enum Priority { Pass, Chi, Pon, Ron }`（宣言順が優先度）
  - `pub struct ReactionWindow` — `open`, `respond`, `resolve`
  - `pub enum Outcome { Pending, Ron(Vec<Seat>), Call { seat, response }, PassAll }`

**解決の規則（仕様 6.4）:**

1. `now_ms < opened_at + min_wait_ms` なら `Pending`
2. 確定している最高優先度を `best` とする
3. 未応答者のうち `best` **以上**を出せる者がいれば `Pending`（締切前のみ）
4. それ以外は確定。ロンなら**ロンした全員**を返す
5. 締切を過ぎた未応答はパス

- [ ] **Step 1: 失敗するテストを書く**

（前稿の11テストをそのまま使う。内容は検証済みである。）

加えて次を足す。

```rust
    /// 槍槓のウィンドウはロンしか受け付けない。
    #[test]
    fn a_chankan_window_offers_only_ron() {
        let w = ReactionWindow::open_chankan(
            2,
            Seat::new(0),
            parse_tile("5s").unwrap(),
            [vec![], ron_only(), vec![], vec![]],
            0,
            5_000,
        );
        assert!(w.is_chankan());
    }

    /// 非ロンの同順位が同時に成立することは牌の枚数上ありえない。
    /// 席順で解決するロジックを書かず、発生しないことを主張する。
    #[test]
    fn non_ron_ties_are_impossible_by_tile_count() {
        // 2人がポンするには各自2枚＋捨て牌1枚で5枚必要だが、牌は1種4枚しかない。
        // ポンと明槓の競合も6枚必要で成立しない。
        // ここでは検査関数が同順位を検出しないことだけを確かめる。
        let mut w = window([vec![], pon_only(), vec![], chi_only()]);
        w.respond(Seat::new(1), CallResponse::Pon { tiles: [parse_tile("3p").unwrap(); 2] })
            .unwrap();
        assert!(w.non_ron_ties().is_empty());
    }
```

- [ ] **Step 2〜5: 実装・検証・コミット**

Run: `cargo test --package mahjong-engine reaction`
Expected: 13テスト PASS

```bash
git commit -m "feat(engine): 反応ウィンドウの早期確定を実装"
```

---

### Task 3: 局の状態

**Files:**
- Modify: `crates/mahjong-engine/src/state.rs`

**Interfaces:**
- Produces:
  - `pub struct Discarded { pub tile: Tile, pub manner: DiscardManner, pub called_by: Option<Seat> }`
  - `pub struct RiichiState { pub step: RiichiStep, pub declared_at: usize, pub ippatsu: bool, pub double: bool }`
  - `pub struct SeatState { ... }`
  - `pub struct RoundState { ... }`

**`HandContext` を組み立てるために必要な状態を、ここで漏れなく持つ。**
前稿はこれが欠落しており、嶺上開花・槍槓・海底・河底・天和・地和が付けられなかった。

| `HandContext` の項目 | 由来する状態 |
|---|---|
| `riichi` / `double_riichi` | `SeatState::riichi`（`step == Accepted` のみ真） |
| `ippatsu` | `RiichiState::ippatsu` |
| `rinshan` | `RoundState::last_draw_source == DeadWall` |
| `chankan` | `RoundState::pending_kan.is_some()` |
| `haitei` | `wall.live_remaining() == 0` かつ ツモ和了 |
| `houtei` | `wall.live_remaining() == 0` かつ ロン和了 |
| `tenhou` | 親 かつ `turn_count == 0` かつ 誰も鳴いていない |
| `chiihou` | 子 かつ その席の初回ツモ かつ 誰も鳴いていない |

追加で持つ状態を明記する。

```rust
pub struct RoundState {
    // ... 前稿の内容 ...
    /// 直前のツモがどこから来たか。嶺上開花の判定に使う。
    pub last_draw_source: DrawSource,
    /// 各席が何回ツモしたか。天和・地和の判定に使う。
    pub draw_count: [u32; 4],
    /// 局を通して誰か1人でも鳴いたか。天和・地和はこれが偽のときのみ。
    pub any_call_made: bool,
    /// 槍槓の受付中かどうか。受付中なら和了は chankan になる。
    pub pending_kan: Option<PendingKan>,
    /// 席ごとの確定した槓の数。四開槓の判定に使う。
    pub kan_count: [u8; 4],
    /// 1巡目に切られた風牌。四風連打の判定に使う。
    pub first_turn_winds: Vec<TileKind>,
}

pub struct SeatState {
    // ... 前稿の内容 ...
    /// 同巡内フリテン。自分のツモで解除される。
    pub passed_this_turn: Vec<TileKind>,
    /// リーチ後にロンを見逃した待ち。**局の終わりまで解除されない。**
    pub permanent_furiten: Vec<TileKind>,
    /// 一度でも鳴かれていない自分の捨て牌だけか（流し満貫の判定）。
    pub nagashi_alive: bool,
}
```

- [ ] **Step 1: 失敗するテストを書く**

前稿の6テストに加え、次を足す。

```rust
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
        assert!(state.seat(seat).passed_this_turn.is_empty(), "同巡内は解除される");
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
    }

    #[test]
    fn every_seat_starts_eligible_for_nagashi() {
        let state = fresh();
        for seat in Seat::ALL {
            assert!(state.seat(seat).nagashi_alive);
        }
    }
```

- [ ] **Step 2〜5: 実装・検証・コミット**

Run: `cargo test --package mahjong-engine state`
Expected: 9テスト PASS

```bash
git commit -m "feat(engine): 局の状態を実装"
```

---

### Task 4: 締切とバンクの計算

**Files:**
- Create: `crates/mahjong-engine/src/timing.rs`
- Modify: `crates/mahjong-engine/src/lib.rs` は編集できないため、**`timing` は `round.rs` から `#[path]` で読み込む**

`lib.rs` を編集せずに新しいモジュールを足すため、`round.rs` の先頭に次を書く。

```rust
#[path = "timing.rs"]
mod timing;
pub use timing::{charge_bank, deadline_for, lead_in_of};
```

**Interfaces:**
- Produces:
  - `pub fn lead_in_of(events: &[Event]) -> u32`
  - `pub fn deadline_for(rules: &Ruleset, now_ms: u64, bank_remaining_ms: u32, lead_in_ms: u32) -> u64`
  - `pub fn charge_bank(rules: &Ruleset, bank_remaining_ms: u32, elapsed_ms: u64, lead_in_ms: u32) -> u32`
  - `pub fn remaining_for_event(absolute_deadline: u64, now_ms: u64) -> u32`

**バンクの課金式（仕様 6.2.2）:**

```
思考に使った時間 = max(0, 実時間 − lead_in − 通信猶予)
引き落とし       = max(0, 思考に使った時間 − 基準思考時間)
```

**通信猶予を引くのを忘れない。** 前稿はこれが抜けており、基準時間内に答えても
500ms がバンクから引かれていた。

- [ ] **Step 1: 失敗するテストを書く**

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

    /// 連続カンの例（仕様 6.2.1）。
    /// 槓の演出は宣言が持ち、成立は0。KanDeclared 1100 + DoraReveal 800
    /// + Draw 250 + Discard 350 = 2500。
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
            Event::DoraReveal { indicator: parse_tile("1z").unwrap() },
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

    /// **通信猶予はバンクから引かない。** 基準時間ちょうど＋猶予で答えても減らない。
    #[test]
    fn the_network_grace_is_not_charged() {
        assert_eq!(charge_bank(&rules(), 20_000, 5_500, 0), 20_000);
        // 猶予を1ms超えた分だけ引かれる
        assert_eq!(charge_bank(&rules(), 20_000, 5_501, 0), 19_999);
    }

    /// 演出を見ていた時間も課金しない。
    #[test]
    fn the_lead_in_is_not_charged() {
        // 実時間 8000、演出 1800、猶予 500 → 思考 5700、超過 700
        assert_eq!(charge_bank(&rules(), 20_000, 8_000, 1_800), 19_300);
    }

    #[test]
    fn overtime_comes_out_of_the_bank() {
        // 実時間 8000、猶予 500 → 思考 7500、超過 2500
        assert_eq!(charge_bank(&rules(), 20_000, 8_000, 0), 17_500);
    }

    #[test]
    fn the_bank_never_goes_below_zero() {
        assert_eq!(charge_bank(&rules(), 1_000, 30_000, 0), 0);
    }

    /// イベントへ載せる残り時間は u32 の相対値。
    #[test]
    fn the_event_carries_a_relative_deadline() {
        assert_eq!(remaining_for_event(35_500, 10_000), 25_500);
        assert_eq!(remaining_for_event(10_000, 10_000), 0);
        // 既に過ぎていたら 0
        assert_eq!(remaining_for_event(9_000, 10_000), 0);
    }
}
```

- [ ] **Step 2〜5: 実装・検証・コミット**

Run: `cargo test --package mahjong-engine timing`
Expected: 9テスト PASS

```bash
git commit -m "feat(engine): 締切とバンクの計算を実装"
```

---

### Task 5: 局の基本進行

**この Task は打牌と鳴きだけを扱う。** 槓・リーチ・途中流局は Task 6/7 で足す。
一度に全部やると検証しきれないため分ける。

**Files:**
- Modify: `crates/mahjong-engine/src/round.rs`

**Interfaces:**
- Produces:
  - `pub enum Phase { AwaitingAction { seat, window_id }, Reacting(ReactionWindow), Finished(RoundOutcome) }`
  - `pub struct RoundEngine` — `new`, `apply`, `tick`, `phase`, `state`, `next_window_id`
  - `pub enum Rejection { NotYourTurn, IllegalCommand, UnknownWindow, AlreadyResponded }`

**`lead_in` の席別カーソル。** `RoundEngine` は席ごとに「前回その席へ `RequestAction` を
送った時点のイベント連番」を持つ。要求を出すとき、そのカーソルから現在までの
イベントを切り出して `lead_in_of` に渡す。

```rust
/// 席ごとの、前回 RequestAction を送った時点のイベント数。
last_request_at: [usize; 4],
```

**ロンの可否は「最低1役」を確認する。** `mahjong_core::score::score()` を
ロン用の `HandContext` で呼び、`Some` が返る席だけをロン候補にする。
振聴の席は候補から外す。

- [ ] **Step 1: 失敗するテストを書く**

前稿の6テストに加え、次を足す。

```rust
    /// 役なしの完成形はロンできない。ドラだけでは和了できない。
    #[test]
    fn a_yakuless_hand_is_not_offered_ron() {
        // 123m 345m 456p 789s ＋ 西の単騎。門前ロンだが役が無い。
        let mut engine = engine_with_hand(Seat::new(1), "123m345m456p789s3z");
        let events = engine.force_discard(Seat::new(0), "3z", 1_000);
        let offered = engine.pending_options(Seat::new(1));
        assert!(
            !offered.iter().any(|o| matches!(o, ActionOption::Ron)),
            "役無しにロンを提示した"
        );
        let _ = events;
    }

    /// 振聴の席にはロンを提示しない。
    #[test]
    fn a_furiten_seat_is_not_offered_ron() {
        let mut engine = engine_with_hand(Seat::new(1), "234567m23478p22s");
        engine.state_mut().seat_mut(Seat::new(1)).river.push(Discarded {
            tile: parse_tile("6p").unwrap(),
            manner: DiscardManner::Tsumogiri,
            called_by: None,
        });
        engine.force_discard(Seat::new(0), "6p", 1_000);
        let offered = engine.pending_options(Seat::new(1));
        assert!(!offered.iter().any(|o| matches!(o, ActionOption::Ron)));
    }

    /// lead_in は席ごとに数える。他家への要求を挟んでも混ざらない。
    #[test]
    fn lead_in_is_measured_per_seat() {
        let mut engine = start().0;
        // 席0が打牌 → 席1へ要求 → 席1が打牌 → 席2へ要求
        // 席2の lead_in には席1への要求より前のイベントを含めない。
        let first = engine.lead_in_for(Seat::new(2));
        engine.force_discard(Seat::new(0), "1m", 1_000);
        engine.force_discard(Seat::new(1), "1m", 2_000);
        let second = engine.lead_in_for(Seat::new(2));
        assert!(second >= first);
        // 席1が2回要求を受けた場合、2回目の lead_in は1回目以降だけを数える
        let before = engine.lead_in_for(Seat::new(1));
        engine.force_discard(Seat::new(2), "1m", 3_000);
        engine.force_discard(Seat::new(3), "1m", 4_000);
        let after = engine.lead_in_for(Seat::new(1));
        assert!(after < before + 10_000, "区間が累積している疑い");
    }

    /// 応答は現在開いているウィンドウの ID と一致しなければ捨てる。
    #[test]
    fn a_response_with_a_stale_window_id_is_rejected() {
        let mut engine = start().0;
        engine.force_discard(Seat::new(0), "1m", 1_000);
        let current = engine.current_window_id().expect("開いている");
        let stale = protocol::command::Command::CallResponse {
            window_id: current - 1,
            response: protocol::command::CallResponse::Pass,
        };
        assert!(matches!(
            engine.apply(Seat::new(1), stale, 1_100),
            Err(Rejection::UnknownWindow)
        ));
    }
```

`engine_with_hand` / `force_discard` / `pending_options` / `lead_in_for` /
`current_window_id` はテスト補助として `#[cfg(test)]` で公開する。

- [ ] **Step 2〜5: 実装・検証・コミット**

Run: `cargo test --package mahjong-engine round`
Expected: 10テスト PASS

```bash
git commit -m "feat(engine): 局の基本進行を実装"
```

---

### Task 6: 槓とリーチ

**Files:**
- Modify: `crates/mahjong-engine/src/round.rs`

**槓の順序:**

```
KanDeclared → 槍槓ウィンドウ（加槓・暗槓のみ）→ ロンが無ければ
Call(槓) → DoraReveal → 嶺上ツモ（Draw source=DeadWall）→ 打牌へ
```

- 明槓（他家の打牌から）は槍槓を受け付けない。`KanDeclared` の直後に `Call`
- 加槓は全員がロン可能なら槍槓を受け付ける
- **暗槓は国士無双でのみ槍槓できる**（コーディネータ決定）
- 新ドラは槓が成立した直後にめくる

**リーチの二段階:**

```
Riichi{Declare} → Discard（宣言牌）→ 反応ウィンドウ
  → 誰かが鳴いた／ロンした  → Accepted を出さない。1000点も取らない
  → 誰も反応しなかった      → Riichi{Accepted} + 1000点を供託へ
```

**一発の消滅条件:**

- 宣言者の次の打牌が完了した時点で消える
- **誰かが鳴いた（チー・ポン・明槓・暗槓・加槓）時点で全席の一発が消える**

- [ ] **Step 1: 失敗するテストを書く**

```rust
    /// 加槓は宣言と成立の間に槍槓を受け付ける。
    #[test]
    fn a_kakan_opens_a_chankan_window() { /* ... */ }

    /// 明槓は槍槓を受け付けない。
    #[test]
    fn a_minkan_goes_straight_to_completion() { /* ... */ }

    /// 暗槓を槍槓できるのは国士無双だけ。
    #[test]
    fn an_ankan_can_only_be_robbed_by_kokushi() { /* ... */ }

    /// 新ドラは槓の成立直後にめくる。
    #[test]
    fn a_new_dora_is_revealed_right_after_the_kan_completes() { /* ... */ }

    /// 宣言牌が鳴かれたらリーチは成立せず、1000点も取らない。
    #[test]
    fn a_called_riichi_tile_leaves_the_riichi_pending() { /* ... */ }

    /// 宣言牌でロンされたらリーチは成立しない。
    #[test]
    fn a_ronned_riichi_tile_leaves_the_riichi_pending() { /* ... */ }

    /// 誰も反応しなければリーチが成立し、1000点が供託へ移る。
    #[test]
    fn an_unanswered_riichi_tile_accepts_the_riichi() { /* ... */ }

    /// 鳴きが入ると全席の一発が消える。
    #[test]
    fn any_call_clears_ippatsu_for_everyone() { /* ... */ }

    /// 一発は次の打牌が終われば消える。
    #[test]
    fn ippatsu_expires_after_the_next_discard() { /* ... */ }

    /// 嶺上ツモでの和了は rinshan、槍槓での和了は chankan が立つ。
    #[test]
    fn the_hand_context_reflects_how_the_win_happened() { /* ... */ }
```

- [ ] **Step 2〜5: 実装・検証・コミット**

```bash
git commit -m "feat(engine): 槓とリーチの段階処理を実装"
```

---

### Task 7: 途中流局と荒牌平局

**Files:**
- Modify: `crates/mahjong-engine/src/round.rs`

**判定の順序と時点:**

| 流局 | 成立条件 | 判定時点 |
|---|---|---|
| 九種九牌 | 配牌＋第一ツモに幺九牌が9種以上、その席が宣言 | 第一ツモ直後の打牌前。鳴きが入っていたら不可 |
| 四風連打 | 1巡目に4人が同じ風牌を切った | 4人目の打牌への反応が解決した後 |
| 四家立直 | 4人目のリーチが**成立**した | `Riichi{Accepted}` の直後 |
| 四開槓 | 4つの槓が**2人以上**に分かれた | 4つ目の槓の打牌への反応が解決した後 |
| 三家和 | 3人が同時にロン | 反応ウィンドウの解決時 |
| 荒牌平局 | 生牌が尽きた | 最後の打牌への反応が解決した後 |

**和了が優先される。** 途中流局の判定時点でロンが成立していれば、和了を採る。

**荒牌平局の精算:** 形式テンパイを認め、ノーテン罰符は合計3000点。
テンパイ者と ノーテン者の人数で分ける（1人テンパイなら3000、2人なら1500ずつ、3人なら1000ずつ）。
全員テンパイまたは全員ノーテンなら移動なし。

**流し満貫:** 自分の捨て牌がすべて幺九牌で、かつ一度も鳴かれていない。
荒牌平局時に判定し、テンパイ料より優先する。成立者は満貫を得る（親8000／子4000相当）。

- [ ] **Step 1: 失敗するテストを書く**（各流局につき成立・不成立の両方）

- [ ] **Step 2〜5: 実装・検証・コミット**

```bash
git commit -m "feat(engine): 途中流局と荒牌平局を実装"
```

---

### Task 8: 不変条件

**Files:**
- Modify: `crates/mahjong-engine/src/invariant.rs`

**Interfaces:**
- `pub fn assert_tiles_conserved(state: &RoundState)`
- `pub fn assert_scores_conserved(before: &[i32; 4], after: &[i32; 4], sticks_delta: i32)`
- `pub fn assert_no_simultaneous_non_ron(window: &ReactionWindow)`

- [ ] **Step 1〜5: TDD で実装**

```bash
git commit -m "feat(engine): 不変条件の検査を実装"
```

---

### Task 9: 半荘の進行

**Files:**
- Modify: `crates/mahjong-engine/src/match_flow.rs`

**`MatchEngine` が `window_id` のカウンタを所有する。** 局を終えるとき
`RoundEngine` から次の値を回収し、次局へ渡す。これがないと局ごとに1へ戻り、
遅延した旧局の応答を受理してしまう。

**連荘の規則（和了者の集合で判断する）:**

| 局の結果 | 親 | 本場 | 理由 |
|---|---|---|---|
| 和了者に親が**含まれる** | 続投 | +1 | `DealerWin` |
| 和了者に親が含まれない | 流れる | 0 | `DealerLoss` |
| 荒牌平局・親テンパイ | 続投 | +1 | `DealerTenpai` |
| 荒牌平局・親ノーテン | 流れる | +1 | `DealerLoss` |
| 途中流局 | 続投 | +1 | `AbortiveDraw` |
| 流し満貫の成立者に親が**含まれる** | 続投 | +1 | `NagashiMangan` |
| 流し満貫の成立者に親が含まれない | 流れる | +1 | `NagashiMangan` |

**ダブロンで親が含まれる場合は続投である。**集合で判断すれば迷いがない。

**終局条件:**

- 東風戦: 東4局終了。ただし西入と同じ条件で東家が増える場合がある → **東風戦に西入は無い**。東4局で終局
- 半荘戦: 南4局終了時に誰かが `return_score` 以上なら終局。誰も達していなければ**西入**し、西4局終了または誰かが到達で終局
- **飛び:** 誰かが0点未満になった時点で即終局
- **アガリ止め:** オーラスで親が和了し、かつ親がトップなら続行しない
- **テンパイ止め:** オーラス荒牌平局で親がテンパイし、かつ親がトップなら続行しない

- [ ] **Step 1: 失敗するテストを書く**（上の2表をそのままテストにする）

```rust
    /// window_id は局をまたいで増え続ける。
    #[test]
    fn window_ids_keep_increasing_across_two_rounds() {
        let mut m = MatchEngine::new(/* ... */);
        let first_round_last = m.run_round_to_completion();
        let second_round_first = m.current_round().next_window_id();
        assert!(
            second_round_first > first_round_last,
            "局をまたいで window_id が戻った"
        );
    }

    /// 南4局で誰も30000に達していなければ西入する。
    #[test]
    fn a_hanchan_extends_to_the_west_round_when_nobody_reaches_the_return_score() { /* ... */ }

    /// 誰かが30000以上なら南4局で終局する。
    #[test]
    fn a_hanchan_ends_at_south_four_when_someone_reaches_the_return_score() { /* ... */ }

    /// 東風戦に西入は無い。
    #[test]
    fn a_tonpuu_never_extends() { /* ... */ }

    /// 誰かが0点未満になったら即終局。
    #[test]
    fn going_below_zero_ends_the_match_immediately() { /* ... */ }

    /// オーラスで親が和了しトップなら続行しない。
    #[test]
    fn the_dealer_may_stop_after_winning_the_final_round_in_the_lead() { /* ... */ }
```

- [ ] **Step 2〜5: 実装・検証・コミット**

```bash
git commit -m "feat(engine): 半荘の進行と連荘規則を実装"
```

---

### Task 10: 通し対局と決定性の検証

**Files:**
- Create: `crates/mahjong-engine/tests/full_game.rs`

**決定性のテストは、シードだけでなく入力トレース全体を固定する。**
時刻を外から注入する設計なので、`now_ms` の列も同じでなければ同じ結果にならない。

- [ ] **Step 1: テストを書く**

```rust
//! ツモ切りだけの打ち手で半荘を最後まで回す。

/// 入力トレース。シード・コマンド・時刻をすべて含む。
struct Trace {
    seeds: Vec<Seed>,
    steps: Vec<(Seat, Command, u64)>,
}

#[test]
fn a_whole_match_runs_to_completion() { /* 無限ループにならないこと */ }

#[test]
fn the_same_trace_produces_the_same_event_stream() {
    // seed・command・now_ms をすべて固定し、2回走らせて全イベントが一致すること。
}

#[test]
fn scores_are_conserved_across_the_whole_match() {
    // 供託を含めた合計が常に 100000 のまま。
}

#[test]
fn two_hundred_matches_never_panic() {
    // シードを変えて200半荘。不変条件が一度も破れないこと。
    // 前稿は1000だったが、向聴計算が 3.7µs であることを踏まえ現実的な数へ落とす。
}
```

- [ ] **Step 2〜4: 実装・検証・コミット**

```bash
git commit -m "test(engine): 通し対局と決定性を検証"
```

---

## Wave 2a 完了の判定

- [ ] `cargo test --workspace` が通る
- [ ] `cargo clippy --all-targets -- -D warnings` が通る
- [ ] `cargo fmt --check` が通る
- [ ] ツモ切りの打ち手で半荘が最後まで回る
- [ ] 同じ入力トレース（シード・コマンド・時刻）から同じイベント列が出る
- [ ] 固定シードの golden vector が一致する
- [ ] 牌136枚と点棒合計が全イベントで保たれる
- [ ] 王牌14枚の位置が重複しない
- [ ] 途中流局4種すべてが生成されうる
- [ ] `protocol` と `mahjong-core` を編集していない
- [ ] `rand` を依存に足していない（`sha2` のみ）
- [ ] `Instant::now()` を呼んでいない
- [ ] `lib.rs` を編集していない

## Wave 2c（server）へ渡す境界

Wave 2c の着手前に、engine と server の責務をここで固定する。

| 項目 | 所有者 |
|---|---|
| 時刻の基準 | server。卓の開始を 0 とする絶対時刻（ms）を `now_ms` で渡す |
| `tick` を呼ぶ契機 | server。最も近い締切に合わせてタイマーを張る |
| イベントの `seq` | server。engine は `Event` だけを返し、連番は付けない |
| 締切の表現 | engine 内部は絶対（`u64`）。`Event` へ載せるとき相対（`u32`）へ変換 |
| 拒否時の合法手再送 | server。`Rejection` を受けたら `engine.pending_options(seat)` を引いて送り直す |
| 切断中の自動打牌 | server。締切超過で engine へツモ切りコマンドを送る |
| CPU 代打ちへの切り替え | server |
