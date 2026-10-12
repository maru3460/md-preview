# md-preview

AIの作ったマークダウンをぱっと読むためのツール。
HTMLも見れます。

```bash
md path/to/file.md
```

https://github.com/user-attachments/assets/008de3df-165f-4abf-86e3-c70411997ff0

注意: mac限定です。

## インストール

```bash
brew install maru3460/tap/md-preview
```

更新は `brew upgrade md-preview` です。

## アンインストール

```bash
md uninstall
brew uninstall md-preview
```

`md uninstall` を実行しておくと、設定とキャッシュも消えます。

## コメント（`c`）

`c` キーで**コメントモード**に入れます。

https://github.com/user-attachments/assets/f34e4e47-6218-4da9-ae91-5455e4401185

コピーされるコメントの例

```text
- notes/curry.md:4
> 玉ねぎは飴色になるまで 40 分炒める。

40 分は長い。もっと早く済ませたい
```

## テーマ

`md theme` コマンドか、窓の ⌘,（設定）で配色を丸ごと切り替えられます。

```bash
md theme # テーマ一覧を表示
md theme nord # テーマを切り替える
```

一覧と見本は [Themes](https://maru3460.github.io/md-preview/themes/) にあります。

## Claude Code プラグイン

このリポジトリは [Claude Code](https://claude.com/claude-code) のプラグインとしても配布しています。
詳しくは[README](plugins/README.md)を見てください。
使い方に合わせて `md-preview-open`（その都度開く）と `md-preview-notify`（窓を開いたままにしてベルに積む）の 2 つがあり、どちらか片方を入れます。

インストール

```
/plugin marketplace add maru3460/md-preview
/plugin install md-preview-open@md-preview     # または md-preview-notify@md-preview
```

アンインストール

```
/plugin uninstall md-preview-open@md-preview   # 入れている方
/plugin marketplace remove md-preview   # 登録ごと消す場合
```

## もっと詳しく

使い方の全体は https://maru3460.github.io/md-preview/ にまとめてあります。
