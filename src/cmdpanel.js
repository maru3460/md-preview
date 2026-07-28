// コマンドパネル（⌘/）。使えるコマンドをカテゴリ別に右サイドバーへ常時表示し、
// 「覚えるまで見ながら操作する」ための学習装置。表示内容は keymap.js の MdKeymap から
// 導出する（このファイルは文言を持たない）。
//
// `?` のショートカット一覧とは役割が別:
//   ?   … 辞書。ど忘れした時にモーダルで引いて閉じる
//   ⌘/  … 隣で指差してくれる人。開いたまま操作する
//
// 【重要】このパネルは非モーダルである。MdCommon.isOverlayOpen() の判定対象に
// 加えてはいけない。加えると、開いている間 j/k 等の素キーが全部死ぬ
// （keyscroll.js / comment.js が isOverlayOpen() で素キーを止めるため）。
//
// 配色は help / context-menu と同じくシステムカラー + color-mix で theme-agnostic に
// 組む（themes/*.css への追記は不要）。
(function() {
  // 2 段組みに広げてよい最小幅。広げたパネル自体が入らない幅では意味がない。
  var WIDE_MIN_WIDTH = 620;

  // 本文に残しておきたい幅。これを割ったら飾りの外周余白を削って本文へ回す。
  var CONTENT_MIN = 560;

  var LS_KEY = 'md-cmdpanel-on';

  var panel = null;   // パネル本体（初回 open で作り、以降は hidden で開閉する）
  var open = false;
  var rows = [];      // [{ el, bind }] 行の DOM 参照。状態連動は DOM を作り直さず class だけ差し替える
  var cats = [];      // [{ el, cat }] カテゴリ見出しを含む箱の DOM 参照
  var lastSig = null; // 直前の状態署名。変化が無ければ何もしない

  function lsGet(k) { try { return localStorage.getItem(k); } catch (e) { return null; } }
  function lsSet(k, v) { try { localStorage.setItem(k, v); } catch (e) {} }

  // ── 描画 ────────────────────────────────────────────────────
  function build() {
    panel = document.createElement('aside');
    panel.className = 'md-cmdpanel hidden';
    panel.id = 'md-cmdpanel';

    var head = document.createElement('div');
    head.className = 'md-cmdpanel-head';
    var title = document.createElement('span');
    title.className = 'md-cmdpanel-title';
    title.textContent = 'コマンド';
    var hint = document.createElement('span');
    hint.className = 'md-cmdpanel-hint';
    hint.textContent = '⌘/ で閉じる';
    head.appendChild(title);
    head.appendChild(hint);
    panel.appendChild(head);

    var body = document.createElement('div');
    body.className = 'md-cmdpanel-body';
    body.id = 'md-cmdpanel-body';
    panel.appendChild(body);

    // 本文差し替え（ホットリロード・ファイル移動）で消えないよう body 直下に置く。
    // #preview-pane の中に入れると folder.js が innerHTML を空にする時に消える。
    document.body.appendChild(panel);
    render();
  }

  // MdKeymap から中身を組み立てる。行の集合はモードで決まり実行中に変わらないので、
  // これは実質 1 回だけ走る。状態連動（グレーアウト）は DOM を作り直さず sync() が
  // class を差し替える（j/k 連打のたびに 30 行作り直すのを避ける）。
  function render() {
    var body = panel.querySelector('.md-cmdpanel-body');
    body.textContent = '';
    rows = [];
    cats = [];
    window.MdKeymap.groups().forEach(function(g) {
      var group = document.createElement('div');
      group.className = 'md-cmdpanel-group';

      var cat = document.createElement('div');
      cat.className = 'md-cmdpanel-cat';
      cat.textContent = g.cat.label;
      group.appendChild(cat);
      cats.push({ el: group, cat: g.cat });

      g.binds.forEach(function(b) {
        var row = document.createElement('div');
        row.className = 'md-cmdpanel-row';
        var key = document.createElement('kbd');
        key.className = 'md-cmdpanel-key';
        key.textContent = b.keys;
        var desc = document.createElement('span');
        desc.className = 'md-cmdpanel-desc';
        desc.textContent = b.desc;
        row.appendChild(key);
        row.appendChild(desc);
        group.appendChild(row);
        rows.push({ el: row, bind: b });
      });
      body.appendChild(group);
    });
    lastSig = null; // 作り直したので次の sync で必ず塗り直す
    sync();
    applyLayout(); // 行数が変わったので高さの段階も測り直す
  }

  // ── 状態連動（グレーアウト）──────────────────────────────────
  // 「今この状態で使えないコマンドは薄くする（消さない）」。消すと行が動いて
  // 位置で覚えられなくなるうえ、なぜ無いのかが分からないため。
  function sync() {
    if (!open || !panel) return;
    var km = window.MdKeymap;
    var s = km.state();
    var sig = km.signature(s);
    if (sig === lastSig) return;
    lastSig = sig;

    rows.forEach(function(r) {
      r.el.classList.toggle('is-disabled', !r.bind.enabled(s));
    });
    // モード専用カテゴリ（コメント / ファイルツリー）は、そのモードでない間
    // セクションまるごと薄くする。行単位のグレーより「今はこの塊ごと関係ない」が伝わる。
    cats.forEach(function(c) {
      var inactive = false;
      if (c.cat.mode === 'comment') inactive = !s.comment;
      if (c.cat.mode === 'tree') inactive = !s.tree;
      c.el.classList.toggle('is-inactive', inactive);
    });
  }

  // 状態が変わりうるイベントのあとに 1 回だけ sync する。
  // keydown は「そのキーが状態を変えた後」を見たいので rAF で 1 フレーム待つ。
  var syncTick = false;
  function syncSoon() {
    if (syncTick) return;
    syncTick = true;
    requestAnimationFrame(function() { syncTick = false; sync(); });
  }

  // ── レイアウトの調停 ────────────────────────────────────────
  // 【方針】パネルは本文の上に重ねない。常に右ガターを取り、狭い時は本文の幅が縮む。
  // 重ねると本文が読めなくなる範囲が状況次第で変わって落ち着かないので、
  // 「パネルは必ず全部見える / 本文は狭くなるだけ」に振る（本文は読み手が
  // ⌘/ で閉じるか窓を広げれば戻せる）。
  //
  // 【前提】このパネルにはフォーカスが無く、キーボードでスクロールできない。
  // つまり「入り切らないぶんはスクロールで」は逃げにならず、見えない行は
  // 存在しないのと同じになる（全コマンドを見せるのが本機能の目的なので致命的）。
  // そこで段階的に詰めて必ず高さに収める:
  //   1. そのまま  →  2. 密度を詰める  →  3. 広げて 2 段組み  →  4. 最後の手段としてスクロール
  // 4 は極端に低い窓（パネルの見出しすら入らない）だけの保険。
  // 3 で本文はさらに狭くなるが、見えない行を作るより本文が狭いほうがましと判断する。

  function avail() {
    return window.MdCommon ? MdCommon.availWidth() : (window.innerWidth || 0);
  }

  // 中身が高さに収まっているか。overflow:hidden でも scrollHeight は実寸を返す。
  function fits() {
    var b = panel.querySelector('.md-cmdpanel-body');
    return b.scrollHeight <= b.clientHeight + 1;
  }

  function applyLayout() {
    if (!open || !panel) return;
    var cl = document.body.classList;
    var w = avail();

    cl.remove('md-cmdpanel-compact', 'md-cmdpanel-wide', 'md-cmdpanel-scroll');

    if (!fits()) {
      // 段階 1: 行間・文字・余白を詰める。レイアウトの形は変えないので副作用が小さい。
      cl.add('md-cmdpanel-compact');

      // 段階 2: パネルを広げて 2 段組みにする。高さが半分になる。
      if (!fits() && w >= WIDE_MIN_WIDTH) cl.add('md-cmdpanel-wide');

      // 段階 3（最後の手段）: パネルの見出しすら入らない極端に低い窓。
      if (!fits()) cl.add('md-cmdpanel-scroll');
    }

    // パネルは本文の上に重ねず、常に右ガターを取る（＝本文が狭くなる）。
    // 本文に回せる幅が乏しくなったら、飾りの外周余白を削って本文へ回す。
    cl.toggle('md-cmdpanel-tight', w - panel.offsetWidth < CONTENT_MIN);
  }

  // TOC（⌘T）とは同じ右カラムを取り合うので相互排他にする。判定は toc.js の
  // autoEvaluate 側に持たせてある（パネルが開いている間はレールが埋まっている＝
  // 「収まらない」扱いで自動退避する）ので、ここは再評価を促すだけでよい。
  //
  // MdToc.close() を呼んで閉じる手もあるが、あれは内部で userClosed を立てて
  // 「ユーザーが自分で閉じた」状態にしてしまう。そうするとパネルを閉じた後も
  // TOC が自動で復帰しなくなるため、こちらから閉じには行かない。
  function syncToc() {
    if (window.MdToc && MdToc.reevaluate) MdToc.reevaluate();
  }

  // ── 開閉 ────────────────────────────────────────────────────
  function show(instant) {
    if (open) return;
    if (!panel) build();
    open = true;
    if (instant) {
      panel.classList.add('no-anim');
      requestAnimationFrame(function() {
        requestAnimationFrame(function() { panel.classList.remove('no-anim'); });
      });
    }
    panel.classList.remove('hidden');
    document.body.classList.add('md-cmdpanel-open');
    applyLayout();
    // Web フォントの適用で行の高さが変わることがあるので、次フレームで測り直す。
    requestAnimationFrame(applyLayout);
    lsSet(LS_KEY, '1');
    syncToc(); // 開いた＝レールが埋まったので TOC は退避する
  }

  function hide() {
    if (!open || !panel) return;
    open = false;
    panel.classList.add('hidden');
    document.body.classList.remove(
      'md-cmdpanel-open', 'md-cmdpanel-tight',
      'md-cmdpanel-compact', 'md-cmdpanel-wide', 'md-cmdpanel-scroll'
    );
    lsSet(LS_KEY, '0');
    syncToc(); // レールが空いたので、TOC が出るべき状況なら戻る
  }

  function toggle() {
    if (open) hide(); else show(false);
  }

  // ── キーバインド ────────────────────────────────────────────
  // ⌘/（Cmd/Ctrl + スラッシュ）。/ が検索、? がヘルプなので「ヘルプ family」として覚えやすい。
  // Shift 併用（⌘?）は除外する。iframe 内からの転送は common.js のホワイトリスト経由。
  document.addEventListener('keydown', function(e) {
    if (!(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey) return;
    if (e.key !== '/' && e.code !== 'Slash') return;
    e.preventDefault();
    toggle();
  });

  // キーで状態が変わる（c でコメントモード、Tab でフォーカス移動、⌘F で検索）ので、
  // 押されたら追従させる。そのキーが処理された「後」を見たいので sync は 1 フレーム後。
  document.addEventListener('keydown', syncSoon, true);

  // オーバーレイ（検索バー・右クリメニュー・コメント入力）の開閉には専用イベントが無いので、
  // 状態が変わりうる入力のあとに署名を見て間引きつつ追従する。
  document.addEventListener('mousedown', syncSoon, true);
  document.addEventListener('contextmenu', syncSoon, true);
  // フォーカスの所在（本文 / ツリー / 入力欄）はグレーアウトの主要な条件。
  document.addEventListener('focusin', syncSoon, true);
  document.addEventListener('focusout', syncSoon, true);

  // 窓の大きさが変わったら段階を測り直す（パネル自体は閉じない）。
  // 高さが縮んだ時に収まらなくなるので、幅だけでなく高さの変化でも再評価が必要。
  var tick = false;
  window.addEventListener('resize', function() {
    if (tick) return;
    tick = true;
    requestAnimationFrame(function() { tick = false; applyLayout(); });
  }, { passive: true });

  // 前回 ON のまま終了していれば復元する。既定は OFF。
  function restore() {
    if (lsGet(LS_KEY) !== '1') return;
    show(true);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', restore);
  } else {
    restore();
  }

  window.MdCmdPanel = {
    open: function() { show(false); },
    close: hide,
    toggle: toggle,
    isOpen: function() { return open; },
    // グレーアウトを状態に合わせ直す。専用イベントが無い状態変化の通知口
    // （comment.js のモード切替など）。署名が同じなら何もしないので呼び過ぎても安い。
    sync: sync,
    // 行の集合そのものを組み立て直す（ファイル切替で ⌘R の可否が変わった時など）。
    refresh: function() { if (panel) render(); }
  };
})();
