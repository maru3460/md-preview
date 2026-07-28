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

  // 1 ユニットの引用テキスト。コードは行を保つ、散文は空白を畳む。
  // 💬 バッジ・Copy ボタン・ファイル名ラベルといった UI チップは引用に混ぜない
  // （クローンから取り除いてから textContent を読む）。
  function unitQuote(u) {
    var isCode = u.classList.contains('code-wrapper') || u.tagName === 'PRE';
    var clone = u.cloneNode(true);
    clone.querySelectorAll('.md-cmt-badge, .copy-btn, .code-filename').forEach(function(n) { n.remove(); });
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
    var quote = units.map(unitQuote).filter(Boolean).join('\n');
    // アンカー（マーカー/ポップオーバーの基準）は開始行のユニット。
    var anchor = units[0];
    return { startLine: startLine, endLine: endLine, quote: quote, anchorEl: anchor };
  }

  // ── コメント CRUD ─────────────────────────────────────────────
  function addComment(t, body) {
    comments.push({
      id: nextId++,
      file: currentFile(),
      startLine: t.startLine,
      endLine: t.endLine,
      quote: t.quote,
      body: body
    });
    redraw();
  }
  function updateComment(id, body) {
    var c = findComment(id);
    if (c) { c.body = body; redraw(); }
  }
  function deleteComment(id) {
    comments = comments.filter(function(c) { return c.id !== id; });
    redraw();
  }
  function clearAll() {
    comments = [];
    reviewId = null;
    redraw();
  }
  function findComment(id) {
    for (var i = 0; i < comments.length; i++) if (comments[i].id === id) return comments[i];
    return null;
  }
  // 現在ファイルで、ユニットの行を範囲に含むコメント一覧（ホバープレビュー用）。
  function commentsCoveringUnit(u) {
    var line = parseInt(u.dataset.srcLine, 10);
    var file = currentFile();
    return comments.filter(function(c) {
      var end = c.endLine || c.startLine;
      return c.file === file && c.startLine <= line && end >= line;
    });
  }

  // ── ジャンプ（パネル項目 → 本文の file:line） ─────────────────
  function flashUnit(u) {
    if (!u) return;
    u.classList.add('md-cmt-flash');
    setTimeout(function() { u.classList.remove('md-cmt-flash'); }, 1400);
  }
  // 現在ファイル内の行までスクロール＆点滅。見つかった要素を返す。
  // モード中はキーボード・カーソルも着地点へ移す（e/x の対象＝視覚的な現在地を一致させる）。
  function scrollToLine(line) {
    var host = hostEl();
    var u = host ? host.querySelector('[data-src-line="' + line + '"]') : null;
    if (u) {
      u.scrollIntoView({ block: 'center', behavior: 'smooth' });
      flashUnit(u);
      if (mode) landKbCursor(u);
    }
    return u;
  }
  // ジャンプの世代トークン。新しいジャンプが始まったら古いリトライ連鎖を無効化する
  // （連続で別ファイルへ飛んだとき、前のリトライが別ファイルの同じ行番号へ誤着地するのを防ぐ）。
  var jumpGen = 0;
  // 別ファイルジャンプ: 対象ファイルが開かれて行が現れるまでリトライ。ただし
  //  ・自分より新しいジャンプが始まったら中断（world 不一致）
  //  ・現在ファイルが目的ファイルと違う間は「まだ読み込み中」としてスクロールしない
  function scrollToLineWhenReady(file, line, tries, gen) {
    if (gen !== jumpGen) return;                 // 追い越された
    if (currentFile() === file) {                // 目的ファイルが開けた
      if (scrollToLine(line)) return;
    }
    if (tries <= 0) return;
    setTimeout(function() { scrollToLineWhenReady(file, line, tries - 1, gen); }, 60);
  }
  // コメントの file:line へ飛ぶ。別ファイルは folder モードなら開いてから飛ぶ。
  function gotoComment(c) {
    if (!c.startLine) return;
    jumpGen++;
    if (c.file && c.file !== currentFile()) {
      if (opts && typeof opts.openFile === 'function') {
        opts.openFile(c.file);
        scrollToLineWhenReady(c.file, c.startLine, 40, jumpGen);
      }
      // single / stdin は別ファイルへ移れないので何もしない。
      return;
    }
    scrollToLine(c.startLine);
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
    toast((i + 1) + ' / ' + list.length + '  ' + locLabel(c));
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
  // e: 対象コメントを編集（同一ファイルならその場、別ファイルなら開いてから）。
  function editCurrent() {
    var c = currentComment();
    if (!c) { toast('編集するコメントがありません'); return; }
    var host = hostEl();
    var anchor = (c.file === currentFile() && host) ? host.querySelector('[data-src-line="' + c.startLine + '"]') : null;
    if (anchor) {
      openEditPopover(anchor, c);
    } else if (c.file !== currentFile()) {
      // 別ファイル: 開いて行が出るのを待ってから編集を開く。gotoComment と同じ jumpGen を
      // 捕捉し、その後に別ジャンプが始まったら中断（古い対象で誤って開かない）。
      gotoComment(c);
      var myGen = jumpGen;
      var tries = 40;
      (function waitEdit() {
        if (myGen !== jumpGen) return;   // 追い越された
        if (c.file === currentFile()) {
          var a = hostEl() && hostEl().querySelector('[data-src-line="' + c.startLine + '"]');
          if (a) { openEditPopover(a, c); return; }
        }
        if (tries-- > 0) setTimeout(waitEdit, 60);
      })();
    } else {
      openEditPopover(null, c);
    }
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
  var anchorMap = {};  // startLine -> { el, list } （ホバープレビュー用に保持）

  function clearMarkers(host) {
    host.querySelectorAll('.md-cmt-marked').forEach(function(u) { u.classList.remove('md-cmt-marked'); });
    host.querySelectorAll('.md-cmt-badge').forEach(function(b) { b.remove(); });
  }

  function redraw() {
    var host = hostEl();
    if (host) {
      clearMarkers(host);
      anchorMap = {};
      var file = currentFile();
      var all = allUnits(host);   // 1 回だけ取得して全コメントで使い回す
      comments.forEach(function(c) {
        if (c.file !== file) return;
        var units = unitsInRange(all, c.startLine, c.endLine);
        if (!units.length) {
          var one = host.querySelector('[data-src-line="' + c.startLine + '"]');
          if (one) units = [one];
        }
        units.forEach(function(u) { u.classList.add('md-cmt-marked'); });
        var anchor = units[0];
        if (anchor) {
          var slot = anchorMap[c.startLine] || (anchorMap[c.startLine] = { el: anchor, list: [] });
          slot.list.push(c);
        }
      });
      Object.keys(anchorMap).forEach(function(k) {
        var slot = anchorMap[k];
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
        slot.el.appendChild(badge);
      });

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
    ta.addEventListener('keydown', function(e) {
      if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); commit(); }
      else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closePopover(); }
    });

    document.body.appendChild(pop);
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
    var p = ensurePanel();
    p.innerHTML = '';
    var n = comments.length;

    // モード外の表示は件数で分岐:
    //  ・コメント無し → 何も出さない
    //  ・コメント有り → 「💬 N」ピル（クリックでモードに入り一覧を開く）。本文のブロック
    //    右上バッジは別途 redraw が出しているので、両方で「コメントあり」が分かる。
    if (!mode) {
      p.classList.remove('pill');
      if (n === 0) { p.style.display = 'none'; return; }
      p.style.display = '';
      p.classList.add('pill');
      var pill = document.createElement('button');
      pill.type = 'button';
      pill.className = 'md-cmt-pill';
      pill.textContent = '💬 ' + n;
      pill.title = 'コメント ' + n + ' 件（クリックで一覧）';
      pill.addEventListener('click', function() { setMode(true); });
      p.appendChild(pill);
      return;
    }
    p.style.display = '';
    p.classList.remove('pill');

    // ヘッダ: タイトル＋件数、完了ボタン。
    var head = document.createElement('div');
    head.className = 'md-cmt-panel-head';
    var title = document.createElement('span');
    title.className = 'md-cmt-panel-title';
    title.textContent = 'コメント ' + n;
    head.appendChild(title);

    var spacer = document.createElement('span');
    spacer.style.flex = '1';
    head.appendChild(spacer);

    // パネルが出ている＝モード中なので、ボタンはパネルを「閉じる」（＝モードを抜ける）。c / Esc でも可。
    var doneBtn = document.createElement('button');
    doneBtn.type = 'button';
    doneBtn.className = 'md-cmt-btn';
    doneBtn.textContent = '閉じる';
    doneBtn.title = 'コメントモードを抜ける (c / Esc)';
    doneBtn.addEventListener('click', function() { setMode(false); });
    head.appendChild(doneBtn);
    p.appendChild(head);

    // キー操作の常設ヒント（ヘルプを開かなくても要点が分かるように）。件数で出し分ける。
    var hint = document.createElement('div');
    hint.className = 'md-cmt-hint';
    hint.textContent = (n === 0)
      ? 'j / k で移動、Enter でコメント（Shift+j/k で複数行）。クリック・ドラッグでも可'
      : 'n / p 巡回 · e 編集 · x 削除 · y 全部コピー · ? 全キー';
    p.appendChild(hint);

    // 本文: コメント一覧。0 件のときは一覧を出さない（ヒントが案内を兼ねる）。
    var listEl = document.createElement('div');
    listEl.className = 'md-cmt-list';
    comments.forEach(function(c) {
      var item = document.createElement('div');
      item.className = 'md-cmt-item';
      item.title = 'クリックで ' + locLabel(c) + ' へ移動';
      // 項目クリックでコメント先（file:line）へジャンプ（folder は別ファイルも開く）。
      item.addEventListener('click', function() { gotoComment(c); });

      var loc = document.createElement('div');
      loc.className = 'md-cmt-loc';
      loc.textContent = locLabel(c);
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
        var anchor = host ? host.querySelector('[data-src-line="' + c.startLine + '"]') : null;
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

      // パネル項目にホバー → 本文の該当ユニットをハイライト。
      item.addEventListener('mouseenter', function() {
        if (c.file !== currentFile()) return;
        var host = hostEl();
        var u = host ? host.querySelector('[data-src-line="' + c.startLine + '"]') : null;
        if (u) u.classList.add('md-cmt-flash');
      });
      item.addEventListener('mouseleave', function() {
        var host = hostEl();
        if (host) host.querySelectorAll('.md-cmt-flash').forEach(function(x) { x.classList.remove('md-cmt-flash'); });
      });

      listEl.appendChild(item);
    });
    p.appendChild(listEl);

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
    p.appendChild(foot);
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
      var quoteLines = (c.quote || '').split('\n').map(function(l) { return '> ' + l; }).join('\n');
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
    if (host) host.querySelectorAll('.md-cmt-selecting').forEach(function(u) { u.classList.remove('md-cmt-selecting'); });
  }
  function setSelecting(u1, u2) {
    clearSelecting();
    var s = Math.min(unitStart(u1), unitStart(u2));
    var e = Math.max(unitEnd(u1), unitEnd(u2));
    // ドラッグ中は開始時スナップショット(dragUnits)を使い、mousemove ごとの全走査を避ける。
    var units = dragUnits || allUnits(hostEl());
    unitsInRange(units, s, e).forEach(function(u) { u.classList.add('md-cmt-selecting'); });
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
      var units = kbUnits();
      var s = Math.min(unitStart(kbAnchor), unitStart(u));
      var e = Math.max(unitEnd(kbAnchor), unitEnd(u));
      clearSelecting();
      unitsInRange(units, s, e).forEach(function(x) { x.classList.add('md-cmt-selecting'); });
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

  // ── モード切替 ────────────────────────────────────────────────
  function toggleMode() { setMode(!mode); }
  function setMode(on) {
    var was = mode;
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
      dragUnits = null;
    } else if (!was) {
      // 入った瞬間、見えているユニットにキーボード・カーソルを置く（マウス無しの起点）。
      initKbCursor();
    }
    renderPanel();
    // モードで使えるキーが入れ替わるので、コマンドパネルの表示を追従させる
    // （モード切替はキー以外＝右クリックメニューからも起きるため、専用に通知する）。
    if (was !== mode && window.MdCmdPanel && MdCmdPanel.sync) MdCmdPanel.sync();
  }

  // ── イベント配線 ──────────────────────────────────────────────
  var wired = false;
  function wireOnce() {
    if (wired) return;
    wired = true;

    // モード中の選択（クリック=1 ユニット / ドラッグ=レンジ）。
    document.addEventListener('mousedown', function(e) {
      if (!mode) return;
      if (e.button !== 0) return;   // 右/中クリックは選択に使わない（右クリックはメニューへ）
      if (e.target.closest('.md-cmt-popover') || e.target.closest('.md-cmt-panel') ||
          e.target.closest('.md-cmt-handle') || e.target.closest('.md-cmt-badge')) return;
      var host = hostEl();
      if (!host || !host.contains(e.target)) return;
      var u = e.target.closest('[data-src-line]');
      if (!u) return;
      e.preventDefault();
      dragging = true;
      dragStartUnit = u;
      dragUnits = allUnits(host);   // 以降の mousemove はこのスナップショットで範囲判定
      setSelecting(u, u);
    });

    // モード中は本文クリックのデフォルト遷移（リンク/アンカー）を止め、コメント付与と
    // ナビゲーションの二重発火を防ぐ。バッジ/パネル/ポップオーバーのクリックは通す。
    document.addEventListener('click', function(e) {
      if (!mode) return;
      if (e.target.closest('.md-cmt-badge') || e.target.closest('.md-cmt-panel') ||
          e.target.closest('.md-cmt-popover') || e.target.closest('.md-cmt-handle')) return;
      var host = hostEl();
      if (host && host.contains(e.target)) { e.preventDefault(); e.stopPropagation(); }
    }, true);

    document.addEventListener('mousemove', function(e) {
      if (!mode) return;
      if (dragging) {
        var u = e.target.closest && e.target.closest('[data-src-line]');
        if (u) setSelecting(dragStartUnit, u);
        return;
      }
      // ハンドル自体の上ではそのまま維持（消すとクリックできなくなる）。
      if (handle && handle.contains(e.target)) return;
      // ハンドル追従（ポップオーバー/パネル上では出さない）。
      if (e.target.closest('.md-cmt-popover') || e.target.closest('.md-cmt-panel')) { hideHandle(); return; }
      var host = hostEl();
      var hu = (host && host.contains(e.target)) ? e.target.closest('[data-src-line]') : null;
      moveHandle(hu);
    });

    document.addEventListener('mouseup', function(e) {
      if (!dragging) return;
      dragging = false;
      var u = (e.target.closest && e.target.closest('[data-src-line]')) || dragStartUnit;
      clearSelecting();
      var target = computeTarget(dragStartUnit, u);
      dragStartUnit = null;
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
      // 含む全コメントを出す（先頭行だけの anchorMap 参照だと 2 行目以降で出ない）。
      var list = commentsCoveringUnit(u);
      if (list.length) showPreview(u, list);
    });
    document.addEventListener('mouseout', function(e) {
      if (!preview) return;
      var to = e.relatedTarget;
      if (to && (to.closest && (to.closest('.md-cmt-preview') || to.closest('.md-cmt-marked')))) return;
      hidePreview();
    });

    // キーボード: c でモード / Esc でポップオーバー→モードの順に閉じる。
    document.addEventListener('keydown', function(e) {
      if (e.key === 'Escape') {
        if (popover) { e.preventDefault(); e.stopPropagation(); closePopover(); return; }
        if (mode) {
          // 前面に別オーバーレイ（ヘルプ/検索/メニュー）がある時は、まずそちらに Esc を
          // 譲る（それらは stopPropagation しないため、譲らないと 1 回の Esc で両方閉じる）。
          if (window.MdCommon && MdCommon.isOverlayOpen && MdCommon.isOverlayOpen()) return;
          e.preventDefault(); e.stopPropagation(); setMode(false); return;
        }
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      // 入力欄フォーカス中・他オーバーレイ表示中・ツリーフォーカス中は素キーを奪わない。
      if (window.MdCommon) {
        if (MdCommon.isFieldEl(e.target)) return;
        if (MdCommon.isOverlayOpen && MdCommon.isOverlayOpen()) return;
        if (MdCommon.isSidebarFocused && MdCommon.isSidebarFocused()) return;
      }

      // モード中のキーボード操作（マウス無しでコメント）。
      if (mode) {
        var k = e.key;
        if (k === 'j' || k === 'J' || k === 'ArrowDown') { e.preventDefault(); kbMove(1, e.shiftKey); return; }
        if (k === 'k' || k === 'K' || k === 'ArrowUp') { e.preventDefault(); kbMove(-1, e.shiftKey); return; }
        if (k === 'Enter') { e.preventDefault(); kbCommit(); return; }
        // n / p: 付けたコメントを巡回してジャンプ（別ファイルも自動で開く）。
        if (k === 'n') { e.preventDefault(); jumpToComment(1); return; }
        if (k === 'p') { e.preventDefault(); jumpToComment(-1); return; }
        // e: 対象コメントを編集 / x・Delete・Backspace: 対象コメントを削除。
        if (k === 'e') { e.preventDefault(); editCurrent(); return; }
        // 削除は x / Delete のみ。Backspace は「戻る/文字消し」の筋反射で誤爆しやすいので割り当てない。
        if (k === 'x' || k === 'Delete') { e.preventDefault(); deleteCurrent(); return; }
        // y: 全部コピー（パネルのボタンと同じ。トーストで結果を返す）。
        if (k === 'y') { e.preventDefault(); copyAll(null); return; }
      }

      if (e.key !== 'c') return;
      e.preventDefault();
      toggleMode();
    });

    // ビューポート変化: ポップオーバーはアンカーへ追従（入力中の内容を失わない）、
    // ハンドル/プレビューは畳む。folder のスクロール主体は #preview-pane なので、
    // capture でスクロールを拾って window/preview-pane 双方に効かせる。
    function onViewportChange() {
      if (popover && popoverAnchor) positionPopover(popover, popoverAnchor);
      hidePreview();
      hideHandle();
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
