# 実機で外へ出る接続を確かめる

外部の読み込みと文書の JavaScript が、設定「文書を信頼する」が OFF のとき本物の WKWebView から外へ出ていかないこと（ON のときは出ること）を確かめる手順。
Playwright の WebKit は本物の WKWebView ではないので、CSP の扱いが OS の更新で変わっていないかは、ここで見る。

## 仕組み

- `logger.py` … 127.0.0.1 の 8766 で待つ記録用のサーバ（画像・CSS・フォント・iframe・fetch などの行き先）
- `site/probe.html`・`site/probe.md` … 上のポートを指す文書
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

- `off`（既定。「文書を信頼する」が OFF）: 8766 に何も来ない
- `on`（「文書を信頼する」が ON）: 8766 に `/style.css`・`/font.woff2`・`/image.png`・`/frame.html`・`/fetch`・`/js-image.png`・`/beacon` が来る

- `on` で 8766 に来るのは、記録用のサーバが動いていて、文書が本当に読まれていることの確かめでもある。ここで何も来なければ、手順のどこかが間違っている
- `probe.md` は、`on` で `/md-image.png` と `/md-onerror` が来る
- **`off` で 8766 に 1 行でも来たら、CSP の守りが効いていない**

## 片付け

`server.log` と `app-*.log` は追跡しない（`.gitignore`）。記録用のサーバは Ctrl+C で止める。
