# md-preview プラグイン

Claude Code に「Markdown や HTML を見せるときは `md` で開く」を教えるスキルです。
`md` の使い方に合わせて 2 種類あります。**どちらか片方**を入れてください。

- `md-preview-open` … 見せたいときに窓を開き、読んだら閉じる使い方向け。書き終えたら `md` で開いて見せます
- `md-preview-notify` … `md` の窓を開いたままにしておく使い方向け。書き終えたら `md --notify` でベルに積み、窓を前に出しません

両方入れると同じ場面で両方が呼ばれ、指示がぶつかります。

## 使い方

`md` で見せてと依頼したり、md / html を書いた後に `md` で開くよう指示を入れておいたりするといいかなと思います。

## インストール

Claude Code の中で実行します。

```
/plugin marketplace add maru3460/md-preview
/plugin install md-preview-open@md-preview     # または md-preview-notify@md-preview
```

## 更新

`marketplace add` で入れたプラグインは自動では更新されません。

```
/plugin marketplace update md-preview
/plugin install md-preview-open@md-preview     # 入れている方
```

## アンインストール

```
/plugin uninstall md-preview-open@md-preview   # 入れている方
/plugin marketplace remove md-preview          # 登録ごと消す場合
```
