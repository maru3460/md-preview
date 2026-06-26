# テーマ（着せ替え）機能 設計メモ

> 4ペルソナ（Rust実装 / CLI-UX / CSS-テーミング / YANGI懐疑派）のレビューを反映して確定済み。
> 切り替え方式は **B: 軽量CLI** を採用。

## 背景・動機

- きっかけ: READMEに「`~/.config/md-preview/style.css` に置けば上書きできる」とあるが、**実物の見本がないと書き始められない**。
- ただし利用者は実質自分1人、CSS編集もAIにやらせる前提。なので「人間向けの発見しやすさ」「公開APIとしての安定性」は重視しなくてよい。
- 本当の不満は **「しばらく使って見た目に飽きた → パッと着せ替えたい」** の一点。
- 着せ替えを作ると副産物として「カスタムCSSの見本」「どう書けばどう効くかの実物リファレンス」も勝手に揃う（着せ替えが上位互換）。

→ **着せ替え一点に絞る。** 仕組みは地味でいい。記事化するなら、見た目（配色・タイポ）の作り込みとBefore/After画像に投資する方が刺さる。

## 確定した設計判断（レビュー反映）

### 1. base / skin の分割境界

`style.css`(792行) を **`base.css`(構造) ＋ `themes/default.css`(見た目)** に一度だけ割る。境界は以下で割り切る:

- **base に残す**: `display / position / width / height / flex / grid / margin / padding / border-width / border-style / border-radius / overflow / z-index / opacity / transform`（＝レイアウト・骨組み・動き）＋ `font-feature-settings`
- **theme に出す**: `color / background-color / border-color / font-family / font-size / font-weight / line-height / letter-spacing / text-transform`（＝見た目）
- **グレーゾーンの割り切り**: 見出しの `border-bottom` や blockquote の `border-left` は「幅・スタイルは base、色だけ theme」。`body { padding: 48px }` と `.markdown-body { max-width }` は読み幅＝構造として **base**。

### 2. ダークモードのポリシー（重要）

固定ダークテーマ(dracula等)とOS追従(default)を両立させるルール:

- `@media (prefers-color-scheme: dark)` は **`base.css` だけが持つ**（＝OS追従はdefaultテーマの世界）。
- 固定ダーク/ライトテーマ（dracula, ink等）は **`@media` を使わず無条件に配色を指定し、`color-scheme: dark|light` を宣言**する。
- **テーマCSSには `@media prefers-color-scheme` を書かない**（厳守ルール）。

### 3. シンタックスハイライト（hljs）

- v1では **GitHub Light/Dark 固定**で割り切る（テーマ追従しない）。
- テーマ毎の `hljs-*.css` は将来拡張。

### 4. 切り替え方式 = B（軽量CLI）

- 状態は `~/.config/md-preview/active-theme` に **テーマ名を1行のプレーンテキスト**で保存（**toml依存なし**）。
- `--theme` フラグ（一回限りお試し）と ホットリロード即反映は **後回し（v2）**。

## テーマ ラインナップ案

「モダン／スタイリッシュ」に収束しがちなのは "モダン" の解釈が複数あるだけ。軸をバラして互いに別物にする。

| 名前 | キャラ | フォント | コントラスト/温度 |
|---|---|---|---|
| **paper** | 読み物・長文向け。本/Kindleっぽい | 本文セリフ (Georgia/New York) | 暖色・低コントラスト |
| **nord** | 開発者ウケの定番。静かで疲れない | サンセリフ | 寒色・ソフト |
| **editorial** | 雑誌っぽい"きれいめモダン"。見出しデカい・髪の毛罫線 | 見出しセリフ＋本文サンス | 中・やや高 |
| **minimal** | Notion系。余白おばけ・引き算 | サンセリフ | 低・ニュートラル |
| **ink** | ブルータリスト。純黒白・極太・装飾ゼロ | 重いサンス | 最大（固定ライト） |
| **mono** | ターミナル/タイプライター。全部等幅 | monospace | ライト or 緑/琥珀の暗色 |
| **dracula / synthwave** | 遊び枠の暗色。ネオン | サンス | 暗・高彩度（固定ダーク） |

- 「スタイリッシュ」枠の主力 = **editorial（盛る）/ minimal（引く）/ ink（殴る）**。全部モダンだが性格が真逆なので飽きたら隣へ移れる。
- **最初に作る順（CSSレビュアー推奨）**: `minimal`（引き算で色・フォントだけで表現しやすい）→ `editorial`（盛る）→ `ink`（殴る）。default は分割で自動的にできる。
- 各テーマが「色・フォント・サイズだけ」で表現できるか、デザイン時に1行ずつチェック（margin/paddingに踏み込むものは base 境界に注意）。

## 切り替えの仕組み（B: 軽量CLI）

### テーマの在処（2系統）

