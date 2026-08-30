# 牌譜 —— 保存・一覧・再生 設計仕様書

作成日: 2026-08-30
位置づけ: 全体設計 `2026-08-08-real-mahjong-design.md` の第2段（運用機能）の2枚目。
1枚目は `2026-08-30-rooms-and-seating-design.md`（人と打てる卓）。

---

## 1. 何を作るか

打った半荘を残し、後から**演出付きで見直せる**ようにする。

`crates/server/src/persistence.rs` はいまも1行の空である。一方で、
**牌譜そのものは既に卓の中に溜まっている。**

```rust
pub struct Table {
    ...
    /// 卓が出した真実。再接続の再送に使う。
    log: Vec<EventEnvelope>,
}
```

やることは「溜まっているものを落とさずに残し、後から引ける形にする」だけである。

**実測: 1半荘は 127KB、gzip で 13.7KB。**1日100半荘でも gzip 1.4MB にしかならない。

## 2. 既にできていること

| 必要なもの | 現状 |
|---|---|
| サーバの真実の列 | `Table.log` にそのまま溜まっている |
| 席ごとの視界フィルタ | `project_envelope(&envelope, seat)` が既にある |
| シードの開示 | 半荘終了時に `Event::SeedReveal` が出る。**保存した列だけで検算できる** |
| 保存した列から盤面を組む | `apply()` が既にやっている（`game/replay.test.ts` が2半荘ぶんで通っている） |
| 演出付きの再生 | `Presentation` と `EffectPlayer` が既にある。ソケットの代わりに列を流せばよい |
| 書き出しの前例 | `examples/dump_match.rs` が1席ぶんを JSONL にしている |

## 3. 決めたこと

| 項目 | 決定 | 理由 |
|---|---|---|
| 保存先 | **SQLite**（全体設計の Postgres から変更） | 準備が一切要らず CI でもそのまま走る。書き込みは局に1回で、単一 VPS には過剰なほど足りる。表は3枚なので後で Postgres へ移すのも容易 |
| 範囲 | 保存・一覧・再生 | 残したものが正しいかを目で確かめられる |
| 見える人 | **同卓した人だけ。その人の席の視界で** | 対局中の見え方と一致する。`project()` を通すので新しい漏れ口が生まれない |
| 書く時機 | **局の境界** | 全体設計 8.3 の判断に従う。落ちても失うのは進行中の1局だけ |

### 3.1 全体設計からの変更点

全体設計は「Postgres は永続化と牌譜のみ」と書いている。**ここを SQLite に変える。**

判断の根拠は、この牌譜が Postgres を必要とする形をしていないこと。書き込みは
局の境界で1回、読み出しは対局 id で1件を引くだけ、同時に書く卓はせいぜい数卓。
一方で Postgres は手元にも CI にも常駐の準備を要求する。**得るものが無いのに
段取りだけが増える。**

表は3枚しかないので、複数台へ広げる日が来たら移せる。その日が来るまで
入れない。

## 4. 何を保存するか

**サーバの真実（`Event`）を保存する。**射影した後のものではない。

射影後を保存すると席の数だけ列が要り、しかも「後から別の席の視点で見る」が
永久にできなくなる。真実を1本だけ残し、読み出すたびに `project()` を通す。
**生の配信と同じ関数を通るので、視界フィルタの抜け道が生まれない。**

### 4.1 表

```sql
-- 対局そのもの。
CREATE TABLE records (
  id          TEXT PRIMARY KEY,   -- 対局 id（32桁の乱数16進）
  rules       TEXT NOT NULL,      -- Ruleset の JSON
  started_ms  INTEGER NOT NULL,   -- 卓が立った実時刻（epoch ミリ秒）
  ended_ms    INTEGER,            -- 終局した実時刻。途中で落ちたら NULL
  players     TEXT NOT NULL,      -- 席順の名前の JSON 配列
  result      TEXT                -- 終局の点数と順位の JSON。未了なら NULL
);

-- 席と、その席を見てよい人。
CREATE TABLE record_seats (
  record_id   TEXT NOT NULL REFERENCES records(id),
  seat        INTEGER NOT NULL,
  name        TEXT NOT NULL,
  is_cpu      INTEGER NOT NULL,
  token_hash  TEXT,               -- 席の証明の SHA-256。CPU は NULL
  player_key  TEXT,               -- その人の browser を指す鍵。CPU は NULL
  PRIMARY KEY (record_id, seat)
);
CREATE INDEX record_seats_by_player ON record_seats(player_key);

-- 出来事。**局ごとに束ねて入れる。**1件1行にすると半荘で1,300行になる。
CREATE TABLE record_events (
  record_id   TEXT NOT NULL REFERENCES records(id),
  chunk       INTEGER NOT NULL,   -- 0 から。書いた順
  first_seq   INTEGER NOT NULL,
  last_seq    INTEGER NOT NULL,
  events      BLOB NOT NULL,      -- EventEnvelope の JSONL を gzip したもの
  PRIMARY KEY (record_id, chunk)
);
```

