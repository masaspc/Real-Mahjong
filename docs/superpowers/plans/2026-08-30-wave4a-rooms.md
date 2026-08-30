# 人と打てる卓（部屋・席・トークン）実装計画

> **エージェント向け:** 1タスク＝1コミット。コミットまでは自動、マージと push は人間が判断する。
> `Files:` に無いファイルは触らない。既存テストの期待値を緩めない。

**Goal:** 部屋コードを共有すれば同じ卓に2人以上の人が座れるようにし、同時に「卓 id を知っていれば座れる」穴を塞ぐ。

**Architecture:** `spawn` の手前に「部屋」を挟む。部屋は普通の HTTP JSON でやり取りし、凍結中の `crates/protocol` には触れない。サーバ発行のトークンが席の証明になり、WebSocket はトークンから卓と席を引く。

**Tech Stack:** Rust / axum / tokio、TypeScript / vite / vitest。

## Global Constraints

- `crates/protocol` は凍結。変更しない。必要に見えたら報告して止まる
- 名前は他人が入力した文字列。画面へは必ず `textContent` で入れる
- サーバのコメントと文書は日本語。「何をしたか」ではなく「なぜそうしたか」を書く
- `git add -A` は使わない。対象を明示する
- 仕様は `docs/superpowers/specs/2026-08-30-rooms-and-seating-design.md`

---

### Task 1: 部屋の素材（コード・トークン・名前）

`matchmaking.rs` を `rooms.rs` へ改め、部屋を作るのに要る純粋関数だけを先に置く。
この時点では既存の `Tables` はそのまま動かす。

**Files:**
- Rename: `crates/server/src/matchmaking.rs` → `crates/server/src/rooms.rs`（`git mv`）
- Modify: `crates/server/src/lib.rs`（`pub mod matchmaking;` → `pub mod rooms;`）
- Modify: `crates/server/src/bin/serve.rs`（`use` の付け替えのみ）

**Interfaces:**
- Produces: `Code(String)` / `Token(String)` / `fn new_code() -> Code` / `fn new_token() -> Token` / `fn clean_name(&str) -> String`

- [ ] **Step 1: 失敗する試験を書く**

`0O1I` を含まないこと、6文字であること、1000本引いて重複しないこと。
名前は前後空白を落とし、制御文字を落とし、12文字で切り、空なら「プレイヤー」。

- [ ] **Step 2: 実装する**

字母は `23456789ABCDEFGHJKLMNPQRSTUVWXYZ`（32字）。乱数は `SeedSource` と同じ OS 乱数。
トークンは32桁の16進。

- [ ] **Step 3: 通す**

Run: `cargo test -p server`
Expected: PASS（既存の卓の試験を含め全件）

- [ ] **Step 4: コミット**

```bash
git add crates/server/src/rooms.rs crates/server/src/lib.rs crates/server/src/bin/serve.rs
git commit -m "refactor(server): 卓の台帳を部屋の台帳へ改める土台を置く"
```

---

### Task 2: 部屋の台帳（作る・入る・覗く・始める・席を引く・掃く）

> **計画からの逸脱（2026-08-30）:** 当初 Task 2〜5 に分けていたが、`RoomState::Playing`
> は Task 4 まで、`Room::find` は Task 3 まで構築されないため、途中のコミットが
> `dead_code` で clippy に落ちる。人工的な `#[allow]` を挟むより、1つの塊として
> 出す方が正しい。Task 3〜5 の内容と検証はこのタスクに畳んだ。

**Files:**
- Modify: `crates/server/src/rooms.rs`

**Interfaces:**
- Consumes: Task 1 の `Code` / `Token` / `clean_name`
- Produces: `Rooms::new()` / `Rooms::create(name) -> (Code, Token)` / `Rooms::join(&Code, name) -> Result<Token, JoinError>` / `enum JoinError { NoSuchRoom, Full, AlreadyStarted }`

- [ ] **Step 1: 失敗する試験を書く**

作った人が部屋主。4人で満室、5人目は `Full`。無いコードは `NoSuchRoom`。

- [ ] **Step 2: 実装する**

`Room { members: Vec<Member>, state: RoomState, touched_ms: u64 }`、
`Member { name: String, token: Token, host: bool, seen_ms: u64 }`。
トークンからコードを引く索引 `HashMap<Token, Code>` を併せ持つ。

- [ ] **Step 3: 通す**

Run: `cargo test -p server rooms`
Expected: PASS