- **公式テーマ**: バイナリに `include_str!("themes/nord.css")` で埋め込み（固定・消えない）。テーマ追加時は再コンパイル必要だが、ソロ運用なら許容。
- **ユーザーテーマ**: `~/.config/md-preview/themes/<name>.css` を置く → **ファイル名がテーマ名**。
- 名前衝突時は **ユーザー側が勝つ**（公式 `nord` を自分版で上書き可能。実装は数行）。

### アクティブなテーマの記憶

- `~/.config/md-preview/active-theme` にテーマ名を1行で保存（プレーンテキスト、依存なし）。
- 無ければ `default`。読めない/不明テーマ名なら警告して `default` にフォールバック。

### コマンド

```sh
md theme              # 一覧（公式＋ユーザー、アクティブに★、(official)/(user)で区別）
md theme nord         # active-theme に書く → 次回起動から反映
```

- 不明テーマ名指定時: stderr に `unknown theme 'xxx'` ＋ 利用可能一覧を出す。active-theme は書き換えない。
- `md --theme paper x.md`（一回限り）は v2。

### 読み込み順（HTML内 `<style>` の順序）

```
[base.css]      ← 構造。常に固定
[hljs light/dark] ← コードの色（固定）
[theme.css]     ← 色/フォント/余白の見た目。差し替え対象
[style.css]     ← 個人パッチ。常に最後（テーマ問わず効かせたい微調整）
```

- hljs を theme より前に置く（テーマがコード色を上書きしたくなった時のため）。
- `style.css` を最後に残すので **後方互換維持**（README記載の例もそのまま動く）。

### v2（後回し）

- `md --theme <name> file.md` の一回限りお試し。
- ホットリロードでテーマ切り替えを即反映。既存の `AppEvent::Reload` は「ファイル変更」信号なので、`ReloadKind::ThemeChanged` 等への拡張と JS 側 `window.MdReloadTheme` ハンドラが必要。複雑なので v1 では入れない。

## 作業ステップ

1. ✅ **【完了】`style.css` を `base.css`(構造) ＋ `themes/default.css`(見た目) に分割**
   - 「box vs paint」プロパティ単位で機械分割。2ファイルのプロパティ集合が互いに素なので連結順に依存せず描画不変。
   - 読み込み順は `base → hljs_light → hljs_dark@media → theme → custom`（html.rs の `BASE_CSS` / `DEFAULT_THEME_CSS`）。
   - 検証済み: build/test通過、宣言マルチセット差分はborderショートハンド分割のみ（漏れ・重複ゼロ）、base.cssに塗りゼロ/default.cssに箱ゼロ、計画レビュー＋実装レビュー（サブエージェント）通過。
   - 未確認: 実機での目視（light/dark）はユーザー側で `md THEME_PLAN.md` 等で確認推奨。
2. ✅ **【完了】`themes/minimal.css` `editorial.css` `ink.css` を追加**（固定ライト、各 default.css の全67セレクタを網羅）。
3. ✅ **【完了】テーマ解決機構 + `md theme [name]` コマンド**
   - `src/theme.rs`: BUILTIN レジストリ（name, css, Appearance）、active-theme 読み書き、user テーマ優先解決、不明名→default フォールバック、名前検証でパストラバーサル防止、`list()`。
   - `md theme` 一覧（★=アクティブ, official/user 区別）/ `md theme <name>` 設定 / 不明名→exit 2。
   - `theme_css` を build_html / build_folder_html / handle_request / handle_asset の全モードに配線。
   - **hljs はテーマの appearance に紐付け**（固定ライトは hljs もライト固定、default=Auto のみ OS 追従）。固定ライト×OSダークでコードだけ暗くなるチグハグを修正。
   - 検証済み: build/test（9 passed）、コマンド全挙動、セレクタ網羅、計画レビュー＋実装レビュー（サブエージェント）通過。実装レビューで発見した hljs チグハグを修正済み。
   - 未確認: 実機での各テーマ目視。
4. (v2) `--theme` フラグ（一回限り）、ホットリロード即反映、テーマ毎 hljs（固定ダークテーマ追加時）。

## 残タスク（任意）
- README に「テーマ」セクション追記（`md theme` の使い方、利用可能テーマ、ユーザーテーマの置き場所）。
- 実機目視 → 配色微調整。
- 固定ダークテーマ（dracula 等）を追加するなら Appearance::Dark を使う。

→ ステップ1の分割が全ての前提。1→3を一気にやると config 読み込みロジックの重複が避けられる（Rustレビュアー指摘）。

## レビューで出た残リスク（実装時に注意）

- **分割時の特異度バトル**: base は「構造的に必要な最小限」だけに保つ。同セレクタを base/theme 両方で触ると、後の base 追記で特異度が上がって壊れうる。
- **ユーザーテーマの再コンパイル不要性**: 公式は include_str! で再コンパイル要だが、ユーザーテーマはファイル読みなので「パッと試す」はユーザーテーマ側で可能。
- **font-feature-settings は base に**（テーマが上書きすると打ち消し合う）。
- **macOS WebKit(wry)**: CSS変数・mask-image・clip-path 等モダン機能は使用可。互換の心配なし。
