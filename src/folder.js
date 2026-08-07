(function() {
  var expandedDirs = new Set();
  var currentFilePath = null;
  var windowReady = false;
  var mdCheckQueue = [];
  var mdDotCache = {};

  function doHasMdCheck(path, row) {
    if (path in mdDotCache) {
      if (mdDotCache[path]) {
        row.classList.add('has-md');
      }
      return;
    }
    fetch('/?has_md=' + encodeURIComponent(path))
      .then(function(r) { return r.json(); })
      .then(function(data) {
        mdDotCache[path] = !!data.has_md;
        if (data.has_md) {
          row.classList.add('has-md');
        }
      })
      .catch(function() {});
  }

  function scheduleHasMdCheck(path, row) {
    if (windowReady) {
      doHasMdCheck(path, row);
    } else {
      mdCheckQueue.push({path: path, row: row});
    }
  }

  function renderItems(items, parentEl, depth) {
    items.forEach(function(item) {
      var row = document.createElement('div');
      row.className = 'tree-item';
      row.style.paddingLeft = (8 + depth * 16) + 'px';
      // ツリー項目の右クリック（contextmenu.js）でパスを引けるよう保持しておく。
      row.dataset.path = item.path;
      row.dataset.kind = item.kind;

      var icon = document.createElement('span');
      icon.className = 'icon';

      if (item.kind === 'dir') {
        icon.textContent = '›';
        var children = document.createElement('div');
        children.className = 'tree-children';
        var loaded = false;

        row.appendChild(icon);
        row.appendChild(document.createTextNode(item.name));
        parentEl.appendChild(row);
        parentEl.appendChild(children);

        scheduleHasMdCheck(item.path, row);

        // 子要素を読み込んで展開する。子の描画完了で解決する Promise を返す。
        function expand() {
          children.classList.add('open');
          row.classList.add('dir-open');
          expandedDirs.add(item.path);
          if (loaded) return Promise.resolve();
          loaded = true;
          return fetch('/?dir=' + encodeURIComponent(item.path))
            .then(function(r) { return r.json(); })
            .then(function(subItems) { renderItems(subItems, children, depth + 1); })
            .catch(function() {});
        }
        // revealFile から祖先フォルダをプログラム的に開けるよう保持しておく。
        row._expand = expand;

        row.addEventListener('click', function(e) {
          e.stopPropagation();
          if (children.classList.contains('open')) {
            children.classList.remove('open');
            row.classList.remove('dir-open');
            expandedDirs.delete(item.path);
          } else {
            expand();
          }
        });
      } else {
        if (isRenderablePath(item.name)) {
          row.classList.add('md-file');
        }
        icon.textContent = '';
        row.appendChild(icon);
        row.appendChild(document.createTextNode(item.name));
        parentEl.appendChild(row);

        row.addEventListener('click', function(e) {
          e.stopPropagation();
          loadPreview(item.path);
        });
      }
    });
  }

  // パスと種別に一致するツリー行を、描画済みの中から探す。
  function findRow(path, kind) {
    var rows = document.querySelectorAll('.tree-item');
    for (var i = 0; i < rows.length; i++) {
      if (rows[i].dataset.path === path &&
          (!kind || rows[i].dataset.kind === kind)) {
        return rows[i];
      }
    }
    return null;
  }

  // 開いているファイルに対応するツリー項目をハイライトし、見える位置へスクロールする。
  function updateActiveItem(relPath) {
    // 非同期な reveal の完走中に別ファイルへ切り替わっていたら、現在の
    // ハイライトを壊さないよう何もしない。
    if (relPath !== currentFilePath) return;
    document.querySelectorAll('.tree-item.active').forEach(function(el) {
      el.classList.remove('active');
    });
    var row = findRow(relPath, 'file');
    if (row) {
      row.classList.add('active');
      row.scrollIntoView({ block: 'nearest' });
    }
  }

  // root 相対パスのファイルまで祖先フォルダを順に展開し、最後にハイライトする。
  function revealFile(relPath) {
    var segs = relPath.split('/');
    var ancestors = [];
    for (var i = 0; i < segs.length - 1; i++) {
      ancestors.push(segs.slice(0, i + 1).join('/'));
    }

    function step(idx) {
      if (idx >= ancestors.length) {
        updateActiveItem(relPath);
        return;
      }
      var dirRow = findRow(ancestors[idx], 'dir');
      if (!dirRow || !dirRow._expand) {
        // 祖先が見つからなければ諦めて、今ある範囲でハイライトを試みる。
        updateActiveItem(relPath);
        return;
      }
      Promise.resolve(dirRow._expand()).then(function() { step(idx + 1); });
    }
    step(0);
  }

  function resolveRelativePath(base, rel) {
    var parts = base.split('/');
    parts.pop();
    rel.split('/').forEach(function(seg) {
      if (seg === '..') { if (parts.length > 0) parts.pop(); }
      else if (seg !== '.') { parts.push(seg); }
    });
    return parts.join('/');
  }

  // 通常表示がレンダリング結果になるファイル（md / html）。Raw トグルが意味を持つ対象。
  // 拡張子の一覧は Rust 側（request::RENDERABLE_EXT）が定義元で、起動時に
  // window.MD_RENDERABLE_EXT として注入される。ここに書き写さないこと
  // （書き写すと Rust 側だけ直した時に、開けるのに raw が出ないファイルが生まれる）。
  function isRenderablePath(p) {
    var m = /\.([^./\\]+)$/.exec(p || '');
    if (!m) return false;
    var exts = window.MD_RENDERABLE_EXT || [];
    return exts.indexOf(m[1].toLowerCase()) >= 0;
  }

  // iframe(.html-frame) 内の相対リンククリックを親のプレビュー遷移に回す。true を返すと
  // iframe 内遷移を止める。外部/アンカーや、md/html 以外（画像等）は iframe/wry に任せる。
  function frameLinkClick(href) {
    if (/^(https?:|mailto:|#)/i.test(href)) return false;
    var hashIdx = href.indexOf('#');
    var pathPart = hashIdx !== -1 ? href.slice(0, hashIdx) : href;
    if (!isRenderablePath(pathPart)) return false;
    var resolved = currentFilePath ? resolveRelativePath(currentFilePath, pathPart) : pathPart;
    loadPreview(resolved);
    return true;
  }

  // ファイル取得が非200だった時に、無反応にせず理由をペインへ出す。
  // textContent で組むのでファイル名に < 等が入っても安全。
  function showLoadError(pane, relPath) {
    var name = (relPath || '').split('/').pop() || relPath || '';
    var article = document.createElement('div');
    article.className = 'markdown-body';
    var p = document.createElement('p');
    p.className = 'md-notice';
    p.textContent = 'このファイルは開けませんでした: ' + name
      + '（フォルダ外を指すリンク・権限・壊れたファイルなどの可能性）';
    article.appendChild(p);
    pane.innerHTML = '';
    pane.appendChild(article);
    if (window.MdToc) window.MdToc.refresh();
  }

  function loadPreview(relPath, preserveScroll) {
    var pane = document.getElementById('preview-pane');
    var savedScroll = preserveScroll ? pane.scrollTop : 0;
    currentFilePath = relPath;
    // ファイル切替（ホットリロード以外）では本文ペインへフォーカスを戻し、直後から
    // スクロール素キー(j/k 等)が効くようにする。ホットリロードは現在のフォーカスを保つ。
    if (!preserveScroll) focusPreview();
    // ホットリロード(同一ファイルの再描画)ではツリーを動かさない。
    if (!preserveScroll) revealFile(relPath);
    if (window.MdMenu) window.MdMenu.setCurrentFile(relPath);
    if (window.MdSearch) window.MdSearch.reset();
    // バッジ（変更行数）は表示状態に関わらず、開いているファイルに追従させる。
    if (window.MdDiff) window.MdDiff.refreshStat();
    // md / html は通常表示がレンダリング結果なので Raw（ソース）トグルを有効化する。
    // それ以外は通常表示が既にソースなので raw は無効化（トグルを隠す）。raw 表示中に
    // 無効ファイルへ切り替えたら setAvailable(false) が状態を畳むので通常フェッチに落ちる。
    if (window.MdRaw) window.MdRaw.setAvailable(isRenderablePath(relPath));

    // raw / diff はモードとして維持する。ON のまま別ファイルへ移ったら、そのファイルの
    // ソース / 差分を表示する（本文レンダリングには戻さない）。
    var mode = window.MdViewModes && window.MdViewModes.active();
    if (mode) {
      // ファイル切替（preserveScroll=false）は先頭から、ホットリロードは位置維持。
      if (!preserveScroll) pane.scrollTop = 0;
      mode.refresh();
      return;
    }

    fetch('/?file=' + encodeURIComponent(relPath), preserveScroll ? { cache: 'no-store' } : undefined)
      .then(function(r) { return r.ok ? r.text() : null; })
      .then(function(html) {
        // 非200(html==null)は握りつぶさず理由を表示する。サーバは safe_join が弾いた時
        // （フォルダ外を指すシンボリックリンク等）や読めない時に not_found を返すので、
        // 黙って無反応にならないようメッセージを出す。
        if (html == null) { showLoadError(pane, relPath); return; }
        pane.innerHTML = html;
        pane.scrollTop = savedScroll;
        // html 表示（iframe）の相対リンクは、iframe 内遷移ではなく親のプレビュー遷移に
        // 回す（サイドバーの選択やコメントの現在ファイルを同期させるため）。
        MdCommon.hydrate(pane, { onLinkClick: frameLinkClick });
      })
      .catch(function() { showLoadError(pane, relPath); });
  }

  window.MdReload = function(relPath) {
    if (!currentFilePath) return;
    if (relPath && relPath !== currentFilePath) return;
    // raw / diff 表示中はファイル変更をその再取得に回す（本文には戻さない）。
    var mode = window.MdViewModes && window.MdViewModes.active();
    if (mode) { mode.refresh(); return; }
    loadPreview(currentFilePath, true);
  };

  // ── キーボードナビ（folder モード限定） ──────────────────────────
  // ・[ / ] : 表示中の描画可能ファイルを巡回して即プレビュー
  // ・Tab   : 本文ペイン ⇄ ファイルツリー のフォーカス切替
  // ・ツリーにフォーカス時: j/k 移動・g/G 端・Enter/l 開く&展開・h 畳む/親へ
  // keyscroll.js とは MdCommon.isSidebarFocused() で排他する（役割の二重発火を防ぐ）。
  var cursorRow = null;

  // 画面に見えている .tree-item（畳んだフォルダ内の隠れ行は offsetParent===null で除外）。
  function visibleRows() {
    return Array.prototype.filter.call(
      document.querySelectorAll('.tree-item'),
      function(r) { return r.offsetParent !== null; }
    );
  }
  // 見えている描画可能ファイル行（[ / ] の巡回対象）。
  function visibleFileRows() {
    return Array.prototype.filter.call(
      document.querySelectorAll('.tree-item.md-file'),
      function(r) { return r.offsetParent !== null; }
    );
  }

  function setCursor(row) {
    if (cursorRow) cursorRow.classList.remove('cursor');
    cursorRow = row || null;
    if (cursorRow) {
      cursorRow.classList.add('cursor');
      cursorRow.scrollIntoView({ block: 'nearest' });
    }
  }

  function focusPreview() {
    var pane = document.getElementById('preview-pane');
    // 検索入力などにフォーカス中は奪わない（本文ロード時は search.reset で閉じるので通常は安全）。
    if (window.MdCommon && MdCommon.isFieldEl(document.activeElement)) {
      document.body.classList.remove('nav-tree');
      return;
    }
    if (pane) pane.focus({ preventScroll: true });
    document.body.classList.remove('nav-tree');
  }

  function focusTree() {
    var sb = document.getElementById('sidebar');
    if (!sb) return;
    sb.focus({ preventScroll: true });
    document.body.classList.add('nav-tree');
    // カーソルが未設定/不可視なら、開いているファイル→先頭可視行の順で置く。
    if (!cursorRow || cursorRow.offsetParent === null) {
      var active = document.querySelector('.tree-item.active');
      var rows = visibleRows();
      setCursor((active && active.offsetParent !== null) ? active : (rows[0] || null));
    } else {
      setCursor(cursorRow); // 見える位置へ再スクロール
    }
  }

  function moveCursor(delta) {
    var rows = visibleRows();
    if (!rows.length) return;
    var i = cursorRow ? rows.indexOf(cursorRow) : -1;
    if (i === -1) { setCursor(rows[delta < 0 ? rows.length - 1 : 0]); return; }
    setCursor(rows[Math.max(0, Math.min(rows.length - 1, i + delta))]);
  }
  function cursorEdge(toEnd) {
    var rows = visibleRows();
    if (rows.length) setCursor(rows[toEnd ? rows.length - 1 : 0]);
  }

  // カーソル行を含む .tree-children の直前にある親ディレクトリ行。無ければ null。
  function parentDirRow(row) {
    var container = row && row.parentNode;
    if (container && container.classList && container.classList.contains('tree-children')) {
      var prev = container.previousSibling;
      if (prev && prev.classList && prev.classList.contains('tree-item')) return prev;
    }
    return null;
  }

  function openCursorFile() {
    if (cursorRow && cursorRow.dataset.path) {
      loadPreview(cursorRow.dataset.path); // loadPreview 内で focusPreview 済み
    }
  }

  // l / → : dir=展開（開いていれば最初の子へ）/ file=開く
  function expandOrOpen() {
    if (!cursorRow) return;
    if (cursorRow.dataset.kind === 'dir') {
      if (!cursorRow.classList.contains('dir-open')) {
        cursorRow.click(); // 既存ハンドラで展開（expandedDirs も同期される）
      } else {
        var children = cursorRow.nextSibling;
        var first = (children && children.querySelector) ? children.querySelector('.tree-item') : null;
        if (first && first.offsetParent !== null) setCursor(first);
      }
    } else {
      openCursorFile();
    }
  }
  // Enter : dir=開閉トグル / file=開く
  function toggleOrOpen() {
    if (!cursorRow) return;
    if (cursorRow.dataset.kind === 'dir') cursorRow.click();
    else openCursorFile();
  }
  // h / ← : 開いた dir=畳む（カーソルはその dir に残る）/ それ以外=親 dir へ
  function collapseOrParent() {
    if (!cursorRow) return;
    if (cursorRow.dataset.kind === 'dir' && cursorRow.classList.contains('dir-open')) {
      cursorRow.click();
    } else {
      var parent = parentDirRow(cursorRow);
      if (parent) setCursor(parent);
    }
  }

  // [ / ] : 表示中の描画可能ファイルを DOM 順に巡回。端ではクランプ（wrap しない）。
  function gotoAdjacentFile(delta) {
    var files = visibleFileRows();
    if (!files.length) return;
    var cur = document.querySelector('.tree-item.active');
    var i = (cur && cur.offsetParent !== null) ? files.indexOf(cur) : -1;
    var ni;
    if (i === -1) {
      // 現在ファイルが一覧に無い（相対リンク先など）→ 端から入る。
      ni = delta < 0 ? files.length - 1 : 0;
    } else {
      ni = i + delta;
      if (ni < 0 || ni >= files.length) return; // 端で無反応（誤爆時の被害を抑える）
    }
    var row = files[ni];
    if (!row || !row.dataset.path) return;
    // ツリーで巡回中に [ / ] を押した時はツリーに留まりカーソルを新ファイルへ追従させる
    // （[ / ] は「開いて本文へ移動」ではなく「巡回」なので、フォーカス文脈を保つ）。
    var wasTree = !!(window.MdCommon && MdCommon.isSidebarFocused());
    loadPreview(row.dataset.path); // 内部で focusPreview（本文へ移動）
    if (wasTree) { setCursor(row); focusTree(); }
  }

  // キーの割り当て・効く文脈は keymap.js の表が持つ。ここは実処理だけ。
  //
  // 注: このファイルは初期化スクリプト（document-start）として注入されるので、
  // <head> の各モジュールより **先に** 評価される。MdKeymap はまだ存在しないため、
  // 登録は DOMContentLoaded まで遅らせる。
  function registerKeys() {
    if (!window.MdKeymap) return;
    // Tab / [ / ] はツリー内外どちらでも効くアプリ全体のナビ。
    MdKeymap.on('focus-toggle', function() {
      if (window.MdCommon && MdCommon.isSidebarFocused()) focusPreview();
      else focusTree();
    });
    MdKeymap.on('file-cycle', function(e) {
      gotoAdjacentFile(e.key === '[' ? -1 : 1);
    });
    // ツリーにフォーカスがある時だけ呼ばれる（keymap.js 側の when が保証する）。
    MdKeymap.on('tree', function(e) {
      switch (e.key) {
        case 'j': case 'ArrowDown':  moveCursor(1); break;
        case 'k': case 'ArrowUp':    moveCursor(-1); break;
        case 'g':                    cursorEdge(false); break;
        case 'G':                    cursorEdge(true); break;
        case 'l': case 'ArrowRight': expandOrOpen(); break;
        case 'h': case 'ArrowLeft':  collapseOrParent(); break;
        case 'Enter':                toggleOrOpen(); break;
        default: break;
      }
    });
  }

  // フォーカスがサイドバー外へ出たら、サイドバーのアクティブ枠(nav-tree)を畳む。
  // 本文クリックやトグル操作でツリーから抜けた時に、枠が残って主役表示が嘘になるのを防ぐ。
  document.addEventListener('focusout', function() {
    setTimeout(function() {
      if (window.MdCommon && !MdCommon.isSidebarFocused()) {
        document.body.classList.remove('nav-tree');
      }
    }, 0);
  });

  document.addEventListener('DOMContentLoaded', function() {
    registerKeys();
    var resizer = document.getElementById('resizer');
    var sidebar = document.getElementById('sidebar');
    var isDragging = false;
    var startX, startWidth;
    resizer.addEventListener('mousedown', function(e) {
      isDragging = true;
      startX = e.clientX;
      startWidth = sidebar.offsetWidth;
      resizer.classList.add('dragging');
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
      e.preventDefault();
    });
    document.addEventListener('mousemove', function(e) {
      if (!isDragging) return;
      var newWidth = startWidth + (e.clientX - startX);
      newWidth = Math.max(120, Math.min(newWidth, window.innerWidth - 200));
      sidebar.style.width = newWidth + 'px';
    });
    document.addEventListener('mouseup', function() {
      if (!isDragging) return;
      isDragging = false;
      resizer.classList.remove('dragging');
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      // ツリーを広げて preview-pane が閾値を割ったら TOC を退避、
      // 戻したら復帰させる（window resize を経由しない幅変化のため）。
      if (window.MdToc) window.MdToc.reevaluate();
    });

    if (window.MdSearch) {
      window.MdSearch.init(document.getElementById('preview-pane'));
    }
    if (window.MdToc) {
      window.MdToc.init(document.getElementById('preview-pane'));
    }
    if (window.MdPalette) {
      // ファイル検索（⌘P）。選んだら通常のファイル切替と同じ経路で開く。
      window.MdPalette.init({ openFile: function(rel) { loadPreview(rel); } });
    }
    if (window.MdViewModes) {
      var previewPane = function() { return document.getElementById('preview-pane'); };
      window.MdViewModes.initAll({
        getContainer: previewPane,
        getScroller: previewPane,
        // フォルダモードは対象ファイルを相対パスで渡す（単一ファイルモードの `=1` 番兵
        // と違い、開いているファイルがサーバ側に無いため）。
        url: function(id) {
          return currentFilePath ? '/?' + id + '=' + encodeURIComponent(currentFilePath) : null;
        },
        getStatUrl: function() {
          return currentFilePath ? '/?diffstat=' + encodeURIComponent(currentFilePath) : null;
        },
        reloadNormal: function() { if (currentFilePath) loadPreview(currentFilePath, true); }
      });
    }
    if (window.MdComment) {
      // 対象は #preview-pane。file 部は現在プレビュー中ファイルの root 相対パス。
      // openFile はパネル項目クリックで別ファイルのコメント先へ飛ぶために使う。
      window.MdComment.init({
        getContainer: function() { return document.getElementById('preview-pane'); },
        getFile: function() { return currentFilePath || ''; },
        openFile: function(rel) { loadPreview(rel); }
      });
    }

    fetch('/?dir=')
      .then(function(r) { return r.json(); })
      .then(function(items) {
        renderItems(items, sidebar, 0);
        if (typeof INITIAL_FILE === 'string' && INITIAL_FILE) {
          loadPreview(INITIAL_FILE); // 内部で focusPreview 済み
        } else {
          // ファイル未指定でも本文ペインにフォーカスを置き、スクロール素キーの初期ターゲットにする。
          focusPreview();
        }
        setTimeout(function() {
          window.ipc.postMessage('ready');
          windowReady = true;
          mdCheckQueue.forEach(function(item) { doHasMdCheck(item.path, item.row); });
          mdCheckQueue = [];
          // ファイル検索の一覧を先に温めておく（初回の ⌘P を待たせない）。
          // 初期表示より後に投げるので、起動の体感速度は落とさない。
          if (window.MdPalette) window.MdPalette.prefetch();
        }, 0);
      })
      .catch(function() {
        setTimeout(function() { window.ipc.postMessage('ready'); windowReady = true; }, 0);
      });
  });

  // ⌘W と ⌘A は common.js が keymap 経由で処理する。
  document.addEventListener('click', function(e) {
    var a = e.target.closest('a[href]');
    if (!a) return;
    var href = a.getAttribute('href');
    if (!href) return;
    if (MdCommon.scrollToAnchor(href, e)) {
      // ページ内アンカーは処理済み。
    } else if (!href.startsWith('http://') && !href.startsWith('https://') && !href.startsWith('mailto:')) {
      var hashIdx = href.indexOf('#');
      var pathPart = hashIdx !== -1 ? href.slice(0, hashIdx) : href;
      var anchorPart = hashIdx !== -1 ? href.slice(hashIdx + 1) : '';
      // md / html はプレビュー枠内で遷移させる。html を top-level 遷移させると、text/html
      // 配信になった今はウィンドウ全体が生ページに化けてサイドバー・トグルが消えてしまう。
      if (isRenderablePath(pathPart)) {
        e.preventDefault();
        var resolved = currentFilePath ? resolveRelativePath(currentFilePath, pathPart) : pathPart;
        loadPreview(resolved);
        if (anchorPart) {
          setTimeout(function() {
            var id = decodeURIComponent(anchorPart);
            var target = document.getElementById(id);
            if (target) target.scrollIntoView({ behavior: 'smooth' });
          }, 100);
        }
      }
    }
  });
})();
