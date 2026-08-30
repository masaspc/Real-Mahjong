# 牌譜（保存・一覧・再生）実装計画

> **エージェント向け:** 1タスク＝1コミット。コミットまでは自動、マージと push は人間が判断する。
> `Files:` に無いファイルは触らない。既存テストの期待値を緩めない。

**Goal:** 打った半荘を SQLite に残し、同卓した人が自分の席の視界で演出付きに見直せるようにする。

**Architecture:** サーバの真実（`Event`）を局の境界で塊にして SQLite へ書く。読み出すたびに `project()` を通すので、視界フィルタの抜け道が生まれない。書き手は別の tokio task にして、卓を止めない。

**Tech Stack:** Rust / rusqlite（bundled）/ flate2、TypeScript / vite / vitest。

## Global Constraints

- `crates/protocol` は凍結。変更しない。必要に見えたら報告して止まる
- **射影後ではなくサーバの真実を保存する。**読み出しで `project()` を通す
- トークンは生で持たない。SHA-256 を入れる
- 書き込みの失敗で対局を止めない
- `git add -A` は使わない。対象を明示する
- 仕様は `docs/superpowers/specs/2026-08-30-records-design.md`

---

### Task 1: 牌譜の倉

SQLite を開き、表を作り、対局の見出しと塊を出し入れするところまで。**まだ誰も使わない。**

**Files:**
- Modify: `crates/server/src/persistence.rs`（いまは1行の空）
- Modify: `crates/server/Cargo.toml`

**Interfaces:**
- Produces: `Store::open(path) -> Result<Store>` / `Store::open_in_memory()` /
  `Store::begin_match(head: MatchHead)` / `Store::append(record_id, chunk: EventChunk)` /
  `Store::finish(record_id, ended_ms, result_json)` /
  `Store::seat_of(record_id, token_hash) -> Option<u8>` /
  `Store::events(record_id) -> Vec<EventEnvelope>` /
  `Store::list(player_key) -> Vec<MatchHead>`

- [ ] **Step 1: 失敗する試験を書く**

- 表が無いところから開いても作られる。2度開いても壊れない
- 塊を3つ足して読み戻すと `seq` が連番のまま繋がる
- **CPU の席には `token_hash` も `player_key` も入らない**
- 知らないトークンでは席が返らない
- 終局を書くと `ended_ms` と `result` が埋まる
- `list` は新しい順

- [ ] **Step 2: 実装する**

`rusqlite = { version = "0.32", features = ["bundled"] }`（**bundled にする。**
入っている SQLite の版に左右されると、手元と CI で挙動が変わる）。
`flate2` で塊を gzip する。

- [ ] **Step 3: 通す**

Run: `cargo test -p server persistence`
Expected: PASS

- [ ] **Step 4: コミット**

```bash
git add crates/server/src/persistence.rs crates/server/Cargo.toml Cargo.lock
git commit -m "feat(server): 牌譜の倉を作る"
```

---

### Task 2: 書き手を別の task にする

**Files:**
- Modify: `crates/server/src/persistence.rs`

**Interfaces:**
- Produces: `Scribe::spawn(Store) -> Scribe` / `Scribe::begin/append/finish`（いずれも待たずに戻る）

- [ ] **Step 1: 失敗する試験を書く**

- 投げたものが順に書かれる
- **倉が壊れていても投げ側は止まらない**（書けない Store を渡して、`append` が返ることを確かめる）
- 溢れたら古いものを捨てるのではなく、その対局の牌譜を諦める（欠けたことが分かる印を残す）

- [ ] **Step 2: 実装して通す**

Run: `cargo test -p server persistence`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add crates/server/src/persistence.rs
git commit -m "feat(server): 牌譜の書き込みで卓を止めない"
```

---

### Task 3: 卓から局の境界で渡す

**Files:**
- Modify: `crates/server/src/session.rs`
- Modify: `crates/server/src/table.rs`

**Interfaces:**
- Consumes: `Scribe`（Task 2）
- Produces: `Table::log_since(seq) -> &[EventEnvelope]`、`spawn` が `Option<Scribe>` を受ける

- [ ] **Step 1: 失敗する試験を書く**

- 局が3つ終われば塊が3つ増える
- 終局まで打つと `ended_ms` が埋まる
- **`Scribe` を渡さなくても卓は動く**（既存の試験がそのまま通る）

- [ ] **Step 2: 実装して通す**

Run: `cargo test -p server`
Expected: PASS（既存の108件を含む）

- [ ] **Step 3: コミット**

```bash
git add crates/server/src/session.rs crates/server/src/table.rs
git commit -m "feat(server): 局の境界で牌譜を書き出す"
```

---

### Task 4: browser の鍵と、部屋から倉への受け渡し

**Files:**
- Modify: `crates/server/src/rooms.rs`
- Modify: `crates/server/src/http.rs`

**Interfaces:**
- Produces: `Rooms::new_with(scribe)`、`create/join` が `player_key` を受ける

- [ ] **Step 1: 失敗する試験を書く**

- 部屋を作って開始すると `records` と `record_seats` の行ができる
- **人の席にだけトークンのハッシュと鍵が入り、CPU の席には入らない**
- 鍵を送らなくても対局はできる（一覧に出ないだけ）

- [ ] **Step 2: 実装して通す**

Run: `cargo test -p server`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add crates/server/src/rooms.rs crates/server/src/http.rs
git commit -m "feat(server): 卓が立つときに牌譜の見出しを作る"
```

---

### Task 5: 牌譜を引く口

**Files:**
- Modify: `crates/server/src/http.rs`

