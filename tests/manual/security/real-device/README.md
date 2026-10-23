# 実機で外へ出る接続を確かめる

先読み（`<link rel=preconnect>`・DNS の先読み）、外部の読み込み、文書の JavaScript が、本物の WKWebView から外へ出ていかないことを確かめる手順。

先読みは CSP では止まらないので、WebKit の非公開の機能フラグで切っている（`platform::without_prefetch`）。
`cargo test` の `tests/webkit_flags.rs` は名前が残っていることまでしか見られない。
名前が残ったまま効かなくなる変化は、実際に接続が来るかで見るしかない。
macOS を更新したら、この手順を回す。

## 仕組み

- `logger.py` … 127.0.0.1 の 2 つのポートで待つ記録用のサーバ
  - 8765 … 先読みの行き先。**どの設定でも、ここに来てはいけない**
  - 8766 … 画像・CSS・フォント・iframe・fetch などの行き先
- `site/probe.html`・`site/probe.md` … 上の 2 つのポートを指す文書
- `run.sh` … 設定を変えて md でファイルを開き、数秒後に閉じて、その間に来た接続を出す

設定は使い捨ての `MD_CONFIG_DIR` に書くので、日常の md の設定には触れない。

## 手順

1. 確かめたい md を入れる。日常の md と混ぜないなら、別の場所に入れる

   ```sh
   cargo install --path . --root ~/.local/md-dev
   ```

2. 別の端末で、記録用のサーバを動かしておく

   ```sh
   cd tests/manual/security/real-device
   python3 -u logger.py > server.log
   ```

3. 2 つの設定で開く（窓が開いて、7 秒後に閉じる）

   ```sh
   cd tests/manual/security/real-device
   export MDBIN=~/.local/md-dev/bin/md
   for m in off on; do ./run.sh $m; done
   ./run.sh on probe.md
   ```

## 期待する結果

- `off`（既定。「文書を信頼する」が OFF）: 8765 にも 8766 にも来ない
- `on`（「文書を信頼する」が ON）: 8766 に `/style.css`・`/font.woff2`・`/image.png`・`/frame.html`・`/fetch`・`/js-image.png`・`/beacon` が来る。8765 には来ない

- `on` で 8766 に来るのは、記録用のサーバが動いていて、文書が本当に読まれていることの確かめでもある。ここで何も来なければ、手順のどこかが間違っている
- `probe.md` は、`on` で `/md-image.png` と `/md-onerror` が来る。`<link>` は本文から落とすので、8765 はやはり来ない
- **8765 に `ACCEPT` が 1 行でも出たら、先読みの停止が効いていない。** 設定画面に出る「⚠ 効いていない」の名前と、`cargo test --test webkit_flags` の結果も合わせて見る

## 片付け

`server.log` と `app-*.log` は追跡しない（`.gitignore`）。記録用のサーバは Ctrl+C で止める。
