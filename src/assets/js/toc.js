(function() {
  // トップのフレームでなければ何もしない（理由は html.rs の PAGE_SCRIPTS）。
  if (window.top !== window) return;
  var panel = null;
  var listEl = null;
  var emptyEl = null;
  var scroller = null;
  var anchorMap = [];
  var initialized = false;
  var open = false;
  var scrollTarget = null;
  var hasFile = null;        // init の opts.hasFile
  // ユーザーが ⌘T / × で閉じたか。TOC は既定で出ていて、これが立っている間だけ出さない。
  // 実際に出すかは、これと「いま md の通常表示か」で決まる（sync）。
  var userClosed = false;

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
  // ⌘T/×起点のモード終了で、sync より優先する行き先（'outline' | 'close'）。
  var pendingExit = null;

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

  // 次の 1 回の出し入れだけスライドさせない。2 フレーム後に外し、以降の操作はアニメする。
  function skipAnim() {
    panel.classList.add('no-anim');
    requestAnimationFrame(function() {
      requestAnimationFrame(function() { panel.classList.remove('no-anim'); });
    });
  }

  // 実表示/非表示のみを担う内部関数。userClosed は触らない。
  // body.md-toc-open を付け外しし、CSS 側で右ガターや chrome UI の退避を効かせる。
  // instant=true のときはスライドさせずに一瞬で出し入れする（本文の差し替えに伴うとき）。
  function show(instant) {
    if (!initialized || open) return;
    open = true;
    if (instant) skipAnim();
    panel.classList.remove('hidden');
    document.body.classList.add('md-toc-open');
    build();
  }

  function hide(instant) {
    if (!open) return;
    open = false;
    if (instant) skipAnim();
    panel.classList.add('hidden');
    document.body.classList.remove('md-toc-open');
  }

  // いま本文ペインに出ているのが md の通常表示か。TOC はこのときだけ出す。
  // html（iframe）・ソース表示（md 以外のファイル、⌘R）・diff（⌘D）は、サーバが包みに
  // html-page / source-page を付けて返す（request.rs の render_file と serve_diff）。
  // 設定タブ・何も開いていない状態は開いているファイルが無い。読めなかったときの
  // お知らせ（folder.js の showNotice）は包みに notice-page が付く。
  // 包みはペインの最初の子（MdDocGuard.fill が直下を 1 つに畳む）で、クラスは文書から書けない。
  // Why not 見出しの数で決める: 手で開いた TOC が、見出しの少ない文書や html への
  // 切替で勝手に消えていた（#70）。出すかどうかは中身の種類だけで決める。
  function isMdView() {
    if (!scroller || !(hasFile && hasFile())) return false;
    var page = scroller.firstElementChild;
    if (!page || !page.classList.contains('markdown-body')) return false;
    return !['source-page', 'html-page', 'notice-page'].some(function(c) {
      return page.classList.contains(c);
    });
  }

  // 実際の表示を「閉じられていない、かつ md の通常表示」に合わせる。
  // 呼ぶのは本文が差し替わったときと開閉の操作のとき。窓幅やサイドバーの開閉では呼ばない。
  // コメントタブは md 以外（ソース表示）でも使うので、表示中は触らない。
  // instant … 本文の差し替えに伴う出し入れか。本文の余白（ガター）は一瞬で変わるので、
  // パネルだけ滑らせると、空いた余白に遅れて来たり、全幅になった本文の上を滑って消えたりする。
  // 滑らせるのは ⌘T / × などの操作で出し入れするときだけ。起動して最初に出るときもここを通る。
  function sync(instant) {
    if (!initialized || activeTab === 'comments') return;
    if (!userClosed && isMdView()) {
      if (open) { build(); return; }
      show(instant);
    } else {
      hide(instant);
    }
  }

  // ユーザー操作の入口。開けば userClosed を解除、閉じれば設定する。
  function openPanel() {
    userClosed = false;
    sync(false);
  }

  function closePanel() {
    userClosed = true;
    hide();
  }

  function toggle() {
    // Comment 表示中の ⌘T は「Outline へ切替」（＝モード終了）として扱う。
    if (activeTab === 'comments' && cm) { pendingExit = 'outline'; cm.onExit(); return; }
    // md 以外の画面では何もしない。押しても画面が変わらないまま、md へ戻ったときの
    // 開閉だけが切り替わるのは分かりにくい。
    if (!isMdView()) return;
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
    // opts.hasFile … いまファイルを開いているか（設定タブ・何も開いていない状態で false）。
    init: function(scrollerEl, opts) {
      scroller = scrollerEl;
      hasFile = opts && opts.hasFile;
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
      }
      sync(true);
    },
    // 本文が差し替わった後（ファイル切替・ホットリロード・raw / diff・設定タブ・全部閉じた）。
    refresh: function() { sync(true); },

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
    // モード入り: コメントタブへ切り替え、閉じていれば開く。
    openComments: function() {
      if (!panel || !cm || activeTab === 'comments') return;
      setTab('comments');
      show(false);
    },
    // モード終了: ⌘T/×の明示操作（pendingExit）が最優先。なければ userClosed と
    // 今の表示に合わせる（モード中に ⌘R を切り替えていれば、入る前とは表示が違う）。
    closeComments: function() {
      if (activeTab !== 'comments') return;
      var exit = pendingExit;
      pendingExit = null;
      setTab('outline');
      // md 以外の画面では ⌘T は開閉を変えない（toggle と同じ）。モードを抜けるだけにする。
      if (exit === 'outline') { if (isMdView()) openPanel(); else sync(false); return; }
      if (exit === 'close') { closePanel(); return; }
      sync(false);
    }
  };
})();