**Interfaces:**
- Produces: `GET /api/records` / `GET /api/records/{id}` / `GET /api/records/{id}/events`

- [ ] **Step 1: 失敗する試験を書く**

- 自分のトークンなら自分の席の視界で返る
- **他人のトークンでは他人の席の視界しか返らない**（自分の手が見えない）
- 証明が無ければ 401
- **存在しない id と資格の無い id が同じ答えになる**
- 一覧は `X-Mahjong-Player` で引ける。他人の鍵では他人のものが出ない

- [ ] **Step 2: 実装して通す**

Run: `cargo test -p server && cargo clippy --all-targets -- -D warnings`
Expected: PASS / 警告なし

- [ ] **Step 3: コミット**

```bash
git add crates/server/src/http.rs
git commit -m "feat(server): 牌譜を席の視界で返す"
```

---

### Task 6: 保存した列が配ったものと一致することを確かめる

**Files:**
- Modify: `crates/server/src/persistence.rs`（結合試験を末尾に）

- [ ] **Step 1: 失敗する試験を書く**

半荘を最後まで打たせ、対局中に席へ配られた `ClientEventEnvelope` を全部控える。
終局後に牌譜を同じ席で引き、**1件ずつ同じであること**を確かめる。

**ここが牌譜の正しさの最後の砦である。**保存も射影も通っているのに、
配ったものと違うものが残る、という壊れ方をこの1本だけが捕まえる。

- [ ] **Step 2: 通す**

Run: `cargo test -p server`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add crates/server/src/persistence.rs
git commit -m "test(server): 牌譜が配ったものと1件ずつ一致することを確かめる"
```

---

### Task 7: 画面から牌譜を引く

**Files:**
- Create: `apps/web/src/records/api.ts`
- Create: `apps/web/src/records/api.test.ts`
- Modify: `apps/web/src/lobby/api.ts`（鍵の出し入れを足す）

**Interfaces:**
- Produces: `playerKey()` / `listRecords()` / `readRecord(id)` / `recordEvents(id)`

- [ ] **Step 1: 失敗する試験を書く**

鍵が無ければ作って `localStorage` に置く。2度呼んでも同じ鍵。
`X-Mahjong-Player` ヘッダで飛ぶ。

- [ ] **Step 2: 実装して通す**

Run: `pnpm --filter @real-mahjong/web test -- records`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add apps/web/src/records apps/web/src/lobby/api.ts
git commit -m "feat(web): 牌譜の口を叩く客体を置く"
```

---

### Task 8: 再生の画面

**Files:**
- Create: `apps/web/src/records/screen.ts`
- Create: `apps/web/src/records/screen.test.ts`
- Create: `apps/web/src/records/records.css`
- Modify: `apps/web/src/main.ts`
- Modify: `apps/web/src/lobby/screen.ts`（ロビーに「牌譜を見る」を足す）

- [ ] **Step 1: 失敗する試験を書く**

- 一覧が新しい順に出る。対局が無ければその旨を言う
- 目次に局の頭が並ぶ（`round_start` の位置）
- 速さの切り替えが `EffectPlayer` の倍率に届く

- [ ] **Step 2: 実装する**

`#/records` が一覧、`#/record/{id}` が再生。ソケットの代わりに保存した列を
`Presentation` へ流す。**盤面も演出も対局中と同じ機構を通る。**

- [ ] **Step 3: 通す**

Run: `pnpm --filter @real-mahjong/web test && pnpm --filter @real-mahjong/web typecheck && pnpm --filter @real-mahjong/web build`
Expected: PASS

- [ ] **Step 4: コミット**

```bash
git add apps/web/src/records apps/web/src/main.ts apps/web/src/lobby/screen.ts
git commit -m "feat(web): 牌譜の一覧と再生を作る"
```

---

### Task 9: 倉の置き場所と後始末

**Files:**
- Modify: `crates/server/src/bin/serve.rs`
- Modify: `README.md`
- Modify: `.gitignore`

- [ ] **Step 1: 失敗する試験を書く**

`RECORDS` が無ければ既定の場所、渡せばそこ。`:memory:` なら残さない。

- [ ] **Step 2: 実装する**

既定は `data/records.sqlite`。`.gitignore` に `data/` を足す。
README に牌譜の見方と置き場所を書く。

- [ ] **Step 3: 通す**

Run: `cargo test -p server && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: PASS / 警告なし

- [ ] **Step 4: コミット**

```bash
git add crates/server/src/bin/serve.rs README.md .gitignore
git commit -m "feat(server): 牌譜の置き場所を渡せるようにする"
```

---

## 実装後の追記（2026-08-31）

計画に無かったが必要になったもの。

- **牌譜を browser の鍵でも開けるようにした。**席の証明は部屋ごとに配られる
  ので、対局が終わって画面を閉じれば手元に残らない。「一覧に出ているのに
  開けない」という食い違いが実装して初めて見えた（Task 5 の後）
- **一覧に `DISTINCT` を足した。**同じ browser が同じ卓の2席に座ると、
  JOIN が席の数だけ行を返す。手元で確かめていて並んだ
- **書き手を `spawn_blocking` から通常の task へ移した。**止まったままの
  blocking task があると `start_paused` の試験で時計が進まない
- **試験用の SQLite に一意な名前を付け、`-wal`/`-shm` も消すようにした。**
  pid だけの名前は使い回されると前回の WAL を掴む。10回に1回落ちていた
- **待合の見張りが壊れた応答で止まる**のを直した（Task 8 の作業中に発見）
