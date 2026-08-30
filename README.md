# Real Mahjong

四人打ちリーチ麻雀。雀魂「金の間」準拠。

Rust のエンジンをサーバとクライアントで共有し、卓の進行はサーバが持つ。
**山と他家の手牌はクライアントへ出ない。**

## 遊ぶ

```bash
pnpm --dir apps/web build
cargo run -p server --bin serve
```

`http://127.0.0.1:8080` を開くとロビーが出る。入口は3つ。

- **ひとりで打つ** —— CPU 3人とすぐ始まる
- **部屋を作る** —— 6文字の合言葉が出る。相手に伝えると同じ卓に着く
- **部屋に入る** —— 合言葉を入れる

席は開始を押した瞬間にランダムで決まり、空いた席は CPU が埋める。
**ブラウザを再読み込みしても続きから始まる。**席の証明は `localStorage` に
置いたトークンが持つ。作り直すなら開発者コンソールで `newTable()` を呼ぶ。

### 外から届かせる

既定では手元にしか開かない。**同じ網の中の誰かと打つには待ち受け先を渡す。**

```bash
BIND=0.0.0.0 PORT=8080 cargo run -p server --bin serve
```

TLS を張った先に置く場合、画面は頁の配られ方に従って `wss://` を選ぶので、
こちら側で切り替えるものは無い。

## 牌譜を見る

打った半荘は自動で残る。ロビーの **牌譜を見る** から選ぶと、対局中と同じ
演出で見直せる。速さは1/2/4倍、局の頭へ飛べる。

- **見えるのは自分の席の視界だけ。**他家の手牌は牌譜にも入っていない
- 一覧はこの端末に紐づく。**`localStorage` を消すと辿れなくなる**
  （アカウントを入れるまでの繋ぎ）

倉は SQLite ファイル1つ。既定は `data/records.sqlite`。

```bash
RECORDS=/var/lib/mahjong/records.sqlite cargo run -p server --bin serve
RECORDS=:memory: cargo run -p server --bin serve   # 残さない
```

## 卓の見た目だけを見る

`http://127.0.0.1:8080/preview.html` は**動かない盤面**である。副露5種・リーチ
宣言牌・21枚の河・山が最初から並んでいて、`?viewer=0..3` で見る席を変えられる。

**対局しながら見た目を確かめるのは運任せである。**加槓や暗槓は出るまで打つしか
なく、出ない半荘もある。牌の並びや向きを直したときはここで見る。

## 対局を眺める

人が座らずに、CPU 4人の半荘を最後まで流して牌譜を出す。

```bash
cargo run -p server --example watch_a_match          # 半荘1回を数秒で
cargo run -p server --example watch_a_match -- 7     # シードを変える
cargo run -p server --example watch_a_match -- live  # 実時間で（数分）
```

席0に届くイベントだけを出すので、**他家のツモ牌が見えないこと**もそのまま確認できる。
終局後にシードが開示され、局頭に配ったハッシュと照合できる。

## 検査

```bash
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --check
pnpm --dir apps/web test
pnpm --dir apps/web typecheck
```

## 構成

```
crates/
  protocol/        イベント / コマンド / 演出カタログ ← 最初に凍結した契約
  mahjong-core/    純粋判定。乱数もI/Oも時間も持たない
  mahjong-engine/  局・半荘の進行。シードを注入されイベントを吐く
  mahjong-ai/      ルールベースCPU。core にのみ依存
  mahjong-wasm/    core の判定系のみをクライアントへ公開
  server/          唯一 I/O と時間を持つ層。WS・卓Actor・台帳
apps/web/          Vite + TypeScript（3D は Three.js）
docs/superpowers/  設計仕様と実装計画
```

**`mahjong-wasm` は `mahjong-engine` に依存しない。**山を持つ側をクライアントへ出すと、
そこから山を復元できてしまう。この依存の向きが不正防止の骨格である。

## いま出来ていること

| | |
|---|---|
| 麻雀の判定・点数計算 | 完成 |
| 局・半荘の進行、鳴き・リーチ・カン・流局・連荘 | 完成 |
| CPU 雀士 | 完成 |
| 卓が実時間で動く（1卓 = 1 tokio task） | 完成 |
| WebSocket でブラウザから座る | 完成 |
| 再接続（`seq` 以降の再送） | 完成 |
| 3D の卓で遊べる | 完成 |
| 演出タイムライン（`timeline/` は完成済み。結線が Wave 3f） | 実装中 |
| 2D キャラ・牌の移動補間・音 | 未着手 |
| 認証・永続化・マッチング・段位とレート | 未着手 |

## 開発の進め方

`AGENTS.md` に作業規約がある。要点は3つ。

- **テストは仕様。**期待値を実装に合わせて書き換えない
- **計画は Codex のレビューでブロッカーがゼロになるまで書き直す**
- **計画に載せるコードは、渡す前に実際にコンパイルして走らせる**
