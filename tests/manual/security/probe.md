# セキュリティの検証用（md）

文書の JS・外部の読み込み・mermaid・右クリック・特殊なファイルが、いまどう扱われているかを目で見るためのファイル。
`.html` 側は [probe.html](./probe.html) にある。

既定（「文書を信頼する」が OFF）では、開いても外へは何も読み込まない。ON にすると、外部の節が `img.shields.io` と `example.com` を読み込む。

## 読み方

各項目の「結果」は、何もしなければ `⚪ 止まった` のまま。文書の JS が動いたときだけ `🔴 動いた` に書き換わる。

期待する結果は、設定「文書を信頼する」で変わる。

- 既定（OFF）: 文書の JS は全部 `⚪`。外部の画像・iframe は「開く」リンクに置き換わり、外部のフォントは効かない
- ON: イベント属性と `javascript:` が `🔴` になる（`<script>` 要素は `⚪` のまま）。外部の画像・iframe が出る。外部のフォントは md では効かないまま（2 の Web フォントの項）

どの設定でも変わらないもの:

- 何も押さないうちに既定のブラウザが開くことは無い。外へ開くのはリンクのクリックだけ

設定は ⌘Q で終了して開き直したときから効く。

## 1. 文書の JavaScript

<style>
.probe-result { font-family: ui-monospace, monospace; padding: 0 4px; border-radius: 3px; background: rgba(127,127,127,.15); }
</style>

- **画像の `onerror`**（読み込みに失敗した画像のイベント属性。開いた瞬間に判定される）
  - 結果: <img src="./probe-assets/missing.png" alt="" style="display:none" onerror="this.nextElementSibling.textContent='🔴 動いた'"><span class="probe-result">⚪ 止まった</span>
- **ボタンの `onclick`**（押すと判定）
  - <button onclick="this.nextElementSibling.textContent='🔴 動いた'">押す</button> 結果: <span class="probe-result">⚪ 止まった</span>
- **`javascript:` のリンク**（押すと判定）
  - <a href="javascript:void(document.querySelectorAll('.probe-js-link').forEach(function(e){e.textContent='🔴 動いた'}))">押す</a> 結果: <span class="probe-result probe-js-link">⚪ 止まった</span>
- **`<details>` の `ontoggle`**（開くと判定）
  - <details ontoggle="this.querySelector('.probe-result').textContent='🔴 動いた'"><summary>開く</summary>結果: <span class="probe-result">⚪ 止まった</span></details>
- **`<script>` 要素**（期待は常に `⚪`。本文は innerHTML で入るので、窓の中では設定に関係なく動かない）
  - <script>document.querySelectorAll('.probe-script').forEach(function(e){e.textContent='🔴 動いた'})</script>結果: <span class="probe-result probe-script">⚪ 止まった</span>

## 2. 外部の読み込み

