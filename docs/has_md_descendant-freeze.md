# フォルダモードの固まり: `has_md_descendant` の無制限全走査

## 症状

`md /`（や巨大なディレクトリ）をフォルダモードで開き、サイドバーのツリーを
展開したりファイルを切り替えたりしていると、UI が数秒〜それ以上フリーズする
ことがある。**再現性は低い** ――「どのフォルダを展開したか」に依存するため。

## 根本原因

サイドバーの各フォルダ行には「配下に Markdown があるか」を示すドット
（`.has-md`）が付く。この判定が犯人。

- `src/folder.js:15` `doHasMdCheck()` — フォルダ行ごとに
  `fetch('/?has_md=' + path)` を投げる（結果は `mdDotCache` にキャッシュされるが、
  **初回は必ず走る**）。
- `src/main.rs`（`has_md=` ハンドラ）— リクエストを受けると
  `std::thread::spawn` で別スレッドを起こし、`has_md_descendant()` を呼ぶ。
- `src/request.rs:94` `has_md_descendant()` — **深さ無制限の再帰**でディレクトリ
  ツリーを走査する。

```rust
pub fn has_md_descendant(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    for entry in entries.flatten() {
        let p = entry.path();
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            if has_md_descendant(&p) {      // ← 深さ上限なしで潜り続ける
                return true;
            }
        } else if is_md(&p) {
            return true;                    // ← .md が 1 つでもあれば即 true（early return）
        }
    }
    false                                   // ← .md が 1 つも無いと最後まで歩ききる
}
```

### なぜ固まるか

1. **`.md` が無い枝は最後まで全走査する。** early return は「見つかったら」
   効くだけ。`/usr`・`/System`・`node_modules` のような Markdown を含まない
   巨大ツリーを踏むと、数十万エントリを最後まで舐める。
2. **サイドバーに見えるサブフォルダの数だけ同時発火する。** フォルダを 1 つ
   展開すると、その直下の各サブフォルダ行が一斉に `has_md=` を投げる。重い
   全走査スレッドが同時多発し、CPU / ディスク I/O を食い潰す。
3. **走査を止める手段がない。** スレッドに逃がしてはいるが、走査自体は
   キャンセルもタイムアウトもされないので、裏で回り続けて全体を重くする。

### symlink ループではない（誤解しやすい点）

`entry.file_type()` は **シンボリックリンクを辿らない**（`readdir` の型を返す）。
シンボリックリンクは `is_dir() == false` になるので、`has_md_descendant` は
リンク先へ再帰しない。したがって「symlink ループによる無限再帰」ではなく、
**実在する巨大ツリーの全走査**が原因。

## 裏付け（モンキーテスト）

`tests/monkey.rs` でフォルダ操作をランダムに叩き込むと、**最も遅い操作は毎回
`HasMd`（＝ `has_md_descendant`）**になる。実ツリーを歩かせると容易に固まりを
検出できる。

```sh
# 本物の `md /` 相当を再現（>2s かかった操作を固まりとして報告）
MONKEY_ROOT=/ MONKEY_FREEZE_MS=2000 cargo test --test monkey -- --ignored --nocapture

# 出力例（target/ を低い閾値で歩かせたとき）
# 固まり検出（>15ms）！ 再現: MONKEY_SEED=7 step=10 action=HasMd("debug")
```

## 対策案（未着手・要検討）

優先度順の案。組み合わせも可。

1. **深さ上限を設ける。** 例: root から数階層だけ見て、それ以深は「不明」扱い
   にする。ドット表示のためだけに全走査するのは割に合わない。
2. **時間 / エントリ数の予算。** 一定件数（例: 数千エントリ）または一定時間で
   打ち切り、「不明」を返す。VSCode の `largeFolderOptimizations` 的な割り切り。
3. **キャンセル機構。** フォルダを畳んだ / 別を開いたら、進行中の走査を
   `AtomicBool` 等で中断できるようにする。
4. **並行数の制限。** 同時に走る `has_md=` を数本に絞る（セマフォ / キュー）。
   folder.js 側で直列化してもよい。
5. **そもそもドットを遅延 / オンデマンドにする。** 展開して実際に見える範囲だけ
   判定する、あるいはホバー時に判定するなど。

いずれも「ドット表示は"あれば嬉しい"程度の飾りで、そのために巨大ツリーを
全走査する価値はない」という前提に立つのがよい。

## 関連ファイル

- `src/request.rs:94` — `has_md_descendant()`
- `src/main.rs` — `has_md=` クエリのハンドラ（`std::thread::spawn`）
- `src/folder.js:8-32` — `doHasMdCheck()` / `scheduleHasMdCheck()`
- `tests/monkey.rs` — 固まりを再現・検出するモンキーテスト