**トークンは生で持たない。**漏れた DB がそのまま席の証明にならないよう
SHA-256 を入れる。照合は引く側がハッシュして比べる。

### 4.2 なぜ局ごとに束ねるのか

1イベント1行にすると半荘で 1,300 行、100 半荘で 13 万行になる。読み出しは
必ず「対局まるごと」なので、行に割る利点が無い。**局ごとの塊にすれば
書き込みは10回前後、読み出しは10行の連結で済む。**

gzip するのは、中身が JSON という圧縮のよく効く文字列だからである
（実測 127KB → 13.7KB）。

## 5. 誰が見られるか

**同卓した人だけが、自分の席の視界で見られる。**

引くときに `X-Mahjong-Token` を出す。サーバはそれをハッシュして
`record_seats` を引き、席が決まったら保存した列を `project(event, seat)` に
通して返す。**対局中とまったく同じ関数を通る。**

### 5.1 一覧のための鍵

トークンは部屋ごとに配られるので、1本＝1対局にしかならない。
**「自分の打った半荘の一覧」には、対局をまたいで続く名札が要る。**

アカウントはまだ無いので、**browser ごとの鍵**を置く。

- 画面が初回に32桁の乱数を作り、`localStorage["real-mahjong.player"]` に置く
- 部屋を作る／入るときに一緒に送る
- `record_seats.player_key` に入る
- 一覧はこの鍵で引く

**この鍵の弱さを明記しておく。**`localStorage` を消すと一覧が消える。別の
機械からは見えない。これはアカウントを入れるまでの繋ぎであり、アカウントが
入ったら鍵を利用者に結び直す。

## 6. HTTP の口

| | |
|---|---|
| `GET /api/records` | その browser が打った対局の一覧。`X-Mahjong-Player` で引く |
| `GET /api/records/{id}` | 1対局の見出し（players・rules・結果・自分の席） |
| `GET /api/records/{id}/events` | **自分の席の視界に射影した** `ClientEventEnvelope` の JSONL |

いずれも証明が要る。持っていなければ 401。**存在しない id と、見る資格の
無い id を区別しない。**id の総当たりに手がかりを与えない。

## 7. 書く時機

局の境界で、その局のぶんを1塊として書く。全体設計 8.3 の判断に従う——
落ちても失うのは進行中の1局だけである。

- 卓が立った時点で `records` と `record_seats` の行を作る
- 局が終わるたびに `record_events` へ1塊足す
- 終局したら `ended_ms` と `result` を埋める

**書き込みで卓を止めない。**SQLite への書き込みは同期的なので、卓の
Actor から直接叩くと局の切れ目で全員が待つ。書き手を別の tokio task に
し、チャネルへ投げて戻る。**落としても対局は続く**——牌譜が欠けることは
あっても、打っている最中が固まるよりはよい。

## 8. 再生の画面

`#/record/{id}` を開くと、保存した列を `Presentation` へ流す。

- **ソケットの代わりに列を流すだけ。**盤面も演出も対局中と同じ機構を通る
- 再生の速さは1倍・2倍・4倍。飛ばす／戻すは局単位
- 局の頭へ飛べるように、`RoundStart` の位置を目次として出す

終局の板と終局の表は既にあるので、そのまま出る。

## 9. 試験

| 何を確かめるか | どこで |
|---|---|
| 局の境界で塊が増える | server 単体 |
| 途中で落ちても、それまでの局は読める | server 単体 |
| **他人のトークンでは他人の席の視界が返らない** | server 単体 |
| 資格の無い id と存在しない id が同じ答えになる | server 単体 |
| 保存した列を射影すると、対局中に配ったものと1件ずつ一致する | server 結合 |
| CPU の席にはトークンも鍵も入らない | server 単体 |
| 書き手が落ちても卓が止まらない | server 単体 |
| 一覧が browser の鍵で引ける | server 単体 |
| 保存した列から盤面が組み上がる | web 単体（既存の `replay.test.ts` を土台に） |
| 再生の画面が局の目次を出す | web 単体 |

## 10. やらないこと

- 途中で落ちた対局の**再開**（8.3 の続き）。牌譜は残すが卓は戻らない
- 牌譜の共有（他人に見せる URL）
- 全席を見る観戦モード
- 牌譜の検索・絞り込み（一覧は新しい順に並べるだけ）
- シードの検算を画面でやること（データは揃っているが、道具は作らない）
- Postgres への移行
