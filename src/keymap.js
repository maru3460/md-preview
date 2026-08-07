// キーボードショートカットの単一定義元（Single Source of Truth）。
//
// 「キー・説明・カテゴリ・表示範囲・効く文脈・実処理」をこの表だけに置く。
// ヘルプ（`?` 一覧）の表示も、実際の keydown のディスパッチも、両方ここから出る。
// 以前は表が表示専用で、実処理は 8 ファイルに散った keydown リスナが持っていたため、
// キーを変えるにはハンドラと表の両方を直す必要があった（＝必ずズレる形だった）。
//
// 実処理そのものは各モジュールが持ち、`MdKeymap.on(<run>, fn)` で差し込む。
// この表は「どのキーが、どの文脈で、どのハンドラを呼ぶか」だけを決める。
//
// JS を通らないキーもある: ⌃⌘F（フルスクリーン）と ⌘Q ⌘Z ⌘X ⌘C ⌘V は macOS の
// メニュー項目（platform.rs の setup_menu）が処理するので keydown が WebView に届かない。
// それらは表示専用の行（run 無し）として並べる。
(function() {
  // カテゴリ。BINDS の並びもこの順に揃える。`?` 一覧は見出しを出さず平坦に流すので、
  // ここでの順序がそのまま表示順になる。
  var CATEGORIES = [
    { id: 'scroll',  label: '読み進める' },
    { id: 'find',    label: '探す・飛ぶ' },
    { id: 'view',    label: '表示を切り替える' },
    { id: 'comment', label: 'コメント' },
    { id: 'files',   label: 'ファイル移動' },
    { id: 'tree',    label: 'ファイルツリー' },
    { id: 'app',     label: 'ウィンドウ・ヘルプ' }
  ];

  // ── 文脈の述語 ────────────────────────────────────────────────
  // 排他は stopPropagation ではなく「フォーカスの所在」と「モードの有無」で取る。
  // 同じキーを複数の行が持つ場合（j/k は 本文 / ツリー / コメント中 の 3 つ）、
  // when が互いに排他になるよう書く。そうしておけば表の順序に依存しない。
  function inField(e) { return !!(window.MdCommon && MdCommon.isFieldEl(e.target)); }
  function overlayOpen() { return !!(window.MdCommon && MdCommon.isOverlayOpen()); }
  function inTree() { return !!(window.MdCommon && MdCommon.isSidebarFocused()); }
  function interactive() { return !!(window.MdCommon && MdCommon.isInteractiveFocus()); }
  function inCommentMode() { return !!(window.MdComment && MdComment.isMode && MdComment.isMode()); }

  // 素キー（修飾なし）を「アプリの操作」として受け取ってよい状況か。
  // 入力欄とオーバーレイ表示中は本文の操作を止める。
  function bare(e) {
    return !e.metaKey && !e.ctrlKey && !e.altKey && !inField(e) && !overlayOpen();
  }
  // 本文のスクロール・コメント操作。ボタンやリンクにフォーカスがある間は譲る
  // （Space でボタンが押されてしまうため）。
  function body(e) { return bare(e) && !interactive(); }
  // ⌘（または Ctrl）+ 文字。入力欄ではブラウザ既定に委ねる。
  function cmd(e) { return (e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && !inField(e); }
  // 同じく ⌘+文字だが、入力欄でも奪う。**開閉トグル**に使う: 自分の入力欄に
  // フォーカスがある状態から閉じられないと、オーバーレイに閉じ込められてしまう。
  function cmdAnywhere(e) { return (e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey; }
  // ⌘ のみ（Ctrl は含めない）。⌃W は入力欄で「単語削除」の意味を持つので巻き込まない。
  function metaOnly(e) { return e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey; }

  // ── キーの一致判定 ──────────────────────────────────────────
  // e.key を厳密に見る（'g' と 'G' を区別する必要があるため）。
  function keys() {
    var want = Array.prototype.slice.call(arguments);
    return function(e) { return want.indexOf(e.key) >= 0; };
  }
  // ⌘系の文字キー。レイアウトや IME で e.key が化けても拾えるよう code も見る。
  function letter(ch) {
    return function(e) {
      return (e.key || '').toLowerCase() === ch || e.code === 'Key' + ch.toUpperCase();
    };
  }

  // ── 表 ───────────────────────────────────────────────────────
  // scope（ヘルプに出す範囲）:
  //   'all'     … 全モード（stdin / single / folder）
  //   'nostdin' … stdin 以外（ファイル/git がある single・folder）
  //   'folder'  … folder モードのみ（サイドバーとファイル移動がある時）
  // run: ハンドラ名。複数の行が同じハンドラを共有してよい（表示は分けたいが処理は
  //      1 本、というものがある。スクロール系とツリー操作がそれ）。
  var BINDS = [
    // ── 読み進める（keyscroll.js） ──
    { cat: 'scroll', keys: 'j / k', desc: '1 行スクロール 下 / 上', scope: 'all',
      run: 'scroll', match: keys('j', 'k'),
      when: function(e) { return body(e) && !inTree() && !inCommentMode(); } },
    { cat: 'scroll', keys: 'd / u', desc: '半ページ 下 / 上', scope: 'all',
      run: 'scroll', match: keys('d', 'u'),
      when: function(e) { return body(e) && !inTree(); } },
    { cat: 'scroll', keys: 'Space', desc: '1 ページ送り（Shift+Space で戻る）', scope: 'all',
      run: 'scroll', match: keys(' '),
      when: function(e) { return body(e) && !inTree(); } },
    { cat: 'scroll', keys: 'g / G', desc: '冒頭 / 末尾へ', scope: 'all',
      run: 'scroll', match: keys('g', 'G'),
      when: function(e) { return body(e) && !inTree(); } },

    // ── 探す・飛ぶ（search.js / toc.js / palette.js） ──
    // `/` と `?` はどのフォーカスでも開けるようにする（ツリー内でも効く）。
    { cat: 'find', keys: '/', desc: '検索を開く（開くだけ。閉じるのは Esc / ⌘F）', scope: 'all',
      run: 'search-open', match: keys('/'), when: bare },
    { cat: 'find', keys: '⌘F', desc: '検索を開閉', scope: 'all',
      run: 'search-toggle', match: letter('f'),
      // ⌃⌘F は macOS 標準のフルスクリーン（下の表示専用行）なので譲る。WKWebView は
      // ⌘ 系を先に web へ流すので、ここで preventDefault するとメニュー項目に届かない。
      when: function(e) { return cmdAnywhere(e) && !(e.metaKey && e.ctrlKey); } },
    { cat: 'find', keys: '⌘T', desc: 'アウトライン（見出しナビ）を開閉', scope: 'all',
      run: 'toc-toggle', match: letter('t'), when: cmdAnywhere },
    { cat: 'find', keys: '⌘P', desc: 'ファイル検索（あいまい検索。未入力なら git 変更ファイルが先頭）', scope: 'folder',
      run: 'palette-toggle', match: letter('p'), when: cmdAnywhere },

    // ── 表示を切り替える（viewmode.js） ──
    { cat: 'view', keys: '⌘D', desc: 'git 差分表示を切り替え', scope: 'nostdin',
      run: 'view-diff', match: letter('d'), when: cmd },
    { cat: 'view', keys: '⌘R', desc: 'raw（ソース）表示を切り替え', scope: 'nostdin',
      run: 'view-raw', match: letter('r'), when: cmd },

    // ── コメント（comment.js） ──
    { cat: 'comment', keys: 'c', desc: 'コメントモード開始/終了', scope: 'all',
      run: 'comment-toggle', match: keys('c'),
      when: function(e) { return body(e) && !inTree(); } },
    { cat: 'comment', keys: 'j / k / Enter', desc: 'コメント中: 移動（Shift+j/k でレンジ）/ Enter で付与', scope: 'all',
      run: 'comment-mode', match: keys('j', 'J', 'k', 'K', 'Enter', 'ArrowDown', 'ArrowUp'),
      when: function(e) { return body(e) && !inTree() && inCommentMode(); } },
    { cat: 'comment', keys: 'n / p / e / x / y', desc: 'コメント中: 巡回 / 編集 / 削除(Delete可) / 全部コピー', scope: 'all',
      run: 'comment-mode', match: keys('n', 'p', 'e', 'x', 'y', 'Delete'),
      when: function(e) { return body(e) && !inTree() && inCommentMode(); } },

    // ── ファイル移動（folder.js） ──
    { cat: 'files', keys: '] / [', desc: '次 / 前のファイルへ（表示中のファイルを巡回）', scope: 'folder',
      run: 'file-cycle', match: keys('[', ']'), when: bare },
    { cat: 'files', keys: 'Tab', desc: '本文 ⇄ ファイルツリー のフォーカス切替', scope: 'folder',
      run: 'focus-toggle', match: keys('Tab'), when: bare },

    // ── ファイルツリー（folder.js。ツリーにフォーカスがある時） ──
    { cat: 'tree', keys: 'ツリー内', desc: 'j/k 移動・g/G 端・Enter/l 開く&展開・h 畳む/親へ', scope: 'folder',
      run: 'tree', match: keys('j', 'k', 'g', 'G', 'l', 'h', 'Enter',
                               'ArrowDown', 'ArrowUp', 'ArrowLeft', 'ArrowRight'),
      when: function(e) { return bare(e) && inTree(); } },

    // ── ウィンドウ・ヘルプ ──
    { cat: 'app', keys: '⌘A', desc: '本文を全選択', scope: 'all',
      run: 'select-body', match: letter('a'), when: cmd },
    // JS を通らない（macOS のメニュー項目が処理する）。表示のためだけの行。
    { cat: 'app', keys: '⌃⌘F', desc: 'フルスクリーンを切り替え（緑ボタンと同じ）', scope: 'all' },
    { cat: 'app', keys: '⌘W', desc: 'ウィンドウを閉じる', scope: 'all',
      run: 'window-close', match: letter('w'), when: metaOnly },
    { cat: 'app', keys: '⌘Q', desc: '終了', scope: 'all' },
    { cat: 'app', keys: '右クリック', desc: 'コンテキストメニュー', scope: 'all' },
    { cat: 'app', keys: '?', desc: 'このヘルプを開閉', scope: 'all',
      run: 'help-toggle', match: keys('?'),
      // ヘルプ自身が開いている時も ? で閉じられるよう、オーバーレイ判定は使わない。
      when: function(e) { return !e.metaKey && !e.ctrlKey && !e.altKey && !inField(e); } },
    // Esc は common.js のオーバーレイ レジストリが一括で受ける（最前面の 1 つを閉じる）。
    { cat: 'app', keys: 'Esc', desc: '検索 / メニュー / ヘルプ / コメントを閉じる', scope: 'all' }
  ];

  // ── ディスパッチ ─────────────────────────────────────────────
  var handlers = {};

  // 現在のモードでこの scope を表示するか。MD_MENU_MODE は起動時に決まるが、
  // 参照は呼び出し時に行う（スクリプトの評価順に依存しないため）。
  function inScope(scope) {
    var mode = window.MD_MENU_MODE;
    if (scope === 'nostdin') return mode !== 'stdin';
    if (scope === 'folder') return mode === 'folder';
    return true; // 'all' と未指定
  }

  // 現在のモードで表示すべきバインドを、定義順（＝カテゴリ順）で返す。
  function visible() {
    return BINDS.filter(function(b) { return inScope(b.scope); });
  }

  document.addEventListener('keydown', function(e) {
    // オーバーレイ内の入力欄など、より内側のハンドラが既に処理したキーは触らない
    // （例: ファイル検索の入力欄の ⌃p は「上へ移動」。⌘P のトグルに奪わせない）。
    if (e.defaultPrevented) return;
    for (var i = 0; i < BINDS.length; i++) {
      var b = BINDS[i];
      if (!b.run || !b.match) continue;          // 表示専用の行
      if (!inScope(b.scope)) continue;           // そのモードに無いキー
      var fn = handlers[b.run];
      if (!fn) continue;                         // 担当モジュールが未初期化
      if (!b.match(e)) continue;
      if (b.when && !b.when(e)) continue;
      e.preventDefault();
      fn(e);
      return;                                    // 最初に当たった 1 つだけ
    }
  });

  window.MdKeymap = {
    categories: CATEGORIES,
    binds: BINDS,
    inScope: inScope,
    visible: visible,
    // 実処理を差し込む。同名を複数回呼んだら後勝ち（モジュールは 1 度しか呼ばない）。
    on: function(run, fn) { handlers[run] = fn; }
  };
})();
