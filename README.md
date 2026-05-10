# md-preview

MarkdownファイルをmacOSのネイティブウィンドウで表示するだけのツール。

```sh
md README.md
```

macOS WebKit（[wry](https://github.com/tauri-apps/wry)）でレンダリングするので軽量・高速。GitHub風スタイル・ダークモード・Mermaid図・シンタックスハイライトに対応。

## インストール

[Rust](https://rustup.rs/) が必要です。

```sh
cargo install --path .
```

`~/.cargo/bin/md` としてインストールされます。`PATH` に含まれていることを確認してください。

アンインストールは `cargo uninstall md-preview`。

## 使い方

```sh
md path/to/file.md   # 単ファイルモード（900px幅）
md .                 # フォルダモード（カレントディレクトリをサイドバー表示）
md docs/             # フォルダモード（指定ディレクトリをサイドバー表示）
md --sample          # サンプルMarkdownを stdout に出力
```

カレントディレクトリ内のファイルを指定した場合はフォルダモード（1200px幅）で開き、そのファイルを初期表示します。

```sh
cd ~/docs
md intro.md   # フォルダモードで開き、intro.md をプレビュー
```

機能を一通り試したいときは：

```sh
md --sample > sample.md && md sample.md
```

## フォルダモード

サイドバーにディレクトリツリーが表示されます。ファイルをクリックするとプレビューペインに表示されます。Markdownファイル内の相対リンクもプレビューペイン内で遷移します。サイドバーの幅はドラッグでリサイズできます。

Markdownファイルを含まないディレクトリは自動的にツリーから除外されます。

## カスタムCSS

`~/.config/md-preview/style.css` にCSSファイルを置くと、デフォルトのスタイルを上書き・拡張できます。

```css
/* 例: コンテンツ幅を広げる */
.markdown-body {
    max-width: 1000px;
}
```

## 対応するMarkdown機能

- 見出し、段落、リンク、画像（相対パス・data URI）
- **太字**、*斜体*、~~取り消し線~~
- コードブロック・インラインコード（[highlight.js](https://highlightjs.org/) によるシンタックスハイライト）
- ファイル名付きコードブロック（` ```rust:src/main.rs ` のように `言語:ファイル名` を指定）
- [Mermaid](https://mermaid.js.org/) 図（` ```mermaid ` フェンスで記述）
- テーブル、タスクリスト、引用、脚注、水平線
- GitHub風アラート（`> [!NOTE]`, `> [!TIP]`, `> [!IMPORTANT]`, `> [!WARNING]`, `> [!CAUTION]`）
- YAML front matter（テーブル形式で表示）
- ダークモード（システム設定に追従）

## 制限事項

- macOS専用
- 外部リンク（http/https）はクリックするとデフォルトブラウザで開く
- 読み取り専用（ファイル変更時の自動リロードなし）

## ライセンス

MIT
