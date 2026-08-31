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
  // 「モード中」だけでは j / k の行き先を決められない。html の iframe 表示・git 差分・
  // 巨大ソースは錨る行ユニットが無く、コメント側が取ってもキーが黙って消えるので、
  // 錨れるかどうかで分ける（錨れないならスクロールへ戻す）。
  function canAnchor() { return !!(window.MdComment && MdComment.canAnchor && MdComment.canAnchor()); }

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
  // ⌘⇧+文字（タブ操作）。metaOnly が Shift を弾くので、両者は自動的に排他になる。
  function metaShift(e) { return e.metaKey && !e.ctrlKey && e.shiftKey && !e.altKey; }

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
  // ⌘1..⌘9 の数字キー。
  function digit19(e) { return /^[1-9]$/.test(e.key); }

  // ── 表 ───────────────────────────────────────────────────────
  // 起動モードはフォルダ 1 本なので、ここに出る行はどの起動の仕方でも全部効く
  // （以前は single / stdin 用に scope で出し分けていた）。
  // run: ハンドラ名。複数の行が同じハンドラを共有してよい（表示は分けたいが処理は
  //      1 本、というものがある。スクロール系とツリー操作がそれ）。
  var BINDS = [
    // ── 読み進める（keyscroll.js） ──
    { cat: 'scroll', keys: 'j / k', desc: '1 行スクロール 下 / 上',
      run: 'scroll', match: keys('j', 'k'),
      when: function(e) { return body(e) && !inTree() && !canAnchor(); } },
    { cat: 'scroll', keys: 'd / u', desc: '半ページ 下 / 上',
      run: 'scroll', match: keys('d', 'u'),
      when: function(e) { return body(e) && !inTree(); } },
    { cat: 'scroll', keys: 'Space', desc: '1 ページ送り（Shift+Space で戻る）',
      run: 'scroll', match: keys(' '),
      when: function(e) { return body(e) && !inTree(); } },
    { cat: 'scroll', keys: 'g / G', desc: '冒頭 / 末尾へ',
      run: 'scroll', match: keys('g', 'G'),
      when: function(e) { return body(e) && !inTree(); } },

    // ── 探す・飛ぶ（search.js / toc.js / palette.js） ──
    // `/` と `?` はどのフォーカスでも開けるようにする（ツリー内でも効く）。
    { cat: 'find', keys: '/', desc: '検索を開く（開くだけ。閉じるのは Esc / ⌘F）',
      run: 'search-open', match: keys('/'), when: bare },
    { cat: 'find', keys: '⌘F', desc: '検索を開閉',
      run: 'search-toggle', match: letter('f'),
      // ⌃⌘F は macOS 標準のフルスクリーン（下の表示専用行）なので譲る。WKWebView は
      // ⌘ 系を先に web へ流すので、ここで preventDefault するとメニュー項目に届かない。
      when: function(e) { return cmdAnywhere(e) && !(e.metaKey && e.ctrlKey); } },
    { cat: 'find', keys: '⌘T', desc: 'アウトライン（見出しナビ）を開閉',
      run: 'toc-toggle', match: letter('t'), when: cmdAnywhere },
    { cat: 'find', keys: '⌘P', desc: 'ファイル検索（あいまい検索。未入力なら git 変更ファイルが先頭）',
      run: 'palette-toggle', match: letter('p'), when: cmdAnywhere },

    // ── 表示を切り替える（viewmode.js） ──
    { cat: 'view', keys: '⌘D', desc: 'git 差分表示を切り替え',
      run: 'view-diff', match: letter('d'), when: cmd },
    { cat: 'view', keys: '⌘R', desc: 'raw（ソース）表示を切り替え',
      run: 'view-raw', match: letter('r'), when: cmd },

    // ── コメント（comment.js） ──
    { cat: 'comment', keys: 'c', desc: 'コメントモード開始/終了（HTML 表示・git 差分・巨大ソースは行に付けられない）',
      run: 'comment-toggle', match: keys('c'),
      when: function(e) { return body(e) && !inTree(); } },
    { cat: 'comment', keys: 'j / k / Enter', desc: 'コメント中: 移動（Shift+j/k でレンジ）/ Enter で付与',
      run: 'comment-mode', match: keys('j', 'J', 'k', 'K', 'Enter', 'ArrowDown', 'ArrowUp'),
      when: function(e) { return body(e) && !inTree() && canAnchor(); } },
    // 一覧は他ファイルのコメントも並ぶ横断インデックスなので、錨れない表示（html 等）でも
    // 巡回・全部コピーは使わせる（n / p はフォルダモードなら md 側へ切り替えて着地する）。
    { cat: 'comment', keys: 'n / p / e / x / y', desc: 'コメント中: 巡回 / 編集 / 削除(Delete可) / 全部コピー',
      run: 'comment-mode', match: keys('n', 'p', 'e', 'x', 'y', 'Delete'),
      when: function(e) { return body(e) && !inTree() && inCommentMode(); } },

    // ── ファイル移動（folder.js） ──
    { cat: 'files', keys: '] / [', desc: '次 / 前のファイルへ（表示中のファイルを巡回）',
      run: 'file-cycle', match: keys('[', ']'), when: bare },
    // Tab と ⇧Tab は同じキーを shift の有無で分ける。when が互いに排他なので
    // 表の順序には依存しない。
    { cat: 'files', keys: 'Tab', desc: '本文 ⇄ ファイルツリー のフォーカス切替',
      run: 'focus-toggle', match: keys('Tab'),
      when: function(e) { return bare(e) && !e.shiftKey; } },

    // ── タブ（tabs.js） ──
    { cat: 'files', keys: '⇧Tab', desc: '次のタブへ（端まで行ったら先頭へ折り返す）',
      run: 'tab-cycle', match: keys('Tab'),
      when: function(e) { return bare(e) && e.shiftKey; } },
    // 入力欄とオーバーレイ表示中は譲る。⌘P のパレットを開いたまま裏のファイルだけが
    // 切り替わると、別ファイルの上にオーバーレイが残って何が起きたか分からなくなる。
    { cat: 'files', keys: '⌘1 … ⌘9', desc: 'n 番目のタブへ（⌘9 は最後のタブ）',
      run: 'tab-goto', match: digit19,
      when: function(e) { return cmd(e) && !overlayOpen(); } },

    // ── ファイルツリー（folder.js） ──
    { cat: 'tree', keys: '⌘B', desc: 'ファイルツリー（左サイドバー）を開閉',
      run: 'sidebar-toggle', match: letter('b'), when: cmdAnywhere },
    // 以下はツリーにフォーカスがある時だけ。
    { cat: 'tree', keys: 'ツリー内', desc: 'j/k 移動・g/G 端・Enter/l 開く&展開・h 畳む/親へ',
      run: 'tree', match: keys('j', 'k', 'g', 'G', 'l', 'h', 'Enter',
                               'ArrowDown', 'ArrowUp', 'ArrowLeft', 'ArrowRight'),
      when: function(e) { return bare(e) && inTree(); } },

    // ── ウィンドウ・ヘルプ ──
    { cat: 'app', keys: '⌘A', desc: '本文を全選択',
      run: 'select-body', match: letter('a'), when: cmd },
    // JS を通らない（macOS のメニュー項目が処理する）。表示のためだけの行。
    { cat: 'app', keys: '⌃⌘F', desc: 'フルスクリーンを切り替え（緑ボタンと同じ）' },
    // ⌘W はタブを閉じる（VSCode と同じ）。最後の 1 枚まで閉じたらウィンドウが
    // 閉じるので、押し続けた時の終端は変わらない。
    { cat: 'app', keys: '⌘W', desc: 'タブを閉じる（最後の 1 枚ならウィンドウを閉じる）',
      run: 'tab-close', match: letter('w'), when: metaOnly },
    { cat: 'app', keys: '⌘⇧W', desc: 'ウィンドウを閉じる',
      run: 'window-close', match: letter('w'), when: metaShift },
    { cat: 'app', keys: '⌘Q', desc: '終了' },
    { cat: 'app', keys: '右クリック', desc: 'コンテキストメニュー' },
    { cat: 'app', keys: '?', desc: 'このヘルプを開閉',
      run: 'help-toggle', match: keys('?'),
      // ヘルプ自身が開いている時も ? で閉じられるよう、オーバーレイ判定は使わない。
      when: function(e) { return !e.metaKey && !e.ctrlKey && !e.altKey && !inField(e); } },
    // Esc は common.js のオーバーレイ レジストリが一括で受ける（最前面の 1 つを閉じる）。
    { cat: 'app', keys: 'Esc', desc: '検索 / メニュー / ヘルプ / コメントを閉じる' }
  ];

  // ── ディスパッチ ─────────────────────────────────────────────
  var handlers = {};

  document.addEventListener('keydown', function(e) {
    // オーバーレイ内の入力欄など、より内側のハンドラが既に処理したキーは触らない
    // （例: ファイル検索の入力欄の ⌃p は「上へ移動」。⌘P のトグルに奪わせない）。
    if (e.defaultPrevented) return;
    for (var i = 0; i < BINDS.length; i++) {
      var b = BINDS[i];
      if (!b.run || !b.match) continue;          // 表示専用の行
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
    // 実処理を差し込む。同名を複数回呼んだら後勝ち（モジュールは 1 度しか呼ばない）。
    on: function(run, fn) { handlers[run] = fn; },
    // この run に担当モジュールが居るか。ディスパッチは居なければ黙って読み飛ばす
    // （＝キーが無反応になるだけでエラーにならない）ので、テストから覗けるようにしておく。
    has: function(run) { return typeof handlers[run] === 'function'; }
  };
})();
