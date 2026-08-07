// 右クリック コンテキストメニュー。<head> に inline され window.MdMenu を公開する。
// init.js（単一/stdin）と folder.js（フォルダ）の両モードで共有する。
//
// 対象の決まり方:
//  - サイドバーのツリー項目（.tree-item）を右クリック → その項目を対象（VS Code の
//    Explorer 的な操作）。プレビューに何も開いていなくても効く。
//  - それ以外（プレビュー領域など）→ 現在プレビュー中のファイル。
//
// 安全性の考え方:
//  - 相対パス / 選択テキストのコピーは navigator.clipboard で JS 完結（IPC を介さない）。
//  - 絶対パスのコピー・Finder表示・既定アプリで開く だけ Rust へ IPC する。
//    Rust 側は root_dir / single_file_path から safe_join でパスを解決し、
//    実行系拡張子（.app/.command 等）は弾く。
//  - ページ全体に CSP がかかっており、本文（untrusted な Markdown）に埋め込まれた
//    <script> は実行されない。これがドライブバイ IPC の根本的な防御。
(function() {
  // 'folder' | 'single' | 'stdin'。init_script で注入される。
  var mode = window.MD_MENU_MODE || 'single';
  // フォルダモードで現在プレビュー中ファイルの相対パス。未ロード時は null。
  var currentRel = null;
  var menuEl = null;

  // 右クリック位置から操作対象を決める。
  //   rel   … IPC / コピーに使う相対パス（単一ファイルモードは '' で Rust が
  //           single_file_path を使う）
  //   relOk … 「相対パスをコピー」が意味を持つか
  //   has   … パス系の操作（絶対/Finder/開く）が可能か
  function resolveContext(e) {
    var ti = e.target.closest && e.target.closest('.tree-item');
    if (ti && ti.dataset && ti.dataset.path) {
      return { rel: ti.dataset.path, relOk: true, has: true };
    }
    if (mode === 'single') return { rel: '', relOk: false, has: true };
    if (mode === 'folder' && currentRel) return { rel: currentRel, relOk: true, has: true };
    return { rel: '', relOk: false, has: false };
  }

  function close() {
    if (!menuEl) return;
    menuEl.remove();
    menuEl = null;
    document.removeEventListener('mousedown', onDocMouseDown, true);
    document.removeEventListener('scroll', close, true);
    window.removeEventListener('blur', close);
    window.removeEventListener('resize', close);
  }

  function onDocMouseDown(e) {
    if (menuEl && !menuEl.contains(e.target)) close();
  }

  // Esc の受け手は MdCommon が一括で持つ（最前面の 1 つだけを閉じる）。
  // メニューは開いている間だけ DOM にあるので、存在がそのまま開閉状態。
  if (window.MdCommon && MdCommon.registerOverlay) {
    MdCommon.registerOverlay({
      id: 'md-context-menu',
      isOpen: function() { return !!menuEl; },
      close: close,
      priority: 40
    });
  }

  function dispatch(action, selection, ctx) {
    switch (action) {
      case 'copy-selection':
        if (selection) navigator.clipboard.writeText(selection).catch(function() {});
        break;
      case 'copy-rel':
        if (ctx.rel) navigator.clipboard.writeText(ctx.rel).catch(function() {});
        break;
      case 'copy-abs':
        window.ipc.postMessage('menu:abs:' + ctx.rel);
        break;
      case 'reveal':
        window.ipc.postMessage('menu:reveal:' + ctx.rel);
        break;
      case 'open':
        window.ipc.postMessage('menu:open:' + ctx.rel);
        break;
      case 'reload':
        if (window.MdReload) window.MdReload();
        break;
      case 'palette':
        if (window.MdPalette) window.MdPalette.open();
        break;
      case 'help':
        if (window.MdHelp) window.MdHelp.open();
        break;
    }
  }

  // 先頭/末尾/連続するセパレータを除去する。
  function trimSeparators(items) {
    var out = [];
    items.forEach(function(it) {
      if (it.sep) {
        if (out.length === 0 || out[out.length - 1].sep) return;
      }
      out.push(it);
    });
    while (out.length && out[out.length - 1].sep) out.pop();
    return out;
  }

  function buildItems(selection, ctx) {
    var items = [];
    if (selection) {
      items.push({ label: 'コピー', action: 'copy-selection', enabled: true });
      items.push({ sep: true });
    }
    // stdin はファイルが無くサイドバーも無いので、パス系グループごと出さない。
    if (mode !== 'stdin') {
      items.push({ label: '相対パスをコピー', action: 'copy-rel', enabled: ctx.relOk });
      items.push({ label: '絶対パスをコピー', action: 'copy-abs', enabled: ctx.has });
      items.push({ sep: true });
      items.push({ label: 'Finderで表示', action: 'reveal', enabled: ctx.has });
      items.push({ label: 'デフォルトアプリで開く', action: 'open', enabled: ctx.has });
      items.push({ sep: true });
    }
    // ファイル検索はフォルダモードだけの機能（別ファイルを開く入口があるのがそこだけ）。
    if (mode === 'folder') {
      items.push({ label: 'ファイル検索 (⌘P)', action: 'palette', enabled: true });
      items.push({ sep: true });
    }
    items.push({ label: '再読み込み', action: 'reload', enabled: true });
    items.push({ sep: true });
    items.push({ label: 'ショートカット一覧 (?)', action: 'help', enabled: true });
    return trimSeparators(items);
  }

  function render(items, x, y, selection, ctx) {
    close();
    var menu = document.createElement('div');
    menu.className = 'md-context-menu';
    menu.id = 'md-context-menu'; // MdCommon.isOverlayOpen が O(1) で存在を見るため
    // メニュー上の mousedown で選択やフォーカスを奪わない。
    menu.addEventListener('mousedown', function(e) { e.preventDefault(); });

    items.forEach(function(it) {
      if (it.sep) {
        var sep = document.createElement('div');
        sep.className = 'md-context-menu-sep';
        menu.appendChild(sep);
        return;
      }
      var row = document.createElement('div');
      row.className = 'md-context-menu-item' + (it.enabled ? '' : ' disabled');
      row.textContent = it.label;
      if (it.enabled) {
        row.addEventListener('click', function() {
          close();
          dispatch(it.action, selection, ctx);
        });
      }
      menu.appendChild(row);
    });

    document.body.appendChild(menu);

    // 画面端でのフリップ。
    var rect = menu.getBoundingClientRect();
    var px = x, py = y;
    if (px + rect.width > window.innerWidth) px = Math.max(0, window.innerWidth - rect.width - 4);
    if (py + rect.height > window.innerHeight) py = Math.max(0, window.innerHeight - rect.height - 4);
    menu.style.left = px + 'px';
    menu.style.top = py + 'px';

    menuEl = menu;
    // 同期で開いた直後の click/contextmenu を拾わないよう次tickで登録。
    setTimeout(function() {
      document.addEventListener('mousedown', onDocMouseDown, true);
      document.addEventListener('scroll', close, true);
      window.addEventListener('blur', close);
      window.addEventListener('resize', close);
    }, 0);
  }

  document.addEventListener('contextmenu', function(e) {
    // ネイティブメニューは常に抑止する。
    e.preventDefault();
    // サイドバーのリサイザ上・ドラッグ中は出さない。
    if (e.target.closest && e.target.closest('#resizer')) { close(); return; }
    if (document.body.style.cursor === 'col-resize') { close(); return; }

    // 選択テキストは発火時点で同期取得（後続の DOM 追加で消えないように）。
    var selection = window.getSelection ? String(window.getSelection()) : '';
    selection = selection && selection.trim() ? selection : '';

    var ctx = resolveContext(e);
    render(buildItems(selection, ctx), e.clientX, e.clientY, selection, ctx);
  });

  window.MdMenu = {
    // folder.js が loadPreview 時に現在ファイルの相対パスを通知する。
    setCurrentFile: function(rel) { currentRel = rel || null; },
    // iframe(html-frame)内のクリック/スクロールは親 document のリスナーに届かない。
    // common.js の bindFrame がここを呼んで閉じる（未オープン時は no-op）。
    close: close
  };
})();
