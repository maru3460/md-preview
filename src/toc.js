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
  // 狭→広での再表示はアニメする。folder モードは本文が非同期ロードで
  // 実際に開くのが後になるので、init 後の固定フレームではなく
  // 「実際に開く瞬間」で判定する（単一/フォルダ両モードで一貫）。
  var autoFirstPending = true;

  // 自動表示のしきい値。見出しがこの数以上あり、かつ本文＋TOC が収まる幅が
  // あるときだけ自動で開く。狭い/見出しが少ないときは引っ込めて本文に被せない。
  var MIN_HEADINGS = 3;
  // 本文＋TOC が収まる最小幅。単一ファイルは viewport 幅、folder モードは
  // preview-pane 幅で判定する。デフォルトのウィンドウ幅はこれを上回るよう
  // main.rs 側で設定してあり、初期表示でサイドバーが出る。
  var MIN_WIDTH = 900;

  function buildPanel() {
    panel = document.createElement('div');
    panel.className = 'md-toc-panel hidden';
    panel.innerHTML =
      '<div class="md-toc-header">' +
        '<span class="md-toc-title">Outline</span>' +
        '<button type="button" class="md-toc-close" title="Close" aria-label="Close">×</button>' +
      '</div>' +
      '<div class="md-toc-empty" hidden>No headings</div>' +
      '<ul class="md-toc-list"></ul>';
    document.body.appendChild(panel);
    listEl = panel.querySelector('.md-toc-list');
    emptyEl = panel.querySelector('.md-toc-empty');
    panel.querySelector('.md-toc-close').addEventListener('click', closePanel);
  }

  function build() {
    if (!scroller) return;
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
    if (!open || !anchorMap.length) return;
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
  // ・幅不足/見出し不足/レール使用中 → 開いていれば退避（userClosed は変えない＝条件が戻れば復帰）
  // ・条件を満たす                   → ユーザーが閉じていなければ開く
  //
  // コマンドパネル(⌘/)は同じ右レールを使うので、開いている間は「収まらない」扱いにして
  // 相互排他にする。userClosed を触らずに退避するので、パネルを閉じればここで自然に戻る。
  function autoEvaluate() {
    if (!initialized) return;
    var enough = headingCount() >= MIN_HEADINGS;
    var fits = availWidth() >= MIN_WIDTH;
    var railTaken = !!(window.MdCmdPanel && MdCmdPanel.isOpen());
    if (!enough || !fits || railTaken) {
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

  // 本文＋TOC が収まるか判定するための利用可能幅。実装は MdCommon.availWidth に
  // 一本化してある（コマンドパネルが同じ閾値判定を使うため）。
  function availWidth() {
    return window.MdCommon ? MdCommon.availWidth() : (window.innerWidth || 0);
  }

  // ユーザー操作の入口。手動で開けば userClosed を解除、閉じれば設定する。
  function openPanel() {
    userClosed = false;
    // コマンドパネル(⌘/)と同じ右レールを使うので、ユーザーが明示的に TOC を開いたら
    // そちらに譲ってもらう（相互排他。閉じた側が reevaluate を呼ぶのでここで開く）。
    if (window.MdCmdPanel && MdCmdPanel.isOpen()) MdCmdPanel.close();
    show(false);
  }

  function closePanel() {
    userClosed = true;
    hide();
  }

  function toggle() {
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
        buildPanel();
        initialized = true;
        document.addEventListener('keydown', function(e) {
          if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && (e.key === 't' || e.key === 'T' || e.code === 'KeyT')) {
            e.preventDefault();
            toggle();
          }
        });
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
    // （folder モードのファイルツリー リサイズ等）から呼ぶ。
    reevaluate: autoEvaluate,
    close: closePanel,
    open: openPanel
  };
})();
