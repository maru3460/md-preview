// プレビューにコメント → まとめてコピーして Claude Code へ貼る機能。<head> に inline され
// window.MdComment を公開する。init.js（単一/stdin）と folder.js（フォルダ）の両方から
// init() で使う。コメントの真実は JS 配列 comments[] が持ち、DOM のマーカーは表示のための
// 派生物（ホットリロードで消えても配列から redraw() で貼り直す）。
//
// 対象ユニットは html.rs が振った [data-src-line] を持つ要素（見出し・段落・リスト項目・
// 表の行・引用・コードブロック）。クリックで最寄り 1 ユニット、ドラッグで複数行レンジ。
//
// 安全性: クリップボード書き込みは navigator.clipboard で JS 完結（IPC を介さない）。
// data-src-line は数値のみで、コメント本文/引用は textContent 経由でしか DOM に入れない。
(function() {
  // ── 状態（配列がコメントの真実） ──────────────────────────────
  var comments = [];      // { id, file, startLine, endLine, quote, body }
  var nextId = 1;
  var mode = false;       // コメントモードの ON/OFF（全モード共通・ファイル切替で保持）
  var opts = null;        // { getContainer, getFile }

  // ドラッグ選択の途中状態。
  var dragging = false;
  var dragStartUnit = null;
  var dragEndUnit = null; // ドラッグ中に最後に塗った端。埋め込みの上で離しても範囲を保つ
  var dragUnits = null;   // ドラッグ開始時のユニット配列スナップショット（毎 mousemove の全走査を避ける）
  var handleUnit = null;  // 「+」ハンドルが今指しているユニット

  // ── 環境ヘルパ ────────────────────────────────────────────────
  // 本文を含むホスト要素。単一=.markdown-body / フォルダ=#preview-pane。
  function hostEl() { return opts && opts.getContainer ? opts.getContainer() : null; }
  // 現在ファイルの相対パス（file:line の file 部）。
  function currentFile() {
    var f = opts && opts.getFile ? opts.getFile() : null;
    return f || '';
  }

  // いま raw 表示（⌘R）で見ているか。コメントを付けた表示を覚えるのに使う。
  // 非 md ファイルのソース表示は「その人の通常表示」なので false——raw トグル自体が
  // 無いファイルなので、巡回で切り替える相手もない。
  function isRawView() { return !!(window.MdRaw && MdRaw.isActive && MdRaw.isActive()); }
  // 現在ファイルで raw トグルが意味を持つか（md なら true、非 md は false）。
  function rawAvailable() { return !!(window.MdRaw && MdRaw.isAvailable && MdRaw.isAvailable()); }

  // コメントが「今の表示」のものか。本文への描画（マーカー/バッジ/埋め込み/ホバー）は
  // 付けた表示と現在の表示が一致するものだけに絞る——raw の行コメントがプレビューの
  // ブロック全体に落ちる（コードや mermaid の全体に色帯が付く）粗い錨を見せない。
  // サイドバー一覧は全件出す（横断インデックス。n/p・クリックが表示ごと切り替えて
  // 着地する）。非 md のソースビューは表示が 1 つしかないので常に一致。
  function inCurrentView(c) {
    if (!rawAvailable()) return true;
    return !!c.raw === isRawView();
  }

  // ── raw / ソース表示の行ユニット ──────────────────────────────
  // ソース表示（raw トグル・非 md ファイル）は全文が 1 個の <pre><code> で、行の単位に
  // なる要素が DOM に無い。1 行ずつ span で包むと hljs のハイライト（複数行にまたがる
  // 文字列・コメント）が壊れるので <code> には触れず、透明な行レイヤを上に重ねて
  // [data-src-line] を持たせる。これで既存のユニット処理がそのまま raw にも効く。
  //
  // 位置合わせは座標計算をしない。行レイヤは pre と同じ font-size / line-height を共有し、
  // 各行は空でも 1 行分の高さを持つ（base.css の ::before）ので、行ボックスを積むだけで
  // コード側の行と揃う（ソースビューの pre は折り返さないので 1 行 = 1 段）。
  var SRC_ROWS_MAX = 10000;   // hljs 無効化と同じ規模。これを超えたら行ユニットは作らない
  var srcTooBig = false;      // 直近の ensureSourceRows が大きすぎて諦めたか（案内用）

  function ensureSourceRows(host) {
    srcTooBig = false;
    if (!host) return;
    host.querySelectorAll('.source-main').forEach(function(main) {
      if (main.querySelector('.md-src-rows')) return;
      var code = main.querySelector('pre code');
      if (!code) return;
      // 末尾の改行は「余分な空行」なので数えない（行番号ガターと同じ数え方に揃える）。
      // textContent は hljs 適用後も元のソースのままなので、引用にもそのまま使える。
      var lines = code.textContent.replace(/\n$/, '').split('\n');
      if (lines.length > SRC_ROWS_MAX) { srcTooBig = true; return; }
      var layer = document.createElement('div');
      layer.className = 'md-src-rows';
      var frag = document.createDocumentFragment();
      for (var i = 0; i < lines.length; i++) {
        var row = document.createElement('div');
        row.className = 'md-src-row';
        row.dataset.srcLine = String(i + 1);
        frag.appendChild(row);
      }
      layer.appendChild(frag);
      // 引用元の行はレイヤが持つ（ユニットごとに全文 textContent を読み直さないため）。
      layer.mdLines = lines;
      // 重ねるのは pre を包む箱（Copy ボタンの .code-wrapper）。行番号ガターの上には
      // かぶせない——色帯や選択の塗りが番号を潰さないようにするため。hydrate は
      // addCopyButtons → reanchor の順なので普通はもう在るが、無ければ自分で用意する
      // （.source-main に直接重ねるとガターまで覆ってしまうため）。
      var pre = code.parentNode;
      var box = pre.parentElement;
      if (!box || !box.classList.contains('code-wrapper')) {
        box = document.createElement('div');
        box.className = 'code-wrapper';
        main.insertBefore(box, pre);
        box.appendChild(pre);
      }
      box.appendChild(layer);
      // モード中はレイヤがホイールを受けるので、そのままだとコードの横スクロールが死ぬ
      // （実際の横スクローラは overflow-x を持つ <code>（or pre）で、レイヤはその外側）。
      // 横成分だけ手で流す。縦はそのまま親（ページ）へ抜けるので触らない。
      layer.addEventListener('wheel', function(e) {
        if (!e.deltaX) return;
        var sc = (code.scrollWidth > code.clientWidth) ? code : pre;
        sc.scrollLeft += e.deltaX;
      }, { passive: true });
    });
  }

  // 行ユニットが要るのは「コメントモード中」か「このファイルにコメントがある」時だけ。
  // 素の閲覧では 1 行 1 要素を作らない（ソースを開くだけの経路を軽いままにする）。
  var tooBigNotified = -1;   // 案内トーストを出した本文の差し替え世代（重複表示を防ぐ）
  function syncSourceRows(host) {
    if (!host) return;
    var file = currentFile();
    var needed = mode || comments.some(function(c) { return c.file === file && inCurrentView(c); });
    if (!needed) return;
    ensureSourceRows(host);
    // なぜ行を掴めないのかを伝える。モードに入った時だけでなく、モード中に巨大ファイルへ
    // 移った時にも出す（同じ本文で何度も出さないよう世代で抑える）。
    if (mode && srcTooBig && tooBigNotified !== bodyGen()) {
      tooBigNotified = bodyGen();
      toast('大きなファイルなので行コメントは使えません');
    }
  }

  // ── ユニット/引用の抽出 ───────────────────────────────────────
  function unitStart(u) { return parseInt(u.dataset.srcLine, 10); }
  function unitEnd(u) {
    return u.dataset.srcEndLine ? parseInt(u.dataset.srcEndLine, 10) : unitStart(u);
  }

  // host 内の全 [data-src-line] ユニットを配列で返す。呼び出し側で 1 回取得して
  // unitsInRange に渡し回すことで、ドラッグ中や redraw の全走査を減らす。
  function allUnits(host) {
    return host ? Array.prototype.slice.call(host.querySelectorAll('[data-src-line]')) : [];
  }

  // [startLine, endLine] に完全に収まるユニットのうち、他の対象ユニットに入れ子で
  // ない「トップレベル」だけを返す。引用テキストの二重取りを防ぐ。
  // units は allUnits() で事前取得した配列（毎回 querySelectorAll しないため）。
  function unitsInRange(units, startLine, endLine) {
    var inRange = units.filter(function(u) {
      var s = unitStart(u), e = unitEnd(u);
      return s >= startLine && e <= endLine;
    });
    var set = new Set(inRange);
    return inRange.filter(function(u) {
      var p = u.parentElement;
      while (p) { if (set.has(p)) return false; p = p.parentElement; }
      return true;
    });
  }

  // 行番号 → 錨ユニット。その行から始まるユニットを優先し、無ければその行を範囲に含む
  // いちばん小さいユニットへ落とす。raw（1 行 1 ユニット）で段落の途中の行に付けた
  // コメントも、プレビュー表示（段落は開始〜終了行で 1 ユニット）で錨を見つけられる。
  // units は allUnits() の結果を使い回すための任意引数。
  function unitAtLine(host, line, units) {
    if (!host || !line) return null;
    var exact = host.querySelector('[data-src-line="' + line + '"]');
    if (exact) return exact;
    var best = null, bestSpan = Infinity;
    (units || allUnits(host)).forEach(function(u) {
      var s = unitStart(u), e = unitEnd(u);
      if (s <= line && line <= e && (e - s) < bestSpan) { best = u; bestSpan = e - s; }
    });
    return best;
  }

  // 1 ユニットの引用テキスト。コードは行を保つ、散文は空白を畳む。
  // 💬 バッジ・Copy ボタン・ファイル名ラベルといった UI チップは引用に混ぜない
  // （クローンから取り除いてから textContent を読む）。
  function unitQuote(u) {
    // 行レイヤの行は空要素なので、引用はレイヤが持つソース行から取る（1 行 1 ユニット）。
    if (u.classList.contains('md-src-row')) {
      var lines = u.parentElement && u.parentElement.mdLines;
      var i = unitStart(u) - 1;
      return (lines && lines[i] != null) ? lines[i].replace(/\s+$/, '') : '';
    }
    var isCode = u.classList.contains('code-wrapper') || u.tagName === 'PRE';
    var clone = u.cloneNode(true);
    clone.querySelectorAll('.md-cmt-badge, .md-cmt-embed, .copy-btn, .code-filename').forEach(function(n) { n.remove(); });
    var text = clone.textContent || '';
    if (isCode) return text.replace(/\s+$/, '');
    return text.replace(/\s+/g, ' ').trim();
  }

  // 2 ユニット（クリックなら同一）からコメント対象を組み立てる。
  function computeTarget(u1, u2) {
    var startLine = Math.min(unitStart(u1), unitStart(u2));
    var endLine = Math.max(unitEnd(u1), unitEnd(u2));
    var host = hostEl();
    var units = host ? unitsInRange(allUnits(host), startLine, endLine) : [];
    if (!units.length) units = [u1];
    // 引用は 1 ユニット 1 行。中身が空になるユニットは落とすが、ソース表示の空行は
    // ソースの一部なので残す（詰めると引用と実際の行がずれる）。
    var quote = units.map(function(u) { return { u: u, q: unitQuote(u) }; })
      .filter(function(x) { return x.q || x.u.classList.contains('md-src-row'); })
      .map(function(x) { return x.q; })
      .join('\n');
    // アンカー（マーカー/ポップオーバーの基準）は開始行のユニット。
    var anchor = units[0];
    return { startLine: startLine, endLine: endLine, quote: quote, anchorEl: anchor };
  }

  // ── コメント CRUD ─────────────────────────────────────────────
  function addComment(t, body) {
    comments.push({
      id: nextId++,
      file: currentFile(),
      // どの表示で付けたか。n/p のジャンプで同じ見え方へ戻すのに使う（raw は 1 行
      // 単位、プレビューは段落単位なので、着地先が変わると指しているものも変わる）。
      raw: isRawView(),
      startLine: t.startLine,
      endLine: t.endLine,
      quote: t.quote,
      body: body
    });
    anchorScroll(redraw);
  }
  function updateComment(id, body) {
    var c = findComment(id);
    if (c) { c.body = body; anchorScroll(redraw); }
  }
  function deleteComment(id) {
    comments = comments.filter(function(c) { return c.id !== id; });
    anchorScroll(redraw);
  }
  function clearAll() {
    comments = [];
    reviewId = null;
    anchorScroll(redraw);
  }
  function findComment(id) {
    for (var i = 0; i < comments.length; i++) if (comments[i].id === id) return comments[i];
    return null;
  }
  // 現在ファイルで、ユニットの行範囲と重なるコメント一覧（ホバープレビュー用）。
  // 開始行だけで見ると、raw（1 行 1 ユニット）で段落の途中の行に付けたコメントが、
  // プレビュー側の段落ユニット（開始〜終了行）にホバーしても出てこない。
  function commentsCoveringUnit(u) {
    var s = unitStart(u), e = unitEnd(u);
    var file = currentFile();
    return comments.filter(function(c) {
      var end = c.endLine || c.startLine;
      return c.file === file && inCurrentView(c) && c.startLine <= e && end >= s;
    // 行順に返す。大きなユニット（長いコードブロック等）に複数のコメントが重なるとき、
    // e/x の対象（先頭）が追加順で変わらないようにする。
    }).sort(function(a, b) { return a.startLine - b.startLine; });
  }

  // ── ジャンプ（パネル項目 → 本文の file:line） ─────────────────
  function flashUnit(u) {
    if (!u) return;
    u.classList.add('md-cmt-flash');
    setTimeout(function() { u.classList.remove('md-cmt-flash'); }, 1400);
  }
  // ユニットへ着地する（スクロール＆点滅）。モード中はキーボード・カーソルも
  // 着地点へ移す（e/x の対象＝視覚的な現在地を一致させる）。
  function landOn(u) {
    if (!u) return null;
    u.scrollIntoView({ block: 'center', behavior: 'smooth' });
    flashUnit(u);
    if (mode) landKbCursor(u);
    return u;
  }
  // 現在ファイル内の行までスクロール＆点滅。見つかった要素を返す。
  function scrollToLine(line) { return landOn(unitAtLine(hostEl(), line)); }

  // 差分（⌘D）が出ているか。行の錨を持たないので、出たままでは着地できない。
  function diffShown() { return !!(window.MdDiff && MdDiff.isActive && MdDiff.isActive()); }
  // コメントを付けた表示と今の表示が違うか（違えば applyView が切り替える）。
  function viewWillChange(c) {
    if (!!c.raw && rawAvailable()) return !isRawView();
    return isRawView() || diffShown();
  }
  // コメントを付けた表示（raw / 通常）へ切り替える。切り替えたら true。
  function applyView(c) {
    if (!viewWillChange(c)) return false;
    // raw を出す / 畳む: raw のトグルが他モード（差分）も畳むので、差分は触らない。
    if (!!c.raw && rawAvailable()) { MdRaw.toggle(); return true; }
    if (isRawView()) { MdRaw.toggle(); return true; }
    MdDiff.toggle();
    return true;
  }
  // 目的の表示に本文が入れ替わったか。isActive() はトグル直後に立つが、本文の差し替えは
  // フェッチ後なので、錨の種類（行ユニットかどうか）も見て「入れ替わり済み」を判定する。
  function viewSettled(c, u) {
    if (!rawAvailable()) return true;
    var want = !!c.raw;
    return isRawView() === want && u.classList.contains('md-src-row') === want;
  }

  // ジャンプの世代トークン。新しいジャンプが始まったら古いリトライ連鎖を無効化する
  // （連続で別ファイルへ飛んだとき、前のリトライが別ファイルの同じ行番号へ誤着地するのを防ぐ）。
  var jumpGen = 0;
  // 本文の差し替え世代（MdCommon.hydrate のたびに増える）。
  function bodyGen() { return (window.MdCommon && MdCommon.bodyGen) ? MdCommon.bodyGen() : 0; }

  // 目的のファイルと表示が揃ってから着地する。ファイルを開くのも表示の切り替えも非同期で、
  // 本文が差し替わる前にスクロールしても folder / viewmode 側のスクロール復元で流れてしまう。
  // しかも `currentFile()` は差し替えより先に切り替わる（folder.js は currentFilePath を
  // 同期で更新し、本文はフェッチ後に流し込む）ので、ファイル名だけでは「もう目的ファイルの
  // DOM か」を判断できない——**本文の差し替え世代が進んだか**で見る。
  //  ・自分より新しいジャンプが始まったら中断（world 不一致）
  //  ・表示（raw / 通常）の切り替えは目的ファイルの本文が出てから。raw の可否は現在ファイル
  //    基準だし、開くのと同時に切り替えるとフェッチが二重に走って遅い方が勝ってしまう
  //  ・切り替えたら、その差し替えもまた待ってから着地する
  // st は { waitGen: この世代を超える差し替えを待つ（null なら待たない）, switched: 表示を
  // 合わせ終わったか }。
  function landWhenReady(c, tries, gen, st) {
    if (gen !== jumpGen) return;                 // 追い越された
    var next = function() {
      if (tries > 0) { setTimeout(function() { landWhenReady(c, tries - 1, gen, st); }, 60); return; }
      // 待っても揃わないとき（巨大ファイルで行ユニットを作らない等）は、いま出ている
      // 表示のいちばん近いユニットへ落とす。何も出ないより位置が分かる方がよい。
      if (currentFile() === c.file) scrollToLine(c.startLine);
    };
    if (currentFile() !== c.file) return next();
    if (st.waitGen !== null && bodyGen() <= st.waitGen) return next();   // まだ前の本文
    st.waitGen = null;
    if (!st.switched) {
      st.switched = true;
      if (applyView(c)) { st.waitGen = bodyGen(); return next(); }
    }
    var u = unitAtLine(hostEl(), c.startLine);
    if (u && viewSettled(c, u)) { landOn(u); return; }
    next();
  }
  // コメントの file:line へ飛ぶ。別ファイルは folder モードなら開いてから、
  // 付けた表示（raw / 通常）が違えば切り替えてから着地する。
  function gotoComment(c) {
    if (!c.startLine) return;
    jumpGen++;
    var st = { waitGen: null, switched: false };
    if (c.file && c.file !== currentFile()) {
      // single / stdin は別ファイルへ移れないので何もしない。
      if (!opts || typeof opts.openFile !== 'function') return;
      st.waitGen = bodyGen();   // 目的ファイルの本文が届くまで待つ
      opts.openFile(c.file);
    }
    landWhenReady(c, 40, jumpGen, st);
  }

  // n / p でコメントを file→行順に巡回してジャンプ（マウス無しのレビュー導線）。
  // 巡回対象は index ではなく id で覚える（追加/削除/再ソートを跨いでも同じコメントを指す）。
  var reviewId = null;
  function jumpToComment(delta) {
    var list = sortedComments();
    if (!list.length) { toast('コメントはまだありません'); return; }
    var i = -1;
    if (reviewId != null) {
      for (var n = 0; n < list.length; n++) { if (list[n].id === reviewId) { i = n; break; } }
    }
    i = (i < 0) ? (delta < 0 ? list.length - 1 : 0) : i + delta;
    if (i < 0) i = list.length - 1;
    if (i >= list.length) i = 0;
    var c = list[i];
    reviewId = c.id;
    gotoComment(c);
    // サイドバー一覧も巡回対象へ追従させる（強ハイライト＋見える位置へスクロール）。
    updateSideHighlights();
    for (var s = 0; s < sideItems.length; s++) {
      if (sideItems[s].c.id === c.id) { sideItems[s].el.scrollIntoView({ block: 'nearest' }); break; }
    }
    toast((i + 1) + ' / ' + list.length + '  ' + locLabel(c) + (c.raw ? ' (raw)' : ''));
  }

  // いま操作対象のコメント（キーボードの e / x 用）。n/p 直後はその巡回対象、
  // そうでなければキーボード・カーソルが乗っているユニットのコメント。
  function currentComment() {
    if (reviewId != null) {
      var c = findComment(reviewId);
      if (c) return c;
    }
    if (kbCursor && kbCursor.isConnected) {
      var here = commentsCoveringUnit(kbCursor);
      if (here.length) return here[0];
    }
    return null;
  }
  // e: 対象コメントを編集。錨が今の表示に出ていればその場、違うファイル / 違う表示
  // （raw ↔ プレビュー）なら合わせてから開く。
  function editCurrent() {
    var c = currentComment();
    if (!c) { toast('編集するコメントがありません'); return; }
    var anchor = (c.file === currentFile()) ? unitAtLine(hostEl(), c.startLine) : null;
    if (anchor && viewSettled(c, anchor)) { openEditPopover(anchor, c); return; }
    // 同じファイル・同じ表示のまま錨が無い（行が消えた / 巨大ファイルで行ユニットを作らない）
    // なら、待っても出てこないのでその場で錨無しで開く。
    if (c.file === currentFile() && !viewWillChange(c)) { openEditPopover(null, c); return; }
    // ファイル / 表示を合わせて、錨が出るのを待ってから編集を開く。gotoComment と同じ
    // jumpGen を捕捉し、その後に別ジャンプが始まったら中断（古い対象で誤って開かない）。
    gotoComment(c);
    var myGen = jumpGen;
    var tries = 40;
    (function waitEdit() {
      if (myGen !== jumpGen) return;   // 追い越された
      if (c.file === currentFile()) {
        var a = unitAtLine(hostEl(), c.startLine);
        if (a && viewSettled(c, a)) { openEditPopover(a, c); return; }
      }
      if (tries-- > 0) { setTimeout(waitEdit, 60); return; }
      // 待っても錨が出ないとき（別ファイルへ移れない single / 巨大ファイル）は
      // 錨無しで開く。編集そのものはできる。
      openEditPopover(null, c);
    })();
  }
  // x: 対象コメントを削除（確認なしの方針。トーストで結果を返す）。
  function deleteCurrent() {
    var c = currentComment();
    if (!c) { toast('削除するコメントがありません'); return; }
    deleteComment(c.id);
    if (reviewId === c.id) reviewId = null;   // 消したコメントは巡回対象から外す
    toast('コメントを削除しました');
  }

  // 短いフィードバックの浮遊トースト（キーボード操作の結果をボタン無しで返す）。
  var toastEl = null, toastTimer = null;
  function toast(msg) {
    if (!toastEl) {
      toastEl = document.createElement('div');
      toastEl.className = 'md-cmt-toast';
      // スクリーンリーダーにも結果（コピー/削除/巡回位置）を伝える。
      toastEl.setAttribute('role', 'status');
      toastEl.setAttribute('aria-live', 'polite');
      document.body.appendChild(toastEl);
    }
    toastEl.textContent = msg;
    toastEl.classList.add('show');
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(function() { if (toastEl) toastEl.classList.remove('show'); }, 1500);
  }

  // ── マーカー描画（配列 → DOM） ────────────────────────────────
  // コメント id -> マーカーを付けた要素。スクロールごとの inview 判定で使い回す
  // （毎フレーム全ユニットを走査すると、行ユニットが数千ある raw 表示で重くなる）。
  var markEls = {};

  function clearMarkers(host) {
    host.querySelectorAll('.md-cmt-marked').forEach(function(u) { u.classList.remove('md-cmt-marked'); });
    host.querySelectorAll('.md-cmt-badge').forEach(function(b) { b.remove(); });
    host.querySelectorAll('.md-cmt-embed').forEach(function(b) { b.remove(); });
    host.querySelectorAll('.md-cmt-badge-holder').forEach(function(b) { b.remove(); });
  }

  function redraw() {
    var host = hostEl();
    if (host) {
      clearMarkers(host);
      markEls = {};
      // 錨の要素 -> { el, list }。行番号ではなく解決後の要素で束ねる——raw の別々の行に
      // 付けたコメントが、プレビューでは同じ段落へ落ちることがあるため（💬 が 2 個並ぶ）。
      var slots = new Map();
      var file = currentFile();
      // raw / ソース表示は行ユニットが DOM に無いので、必要なときだけ先に作る。
      syncSourceRows(host);
      var all = allUnits(host);   // 1 回だけ取得して全コメントで使い回す
      // インライン埋め込みはモード中だけ。錨の要素 -> そこに出すコメント配列。
      var embedSlots = mode ? new Map() : null;
      comments.forEach(function(c) {
        if (c.file !== file || !inCurrentView(c)) return;
        var units = unitsInRange(all, c.startLine, c.endLine);
        if (!units.length) {
          var one = unitAtLine(host, c.startLine, all);
          if (one) units = [one];
        }
        units.forEach(function(u) { u.classList.add('md-cmt-marked'); });
        markEls[c.id] = units;
        var anchor = units[0];
        if (anchor) {
          var slot = slots.get(anchor);
          if (!slot) { slot = { el: anchor, list: [] }; slots.set(anchor, slot); }
          slot.list.push(c);
        }
        // 埋め込みは範囲の最後のユニットの直後に出す（GitHub 風）。ソース表示の
        // 行レイヤ（.md-src-row）は行ボックスの積み上げで位置を合わせていて、間に
        // ブロックを挟めないので出さない（raw のコメントはサイドバー一覧で読む）。
        if (embedSlots) {
          var tail = units[units.length - 1];
          if (tail && !tail.classList.contains('md-src-row')) {
            var elist = embedSlots.get(tail);
            if (!elist) { elist = []; embedSlots.set(tail, elist); }
            elist.push(c);
          }
        }
      });
      slots.forEach(function(slot) {
        var badge = document.createElement('span');
        badge.className = 'md-cmt-badge';
        badge.setAttribute('contenteditable', 'false');
        badge.textContent = slot.list.length > 1 ? ('💬' + slot.list.length) : '💬';
        badge.addEventListener('click', function(e) {
          e.stopPropagation();
          // 複数件でもパネル（モード）を確実に開いた上で、先頭コメントの編集を開く。
          // 残りはパネル一覧で編集/削除できる（モード外クリックでも無反応にしない）。
          if (!mode) setMode(true);
          openEditPopover(slot.el, slot.list[0]);
        });
        // mermaid はユニットの textContent がそのまま図のソースで、ホットリロード時は
        // バッジ貼り(reanchor・同期)の後に mermaid.run(非同期)が走るため、中に置くと
        // 「💬」が混ざって構文エラーになる。0 高さのホルダーを直前に挟んでそこへ載せる。
        if (slot.el.classList.contains('mermaid')) {
          var holder = document.createElement('div');
          holder.className = 'md-cmt-badge-holder';
          holder.setAttribute('contenteditable', 'false');
          holder.appendChild(badge);
          slot.el.parentNode.insertBefore(holder, slot.el);
        } else {
          slot.el.appendChild(badge);
        }
      });
      if (embedSlots) {
        embedSlots.forEach(function(list, tail) {
          var embed = buildEmbed(list.slice().sort(byFileLine));
          tail.parentNode.insertBefore(embed, tail.nextSibling);
        });
      }

      // 本文が入れ替わった可能性があるのでユニット配列キャッシュを捨てる。
      invalidateKbUnits();
      // モード中は、リロード/ファイル切替で宙に浮いたキーボード・カーソルを立て直す。
      // 同一 DOM の add/delete では kbCursor は生きているので触らない。
      if (mode) {
        if (kbCursor && !kbCursor.isConnected) {
          var reU = host.querySelector('[data-src-line="' + kbCursor.dataset.srcLine + '"]');
          if (reU) { kbCursor = null; landKbCursor(reU); }
          else { clearKb(); initKbCursor(); }
        } else if (!kbCursor) {
          initKbCursor();
        }
      }
    }
    renderPanel();
  }

  // ホットリロード後などに markers を貼り直す（配列は生きている）。
  function reanchor() { redraw(); }

  // ── インライン埋め込み（モード中、アンカー直下に出す GitHub 風の表示） ──
  // ユニットの「兄弟」として直後に差し込む（子に入れるとユニット自身のレイアウトを
  // 壊すため）。data-src-line を持たせず contenteditable=false にして（バッジと同じ
  // 扱い）、クリック/ドラッグ/j·k のユニット選択の対象から外す。真実は comments[] の
  // ままで、これも redraw が毎回貼り直す派生物（ホットリロードにもそのまま乗る）。
  function buildEmbed(list) {
    var box = document.createElement('div');
    box.className = 'md-cmt-embed';
    box.setAttribute('contenteditable', 'false');
    list.forEach(function(c) {
      var item = document.createElement('div');
      item.className = 'md-cmt-embed-item';

      var head = document.createElement('div');
      head.className = 'md-cmt-embed-head';
      var loc = document.createElement('span');
      loc.className = 'md-cmt-embed-loc';
      // レンジコメントは最後のユニットの下に出るので、どこからの範囲かを行で示す。
      loc.textContent = '💬 ' + locLabel(c);
      head.appendChild(loc);
      var edit = document.createElement('button');
      edit.type = 'button';
      edit.className = 'md-cmt-link';
      edit.textContent = '編集';
      edit.addEventListener('click', function(e) {
        e.stopPropagation();
        // 錨は埋め込み自身ではなく直前のユニットにする——埋め込みは redraw のたびに
        // 作り直される使い捨てで、開いている間にホットリロード等の redraw が走ると
        // 錨が宙に浮いてスクロール追従が壊れる。範囲の最後のユニット（＝埋め込みの
        // 直前）なら位置はほぼ同じままで、バッジ/一覧の編集と同じ耐久性になる。
        var units = markEls[c.id];
        var anchor = (units && units[units.length - 1]) || unitAtLine(hostEl(), c.startLine);
        openEditPopover(anchor, c);
      });
      var del = document.createElement('button');
      del.type = 'button';
      del.className = 'md-cmt-link md-cmt-link-danger';
      del.textContent = '削除';
      del.addEventListener('click', function(e) {
        e.stopPropagation();
        if (reviewId === c.id) reviewId = null;
        deleteComment(c.id);
      });
      head.appendChild(edit);
      head.appendChild(del);
      item.appendChild(head);

      var body = document.createElement('div');
      body.className = 'md-cmt-embed-body';
      body.textContent = c.body;
      item.appendChild(body);
      box.appendChild(item);
    });
    return box;
  }

  // ── ポップオーバー（新規/編集の textarea） ───────────────────
  var popover = null;
  var popoverAnchor = null;  // スクロール追従の基準要素
  var popoverPrevFocus = null;  // 開く前のフォーカス（閉じたら戻す）

  function closePopover() {
    if (!popover) return;
    popover.remove();
    popover = null;
    popoverAnchor = null;
    // 開く前のフォーカスへ戻す（キーボード操作で迷子にならないように）。
    var prev = popoverPrevFocus;
    popoverPrevFocus = null;
    if (prev && typeof prev.focus === 'function' && document.contains(prev)) {
      try { prev.focus({ preventScroll: true }); } catch (e) { prev.focus(); }
    }
  }

  function buildPopover(anchorEl, initialBody, onSave) {
    // closePopover が prevFocus を消すので、開く前のフォーカスを先に退避する。
    var prevFocus = document.activeElement;
    closePopover();
    popoverPrevFocus = prevFocus;
    popoverAnchor = anchorEl;
    var pop = document.createElement('div');
    pop.className = 'md-cmt-popover';
    pop.id = 'md-cmt-popover';   // MdCommon.isOverlayOpen が O(1) で存在を見るため
    pop.setAttribute('role', 'dialog');
    pop.setAttribute('aria-label', 'コメントを入力');
    pop.addEventListener('mousedown', function(e) { e.stopPropagation(); });

    var ta = document.createElement('textarea');
    ta.className = 'md-cmt-textarea';
    ta.value = initialBody || '';
    ta.placeholder = 'コメント… (⌘+Enter で保存 / Esc で取消)';
    pop.appendChild(ta);

    // 入力に合わせて縦へ自動で伸ばす（上限は画面の 40%。超えたらスクロール）。
    // 伸びたら再配置して画面内に収める。手動リサイズは自動グローと競合するので
    // 持たない（CSS 側で resize: none）。
    function autoGrow() {
      var cap = Math.round(window.innerHeight * 0.4);
      ta.style.height = 'auto';
      ta.style.height = Math.min(ta.scrollHeight, cap) + 'px';
      ta.style.overflowY = ta.scrollHeight > cap ? 'auto' : 'hidden';
      positionPopover(pop, popoverAnchor);
    }
    ta.addEventListener('input', autoGrow);

    var actions = document.createElement('div');
    actions.className = 'md-cmt-actions';
    var save = document.createElement('button');
    save.type = 'button';
    save.className = 'md-cmt-btn md-cmt-btn-primary';
    save.textContent = '保存';
    var cancel = document.createElement('button');
    cancel.type = 'button';
    cancel.className = 'md-cmt-btn';
    cancel.textContent = '取消';
    actions.appendChild(cancel);
    actions.appendChild(save);
    pop.appendChild(actions);

    function commit() {
      var body = ta.value.trim();
      if (!body) { closePopover(); return; }
      onSave(body);
      closePopover();
    }
    save.addEventListener('click', commit);
    cancel.addEventListener('click', closePopover);
    // Esc（取消）の受け手は MdCommon が一括で持つ。
    ta.addEventListener('keydown', function(e) {
      if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); commit(); }
    });

    document.body.appendChild(pop);
    autoGrow();   // 編集時の長文も開いた時点でちょうどの高さにする
    positionPopover(pop, anchorEl);
    popover = pop;
    setTimeout(function() { ta.focus(); }, 0);
  }

  // アンカー要素の近く（右上寄り）に置き、画面端ではフリップして収める。
  function positionPopover(pop, anchorEl) {
    var rect = anchorEl ? anchorEl.getBoundingClientRect() : { left: 40, top: 40, right: 40, bottom: 60 };
    var pw = pop.offsetWidth, ph = pop.offsetHeight;
    var x = rect.left;
    var y = rect.bottom + 6;
    if (x + pw > window.innerWidth - 8) x = Math.max(8, window.innerWidth - pw - 8);
    if (y + ph > window.innerHeight - 8) y = Math.max(8, rect.top - ph - 6);
    pop.style.left = x + 'px';
    pop.style.top = y + 'px';
  }

  function openNewPopover(target) {
    buildPopover(target.anchorEl, '', function(body) { addComment(target, body); });
  }
  function openEditPopover(anchorEl, c) {
    buildPopover(anchorEl, c.body, function(body) { updateComment(c.id, body); });
  }

  // ── ホバープレビュー（モード外でも確認できる浮遊パネル） ──────
  var preview = null;
  function showPreview(anchorEl, list) {
    hidePreview();
    var box = document.createElement('div');
    box.className = 'md-cmt-preview';
    list.forEach(function(c) {
      var row = document.createElement('div');
      row.className = 'md-cmt-preview-item';
      row.textContent = c.body;
      box.appendChild(row);
    });
    document.body.appendChild(box);
    var rect = anchorEl.getBoundingClientRect();
    var x = Math.min(rect.left, window.innerWidth - box.offsetWidth - 8);
    var y = rect.bottom + 4;
    if (y + box.offsetHeight > window.innerHeight - 8) y = Math.max(8, rect.top - box.offsetHeight - 4);
    box.style.left = Math.max(8, x) + 'px';
    box.style.top = y + 'px';
    preview = box;
  }
  function hidePreview() {
    if (preview) { preview.remove(); preview = null; }
  }

  // ── パネル（右下フロート） ────────────────────────────────────
  var panel = null;

  function ensurePanel() {
    if (panel) return panel;
    panel = document.createElement('div');
    panel.className = 'md-cmt-panel';
    panel.id = 'md-cmt-panel';
    // diff/raw トグルと同じ右下スタックへ入れ、下から詰めて並ぶ（隙間を作らない）。
    var stack = (window.MdCommon && MdCommon.cornerStack) ? MdCommon.cornerStack() : document.body;
    stack.appendChild(panel);
    return panel;
  }

  function renderPanel() {
    if (window.MdToc && MdToc.setCommentsCount) MdToc.setCommentsCount(comments.length);
    renderCorner();
    renderSide();
  }

  // 右下コーナーはモード外の「💬 N」ピル（一覧への入口）だけを出す。モード中の
  // 一覧はサイドバー（コメントタブ）側が持つ。本文のブロック右上バッジは別途
  // redraw が出しているので、両方で「コメントあり」が分かる。
  function renderCorner() {
    var p = ensurePanel();
    p.innerHTML = '';
    var n = comments.length;
    if (mode || n === 0) { p.style.display = 'none'; return; }
    p.style.display = '';
    var pill = document.createElement('button');
    pill.type = 'button';
    pill.className = 'md-cmt-pill';
    pill.textContent = '💬 ' + n;
    pill.title = 'コメント ' + n + ' 件（クリックで一覧）';
    pill.addEventListener('click', function() { setMode(true); });
    p.appendChild(pill);
  }

  // ── サイドバー（コメントタブ）への一覧描画 ────────────────────
  var sideEl = null;     // タブの中身。MdToc.mountComments でパネル内に常駐する
  var sideItems = [];    // 一覧項目とコメントの対応（in-view/巡回ハイライト用）

  function ensureSide() {
    if (sideEl) return sideEl;
    if (!(window.MdToc && MdToc.mountComments)) return null;
    sideEl = document.createElement('div');
    sideEl.className = 'md-cmt-side';
    // Outline タブ/×/⌘T によるモード終了の要求は setMode に集約する
    // （実際のタブ切替は setMode が openComments/closeComments を呼び返す）。
    MdToc.mountComments({
      contentEl: sideEl,
      onExit: function() { setMode(false); }
    });
    return sideEl;
  }

  function renderSide() {
    var side = ensureSide();
    if (!side) return;
    side.innerHTML = '';
    sideItems = [];
    var n = comments.length;

    // キー操作の常設ヒント（ヘルプを開かなくても要点が分かるように）。件数で出し分ける。
    var hint = document.createElement('div');
    hint.className = 'md-cmt-hint';
    hint.textContent = (n === 0)
      ? 'j / k で移動、Enter でコメント（Shift+j/k で複数行）。クリック・ドラッグでも可'
      : 'n / p 巡回 · e 編集 · x 削除 · y 全部コピー · ? 全キー';
    side.appendChild(hint);

    // 一覧は file→行順（n/p の巡回順・全部コピーの出力順と同じ）。巡回が一覧を
    // 上から下へ歩くようにする。
    var listEl = document.createElement('div');
    listEl.className = 'md-cmt-list';
    sortedComments().forEach(function(c) {
      var item = document.createElement('div');
      item.className = 'md-cmt-item';
      item.title = 'クリックで ' + locLabel(c) + (c.raw ? '（raw 表示）' : '') + ' へ移動';
      // 項目クリックでコメント先（file:line）へジャンプ（folder は別ファイルも開く）。
      // 巡回対象（reviewId）もそこへ移す＝クリック後の e / x / n / p がそこから続く。
      item.addEventListener('click', function() {
        reviewId = c.id;
        gotoComment(c);
        updateSideHighlights();
      });

      var loc = document.createElement('div');
      loc.className = 'md-cmt-loc';
      loc.textContent = locLabel(c);
      // raw で付けたコメントは行単位。プレビューで付けたもの（段落単位）と着地先が
      // 変わるので、一覧でも見分けられるようにする。
      if (c.raw) {
        var tag = document.createElement('span');
        tag.className = 'md-cmt-tag';
        tag.textContent = 'raw';
        loc.appendChild(tag);
      }
      item.appendChild(loc);

      var bodyEl = document.createElement('div');
      bodyEl.className = 'md-cmt-body';
      bodyEl.textContent = c.body;
      item.appendChild(bodyEl);

      var row = document.createElement('div');
      row.className = 'md-cmt-item-actions';
      var edit = document.createElement('button');
      edit.type = 'button';
      edit.className = 'md-cmt-link';
      edit.textContent = '編集';
      edit.addEventListener('click', function(e) {
        e.stopPropagation();  // 項目クリック（ジャンプ）を発火させない
        var host = hostEl();
        var anchor = unitAtLine(host, c.startLine);
        if (c.file === currentFile() && anchor) {
          anchor.scrollIntoView({ block: 'center' });
          openEditPopover(anchor, c);
        } else {
          openEditPopover(null, c);
        }
      });
      var del = document.createElement('button');
      del.type = 'button';
      del.className = 'md-cmt-link md-cmt-link-danger';
      del.textContent = '削除';
      del.addEventListener('click', function(e) { e.stopPropagation(); deleteComment(c.id); });
      row.appendChild(edit);
      row.appendChild(del);
      item.appendChild(row);

      // 一覧項目にホバー → 本文の該当ユニットをハイライト。
      item.addEventListener('mouseenter', function() {
        if (c.file !== currentFile() || !inCurrentView(c)) return;
        var host = hostEl();
        var u = unitAtLine(host, c.startLine);
        if (u) u.classList.add('md-cmt-flash');
      });
      item.addEventListener('mouseleave', function() {
        var host = hostEl();
        if (host) host.querySelectorAll('.md-cmt-flash').forEach(function(x) { x.classList.remove('md-cmt-flash'); });
      });

      listEl.appendChild(item);
      sideItems.push({ c: c, el: item });
    });
    side.appendChild(listEl);

    // フッタ: 全部コピー / 全消去。
    var foot = document.createElement('div');
    foot.className = 'md-cmt-panel-foot';
    var copy = document.createElement('button');
    copy.type = 'button';
    copy.className = 'md-cmt-btn md-cmt-btn-primary';
    copy.textContent = '全部コピー';
    copy.disabled = n === 0;
    copy.addEventListener('click', function() { copyAll(copy); });
    var clear = document.createElement('button');
    clear.type = 'button';
    clear.className = 'md-cmt-btn';
    clear.textContent = '全消去';
    clear.disabled = n === 0;
    clear.addEventListener('click', function() { if (n) clearAll(); });
    foot.appendChild(clear);
    foot.appendChild(copy);
    side.appendChild(foot);

    updateSideHighlights();
  }

  // サイドバー一覧のハイライト。inview（アンカーが画面内・複数可・薄）と review
  // （n/p の巡回対象・1 件・強）は役割を分ける——前者はスクロールで受動的に変わり、
  // 後者は n/p と一覧クリックでだけ動く（「再生中の曲」と「見えてる曲」の関係）。
  function updateSideHighlights() {
    if (!sideItems.length) return;
    var host = hostEl();
    var file = currentFile();
    var vh = window.innerHeight;
    sideItems.forEach(function(it) {
      var c = it.c;
      var vis = false;
      if (host && c.file === file && c.startLine) {
        // 錨の解決は redraw で済んでいる。ここはスクロールのたびに走るので、
        // ユニットの走査（raw では数千件）をやり直さない。
        var units = markEls[c.id] || [];
        for (var i = 0; i < units.length && !vis; i++) {
          var r = units[i].getBoundingClientRect();
          vis = r.bottom > 0 && r.top < vh;
        }
      }
      it.el.classList.toggle('inview', vis);
      it.el.classList.toggle('review', c.id === reviewId);
    });
  }

  // ハイライト同期の rAF スロットル。reviewId を動かす各所（kbMove / ホバー /
  // ドラッグ掴み直し）とスクロール/リサイズ（onViewportChange）から呼ぶ。
  var hlTick = false;
  function scheduleHl() {
    if (hlTick || !sideItems.length) return;
    hlTick = true;
    requestAnimationFrame(function() { hlTick = false; updateSideHighlights(); });
  }

  // file:line ラベル。行が取れなければ file のみ、レンジなら :start-end。
  function locLabel(c) {
    var f = c.file || '(no file)';
    if (!c.startLine) return f;
    if (c.endLine && c.endLine !== c.startLine) return f + ':' + c.startLine + '-' + c.endLine;
    return f + ':' + c.startLine;
  }

  // ── コピー（クリップボードへ 1 枚に畳む） ─────────────────────
  // file → 開始行 の並び順。コピーと n/p ジャンプで共通に使う。
  function byFileLine(a, b) {
    if (a.file !== b.file) return a.file < b.file ? -1 : 1;
    return (a.startLine || 0) - (b.startLine || 0);
  }
  function sortedComments() { return comments.slice().sort(byFileLine); }

  function formatAll() {
    // 追加順ではなく file → 開始行 でソートしてから畳む。行き来しても・複数ファイルを
    // 跨いでも、貼り先（人 / Claude Code）で順序が予測可能になる。
    var sorted = sortedComments();
    return sorted.map(function(c) {
      var head = '- ' + locLabel(c);
      // 空行は '>' だけにする（'> ' の行末空白を貼り先に持ち込まない）。
      var quoteLines = (c.quote || '').split('\n').map(function(l) { return l ? '> ' + l : '>'; }).join('\n');
      // 引用の直後に空行を 1 行。これが無いと続くコメントが blockquote に飲まれる。
      var block = head + '\n' + quoteLines + '\n\n' + (c.body || '').trim();
      return block;
    }).join('\n\n');
  }

  // btn を渡すとボタン文言でフィードバック、渡さない（キーボードの y）とトーストで返す。
  function copyAll(btn) {
    if (!comments.length) { if (!btn) toast('コメントはまだありません'); return; }
    var text = formatAll();
    var done, fail;
    if (btn) {
      var orig = btn.textContent;
      var flash = function(msg) {
        btn.textContent = msg;
        setTimeout(function() { btn.textContent = orig; }, 1200);
      };
      done = function() { flash('コピーしました'); };
      fail = function() { flash('コピー失敗'); };
    } else {
      done = function() { toast('全部コピーしました'); };
      fail = function() { toast('コピーに失敗しました'); };
    }
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(done).catch(function() { fallbackCopy(text, done, fail); });
    } else {
      fallbackCopy(text, done, fail);
    }
  }
  function fallbackCopy(text, done, fail) {
    var ta = document.createElement('textarea');
    ta.value = text;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    var ok = false;
    try { ok = document.execCommand('copy'); } catch (e) { ok = false; }
    ta.remove();
    if (ok) { done(); } else if (fail) { fail(); }
  }

  // ── 「+」ハンドル（モード中、ホバー中ユニットの左に出す） ─────
  var handle = null;
  function ensureHandle() {
    if (handle) return handle;
    handle = document.createElement('div');
    handle.className = 'md-cmt-handle';
    handle.textContent = '+';
    handle.addEventListener('mousedown', function(e) {
      e.preventDefault();
      e.stopPropagation();
      if (handleUnit) {
        var t = computeTarget(handleUnit, handleUnit);
        openNewPopover(t);
      }
    });
    document.body.appendChild(handle);
    return handle;
  }
  function moveHandle(u) {
    handleUnit = u;
    if (!u) { if (handle) handle.style.display = 'none'; return; }
    var h = ensureHandle();
    var rect = u.getBoundingClientRect();
    h.style.display = 'flex';
    h.style.top = (rect.top + 2) + 'px';
    h.style.left = Math.max(2, rect.left - 26) + 'px';
  }
  function hideHandle() { handleUnit = null; if (handle) handle.style.display = 'none'; }

  // ── レンジ選択のハイライト ────────────────────────────────────
  function clearSelecting() {
    var host = hostEl();
    if (!host) return;
    host.querySelectorAll('.md-cmt-selecting, .md-cmt-anchor').forEach(function(u) {
      u.classList.remove('md-cmt-selecting');
      u.classList.remove('md-cmt-anchor');
    });
  }
  // 範囲全体を塗り、掴んだ側の端（anchor）に印を付ける。動かしている端（moving）には
  // 別途カーソル枠が付くので、両端が同じユニットの時は印を出さない（同じ要素に
  // .md-cmt-anchor と .md-cmt-kbcursor が乗ると後勝ちで枠が破線に化ける）。
  function paintRange(units, anchor, moving, s, e) {
    unitsInRange(units, s, e).forEach(function(u) { u.classList.add('md-cmt-selecting'); });
    if (anchor && anchor !== moving) anchor.classList.add('md-cmt-anchor');
  }
  function setSelecting(u1, u2) {
    // ドラッグでも「動かしている端」に枠を出し、キーボードのレンジと同じ見え方にする。
    // landKbCursor は clearSelecting を含むので、塗る前に呼ぶ。reviewId を落とすのは
    // kbMove / ホバーと同じ理由（掴み直したら e/x の対象もそこへ移す）。
    if (u2 !== kbCursor) { reviewId = null; scheduleHl(); landKbCursor(u2); }
    clearSelecting();
    var s = Math.min(unitStart(u1), unitStart(u2));
    var e = Math.max(unitEnd(u1), unitEnd(u2));
    // ドラッグ中は開始時スナップショット(dragUnits)を使い、mousemove ごとの全走査を避ける。
    var units = dragUnits || allUnits(hostEl());
    paintRange(units, u1, u2, s, e);
  }

  // ── キーボード操作（マウス無しでコメント） ───────────────────
  // モード中、ユニット・カーソルを j/k・↑/↓ で移動し、Enter でコメント。
  // Shift+j/k で複数ユニットのレンジを伸縮する。keyscroll.js とは「モード中の j/k」を
  // 譲ってもらうことで排他する（MdComment.isMode 参照）。
  var kbCursor = null;  // 現在のユニット（カーソル）
  var kbAnchor = null;  // レンジ選択のアンカー
  // モード中の j/k・Shift+j/k は毎キー全走査になりやすいので、ユニット配列をキャッシュする。
  // 本文が入れ替わる（redraw / reanchor / mode 切替）ときに null にして作り直す。
  var kbUnitsCache = null;
  function kbUnits() {
    if (!kbUnitsCache) kbUnitsCache = allUnits(hostEl());
    return kbUnitsCache;
  }
  function invalidateKbUnits() { kbUnitsCache = null; }

  function clearKb() {
    var host = hostEl();
    if (host) host.querySelectorAll('.md-cmt-kbcursor').forEach(function(u) { u.classList.remove('md-cmt-kbcursor'); });
    kbCursor = null;
    kbAnchor = null;
    invalidateKbUnits();
    clearSelecting();
  }

  function setKbCursor(u, extend) {
    if (!u) return;
    if (kbCursor) kbCursor.classList.remove('md-cmt-kbcursor');
    if (!extend) { kbAnchor = u; clearSelecting(); }
    else if (!kbAnchor) { kbAnchor = kbCursor || u; }
    kbCursor = u;
    u.classList.add('md-cmt-kbcursor');
    u.scrollIntoView({ block: 'nearest' });
    if (extend) {
      var s = Math.min(unitStart(kbAnchor), unitStart(u));
      var e = Math.max(unitEnd(kbAnchor), unitEnd(u));
      clearSelecting();
      paintRange(kbUnits(), kbAnchor, u, s, e);
    }
  }

  // n/p ジャンプの着地点にカーソルも移す（e/x の対象を視覚的な現在地と一致させる）。
  // 追加スクロールはしない（scrollToLine 側で済んでいる）。
  function landKbCursor(u) {
    if (!u) return;
    if (kbCursor) kbCursor.classList.remove('md-cmt-kbcursor');
    kbCursor = u;
    kbAnchor = u;
    clearSelecting();
    u.classList.add('md-cmt-kbcursor');
  }

  // モードに入った時、最初に見えているユニットへカーソルを置く（即フィードバック）。
  function initKbCursor() {
    var units = kbUnits();
    if (!units.length) return;
    // ビューポート上端より下にある最初のユニットを選ぶ（見えている所から始める）。
    var pick = units[0];
    for (var i = 0; i < units.length; i++) {
      if (units[i].getBoundingClientRect().bottom > 0) { pick = units[i]; break; }
    }
    setKbCursor(pick, false);
  }

  function kbMove(delta, extend) {
    // カーソルを手で動かしたら n/p の巡回ポインタは無効化（e/x はカーソル位置の
    // コメントを対象にする）。
    reviewId = null;
    scheduleHl();
    var units = kbUnits();
    if (!units.length) return;
    var i = kbCursor ? units.indexOf(kbCursor) : -1;
    if (i === -1) { setKbCursor(units[delta < 0 ? units.length - 1 : 0], false); return; }
    var ni = Math.max(0, Math.min(units.length - 1, i + delta));
    setKbCursor(units[ni], extend);
  }

  function kbCommit() {
    if (!kbCursor) { initKbCursor(); return; }
    var target = computeTarget(kbAnchor || kbCursor, kbCursor);
    clearSelecting();
    kbAnchor = kbCursor;  // レンジは畳む
    openNewPopover(target);
  }

  // ── 埋め込みの出し入れに伴うスクロール補正 ────────────────────
  // 埋め込みの挿入/除去で本文の高さが変わる。基準ユニット（キーボード・カーソル、
  // 無ければ画面内の最初のユニット）の画面上の位置を DOM 変更の前後で合わせて、
  // 見ていた場所が飛ばないようにする。
  // スクロールの主体は単一=window、folder=#preview-pane なので、host から
  // いちばん近いスクロール可能な祖先を探す（host 自身も含む）。
  function scrollerOf(host) {
    var el = host;
    while (el && el !== document.body && el !== document.documentElement) {
      var st = getComputedStyle(el);
      if ((st.overflowY === 'auto' || st.overflowY === 'scroll') && el.scrollHeight > el.clientHeight) return el;
      el = el.parentElement;
    }
    return window;
  }
  function anchorScroll(fn) {
    var host = hostEl();
    var ref = (kbCursor && kbCursor.isConnected) ? kbCursor : null;
    if (!ref && host) {
      var units = allUnits(host);
      for (var i = 0; i < units.length; i++) {
        if (units[i].getBoundingClientRect().bottom > 0) { ref = units[i]; break; }
      }
    }
    if (!ref) { fn(); return; }
    var before = ref.getBoundingClientRect().top;
    fn();
    if (!ref.isConnected) return;
    var delta = ref.getBoundingClientRect().top - before;
    if (delta) scrollerOf(host).scrollBy(0, delta);
  }

  // ── モード切替 ────────────────────────────────────────────────
  function toggleMode() { setMode(!mode); }
  function setMode(on) {
    if (mode === on) return;
    mode = on;
    document.body.classList.toggle('md-cmt-mode', mode);
    if (!mode) {
      hideHandle();
      clearSelecting();
      clearKb();
      // ドラッグ押下中にモードを抜けた場合、離した時に幽霊ポップオーバーが開かないよう
      // 選択状態を確実にリセットする。
      dragging = false;
      dragStartUnit = null;
      dragEndUnit = null;
      dragUnits = null;
      // サイドバーを入る前の状態へ復元（タブ/×クリック起点なら toc.js 側の指定が優先）。
      if (window.MdToc && MdToc.closeComments) MdToc.closeComments();
      // 埋め込みが抜けて本文が縮むぶんはスクロール補正しつつ貼り直す。
      anchorScroll(redraw);
    } else {
      // raw / ソース表示は行の単位が DOM に無いので、ここで行ユニットを作る（コメントが
      // 1 件も無いファイルでは redraw 側の条件に掛からないため）。
      syncSourceRows(hostEl());
      // 先にサイドバー（コメントタブ）を開いてガター分のリフローを済ませてから
      // 埋め込みを差し込む（redraw が入れて、伸びるぶんはスクロール補正。見えている
      // ユニットへのキーボード・カーソルも redraw 内の initKbCursor が置く）。
      if (window.MdToc && MdToc.openComments) { ensureSide(); MdToc.openComments(); }
      anchorScroll(redraw);
    }
  }

  // ── イベント配線 ──────────────────────────────────────────────
  // 直近のマウス座標。本文が動いただけの mousemove（マウスは止まったまま）を見分ける。
  var lastMouseX = null;
  var lastMouseY = null;
  var wired = false;
  function wireOnce() {
    if (wired) return;
    wired = true;

    // モード中の選択（クリック=1 ユニット / ドラッグ=レンジ）。
    document.addEventListener('mousedown', function(e) {
      if (!mode) return;
      if (e.button !== 0) return;   // 右/中クリックは選択に使わない（右クリックはメニューへ）
      if (e.target.closest('.md-cmt-popover') || e.target.closest('.md-cmt-panel') ||
          e.target.closest('.md-cmt-handle') || e.target.closest('.md-cmt-badge') ||
          e.target.closest('.md-cmt-embed')) return;
      var host = hostEl();
      if (!host || !host.contains(e.target)) return;
      var u = e.target.closest('[data-src-line]');
      if (!u) return;
      e.preventDefault();
      dragging = true;
      dragStartUnit = u;
      dragEndUnit = u;
      dragUnits = allUnits(host);   // 以降の mousemove はこのスナップショットで範囲判定
      setSelecting(u, u);
    });

    // モード中は本文クリックのデフォルト遷移（リンク/アンカー）を止め、コメント付与と
    // ナビゲーションの二重発火を防ぐ。バッジ/パネル/ポップオーバーのクリックは通す。
    document.addEventListener('click', function(e) {
      if (!mode) return;
      if (e.target.closest('.md-cmt-badge') || e.target.closest('.md-cmt-panel') ||
          e.target.closest('.md-cmt-popover') || e.target.closest('.md-cmt-handle') ||
          e.target.closest('.md-cmt-embed')) return;
      var host = hostEl();
      if (host && host.contains(e.target)) { e.preventDefault(); e.stopPropagation(); }
    }, true);

    document.addEventListener('mousemove', function(e) {
      // 座標の記録はモード外・ドラッグ中も続ける（モードに入った直後の 1 発を
      // 「動いていない」と誤判定しないため）。
      var moved = (e.clientX !== lastMouseX || e.clientY !== lastMouseY);
      lastMouseX = e.clientX;
      lastMouseY = e.clientY;
      if (!mode) return;
      if (dragging) {
        var u = e.target.closest && e.target.closest('[data-src-line]');
        if (u) { dragEndUnit = u; setSelecting(dragStartUnit, u); }
        return;
      }
      // マウスが止まっていても mousemove は飛んでくる——本文がスクロールしたときや、
      // 「+」の出現などでカーソルの下の要素が変わったときに、同じ座標で 1 発届く。
      // これを「マウスに持ち替えた」と見なすと巡回対象（reviewId）とカーソルを奪われ、
      // n/p のあと少し待ってから押すと巡回位置が飛ぶ。座標が前回と同じなら本当には
      // 動いていないので、下の「持ち替え」だけを飛ばす（moved で分岐）。
      //
      // ここで「+」を消してはいけない: 消す → カーソル下の要素が変わる → また同じ座標の
      // mousemove が来る → 出す…… を繰り返してハンドルがちらつく。位置の付け直しは
      // 座標が同じなら結果も同じなので、そのまま通してよい。その結果、本文がスクロール
      // した直後だけ「+」とカーソルが別の行を指すことがあるが、次に本当にマウスを
      // 動かせば揃う——ちらつきと引き換えに許容する。
      // ハンドル自体の上ではそのまま維持（消すとクリックできなくなる）。
      if (handle && handle.contains(e.target)) return;
      // ハンドル追従（ポップオーバー/パネル上では出さない）。
      if (e.target.closest('.md-cmt-popover') || e.target.closest('.md-cmt-panel') ||
          e.target.closest('.md-cmt-embed')) { hideHandle(); return; }
      // Shift+j/k で掴んでいるレンジは、マウスの微動で消さない。この間は代わりに「+」を
      // 出さないことで、指し示すものをレンジの枠だけに保つ。
      if (kbCursor && kbAnchor && kbAnchor !== kbCursor) { hideHandle(); return; }
      var host = hostEl();
      var hu = (host && host.contains(e.target)) ? e.target.closest('[data-src-line]') : null;
      // マウスに持ち替えたらキーボード・カーソルもホバー先へ寄せ、「+」と枠が別の行を
      // 指したまま並ぶのを防ぐ（j/k を再開する時もそこから続く）。setKbCursor ではなく
      // landKbCursor を使うのは、scrollIntoView でマウス移動中にビューが揺れないため。
      // reviewId を落とすのは kbMove と同じ理由（カーソルを手で動かしたら e/x の対象は
      // n/p の巡回対象ではなくカーソル位置にする）。landKbCursor 側では落とせない
      // ——n/p 自身が reviewId を立てた直後に scrollToLine 経由で呼ぶため。
      if (moved && hu && hu !== kbCursor) { reviewId = null; scheduleHl(); landKbCursor(hu); }
      moveHandle(hu);
    });

    document.addEventListener('mouseup', function(e) {
      if (!dragging) return;
      dragging = false;
      // 埋め込みの上で離すと closest はユニットを見つけられない（入れ子なら外側の
      // ユニットに化ける）。その場合はドラッグ中に最後に塗った端（dragEndUnit）へ
      // 倒し、画面で見えていた選択範囲のままコメントを開く。
      var u = (e.target.closest && !e.target.closest('.md-cmt-embed') && e.target.closest('[data-src-line]'))
        || dragEndUnit || dragStartUnit;
      clearSelecting();
      var target = computeTarget(dragStartUnit, u);
      dragStartUnit = null;
      dragEndUnit = null;
      dragUnits = null;
      openNewPopover(target);
    });

    // ホバープレビュー（モード内外どちらでも）。マーカー済みユニット上で表示。
    document.addEventListener('mouseover', function(e) {
      if (mode || dragging) return;
      if (e.target.closest('.md-cmt-panel') || e.target.closest('.md-cmt-preview')) return;
      var host = hostEl();
      if (!host || !host.contains(e.target)) { return; }
      var u = e.target.closest('.md-cmt-marked');
      if (!u) return;
      // レンジコメントは 2 行目以降のユニットにも色帯が付くので、ホバー行を範囲に
      // 含む全コメントを出す（バッジの束ね先＝先頭行のユニットだけを見ると 2 行目以降で出ない）。
      var list = commentsCoveringUnit(u);
      if (list.length) showPreview(u, list);
    });
    document.addEventListener('mouseout', function(e) {
      if (!preview) return;
      var to = e.relatedTarget;
      if (to && (to.closest && (to.closest('.md-cmt-preview') || to.closest('.md-cmt-marked')))) return;
      hidePreview();
    });

    // Esc の受け手は MdCommon が一括で持つ。ポップオーバー（入力中）はいちばん前面、
    // コメントモード自体はいちばん後ろ——この優先順位のおかげで「入力を取消 → もう一度
    // Esc でモードを抜ける」が 1 回ずつ順に効く。
    // モードは本文の素キーを止めない（d/u/Space のページ送りはモード中も使う）ので
    // blocksKeys:false にする。
    if (window.MdCommon && MdCommon.registerOverlay) {
      MdCommon.registerOverlay({
        id: 'md-cmt-popover',
        isOpen: function() { return !!popover; },
        close: closePopover,
        priority: 50
      });
      MdCommon.registerOverlay({
        id: 'md-cmt-mode',
        isOpen: function() { return mode; },
        close: function() { setMode(false); },
        priority: 10,
        blocksKeys: false
      });
    }

    // キーの割り当て・効く文脈（入力欄 / オーバーレイ / ツリーフォーカスの除外）は
    // keymap.js の表が持つ。ここはモード中の各キーの実処理だけ。
    if (window.MdKeymap) {
      MdKeymap.on('comment-toggle', toggleMode);
      MdKeymap.on('comment-mode', function(e) {
        // キーに持ち替えた合図。ホバーの「+」を畳んで、指し示すものをカーソルの枠 1 つに
        // 保つ（次にマウスを動かせば mousemove 側で戻る）。
        hideHandle();
        switch (e.key) {
          case 'j': case 'J': case 'ArrowDown': kbMove(1, e.shiftKey); return;
          case 'k': case 'K': case 'ArrowUp': kbMove(-1, e.shiftKey); return;
          case 'Enter': kbCommit(); return;
          // n / p: 付けたコメントを巡回してジャンプ（別ファイルも自動で開く）。
          case 'n': jumpToComment(1); return;
          case 'p': jumpToComment(-1); return;
          case 'e': editCurrent(); return;
          // 削除は x / Delete のみ。Backspace は「戻る/文字消し」の筋反射で誤爆しやすい
          // ので割り当てない。
          case 'x': case 'Delete': deleteCurrent(); return;
          // y: 全部コピー（パネルのボタンと同じ。トーストで結果を返す）。
          case 'y': copyAll(null); return;
          default: return;
        }
      });
    }

    // ビューポート変化: ポップオーバーはアンカーへ追従（入力中の内容を失わない）、
    // ハンドル/プレビューは畳む。folder のスクロール主体は #preview-pane なので、
    // capture でスクロールを拾って window/preview-pane 双方に効かせる。
    function onViewportChange() {
      if (popover && popoverAnchor) positionPopover(popover, popoverAnchor);
      hidePreview();
      hideHandle();
      // スクロール/リサイズで in-view ハイライトを追従させる（rAF スロットル）。
      if (mode) scheduleHl();
    }
    window.addEventListener('resize', onViewportChange);
    document.addEventListener('scroll', onViewportChange, true);
  }

  // ── 公開 API ──────────────────────────────────────────────────
  // opts: { getContainer:()=>el, getFile:()=>relPath }
  function init(o) {
    opts = o;
    wireOnce();
    redraw();
  }

  window.MdComment = {
    init: init,
    reanchor: reanchor,   // ホットリロード後に呼ぶ
    toggle: toggleMode,   // 右クリックメニュー等から
    open: function() { setMode(true); },
    // MdCommon.isOverlayOpen が参照する（ポップオーバー表示中かどうか）。
    isPopoverOpen: function() { return !!popover; },
    // keyscroll.js が「モード中の j/k」を譲るために参照する。
    isMode: function() { return mode; }
  };
})();
