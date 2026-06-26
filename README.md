# md-preview

MarkdownファイルをmacOSのネイティブウィンドウで表示するだけのツール。

```sh
md README.md
```

macOS WebKit（[wry](https://github.com/tauri-apps/wry)）でレンダリングするので軽量・高速。GitHub風スタイル・ダークモード・Mermaid図・draw.io図・シンタックスハイライトに対応。図のライブラリは図がある時だけ遅延ロードされるので、通常のMarkdownは軽いまま開けます。

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
md --sample                # サンプルMarkdownを stdout に出力
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

## キーボードショートカット

`⌘W` でウィンドウを閉じる。

## カスタムCSS

`~/.config/md-preview/style.css` にCSSファイルを置くと、デフォルトのスタイルを上書き・拡張できます。

```css
/* 例: コンテンツ幅を広げる */
.markdown-body {
    max-width: 1000px;
}
```

## 対応するMarkdown機能

GFM・コードブロックのシンタックスハイライト・[Mermaid](https://mermaid.js.org/) 図・[draw.io](https://www.drawio.com/) 図・GitHub風アラート（`> [!NOTE]` 等）・ファイル名付きコードブロック（` ```rust:src/main.rs `）・YAML front matter・ダークモード（システム設定に追従）など。

draw.io 図は ` ```drawio ` コードブロックに draw.io の XML（`<mxGraphModel>` または `<mxfile>`）を貼ると描画されます。図をクリックすると全画面表示になり、ズーム・パンできます。Mermaid / draw.io のライブラリは図が含まれるときだけ遅延ロードされます。

実際の出力サンプルは `md --sample > sample.md && md sample.md` で確認できます。

## 自動リロード

表示中のMarkdownファイル（フォルダモードの場合は現在開いているファイル）を編集して保存すると、自動でプレビューが更新されます。スクロール位置は維持されます。

## 制限事項

- macOS専用
- 外部リンク（http/https）はクリックするとデフォルトブラウザで開く

## ライセンス

MIT
