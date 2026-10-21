// 右クリック コンテキストメニュー。<head> に inline され window.MdMenu を公開する。
//
// 対象の決まり方:
//  - タブ（.md-tab）を右クリック → そのタブを対象（開閉の項目が増える）。
//  - サイドバーのツリー項目（.tree-item）を右クリック → その項目を対象（VS Code の
//    Explorer 的な操作）。プレビューに何も開いていなくても効く。
//  - サイドバーのヘッダ（#sidebar-header）→ いまの root が対象。ツリーに行が無い
//    root 自身にも、パスのコピーと Finder 表示を効かせるため。
//  - それ以外（プレビュー領域など）→ 現在プレビュー中のファイル。
//  - 本文（#preview-pane）の中は、何が書かれていても常に現在のファイル。タブ・ツリーの
//    行・ヘッダとして扱うのは、それぞれアプリの領域（#tabbar / #sidebar-col）の中だけ。
//
// 安全性の考え方:
//  - 相対パス / 選択テキストのコピーは MdCommon.copyText。navigator.clipboard が
//    使えない時だけ Rust（NSPasteboard）へ回る。
//  - 絶対パスのコピー・Finder表示・既定アプリで開く は常に Rust へ IPC する。
//    Rust 側は `?file=` と同じ関門（id_to_path）でパスを解決し、
//    実行系拡張子（.app/.command 等）は弾く。
//  - 既定では文書の JS は CSP で止まっているが、設定で動かすと、文書の JS もこの IPC を
//    叩ける。その間に止めているのは Rust 側のパス解決と拡張子ガードだけで、本文を
//    信頼できない前提の防御ではない。
(function() {
  // トップのフレームでなければ何もしない（理由は html.rs の PAGE_SCRIPTS）。
  if (window.top !== window) return;
  // 現在プレビュー中ファイルの識別子（絶対パス）。
  // 何も開いていない（`md .` で起動した直後など）間は null。
  var currentId = null;
  var menuEl = null;

  // アプリの領域（サイドバーの列・タブ帯・本文）。対象探しはこの中に縛る。
  // 本文は同じ document にあるので、`closest('#sidebar')` や `.tree-item` を素で探すと、
  // 本文に同じ id・クラスの要素を書かれただけで「サイドバーの項目」として通ってしまう。
  // 参照は MdCommon が本文の入る前に取ったもの（1 枚ページではどれも null）。
  function regionsNow() {
    return (window.MdCommon && MdCommon.regions)
      ? MdCommon.regions()
      : { sidebarCol: null, tabbar: null, pane: null, resizer: null };
  }

  // `target` から `selector` を辿った要素が、`region` の中にあるときだけ返す。
  function closestIn(region, target, selector) {
    if (!region || !region.contains(target) || !target.closest) return null;
    var el = target.closest(selector);
    return el && region.contains(el) ? el : null;
  }

  // 右クリック位置から操作対象を決める。
  //   id    … IPC / コピーに使う識別子（何も開いていなければ ''）
  //   relOk … 「相対パスをコピー」が意味を持つか
  //   has   … パス系の操作（絶対/Finder/開く）が可能か
  //   dir   … フォルダか（「ここを root にする」が意味を持つか）
  function resolveContext(e) {
    var t = e.target;
    var regions = regionsNow();
    // 本文の中なら、何が書かれていても「今開いているファイル」。
    if (regions.pane && regions.pane.contains(t)) return currentContext();
    // タブを右クリック → そのタブが対象（開閉の操作が増える）。
    var tab = closestIn(regions.tabbar, t, '.md-tab');
    if (tab && tab.dataset && tab.dataset.path) {
      // 設定タブ（#38）はファイルではない。開閉の操作だけを出し、パス系は灰色にする。
      if (window.MdSettings && MdSettings.isTab(tab.dataset.path)) {
        return { id: '', relOk: false, has: false, tab: tab.dataset.path };
      }
      return { id: tab.dataset.path, relOk: relOk(tab.dataset.path), has: true, tab: tab.dataset.path };
    }
    // サイドバーのヘッダ（フォルダ名）→ いまの root が対象（#34）。パスのコピーや
    // Finder 表示を、ツリーに行が無い root 自身に対しても効かせる。
    if (closestIn(regions.sidebarCol, t, '#sidebar-header')) {
      var root = (window.MdCommon && MdCommon.rootDir) ? MdCommon.rootDir() : '';
      if (root) return { id: root, relOk: true, has: true, dir: true };
    }
    // ツリーと Quick Access の行。`#sidebar` ではなく列全体で縛るのは、Quick Access の
    // 一覧（#quick-access-list）が `#sidebar` の兄弟にあるため。
    var ti = closestIn(regions.sidebarCol, t, '.tree-item');
    if (ti && ti.dataset && ti.dataset.path) {
      return { id: ti.dataset.path, relOk: true, has: true, dir: ti.dataset.kind === 'dir' };
    }
    return currentContext();
  }

  function currentContext() {
    if (currentId) return { id: currentId, relOk: relOk(currentId), has: true };
    return { id: '', relOk: false, has: false };
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

  // パイプ入力（#68）には相対パスが無い。表示名（`--title` か `stdin.md`）はパスでは
  // ないので、「相対パスをコピー」で名前を渡すと、パスのつもりで貼った先で迷う。
  function relOk(id) { return !(window.MdCommon && MdCommon.isStdinId && MdCommon.isStdinId(id)); }

  function dispatch(action, selection, ctx) {
    switch (action) {
      case 'copy-selection':
        if (selection) MdCommon.copyText(selection);
        break;
      case 'copy-rel':
        // 識別子は絶対パスなので、ここで root を剥ぐ。剥がないと「絶対パスをコピー」
        // と同じものが 2 行並ぶ。root の外のファイルは剥ぐものが無く絶対パスのまま。
        if (ctx.id) MdCommon.copyText(MdCommon.idToDisplay(ctx.id));
        break;
      case 'copy-abs':
        window.ipc.postMessage('menu:abs:' + ctx.id);
        break;
      case 'reveal':
        window.ipc.postMessage('menu:reveal:' + ctx.id);
        break;
      case 'open':
        window.ipc.postMessage('menu:open:' + ctx.id);
        break;
      case 'tab-close':
        if (window.MdTabs) MdTabs.closeByPath(ctx.tab);
        break;
      case 'tab-close-others':
        if (window.MdTabs) MdTabs.closeOthers(ctx.tab);
        break;
      case 'tab-close-all':
        if (window.MdTabs) MdTabs.closeAll();
        break;
      case 'set-root':
        if (window.MdRoot) window.MdRoot.set(ctx.id);
        break;
      case 'quick-toggle':
        if (window.MdQuick) window.MdQuick.toggle(ctx.id, !!ctx.dir);
        break;
      case 'palette':
        if (window.MdPalette) window.MdPalette.open();
        break;
      case 'sidebar-toggle':
        if (window.MdSidebar) window.MdSidebar.toggle();
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
    if (ctx.tab) {
      items.push({ label: '閉じる (⌘W)', action: 'tab-close', enabled: true });
      items.push({
        label: '他のタブを閉じる', action: 'tab-close-others',
        enabled: !!(window.MdTabs && MdTabs.count() > 1)
      });
      items.push({
        label: 'すべてのタブを閉じる', action: 'tab-close-all',
        enabled: !!(window.MdTabs && MdTabs.count() > 0)
      });
      items.push({ sep: true });
    }
    // フォルダが対象のときだけ。いま居る root には**出さない**——押しても何も
    // 起きないので、灰色で 1 行取る意味が無い（`enabled: false` にすると
    // `render` が disabled の行をそのまま並べる）。
    if (ctx.dir) {
      var root = (window.MdCommon && MdCommon.rootDir) ? MdCommon.rootDir() : '';
      if (ctx.id !== root) {
        items.push({ label: 'ここを root にする', action: 'set-root', enabled: true });
        items.push({ sep: true });
      }
    }
    // Quick Access（#35）。ツリーの行・ヘッダのフォルダ名・タブ・Quick Access の行、
    // どこからでも同じ 1 行を出す。対象が無いときだけ出さない（登録するものが無い）。
    if (ctx.has && window.MdQuick) {
      items.push({
        label: MdQuick.has(ctx.id) ? 'Quick Access から外す' : 'Quick Access に追加',
        action: 'quick-toggle',
        enabled: true
      });
      items.push({ sep: true });
    }
    // パス系。対象が無いとき（何も開いていない本文の右クリック）は disabled で残す。
    items.push({ label: '相対パスをコピー', action: 'copy-rel', enabled: ctx.relOk });
    items.push({ label: '絶対パスをコピー', action: 'copy-abs', enabled: ctx.has });
    items.push({ sep: true });
    items.push({ label: 'Finderで表示', action: 'reveal', enabled: ctx.has });
    items.push({ label: 'デフォルトアプリで開く', action: 'open', enabled: ctx.has });
    items.push({ sep: true });
    items.push({ label: 'ファイル検索 (⌘P)', action: 'palette', enabled: true });
    var treeOpen = !window.MdSidebar || window.MdSidebar.isOpen();
    items.push({
      label: (treeOpen ? 'ファイルツリーを隠す' : 'ファイルツリーを表示') + ' (⌘B)',
      action: 'sidebar-toggle',
      enabled: true
    });
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
    var resizer = regionsNow().resizer;
    if (resizer && resizer.contains(e.target)) { close(); return; }
    if (document.body.style.cursor === 'col-resize') { close(); return; }

    // 選択テキストは発火時点で同期取得（後続の DOM 追加で消えないように）。
    // iframe（.html-frame）内で選択して右クリックした時は、bindFrame が座標だけを
    // 載せた合成イベントを親へ投げてくる——親の選択は空なので、MdCommon.selectionText
    // が iframe の文書まで見て拾う。
    var selection = MdCommon.selectionText();

    var ctx = resolveContext(e);
    render(buildItems(selection, ctx), e.clientX, e.clientY, selection, ctx);
  });

  window.MdMenu = {
    // folder.js が loadPreview 時に現在ファイルの識別子を通知する。
    setCurrentFile: function(id) { currentId = id || null; },
    // iframe(html-frame)内のクリック/スクロールは親 document のリスナーに届かない。
    // common.js の bindFrame がここを呼んで閉じる（未オープン時は no-op）。
    close: close
  };
})();
