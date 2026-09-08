# md-preview プラグイン

Claude Code に「Markdown や HTML を見せるときは `md` で開く」を教えるスキルです。

## 使い方

`md` で見せてと依頼したり、md / html を書いた後に `md` で開くよう指示を入れておいたりするといいかなと思います。

## インストール

Claude Code の中で実行します。

```
/plugin marketplace add maru3460/md-preview
/plugin install md-preview@md-preview
```

## 更新

`marketplace add` で入れたプラグインは自動では更新されません。

```
/plugin marketplace update md-preview
/plugin install md-preview@md-preview
```

## アンインストール

```
/plugin uninstall md-preview@md-preview
/plugin marketplace remove md-preview   # 登録ごと消す場合
```

## 中身

- `skills/md-preview/SKILL.md` … `md` の開き方と、対応している記法（GFM アラート・Mermaid・draw.io・折りたたみ・ファイル名つきコードブロック・ファイルリンクの埋め込みなど）
- `skills/md-preview/references/navigation.md` … キーボード操作、検索・アウトライン・差分・raw 表示、HTML と非 md ファイルの見え方
- `skills/md-preview/references/comment.md` … コメント機能（`c`）の操作

人が読む説明は https://maru3460.github.io/md-preview/ にあります。