- [ ] **Step 4: コミット**

```bash
git add crates/server/src/rooms.rs
git commit -m "feat(server): 部屋を作って入れるようにする"
```

---

### Task 3: 待合を覗く

**Files:**
- Modify: `crates/server/src/rooms.rs`

**Interfaces:**
- Produces: `Rooms::look(&Token, now_ms) -> Result<Lobby, LookError>`、
  `Lobby { code, state: "waiting"|"playing", you: MemberView, members: Vec<MemberView>, can_start: bool }`

- [ ] **Step 1: 失敗する試験を書く**

覗いた本人の `seen_ms` が更新されること。`present` は10秒以内。
`can_start` は部屋主なら真。部屋主の最後の応答から30秒が過ぎていれば他人も真。
知らないトークンは `LookError::BadToken`。

- [ ] **Step 2: 実装して通す**

Run: `cargo test -p server rooms`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add crates/server/src/rooms.rs
git commit -m "feat(server): 待合の様子を返す。部屋主が消えても詰まないようにする"
```

---

### Task 4: 開始 —— 席を配って卓を立てる

**Files:**
- Modify: `crates/server/src/rooms.rs`

**Interfaces:**
- Consumes: `session::spawn` / `session::SeedSource` / `table::Occupant`
- Produces: `Rooms::start(&Token, now_ms) -> Result<(), StartError>`

- [ ] **Step 1: 失敗する試験を書く**

- 人2人で始めると `Occupant` は人2・CPU2 になる
- 100回始めて、部屋主の席が0に偏らない（席決めが効いている）
- 人どうしが同じ席に入らない
- 部屋主でない者の開始は `StartError::NotHost`
- 二度目の開始は `StartError::AlreadyStarted`

- [ ] **Step 2: 実装する**

`[0,1,2,3]` を Fisher–Yates で混ぜ、入室順に配る。残りを `Occupant::Cpu(PlayerId("CPU"))`。
`PlayerId` には整えた名前をそのまま入れる（`MatchStart.players` に載り、盤面に出る）。

- [ ] **Step 3: 通す**

Run: `cargo test -p server rooms`
Expected: PASS

- [ ] **Step 4: コミット**

```bash
git add crates/server/src/rooms.rs
git commit -m "feat(server): 開始で席を配り、空席を CPU が埋める"
```

---

### Task 5: トークンから席を引く・掃除

**Files:**
- Modify: `crates/server/src/rooms.rs`

**Interfaces:**
- Produces: `Rooms::seat_of(&Token) -> Option<(TableHandle, Seat)>` / `Rooms::sweep(now_ms) -> usize`

- [ ] **Step 1: 失敗する試験を書く**

- 待合のうちは `seat_of` は `None`
- 開始後は自分の席が返る。**他人のトークンでは他人の席しか返らない**
- 卓が閉じた部屋は掃かれる
- 最後の操作から30分の `waiting` は掃かれる。29分は残る

- [ ] **Step 2: 実装して通す**

Run: `cargo test -p server rooms`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add crates/server/src/rooms.rs
git commit -m "feat(server): トークンから席を引き、古い部屋を掃く"
```

---

### Task 6: HTTP の口

**Files:**
- Create: `crates/server/src/http.rs`
- Modify: `crates/server/src/lib.rs`

**Interfaces:**
- Produces: `http::router(rooms) -> axum::Router`

- [ ] **Step 1: 失敗する試験を書く**

`tower::ServiceExt::oneshot` で叩く。
`POST /api/rooms` が 200 とコード、`join` の満室が 409、`GET` のトークン無しが 401、
`start` の非部屋主が 403。**エラー本文の `error` 文字列まで確かめる**（画面が分岐に使う）。

- [ ] **Step 2: 実装する**

トークンは `X-Mahjong-Token` ヘッダで受ける。クエリに置くとアクセスログに席の証明が残る。

- [ ] **Step 3: 通す**

Run: `cargo test -p server http`
Expected: PASS

- [ ] **Step 4: コミット**

```bash
git add crates/server/src/http.rs crates/server/src/lib.rs
git commit -m "feat(server): 部屋の HTTP の口を開ける"
```

---

### Task 7: WebSocket をトークンで結ぶ

**Files:**
- Modify: `crates/server/src/http.rs`
- Modify: `crates/server/src/bin/serve.rs`

- [ ] **Step 1: 失敗する試験を書く**

トークンの無い `/ws` は接続を張らずに閉じる。知らないトークンも閉じる。

