# Real Mahjong

四人打ちリーチ麻雀。雀魂「金の間」準拠。

Rust のエンジンをサーバとクライアントで共有し、卓の進行はサーバが持つ。
**山と他家の手牌はクライアントへ出ない。**

## 遊ぶ

```bash
pnpm --dir apps/web build
cargo run -p server --bin serve
```

`http://127.0.0.1:8080` を開くと、席0に座って CPU 3人と半荘を打てる。

- **ブラウザを再読み込みしても続きから始まる。**卓は `localStorage` に覚えた id で引き当てる
- 新しい卓を立てるなら、開発者コンソールで `newTable()` を呼ぶ

**このサーバに認証は無い。**卓の id を知っていれば誰でもその席に座れる。
手元で遊んで評価するための段階であり、公開するものではない。

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
| 遊べる 2D の卓 | Wave 3e-1 |
| 3D 卓と 2D キャラ演出 | Wave 3e-2 以降 |
| 認証・永続化・マッチング・段位とレート | 未着手 |

## 開発の進め方

`AGENTS.md` に作業規約がある。要点は3つ。

- **テストは仕様。**期待値を実装に合わせて書き換えない
- **計画は Codex のレビューでブロッカーがゼロになるまで書き直す**
- **計画に載せるコードは、渡す前に実際にコンパイルして走らせる**
