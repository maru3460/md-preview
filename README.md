# md-preview

MarkdownファイルをmacOSのネイティブウィンドウで表示するだけのツール。

```sh
md README.md
```

macOS WebKit（[wry](https://github.com/tauri-apps/wry)）でレンダリングするので軽量・高速。GitHub風スタイル・ダークモード・Mermaid図・draw.io図・シンタックスハイライトに対応。ページ内検索（⌘F）・アウトライン（⌘T）・コードブロックのコピーボタン・右クリックメニューも備えています。図のライブラリは図がある時だけ遅延ロードされるので、通常のMarkdownは軽いまま開けます。

## インストール

[Rust](https://rustup.rs/) が必要です。

```sh
cargo install --path .
```

`~/.cargo/bin/md` としてインストールされます。`PATH` に含まれていることを確認してください。

## アンインストール

```sh
cargo uninstall md-preview
```

## 使い方

```sh
md .                       # フォルダモード（作業ディレクトリをサイドバー表示）
md docs/                   # フォルダモード（指定ディレクトリをサイドバー表示）
md intro.md                # 作業ディレクトリ内のファイル → フォルダモードで開きそのファイルを初期表示
md ~/other/note.md         # 作業ディレクトリ外のファイル → 単ファイルモード
cat note.md | md           # 標準入力からMarkdownを読む（パイプ経由）
md --sample                # サンプルMarkdownを標準出力に出力
md --help                  # 使い方を表示（-h も可）
md --version               # バージョンを表示（-V も可）
```

モードの選ばれ方：

- **作業ディレクトリ外のファイル**を指定 → 単ファイルモード（900px幅）
- それ以外（ディレクトリ指定 or 作業ディレクトリ内のファイル指定）→ フォルダモード（1200px幅）。作業ディレクトリ内のファイルを指定した場合は、作業ディレクトリをルートにしてそのファイルを初期表示

機能を一通り試したいときは：

```sh
md --sample > sample.md && md sample.md
```

## フォルダモード

サイドバーにディレクトリツリーが表示されます。ファイルをクリックするとプレビューペインに表示されます。Markdownファイル内の相対リンクもプレビューペイン内で遷移します。サイドバーの幅はドラッグでリサイズできます。

Markdownファイルを含まないディレクトリは自動的にツリーから除外されます。

## 検索・アウトライン

- **⌘F** … ページ内検索バーを開く。`Enter` / `Shift+Enter`（または `↑` `↓`）で前後のヒットへ移動、`Aa` ボタンで大文字小文字の区別を切り替え、`Esc` で閉じる。選択テキストがあれば初期クエリに入ります。
- **⌘T** … 見出しのアウトラインパネルを開閉する。項目クリックでその見出しへスクロールし、スクロールに追従して現在位置がハイライトされます。

## 右クリックメニュー

プレビュー領域やサイドバーのツリー項目を右クリックすると、コンテキストメニューが開きます。

- 選択テキストのコピー
- 相対パス / 絶対パスのコピー
- Finderで表示
- デフォルトアプリで開く
- 再読み込み

サイドバーのツリー項目を右クリックした場合はその項目が、それ以外ではプレビュー中のファイルが操作対象になります。

## キーボードショートカット

- **⌘F** … ページ内検索
- **⌘T** … アウトライン表示
- **⌘A** … 本文（`.markdown-body`）だけを全選択（サイドバー等は巻き込みません）
- **⌘W** … ウィンドウを閉じる
- 標準の編集メニュー（Undo / Redo / Cut / Copy / Paste / Select All）も利用できます。

## テーマ

`md theme` コマンドで配色を丸ごと切り替えられます。

```sh
md theme                   # テーマ一覧を表示（使用中のテーマに ● 印、ターミナルでは色見本付き）
md theme dracula           # テーマを切り替える（~/.config/md-preview/active-theme に保存される）
```

組み込みテーマ：

| 区分 | テーマ |
| --- | --- |
| light（固定） | `minimal` `editorial` `ink` `paper` `mono` `solarized-light` |
| dark（固定） | `nord` `dracula` `gruvbox` `rose-pine` `terminal` `blueprint` |
| auto（OS設定に追従） | `default` |

light / dark 固定のテーマはシンタックスハイライトの配色もそれに合わせて固定され、OSのダークモード設定の影響を受けません。

### ユーザーテーマ

`~/.config/md-preview/themes/<名前>.css` にCSSファイルを置くと、`md theme <名前>` で選べる独自テーマになります。組み込みと同じ名前を使うと、そちらが優先されます（`md theme` の一覧に `（ユーザー定義で上書き）` と表示されます）。ユーザーテーマはOS設定に追従（auto）します。テンプレートには `default` テーマのCSSを写すのが手軽です。

## カスタムCSS

`~/.config/md-preview/style.css` にCSSファイルを置くと、デフォルトのスタイルを上書き・拡張できます。テーマよりさらに後ろに適用されるので、選んでいるテーマはそのままに細部だけ微調整したいときに使えます。

```css
/* 例: コンテンツ幅を広げる */
.markdown-body {
    max-width: 1000px;
}
```

## 対応するMarkdown機能

GFM・コードブロックのシンタックスハイライト・[Mermaid](https://mermaid.js.org/) 図・[draw.io](https://www.drawio.com/) 図・GitHub風アラート（`> [!NOTE]` 等）・ファイル名付きコードブロック（` ```rust:src/main.rs `）・YAML front matter・ダークモード（システム設定に追従）など。コードブロックにはホバーで現れるコピーボタンが付きます。

draw.io 図は ` ```drawio ` コードブロックに draw.io の XML（`<mxGraphModel>` または `<mxfile>`）を貼ると描画されます。図をクリックすると全画面表示になり、ズーム・パンできます。Mermaid / draw.io のライブラリは図が含まれるときだけ遅延ロードされます。

実際の出力サンプルは `md --sample > sample.md && md sample.md` で確認できます。

## 自動リロード

表示中のMarkdownファイル（フォルダモードの場合は現在開いているファイル）を編集して保存すると、自動でプレビューが更新されます。スクロール位置は維持されます。

## セキュリティ

信頼できないMarkdownを開くことを前提に、ページ全体にContent Security Policy (CSP) を適用しています。本文に埋め込まれた `<script>` は実行されません。右クリックメニューからのパス操作（絶対パスのコピー・Finderで表示・デフォルトアプリで開く）はRust側でパスを検証し、実行系の拡張子（`.app` / `.command` 等）は弾きます。

## 制限事項

- macOS専用
- 外部リンク（http/https）はクリックするとデフォルトブラウザで開く

## Claude Code プラグイン

このリポジトリは [Claude Code](https://claude.com/claude-code) のプラグインとしても配布しています。`md` コマンドの使い方をまとめたスキルが同梱されており、Claude Code から Markdown を人に見せたいときに `md` を使えるようになります。

インストール（Claude Code 内で実行）：

```
/plugin marketplace add maru3460/md-preview
/plugin install md-preview@md-preview
```

### プラグインの更新

`marketplace add` で入れたプラグインは、リポジトリ側でバージョンが上がっても自動では更新されません。最新版に更新するには、マーケットプレイスのメタデータを取り直してから再インストールします（Claude Code 内で実行）：

```
/plugin marketplace update md-preview
/plugin install md-preview@md-preview
```

反映されたかは `/plugin list`、または `/plugin` → **Installed** タブ（**Last updated** 日付）で確認できます。

### 自動更新を有効にする

カスタムマーケットプレイス（GitHub リポジトリ）はデフォルトで自動更新が無効です。有効にすると起動時に自動で最新版へ更新されます。

1. `/plugin` を開く
2. **Marketplaces** タブ → `md-preview` を選択
3. **Enable auto-update** を選ぶ

以降はバージョンを上げて push するだけで、次回起動時に反映されます。

### プラグインのアンインストール

```
/plugin uninstall md-preview@md-preview
```

マーケットプレイスの登録ごと削除する場合は：

```
/plugin marketplace remove md-preview
```

> [!NOTE]
> プラグインに含まれるのはスキル（`md` の使い方チートシート）のみです。`md` コマンド本体は上記「インストール」の `cargo install --path .` で別途セットアップしてください。プラグインを消しても `md` コマンドは残るので、本体も消すには `cargo uninstall md-preview` を実行してください。

## ライセンス

MIT
