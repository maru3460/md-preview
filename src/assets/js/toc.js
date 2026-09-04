(function() {
  var panel = null;
  var listEl = null;
  var emptyEl = null;
  var scroller = null;
  var anchorMap = [];
  var initialized = false;
  var open = false;
  var scrollTarget = null;
  // ユーザーが明示的に閉じたか。true の間は自動表示で開き直さない
  // （幅による自動退避／復帰とは区別する）。
  var userClosed = false;
  // 初回の自動表示が未実施か。デフォルトで開くときの「ぴょん」を防ぐため、
  // この 1 回だけスライドイン（transition）を抑止する。手動 ⌘T や
  // 狭→広での再表示はアニメする。本文が非同期ロードで
  // 実際に開くのが後になるので、init 後の固定フレームではなく
  // 「実際に開く瞬間」で判定する（単一/フォルダ両モードで一貫）。
  var autoFirstPending = true;

  // ── タブ ──────────────────────────────────────────────────────
  // サイドバーの中身は常に 1 つ。普段は Outline、コメントモード中は Comment に
  // まるごと置き換わり、ヘッダのタイトルも差し替わる（タブの並列は見せない——
  // Outline=ただの表示、Comment=モード、という非対称を対等な切替に見せないため）。
  // Comment の中身は comment.js が mountComments で差し込む（このファイルは
  // 枠・開閉・切替だけを持つ）。モードの入口はピル/c/バッジ側、出口は ×/⌘T/Esc/c。
  // ×/⌘T のクリックはここで直接切り替えず onExit で comment.js の setMode に
  // 要求し、setMode が openComments/closeComments を呼び返す（状態遷移の一元化）。
  var activeTab = 'outline';
  var cm = null;             // mountComments で渡される { contentEl, onExit }
  var outlineLabel = null;
  var cmLabel = null;
  // モードに入る前のサイドバー状態（'outline' | 'closed'）。抜けたとき復元する。
  var cmPrev = null;
  // ⌘T/×起点のモード終了で、復元より優先する行き先（'outline' | 'close'）。
  var pendingExit = null;

  // 自動表示のしきい値。見出しがこの数以上あり、かつ本文＋TOC が収まる幅が
  // あるときだけ自動で開く。狭い/見出しが少ないときは引っ込めて本文に被せない。
  var MIN_HEADINGS = 3;
  // 本文＋TOC が収まる最小幅（availWidth() と比べる）。既定のウィンドウ幅は
  // これを上回るよう app_config.rs 側で設定してあり、初期表示でサイドバーが出る。
  var MIN_WIDTH = 900;

  function buildPanel() {
    panel = document.createElement('div');
    panel.className = 'md-toc-panel hidden';
    panel.innerHTML =
      '<div class="md-toc-header">' +
        '<span class="md-toc-title" data-tab="outline">Outline</span>' +
        '<span class="md-toc-title" data-tab="comments" hidden>Comment</span>' +
        '<button type="button" class="md-toc-close" title="Close" aria-label="Close">×</button>' +
      '</div>' +
      '<div class="md-toc-empty" hidden>No headings</div>' +
      '<ul class="md-toc-list"></ul>';
    document.body.appendChild(panel);
    listEl = panel.querySelector('.md-toc-list');
    emptyEl = panel.querySelector('.md-toc-empty');
    outlineLabel = panel.querySelector('[data-tab="outline"]');
    cmLabel = panel.querySelector('[data-tab="comments"]');
    panel.querySelector('.md-toc-close').addEventListener('click', function() {
      // Comment 表示中の × はモード終了＋サイドバーごと閉じる（明示操作）。
      if (activeTab === 'comments' && cm) { pendingExit = 'close'; cm.onExit(); return; }
      closePanel();
    });
  }

  // 中身の実切替。タイトルごと置き換える。Outline の中身は隠れている間に古びるので、
  // 戻るとき再構築する。
  function setTab(tab) {
    activeTab = tab;
    var isC = tab === 'comments';
    outlineLabel.hidden = isC;
    cmLabel.hidden = !isC;
    listEl.style.display = isC ? 'none' : '';
    emptyEl.style.display = isC ? 'none' : '';
    if (cm) cm.contentEl.style.display = isC ? '' : 'none';
    if (!isC) build();
  }

  function build() {
    if (!scroller || activeTab !== 'outline') return;
    var headings = Array.prototype.slice.call(
      scroller.querySelectorAll('h1,h2,h3,h4,h5,h6')
    );
    listEl.innerHTML = '';
    anchorMap = [];
    if (!headings.length) {
      emptyEl.hidden = false;
      listEl.hidden = true;
      return;
    }
    emptyEl.hidden = true;
    listEl.hidden = false;
    headings.forEach(function(h) {
      var id = MdCommon.ensureHeadingId(h);
      var li = document.createElement('li');
      li.className = 'md-toc-item lvl-' + h.tagName.charAt(1);
      var a = document.createElement('a');
      a.href = '#' + id;
      a.textContent = h.textContent;
      a.addEventListener('click', function(e) {
        e.preventDefault();
        h.scrollIntoView({ behavior: 'smooth', block: 'start' });
      });
      li.appendChild(a);
      listEl.appendChild(li);
      anchorMap.push({ heading: h, link: a });
    });
    updateActive();
  }

  function updateActive() {
    if (!open || activeTab !== 'outline' || !anchorMap.length) return;
    var threshold = 80;
    var active = null;
    for (var i = 0; i < anchorMap.length; i++) {
      var top = anchorMap[i].heading.getBoundingClientRect().top;
      if (top <= threshold) active = anchorMap[i];
      else break;
    }
    if (!active) active = anchorMap[0];
    anchorMap.forEach(function(it) { it.link.classList.remove('active'); });
    active.link.classList.add('active');
    var aRect = active.link.getBoundingClientRect();
    var lRect = listEl.getBoundingClientRect();
    if (aRect.top < lRect.top || aRect.bottom > lRect.bottom) {
      active.link.scrollIntoView({ block: 'nearest' });
    }
  }

  // 実表示/非表示のみを担う内部関数。userClosed は触らない。
  // body.md-toc-open を付け外しし、CSS 側で右ガターや chrome UI の退避を効かせる。
  // instant=true のときはスライドインさせずに即表示する（初回の自動表示用）。
  function show(instant) {
    if (!initialized || open) return;
    open = true;
    if (instant) {
      panel.classList.add('no-anim');
      requestAnimationFrame(function() {
        requestAnimationFrame(function() { panel.classList.remove('no-anim'); });
      });
    }
    panel.classList.remove('hidden');
    document.body.classList.add('md-toc-open');
    build();
  }

  function hide() {
    if (!open) return;
    open = false;
    panel.classList.add('hidden');
    document.body.classList.remove('md-toc-open');
  }

  // 現在ページで自動表示すべきか判定して開閉する。
  // ・幅不足/見出し不足 → 開いていれば退避（userClosed は変えない＝広くなれば戻る）
  // ・条件を満たす      → ユーザーが閉じていなければ開く
  // コメントタブは手動で出すものなので、表示中は自動退避も自動切替もしない。
  function autoEvaluate() {
    if (!initialized || activeTab === 'comments') return;
    var enough = headingCount() >= MIN_HEADINGS;
    var fits = availWidth() >= MIN_WIDTH;
    if (!enough || !fits) {
      hide();
    } else if (!open && !userClosed) {
      show(autoFirstPending);
      autoFirstPending = false;
    }
  }

  function headingCount() {
    if (!scroller) return 0;
    return scroller.querySelectorAll('h1,h2,h3,h4,h5,h6').length;
  }

  // 本文＋TOC が収まるか判定するための利用可能幅。
  // #preview-pane があればそれを載せている列(#main-col)の幅、無ければ viewport 幅を見る。
  // ペイン自身の幅は見ない——パネルのガターはペインの margin で確保しているので、
  // 開いている間だけペインが 300px あまり細くなる。それを判定に使うと「開いているか」が
  // 判定の入力になり、本文を差し替えるたび（ファイル切替・ホットリロード）に
  // 開閉が交互に反転する。列の幅はサイドバーと窓幅だけで決まり、TOC の開閉では動かない。
  function availWidth() {
    if (scrollTarget === window || !scroller) {
      return document.documentElement.clientWidth || window.innerWidth || 0;
    }
    return (scroller.parentElement || scroller).clientWidth || 0;
  }

  // ユーザー操作の入口。手動で開けば userClosed を解除、閉じれば設定する。
  function openPanel() {
    userClosed = false;
    show(false);
  }

  function closePanel() {
    userClosed = true;
    hide();
  }

  function toggle() {
    // Comment 表示中の ⌘T は「Outline へ切替」（＝モード終了）として扱う。
    if (activeTab === 'comments' && cm) { pendingExit = 'outline'; cm.onExit(); return; }
    if (open) closePanel(); else openPanel();
  }

  function attachScrollListener() {
    if (!scrollTarget) return;
    var ticking = false;
    var handler = function() {
      if (ticking) return;
      ticking = true;
      requestAnimationFrame(function() {
        ticking = false;
        updateActive();
      });
    };
    scrollTarget.addEventListener('scroll', handler, { passive: true });
  }

  window.MdToc = {
    init: function(scrollerEl) {
      scroller = scrollerEl;
      scrollTarget = (scrollerEl === document.scrollingElement
        || scrollerEl === document.documentElement
        || scrollerEl === document.body)
        ? window
        : scrollerEl;
      if (!initialized) {
        if (!panel) buildPanel();   // mountComments が先に走った場合は構築済み
        initialized = true;
        if (window.MdKeymap) MdKeymap.on('toc-toggle', toggle);
        attachScrollListener();
        // 画面幅が変わったら自動表示条件を再評価（狭くなれば退避、広がれば復帰）。
        var rTick = false;
        window.addEventListener('resize', function() {
          if (rTick) return;
          rTick = true;
          requestAnimationFrame(function() { rTick = false; autoEvaluate(); });
        }, { passive: true });
      }
      // 初期表示の判定。見出しが十分で画面が広ければ自動で開く。
      // （初回自動表示のスライドイン抑止は show() 側で行う。）
      autoEvaluate();
    },
    refresh: function() {
      // 内容が差し替わった後の再構築＋再評価。diff の ON/OFF で見出し数が
      // 0⇔N と変わるので、autoEvaluate で自動的に退避/復帰する。
      var wasOpen = open;
      autoEvaluate();
      if (wasOpen && open) build();
    },
    // 幅ベースの自動表示を再判定する。window の resize 以外に幅が変わる経路
    // （ファイルツリーのリサイズ等）から呼ぶ。
    reevaluate: autoEvaluate,
    close: closePanel,
    open: openPanel,

    // ── コメントタブ連携（comment.js から呼ばれる） ──────────────
    // o: { contentEl, onExit() }。contentEl はパネル内に常駐させ、表示/非表示だけ
    // こちらで切り替える。onExit は Outline タブ/×/⌘T からのモード終了要求。
    mountComments: function(o) {
      cm = o;
      if (!panel) buildPanel();
      cm.contentEl.style.display = 'none';
      panel.appendChild(cm.contentEl);
    },
    setCommentsCount: function(n) {
      if (cmLabel) cmLabel.textContent = n > 0 ? 'Comment (' + n + ')' : 'Comment';
    },
    // モード入り: 現在のサイドバー状態を控えてコメントタブへ切り替え、閉じていれば開く。
    openComments: function() {
      if (!panel || !cm || activeTab === 'comments') return;
      cmPrev = open ? 'outline' : 'closed';
      setTab('comments');
      show(false);
    },
    // モード終了: タブ/×クリックの明示操作（pendingExit）が最優先、なければ入る前へ復元。
    closeComments: function() {
      if (activeTab !== 'comments') return;
      var exit = pendingExit;
      pendingExit = null;
      var prev = cmPrev;
      cmPrev = null;
      setTab('outline');
      if (exit === 'outline') { openPanel(); return; }
      if (exit === 'close') { closePanel(); return; }
      // 復元。userClosed は触らない＝以前の自動表示条件をそのまま引き継ぐ。
      if (prev !== 'outline') hide();
    }
  };
})();