- **外部の画像**（README のバッジと同じ形）
  - 見え方: [![外部の画像](https://img.shields.io/badge/外部の画像-読めた-blue)](https://img.shields.io)
  - OFF なら、バッジがチップ（「外部の画像」の文字）に置き換わる。ON ならバッジが出る
- **外部の Web フォント**（読めていれば、下の文字が袋文字の Rampart One になる。読めなければ等幅の文字のまま）
  - 本文の `<style>` の `@import` と `@font-face` は、設定に関係なく落とす（本文の CSS を本文の枠に閉じ込めるため）。md では、文書を信頼していても等幅の文字のまま出るのが正しい。`.html` の中なら効く（[probe.html](./probe.html) の同じ項目）

  <style>
  @import url('https://fonts.googleapis.com/css2?family=Rampart+One&display=swap');
  .probe-font { font-family: 'Rampart One', ui-monospace, monospace; font-size: 1.6em; }
  </style>
  <span class="probe-font">外部フォント ABC</span>
- **外部の iframe**
  - OFF なら「外部の埋め込み」の「開く」リンクに置き換わる。ON なら枠の中に example.com が出る
  - どちらでも、開いた瞬間に既定のブラウザが開かないこと。ON のとき、枠の中の example.com のリンクを押しても何も開かないこと

  <iframe src="https://example.com" width="480" height="160"></iframe>
- **外部へのリンク**（どの設定でも、押すと既定のブラウザで開く）
  - [example.com を開く](https://example.com)

## 3. 変わらないもの（どの設定でも同じに見えるべきもの）

- 手元の画像（相対パス）: ![手元の PNG](./probe-assets/local.png) ![手元の SVG](./probe-assets/local.svg)
- `data:` の画像: <img alt="data: の画像" src="data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='120' height='24'><rect width='120' height='24' fill='%236a1b9a'/><text x='60' y='17' font-size='12' fill='white' text-anchor='middle' font-family='sans-serif'>data: の画像</text></svg>">
- インライン SVG: <svg width="120" height="24"><rect width="120" height="24" rx="4" fill="#ef6c00"/><text x="60" y="17" font-size="12" fill="#fff" text-anchor="middle" font-family="sans-serif">インライン SVG</text></svg>
- `<kbd>`: <kbd>⌘</kbd> + <kbd>F</kbd>
- `<details>`: <details><summary>開いて閉じられる</summary>中身</details>
- 手元の `.html`: [probe.html](./probe.html) を開くと、iframe の中で描かれる

```mermaid
flowchart LR
  A[mermaid は描ける] --> B["ラベルの <b>太字</b><br>と改行も残る"]
  click A href "javascript:document.title='動いた'"
  click B call alert()
```

上の図のノードを押しても何も起きないこと（mermaid の strict）。

## 4. 右クリックと偽の UI

本文の中に、サイドバーの行やタブに似せた要素を置いてある。

<div class="tree-item md-file" data-path="/etc/hosts" data-kind="file" style="border:1px dashed #c62828;padding:2px 6px">📄 偽のツリーの行（/etc/hosts を指している）</div>
<div class="md-tab" data-path="/etc/hosts" style="border:1px dashed #c62828;padding:2px 6px;margin-top:4px">偽のタブ（/etc/hosts を指している）</div>

- 右クリックして「絶対パスをコピー」すると、`/etc/hosts` ではなく、**このファイル（probe.md）のパス**が入ること
- メニューに「閉じる (⌘W)」や「ここを root にする」が出ないこと
- ツリーにフォーカスして j / k で動いても、この偽の行には乗らないこと（Enter で `/etc/hosts` が開かないこと）

## 5. 特殊なファイル

FIFO や `/dev/zero` へのリンクはリポジトリに置いておけない（ツリーを辿る道具が止まる）。先に手元で作ってから、このファイルを開き直す。

```sh
cd tests/manual/security
ln -s /dev/zero probe-zero.png                         # 下の画像
ln -s /dev/zero probe-zero.md                          # 下の埋め込みと、ツリーで押す用
mkfifo probe-fifo.md                                   # ツリーで押す用
mkfile -n 100m probe-big.md                            # ツリーで押す用
```

- `/dev/zero` を指す画像: <img src="./probe-zero.png" alt="（probe-zero.png は表示されない）" width="80" height="20">
  - 壊れた画像としてすぐ出ること。窓が固まらないこと
- 行の埋め込みで `/dev/zero` を指す（下の 1 行）。埋め込まれず、ただのリンクのまま出ること

[probe-zero.md](./probe-zero.md#L1-3)

- ツリーで `probe-zero.md` / `probe-fifo.md` を押す: 「開けない」になり、固まらないこと。差分のトグルとバッジも固まらないこと
- ツリーで `probe-big.md` を押す: 本文に「ファイルが大きすぎるため表示しません」が出ること

片付け:

```sh
rm probe-zero.png probe-zero.md probe-fifo.md probe-big.md
```

## 6. 部品

文書の JS を止めたまま（既定）でも動く、アプリが配る部品。

- コピーボタン: <button data-md-copy="copy-probe">Copy</button> <code id="copy-probe">コピーされる文字</code>
  - 押すとボタンが Copied に変わり、1 秒ほどで Copy に戻ること。貼り付けると「コピーされる文字」が入ること。ほかに何も出ないこと
- CSS で隠した文字は写らない: <button data-md-copy="copy-probe-hidden">Copy</button> <span id="copy-probe-hidden">見える<span class="probe-hidden">隠した</span>文字</span>
  - 貼り付けた先に「見える文字」だけが出ること

<style>
.probe-hidden { display: none; }
</style>

- 長い中身: <button data-md-copy="copy-probe-long">Copy</button>（41 行）
  - 41 行が全部貼り付けられること

<pre id="copy-probe-long">1 行目
2 行目
3 行目
4 行目
5 行目
6 行目
7 行目
8 行目
9 行目
10 行目
11 行目
12 行目
13 行目
14 行目
15 行目
16 行目
17 行目
18 行目
19 行目
20 行目
21 行目
22 行目
23 行目
24 行目
25 行目
26 行目
27 行目
28 行目
29 行目
30 行目
31 行目
32 行目
33 行目
34 行目
35 行目
36 行目
37 行目
38 行目
39 行目
40 行目
41 行目</pre>

- コードブロックの Copy（下のブロックの右上）も、Copied に変わるだけでほかに何も出ないこと

```sh
echo "コードブロックの Copy"
```

- 図の拡大（下の図を押す）:
  - 本文の枠いっぱいに広がり、右上に「Esc で閉じる · クリックで 1:1」が出ること
  - 図を押すと 1:1 になり、手がかりが「クリックで全体」に変わること。もう一度押すと戻ること
  - Esc か背景のクリックで閉じ、図が元の場所へ戻ること

<figure data-md-zoom>

![拡大できる画像](./probe-assets/local.svg)

</figure>

- 縦に長い図（下の mermaid を押す）: 広げても本文の中より小さくならず、幅に合わせて縦にスクロールできること

<figure data-md-zoom>

```mermaid
flowchart TD
  A[1] --> B[2] --> C[3] --> D[4] --> E[5] --> F[6] --> G[7] --> H[8] --> I[9] --> J[10]
  J --> K[11] --> L[12] --> M[13] --> N[14] --> O[15] --> P[16] --> Q[17] --> R[18]
```

</figure>
