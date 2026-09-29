// 通知ベルと届いたファイルの履歴（#32）。`md --notify <file>` で届いたものを
// タブバー右端のベルに溜めて、押すと一覧が出る。
//
// **一覧を持っているのは Rust 側。** ここは渡されたものを映すだけで、自分では
// 増やしも減らしもしない。既読を押したときも、自分の配列を書き換えるのではなく
// Rust へ要求を送り、返ってきた一覧で描き直す。持ち主を 2 つにすると、窓が
// 描ける前に届いたぶん（Rust だけが知っている）と画面の中身がずれる。
//
// 渡ってくる 1 件は { path, at, read }。path は識別子（絶対パス）、at は届いた
// 時刻（UNIX 秒）、read は既読か。並びは新しい順で、その順に描く。
//
// ドロップダウンは開いている間だけ DOM にある（右クリックメニューと同じ作法）。
// 開いている間は本文の素キーを止めるので、j/k/Enter はここで受ける——keymap.js の
// 表には `b` だけを載せ、中の移動キーは ⌘P のパレットと同じくモジュール側が持つ。
(function() {
  // Rust から渡された全件。ここは写しであって持ち主ではない。
  var list = [];
  var opts = null;          // { openFile(id) }
  var panel = null;         // 開いている間だけ DOM にある
  var bellEl = null;
  var cursor = -1;          // j/k のカーソル。-1 はどこにも居ない

  // 一度に見せる件数。上限 100 件を全部出すと画面の端まで届くので、ここで打ち切って
  // 残りはスクロールさせる（#32 の決め）。1 件の高さ × この数が最大の高さになる。
  var VISIBLE_ROWS = 7;

  function iconsEl() { return document.getElementById('tabbar-icons'); }

  // ── 表示の材料 ──────────────────────────────────────────────
  // 名前は識別子ではなく表示名（root を剥いだ形）から作る。タブと同じ基準にしないと、
  // 同じファイルがベルとタブで違う名前で出る。
  function displayOf(p) {
    return (window.MdCommon && MdCommon.idToDisplay) ? MdCommon.idToDisplay(p) : String(p);
  }
  function baseName(p) {
    var segs = displayOf(p).split('/');
    return segs[segs.length - 1] || p;
  }
  // 直上のフォルダ名。**名前だけでは足りない**ので必ず添える——通知で届くのは AI が
  // 書いた md で、`README.md` や `notes.md` が別のフォルダから並ぶ。
  function parentName(p) {
    var segs = displayOf(p).split('/');
    segs.pop();
    return segs.pop() || '';
  }

  // 届いた時刻を「3 分前」の形にする。
  //
  // 1 日を超えたぶんは経過秒ではなく**暦の日**で数える。23:50 に届いたものが翌 0:10 に
  // 「20 分前」なのは正しいが、そこから先を経過秒で割ると、日付が変わっているのに
  // 「0 日前」と言うことになる。
  function relTime(at) {
    if (!at) return '';
    var then = at * 1000;
    var now = Date.now();
    var sec = Math.floor((now - then) / 1000);
    if (sec < 0) return 'たった今';        // 時計が巻き戻った端末でも嘘を言わない
    if (sec < 60) return 'たった今';
    if (sec < 3600) return Math.floor(sec / 60) + ' 分前';
    if (sec < 86400) return Math.floor(sec / 3600) + ' 時間前';
    var days = dayDiff(then, now);
    if (days <= 1) return '昨日';
    if (days < 7) return days + ' 日前';
    var d = new Date(then);
    return (d.getMonth() + 1) + '/' + d.getDate();
  }
  function dayDiff(a, b) {
    function midnight(ms) {
      var d = new Date(ms);
      return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
    }
    return Math.round((midnight(b) - midnight(a)) / 86400000);
  }

  function unreadCount() {
    var n = 0;
    for (var i = 0; i < list.length; i++) if (!list[i].read) n++;
    return n;
  }

  // ── ベル ────────────────────────────────────────────────────
  function buildBell() {
    var host = iconsEl();
    if (!host || bellEl) return;
    bellEl = document.createElement('button');
    bellEl.type = 'button';
    bellEl.id = 'md-bell';
    bellEl.title = '通知 (b)';
    bellEl.setAttribute('aria-label', '通知');
    bellEl.innerHTML =
      '<svg viewBox="0 0 16 16" width="15" height="15" fill="currentColor" aria-hidden="true">' +
      '<path d="M8 1.2a3.4 3.4 0 0 0-3.4 3.4v2.1c0 .55-.22 1.08-.6 1.47L3 9.2h10l-1-1.03a2.08 2.08 0 0 1-.6-1.47V4.6A3.4 3.4 0 0 0 8 1.2ZM6.4 10.6a1.6 1.6 0 0 0 3.2 0H6.4Z"/>' +
      '</svg><span class="md-bell-count" hidden></span>';
    // click ではなく mousedown。click だと、開いた瞬間に同じ押下の click が
    // 「外を押した」として届いて即座に閉じる。
    bellEl.addEventListener('mousedown', function(e) {
      e.preventDefault();
      if (e.button === 0) toggle();
    });
    host.appendChild(bellEl);
  }

  function renderBell() {
    if (!bellEl) return;
    var n = unreadCount();
    var count = bellEl.querySelector('.md-bell-count');
    // 未読が無いときは数も丸も出さない。ベルがそこに在ること自体は消さない——
    // 消すと「通知という機能がある」ことに気づく手がかりが無くなる。
    count.hidden = n === 0;
    count.textContent = n > 99 ? '99+' : String(n);
    bellEl.classList.toggle('has-unread', n > 0);
    bellEl.title = n > 0 ? '通知 ' + n + ' 件 (b)' : '通知 (b)';
  }

  // ── ドロップダウン ──────────────────────────────────────────
  function isOpen() { return !!panel; }

  function open() {
    if (panel || !bellEl) return;
    panel = document.createElement('div');
    panel.className = 'md-bell-panel';
    panel.id = 'md-bell-panel';
    document.body.appendChild(panel);
    // 最初の 1 件にカーソルを置く。開いてすぐ Enter で最新を開けるようにするため。
    cursor = list.length ? 0 : -1;
    renderPanel();
    place();
    bellEl.classList.add('is-open');
    document.addEventListener('mousedown', onDocMouseDown, true);
    document.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('resize', place);
  }

  function close() {
    if (!panel) return;
    panel.remove();
    panel = null;
    cursor = -1;
    if (bellEl) bellEl.classList.remove('is-open');
    document.removeEventListener('mousedown', onDocMouseDown, true);
    document.removeEventListener('keydown', onKeyDown, true);
    window.removeEventListener('resize', place);
  }

  function toggle() { if (panel) close(); else open(); }

  // ベルの真下・右端揃え。fixed なので窓の座標で置く。
  function place() {
    if (!panel || !bellEl) return;
    var r = bellEl.getBoundingClientRect();
    panel.style.top = (r.bottom + 4) + 'px';
    // 画面の左へはみ出さない下限だけ見る。右端は窓の縁から 8px。
    panel.style.right = Math.max(8, window.innerWidth - r.right - 2) + 'px';
  }

  function renderPanel() {
    if (!panel) return;
    panel.innerHTML = '';

    var head = document.createElement('div');
    head.className = 'md-bell-head';
    var title = document.createElement('span');
    title.className = 'md-bell-title';
    title.textContent = '通知';
    head.appendChild(title);
    // 「すべて既読」は未読があるときだけ。押すものが無いのに置くと、押せるように
    // 見えて何も起きないボタンになる。
    if (unreadCount() > 0) {
      var all = document.createElement('button');
      all.type = 'button';
      all.className = 'md-bell-readall';
      all.textContent = 'すべて既読';
      all.addEventListener('mousedown', function(e) {
        e.preventDefault();
        if (e.button === 0) send('read-all');
      });
      head.appendChild(all);
    }
    panel.appendChild(head);

    if (!list.length) {
      var empty = document.createElement('div');
      empty.className = 'md-bell-empty';
      empty.textContent = 'まだ何も届いていません';
      panel.appendChild(empty);
      return;
    }

    var rows = document.createElement('div');
    rows.className = 'md-bell-list';
    rows.style.maxHeight = 'calc(var(--md-bell-row-h) * ' + VISIBLE_ROWS + ')';
    list.forEach(function(item, i) {
      rows.appendChild(buildRow(item, i));
    });
    panel.appendChild(rows);
  }

  function buildRow(item, i) {
    var row = document.createElement('div');
    row.className = 'md-bell-row' + (item.read ? ' is-read' : ' is-unread') +
      (i === cursor ? ' is-cursor' : '');
    row.dataset.path = item.path;
    row.title = displayOf(item.path);

    var dot = document.createElement('span');
    dot.className = 'md-bell-dot';
    row.appendChild(dot);

    // 名前と「親フォルダ · 相対時刻」の 2 段。1 段に詰めると親フォルダの置き場所が
    // 無くなり、同名のファイルが並んだときに見分けが付かない（#32 の決め）。
    var lines = document.createElement('span');
    lines.className = 'md-bell-lines';
    var name = document.createElement('span');
    name.className = 'md-bell-name';
    name.textContent = baseName(item.path);
    lines.appendChild(name);
    var sub = document.createElement('span');
    sub.className = 'md-bell-sub';
    var dir = parentName(item.path);
    var when = relTime(item.at);
    sub.textContent = dir && when ? dir + ' · ' + when : (dir || when);
    lines.appendChild(sub);
    row.appendChild(lines);

    row.addEventListener('mousedown', function(e) {
      e.preventDefault();
      if (e.button !== 0) return;
      cursor = i;
      activate();
    });
    return row;
  }

  // カーソルの行を開く。**開いても一覧は閉じない**——続けて次を開けるようにする
  // （閉じるのは Esc とベルの押し直し）。行も消さず、既読になって残る。
  function activate() {
    var item = list[cursor];
    if (!item) return;
    if (opts && opts.openFile) opts.openFile(item.path);
    send('read:' + item.path);
  }

  function send(verb) {
    if (window.ipc) window.ipc.postMessage('notify:' + verb);
  }

  function moveCursor(delta) {
    if (!list.length) return;
    cursor = Math.max(0, Math.min(list.length - 1, cursor + delta));
    renderPanel();
    var el = panel && panel.querySelector('.md-bell-row.is-cursor');
    if (el) el.scrollIntoView({ block: 'nearest' });
  }

  function onDocMouseDown(e) {
    if (panel && !panel.contains(e.target) && bellEl && !bellEl.contains(e.target)) close();
  }

  // 開いている間の移動キー。capture で受けて本文へ流さない。
  // Esc は MdCommon が一括で持っている（最前面の 1 つだけを閉じる）ので拾わない。
  function onKeyDown(e) {
    if (!panel) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === 'j' || e.key === 'ArrowDown') { e.preventDefault(); moveCursor(1); }
    else if (e.key === 'k' || e.key === 'ArrowUp') { e.preventDefault(); moveCursor(-1); }
    else if (e.key === 'Enter') { e.preventDefault(); activate(); }
    else if (e.key === 'b') { e.preventDefault(); close(); }
  }

  window.MdNotify = {
    // o: { openFile(id) } … 行を押したときに本文を出す入口。
    init: function(o) {
      opts = o;
      buildBell();
      renderBell();
      if (window.MdKeymap) MdKeymap.on('notify-toggle', toggle);
      if (window.MdCommon && MdCommon.registerOverlay) {
        // 開いている間は本文の素キーを止める（j/k が裏でスクロールしない）。
        // 優先度は右クリックメニュー（40）より下、検索バー（20）より上。
        MdCommon.registerOverlay({
          id: 'md-bell-panel',
          isOpen: isOpen,
          close: close,
          priority: 30
        });
      }
    },
    // Rust から全件を渡される唯一の口（#32）。差分ではないので、毎回まるごと置き換える。
    push: function(next) {
      list = Array.isArray(next) ? next : [];
      if (cursor >= list.length) cursor = list.length - 1;
      renderBell();
      if (panel) { renderPanel(); place(); }
    },
    isOpen: isOpen,
    toggle: toggle,
    close: close
  };
})();