- [ ] **Step 2: 実装する**

`?table=` と `const YOU: u8 = 0` を消す。`?token=` から `seat_of` で卓と席を引く。
WS だけクエリを使うのは、ブラウザが WS 要求にヘッダを付けられないため。理由をコメントに残す。
`serve.rs` は起動だけの薄い binary にする。

- [ ] **Step 3: 通す**

Run: `cargo test -p server && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: PASS / 警告なし

- [ ] **Step 4: コミット**

```bash
git add crates/server/src/http.rs crates/server/src/bin/serve.rs
git commit -m "feat(server): 席をトークンで証明する。卓 id で座れる穴を塞ぐ"
```

---

### Task 8: 画面から部屋を叩く

**Files:**
- Create: `apps/web/src/lobby/api.ts`
- Create: `apps/web/src/lobby/api.test.ts`

**Interfaces:**
- Produces: `createRoom(name)` / `joinRoom(code, name)` / `lookRoom(token)` / `startRoom(token)`、
  `RoomError`（`no_such_room` 等を持つ）、`saveToken/loadToken/clearToken`

- [ ] **Step 1: 失敗する試験を書く**

`fetch` を差し替え、409 の `room_full` が `RoomError` になること、
トークンが `X-Mahjong-Token` ヘッダで飛ぶこと。

- [ ] **Step 2: 実装して通す**

Run: `pnpm --filter @real-mahjong/web test -- lobby`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add apps/web/src/lobby/api.ts apps/web/src/lobby/api.test.ts
git commit -m "feat(web): 部屋の口を叩く客体を置く"
```

---

### Task 9: ロビーと待合の画面

**Files:**
- Create: `apps/web/src/lobby/screen.ts`
- Create: `apps/web/src/lobby/screen.test.ts`
- Create: `apps/web/src/lobby/lobby.css`
- Modify: `apps/web/src/main.ts`

- [ ] **Step 1: 失敗する試験を書く**

- 待合に4枠出る。空きは「空き（CPU が入ります）」
- 部屋主にだけ「開始」が出る
- `<script>` を名前にしても要素が生えない（`textContent` で入れている証拠）
- `state` が `playing` になったら卓へ移る合図が出る

- [ ] **Step 2: 実装する**

`#/` がロビー、`#/room/<code>` が待合。トークンが残っていれば待合へ戻す。
「ひとりで打つ」は部屋を作って即 `start` する。

- [ ] **Step 3: 通す**

Run: `pnpm --filter @real-mahjong/web test && pnpm --filter @real-mahjong/web typecheck && pnpm --filter @real-mahjong/web build`
Expected: PASS

- [ ] **Step 4: コミット**

```bash
git add apps/web/src/lobby apps/web/src/main.ts
git commit -m "feat(web): ロビーと待合を作る"
```

---

### Task 10: 盤面に名前を出す

**Files:**
- Modify: `apps/web/src/game/state.ts`
- Modify: `apps/web/src/game/state.test.ts`
- Modify: `apps/web/src/ui/board.ts`
- Modify: `apps/web/src/ui/board.test.ts`

- [ ] **Step 1: 失敗する試験を書く**

`MatchStart` の `players` が `GameState.players` に入ること。
点棒の行と終局の表が `席0` ではなく名前を出すこと。名前は `textContent` で入ること。

- [ ] **Step 2: 実装して通す**

Run: `pnpm --filter @real-mahjong/web test`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add apps/web/src/game/state.ts apps/web/src/game/state.test.ts apps/web/src/ui/board.ts apps/web/src/ui/board.test.ts
git commit -m "feat(web): 席番号ではなく名前を出す"
```

---

### Task 11: 2人が別の席で同時に打てることを確かめる

**Files:**
- Modify: `crates/server/src/rooms.rs`（結合試験を末尾に足す）

- [ ] **Step 1: 失敗する試験を書く**

部屋を作って2人入れ、開始し、2つのトークンでそれぞれ `attach`。
**別々の席に着き、配牌が異なり、互いの手牌が相手の `Deal` に現れない**ことを確かめる。
ここが視界フィルタの最後の砦になる。

- [ ] **Step 2: 通す**

Run: `cargo test -p server && cargo clippy --all-targets -- -D warnings`
Expected: PASS / 警告なし

- [ ] **Step 3: コミット**

```bash
git add crates/server/src/rooms.rs
git commit -m "test(server): 2人が別の席で打て、互いの手が見えないことを確かめる"
```
