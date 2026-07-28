// キーボードショートカットの単一定義（Single Source of Truth）。
// 「キー・説明・カテゴリ・表示範囲」をここだけに置き、表示側はすべてこの表から導出する。
//   help.js      … `?` のショートカット一覧オーバーレイ
//   cmdpanel.js  … `⌘/` の常時表示コマンドパネル
// 文言を表示側に持たせると実装との乖離が必ず起きるため、表示に使うメタデータは
// 必ずこのファイルへ足す。
//
// 【重要】ここは「表示のための定義」であって、キーを実際に処理するのはこの表ではない。
// 実処理は各モジュールに分散した keydown ハンドラが担う:
//   keyscroll.js   j k d u Space g G /
//   comment.js     c / コメントモード中の j k Enter n p e x y
//   folder.js      ツリー内移動・] [ Tab・⌘W
//   search.js ⌘F / toc.js ⌘T / diff.js ⌘D / raw.js ⌘R / common.js ⌘A
//   help.js ? Esc / cmdpanel.js ⌘/
// 排他は stopPropagation ではなく「フォーカスの所在」と「モードの有無」で取っている。
// よって **キー自体を変える時はハンドラ側とこの表の両方を直す**必要がある。
// （ハンドラをこの表から駆動する案は別スコープ。Esc の譲り合いやフォーカス排他を
//   全面的に作り替えることになるため、表示の一元化とは分けて考える。）
(function() {
  // カテゴリ。この順に見出しを立てて表示する。
  // mode を持つカテゴリは「そのモードでないとまるごと使えない」ことを表し、
  // コマンドパネルがセクション単位でグレーアウトする（行を消さずに薄くする）。
  var CATEGORIES = [
    { id: 'scroll',  label: '読み進める' },
    { id: 'find',    label: '探す・飛ぶ' },
    { id: 'view',    label: '表示を切り替える' },
    { id: 'comment', label: 'コメント（c で開始）', mode: 'comment' },
    { id: 'files',   label: 'ファイル移動' },
    { id: 'tree',    label: 'ファイルツリー（Tab で移動）', mode: 'tree' },
    { id: 'window',  label: 'ウィンドウ' },
    { id: 'help',    label: 'ヘルプ' }
  ];

  // ── 状態の共有述語 ──────────────────────────────────────────
  // enabled() で繰り返し使う「実ハンドラのガードと同じ判定」をここに畳む。
  // 各行の根拠（どのファイルの何行目のガードを写したか）は BINDS のコメントに書く。

  // 修飾なしの素キーが本文側で効く条件。keyscroll.js のガードと同じ。
  function bareKey(s) { return !s.tree && !s.overlay && !s.interactive; }

  // コメント系の素キーが効く条件。comment.js のガードと同じ
  // （keyscroll と違い、ボタン/リンクのフォーカスは見ず入力欄だけを見る）。
  function commentKey(s) { return !s.field && !s.overlay && !s.tree; }

  // raw 表示（⌘R）は .md 以外のファイルでは無効。未公開なら有効側に倒す。
  function rawAvailable() {
    if (window.MdRaw && MdRaw.isAvailable) return MdRaw.isAvailable();
    return true;
  }

  // 表示範囲（scope）は:
  //   'all'     … 全モード（stdin / single / folder）
  //   'nostdin' … stdin 以外（ファイル/git がある single・folder）
  //   'folder'  … folder モードのみ（サイドバーとファイル移動がある時）
  //
  // keys は表示用ラベル。1 行 1 操作になるよう細かく割る（「n / p / e / x / y」を
  // 1 行にまとめると、キーと説明を目で突き合わせる手間が生まれて探しにくい）。
  //
  // enabled は「今この状態でこのキーが効くか」。実ハンドラのガードを写したものなので、
  // ハンドラを変えたらここも直す（乖離すると嘘のグレーアウトになる）。
  var BINDS = [
    // ── 読み進める（keyscroll.js:31-44） ──
    // j/k はコメントモード中は comment.js のカーソル移動に譲る（keyscroll.js:44）。
    // d/u/Space/g/G はモード中もそのまま効く。
    { cat: 'scroll', keys: 'j / k',       desc: '1 行 下 / 上へ', scope: 'all',
      enabled: function(s) { return bareKey(s) && !s.comment; } },
    { cat: 'scroll', keys: 'd / u',       desc: '半ページ 下 / 上へ', scope: 'all',
      enabled: bareKey },
    { cat: 'scroll', keys: 'Space',       desc: '1 ページ送る', scope: 'all',
      enabled: bareKey },
    { cat: 'scroll', keys: 'Shift+Space', desc: '1 ページ戻る', scope: 'all',
      enabled: bareKey },
    { cat: 'scroll', keys: 'g / G',       desc: '冒頭 / 末尾へ', scope: 'all',
      enabled: bareKey },

    // ── 探す・飛ぶ（search.js:213 / toc.js:188 / folder.js:435） ──
    // / はツリーにフォーカスがある時は folder.js が拾うので、そちらでも効く。
    { cat: 'find', keys: '/',  desc: '検索を開く', scope: 'all',
      enabled: function(s) { return !s.overlay && (s.tree || !s.interactive); } },
    { cat: 'find', keys: '⌘F', desc: '検索を開く（/ と同じ）', scope: 'all',
      enabled: function() { return true; } },
    { cat: 'find', keys: '⌘T', desc: 'アウトライン（見出しナビ）を開閉', scope: 'all',
      enabled: function() { return true; } },

    // ── 表示を切り替える（diff.js:157 / raw.js:140） ──
    { cat: 'view', keys: '⌘D', desc: 'git 差分表示を切り替え', scope: 'nostdin',
      enabled: function(s) { return !s.field; } },
    { cat: 'view', keys: '⌘R', desc: 'raw（ソース）表示を切り替え', scope: 'nostdin',
      enabled: function(s) { return !s.field && rawAvailable(); } },

    // ── コメント（comment.js:893-916） ──
    { cat: 'comment', keys: 'c',           desc: 'モードを開始 / 終了', scope: 'all',
      enabled: commentKey },
    { cat: 'comment', keys: 'j / k',       desc: '対象の行・要素を移動', scope: 'all',
      enabled: function(s) { return s.comment && commentKey(s); } },
    { cat: 'comment', keys: 'Shift+j / k', desc: '複数行をレンジ選択', scope: 'all',
      enabled: function(s) { return s.comment && commentKey(s); } },
    { cat: 'comment', keys: 'Enter',       desc: 'コメントを付与', scope: 'all',
      enabled: function(s) { return s.comment && commentKey(s); } },
    { cat: 'comment', keys: 'n / p',       desc: '次 / 前のコメントへジャンプ', scope: 'all',
      enabled: function(s) { return s.comment && commentKey(s); } },
    { cat: 'comment', keys: 'e',           desc: 'コメントを編集', scope: 'all',
      enabled: function(s) { return s.comment && commentKey(s); } },
    { cat: 'comment', keys: 'x',           desc: 'コメントを削除（Delete も可）', scope: 'all',
      enabled: function(s) { return s.comment && commentKey(s); } },
    { cat: 'comment', keys: 'y',           desc: 'コメントを全部コピー', scope: 'all',
      enabled: function(s) { return s.comment && commentKey(s); } },

    // ── ファイル移動（folder.js:409-421。入力欄は見ずオーバーレイだけで休止する） ──
    { cat: 'files', keys: '] / [', desc: '次 / 前のファイルへ', scope: 'folder',
      enabled: function(s) { return !s.overlay; } },
    { cat: 'files', keys: 'Tab',   desc: '本文 ⇄ ファイルツリー のフォーカス切替', scope: 'folder',
      enabled: function(s) { return !s.overlay; } },

    // ── ファイルツリー（folder.js:424-437。ツリーにフォーカスがある時だけ） ──
    { cat: 'tree', keys: 'j / k',     desc: '上 / 下に移動', scope: 'folder',
      enabled: function(s) { return s.tree && !s.overlay; } },
    { cat: 'tree', keys: 'g / G',     desc: '先頭 / 末尾へ', scope: 'folder',
      enabled: function(s) { return s.tree && !s.overlay; } },
    { cat: 'tree', keys: 'Enter / l', desc: '開く & 展開', scope: 'folder',
      enabled: function(s) { return s.tree && !s.overlay; } },
    { cat: 'tree', keys: 'h',         desc: '畳む / 親へ', scope: 'folder',
      enabled: function(s) { return s.tree && !s.overlay; } },

    // ── ウィンドウ（common.js:144 / init.js:87 / contextmenu.js） ──
    { cat: 'window', keys: '⌘A',       desc: '本文を全選択', scope: 'all',
      enabled: function(s) { return !s.field; } },
    { cat: 'window', keys: '右クリック', desc: 'コンテキストメニュー', scope: 'all',
      enabled: function() { return true; } },
    { cat: 'window', keys: '⌘W',       desc: 'ウィンドウを閉じる', scope: 'all',
      enabled: function() { return true; } },
    { cat: 'window', keys: '⌘Q',       desc: '終了', scope: 'all',
      enabled: function() { return true; } },

    // ── ヘルプ（cmdpanel.js / help.js:141-152） ──
    { cat: 'help', keys: '⌘/',  desc: 'コマンドパネルを開閉', scope: 'all',
      enabled: function() { return true; } },
    { cat: 'help', keys: '?',   desc: 'ショートカット一覧を開閉', scope: 'all',
      enabled: function(s) { return !s.field; } },
    { cat: 'help', keys: 'Esc', desc: '検索 / メニュー / ヘルプ / コメントを閉じる', scope: 'all',
      enabled: function() { return true; } }
  ];

  // 現在のモードでこの scope を表示するか。MD_MENU_MODE は起動時に決まるが、
  // 参照は呼び出し時に行う（スクリプトの評価順に依存しないため）。
  function inScope(scope) {
    var mode = window.MD_MENU_MODE;
    if (scope === 'nostdin') return mode !== 'stdin';
    if (scope === 'folder') return mode === 'folder';
    return true; // 'all' と未指定
  }

  // 現在のモードで表示すべきバインドを、定義順で返す。
  function visible() {
    return BINDS.filter(function(b) { return inScope(b.scope); });
  }

  // 現在のモードで表示すべきカテゴリを [{ cat, binds }] の形で返す。
  // 中身が 1 つも無いカテゴリ（stdin での「ファイル移動」等）は落とす。
  // help.js / cmdpanel.js の描画はどちらもこれを回すだけにする。
  function groups() {
    var out = [];
    CATEGORIES.forEach(function(cat) {
      var binds = BINDS.filter(function(b) {
        return b.cat === cat.id && inScope(b.scope);
      });
      if (binds.length) out.push({ cat: cat, binds: binds });
    });
    return out;
  }

  // ── 現在の状態スナップショット ────────────────────────────────
  // enabled() に渡す。状態源はすべて既存モジュールの公開述語で、ここでは新しい状態を持たない。
  function state() {
    var C = window.MdCommon || {};
    return {
      mode: window.MD_MENU_MODE,
      // コメントモード中か（comment.js）
      comment: !!(window.MdComment && MdComment.isMode && MdComment.isMode()),
      // ファイルツリーにフォーカスがあるか（folder モードのツリー操作の起点）
      tree: !!(C.isSidebarFocused && C.isSidebarFocused()),
      // 検索バー / ヘルプ / 右クリメニュー / コメント入力中のいずれかが開いているか
      overlay: !!(C.isOverlayOpen && C.isOverlayOpen()),
      // 文字入力欄にフォーカスがあるか
      field: !!(C.isFieldEl && C.isFieldEl(document.activeElement)),
      // 入力欄・ボタン・リンクのいずれかにフォーカスがあるか（素キーを譲る条件）
      interactive: !!(C.isInteractiveFocus && C.isInteractiveFocus())
    };
  }

  // 状態が変わったかを安く比べるための署名。パネルの再描画を間引くのに使う。
  function signature(s) {
    return [s.mode, s.comment, s.tree, s.overlay, s.field, s.interactive, rawAvailable()].join('|');
  }

  window.MdKeymap = {
    categories: CATEGORIES,
    binds: BINDS,
    inScope: inScope,
    visible: visible,
    groups: groups,
    state: state,
    signature: signature
  };
})();
