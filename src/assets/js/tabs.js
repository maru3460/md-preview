// VSCode 風のタブ。複数ファイルを開いたまま行き来する。
//
// 状態は「パス・スクロール位置・ビューモード」の 3 つだけを持ち、切替のたびに
// 本文は再フェッチする（DOM をタブごとに抱えない）。#preview-pane を 1 枚だけ使う
// 既存の構造をそのまま活かせるのが理由で、hljs / mermaid / drawio の再実行が
// 体感で重くなるようなら「タブごとに DOM を保持して display 切替」へ寄せる。
//
// 入口は folder.js の loadPreview() 1 本。ツリークリック / [ ] 巡回 / ⌘P /
// 本文リンク / iframe 内リンク / コメントのジャンプ は全部そこを通るので、
// onOpen() のフックだけで「開いたものは必ずタブに乗る」が成り立つ。
//
// タブの識別子は loadPreview に渡るパスそのもの（root 相対、または root の外なら
// 絶対パス）。stdin をパイプで渡したときの一時ファイルも、後者としてここに乗る。
(function() {
  // { path, scroll, mode } の配列。並び順がそのままタブバーの並び。
  var tabs = [];
  var activeIdx = -1;
  var opts = null;         // { openFile(path) }
  // タブ操作から loadPreview を呼んでいる間だけ true。loadPreview は入口で
  // onOpen を呼ぶので、この印が無いと自分の呼び出しで状態を二重に書き換える。
  var switching = false;

  function bar() { return document.getElementById('tabbar'); }

  function indexOf(path) {
    for (var i = 0; i < tabs.length; i++) if (tabs[i].path === path) return i;
    return -1;
  }

  function baseName(p) {
    var segs = String(p).split('/');
    return segs[segs.length - 1] || p;
  }
  function parentName(p) {
    var segs = String(p).split('/');
    return segs.length >= 2 ? segs[segs.length - 2] : '';
  }

  // 現在出ているビューモード（raw / diff、無ければ null）。
  function currentMode() {
    return (window.MdViewModes && MdViewModes.currentId) ? MdViewModes.currentId() : null;
  }
  // 表示は切り替えず、モードの ON/OFF だけをタブの記録に合わせる。
  // 実際の再描画はこの直後の loadPreview が行う。
  function applyMode(id) {
    if (window.MdViewModes && MdViewModes.restore) MdViewModes.restore(id);
  }

  // いま表示中のタブへ、離れる直前の状態を書き戻す。
  function saveActiveState() {
    var t = tabs[activeIdx];
    if (!t) return;
    t.scroll = MdCommon.readScroll();
    t.mode = currentMode();
  }

  // activeIdx のタブを実際に表示する。呼ぶ前に activeIdx を確定させておくこと。
  function show() {
    var t = tabs[activeIdx];
    if (!t || !opts) return;
    applyMode(t.mode);
    render();
    switching = true;
    try { opts.openFile(t.path); } finally { switching = false; }
  }

  // ── loadPreview のフック ────────────────────────────────────
  // ファイル切替のたびに（ホットリロードを除いて）呼ばれる。
  function onOpen(path) {
    if (switching) return;  // タブ操作が起点。状態はそちらで確定済み
    var i = indexOf(path);
    // i === -1 は「タブに無い」。activeIdx も 1 枚も無い間は -1 なので、
    // 見つかった時だけ「同じタブ」と判定する（でないと最初の 1 枚が作られない）。
    if (i !== -1 && i === activeIdx) {
      // 開いているファイルをもう一度開いた（ツリー再クリック・同一ファイルへの
      // リンク）。読み位置を捨てずに残す（scrollFor がこれを返す）。
      saveActiveState();
      return;
    }
    // 新しいタブは直前のモードを引き継ぐ。raw のまま次のファイルへ移る、という
    // これまでの「モードは維持される」挙動をタブ越しでも保つため。
    var inherited = currentMode();
    saveActiveState();
    if (i === -1) {
      // 常に新しいタブ。VSCode と同じく現在タブの右隣に挿す。
      i = activeIdx + 1;
      tabs.splice(i, 0, { path: path, scroll: 0, mode: inherited });
    }
    activeIdx = i;
    applyMode(tabs[i].mode);
    render();
  }

  // 起動時（`md a.md b.md`）に複数のタブを並べる。フェッチするのは最初の 1 枚だけで、
  // 残りはタブに載せるだけ（開いた時に取りに行く）。起動を N ファイルぶん遅らせない。
  function openInitial(paths) {
    if (!paths || !paths.length) return;
    paths.forEach(function(p) {
      if (p && indexOf(p) === -1) tabs.push({ path: p, scroll: 0, mode: null });
    });
    if (!tabs.length) return;
    activeIdx = 0;
    show();
  }

  // loadPreview が「このファイルを出した直後に戻すスクロール位置」を訊きに来る。
  function scrollFor(path) {
    var t = tabs[indexOf(path)];
    return t ? (t.scroll || 0) : 0;
  }

  // ── 操作 ────────────────────────────────────────────────────
  function activate(i) {
    if (i < 0 || i >= tabs.length || i === activeIdx) return;
    saveActiveState();
    activeIdx = i;
    show();
  }

  function closeWindow() {
    if (window.MdCommon && MdCommon.closeWindow) MdCommon.closeWindow();
  }

  function closeAt(i) {
    // タブが 1 枚も無い（`md .` で起動してまだ何も開いていない）ときの ⌘W は
    // ウィンドウを閉じる。ここで抜けてしまうと、⌘W に割り当てられているのは
    // tab-close だけなので、キーが完全に無反応になる。
    if (!tabs.length) { closeWindow(); return; }
    if (i < 0 || i >= tabs.length) return;
    // 最後の 1 枚を閉じるのもウィンドウを閉じるのと同じ（⌘W の従来の意味）。
    if (tabs.length === 1) { closeWindow(); return; }
    var wasActive = (i === activeIdx);
    tabs.splice(i, 1);
    if (wasActive) {
      // 右隣へ移る（右端だったら左隣）。
      activeIdx = Math.min(i, tabs.length - 1);
      show();
    } else {
      if (i < activeIdx) activeIdx--;
      render();
    }
  }

  function closeOthers(path) {
    var keep = tabs[indexOf(path)];
    if (!keep) return;
    var wasActive = (tabs[activeIdx] === keep);
    tabs = [keep];
    activeIdx = 0;
    if (wasActive) render();
    else show();
  }

  // ⇧Tab : 次のタブへ。端まで行ったら先頭へ折り返す（1 方向だけなのは、逆回りを
  // 足すより ⌘1..⌘9 で直に飛ぶ方が速いため）。
  function cycle(delta) {
    if (tabs.length < 2) return;
    activate((activeIdx + delta + tabs.length) % tabs.length);
  }

  // ⌘1..⌘9 : n 番目のタブ。⌘9 は VSCode と同じく「最後のタブ」。
  // 無い番号（3 枚しか無いのに ⌘5）は何もしない。端へ寄せると、押し間違いが
  // 「意図しないタブへの移動」になってしまう。
  function goto(n) {
    if (!tabs.length) return;
    if (n >= 9) { activate(tabs.length - 1); return; }
    if (n > tabs.length) return;
    activate(n - 1);
  }

  // ── 描画 ────────────────────────────────────────────────────
  // タブは高々数十枚なので毎回作り直す（差分更新の複雑さに見合わない）。
  function render() {
    var el = bar();
    if (!el) return;
    document.body.classList.toggle('has-tabs', tabs.length > 0);
    el.innerHTML = '';

    // 同じファイル名のタブが並ぶ時だけ、親ディレクトリ名を添えて見分ける。
    var counts = {};
    tabs.forEach(function(t) {
      var n = baseName(t.path);
      counts[n] = (counts[n] || 0) + 1;
    });

    var activeEl = null;
    tabs.forEach(function(t, i) {
      var tab = document.createElement('div');
      tab.className = 'md-tab' + (i === activeIdx ? ' active' : '');
      // 右クリックメニュー（contextmenu.js）が対象を引くために持たせる。
      tab.dataset.path = t.path;
      tab.title = t.path;

      var name = document.createElement('span');
      name.className = 'md-tab-name';
      name.textContent = baseName(t.path);
      tab.appendChild(name);

      var dir = parentName(t.path);
      if (dir && counts[baseName(t.path)] > 1) {
        var sub = document.createElement('span');
        sub.className = 'md-tab-dir';
        sub.textContent = dir;
        tab.appendChild(sub);
      }

      var close = document.createElement('span');
      close.className = 'md-tab-close';
      close.textContent = '×';
      close.title = '閉じる (⌘W)';
      close.addEventListener('mousedown', function(e) {
        // 右クリックはタブのコンテキストメニューに渡す。ここで閉じてしまうと、
        // 先に開いたメニューだけが取り残される（contextmenu が mousedown より先）。
        if (e.button !== 0 && e.button !== 1) return;
        e.preventDefault();
        e.stopPropagation();
        closeAt(i);
      });
      tab.appendChild(close);

      // click ではなく mousedown で受ける。preventDefault してタブへフォーカスが
      // 移るのを防ぐと、切替直後から本文のスクロール素キーがそのまま効く。
      tab.addEventListener('mousedown', function(e) {
        e.preventDefault();
        if (e.button === 1) closeAt(i);        // 中クリックで閉じる
        else if (e.button === 0) activate(i);
      });

      el.appendChild(tab);
      if (i === activeIdx) activeEl = tab;
    });

    if (activeEl) activeEl.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }

  function registerKeys() {
    if (!window.MdKeymap) return;
    MdKeymap.on('tab-close', function() { closeAt(activeIdx); });
    MdKeymap.on('tab-cycle', function() { cycle(1); });
    MdKeymap.on('tab-goto', function(e) { goto(parseInt(e.key, 10)); });
  }

  window.MdTabs = {
    // o: { openFile(path) } … タブ切替で本文を出し直すための入口（loadPreview）。
    init: function(o) {
      opts = o;
      registerKeys();
      // 縦ホイールを横スクロールに回す（トラックパッド以外でもタブを辿れるように）。
      var el = bar();
      if (el) {
        el.addEventListener('wheel', function(e) {
          if (e.deltaX !== 0) return;
          el.scrollLeft += e.deltaY;
          e.preventDefault();
        }, { passive: false });
      }
    },
    onOpen: onOpen,
    openInitial: openInitial,
    scrollFor: scrollFor,
    closeByPath: function(path) { closeAt(indexOf(path)); },
    closeOthers: closeOthers,
    count: function() { return tabs.length; }
  };
})();
