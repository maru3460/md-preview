// init.js / folder.js / toc.js から共有するヘルパ群。<head> に inline され、
// window.MdCommon として公開する。各関数は scope（省略時は document）を受け取り、
// 単一ファイル表示（init: document 全体）とフォルダ表示（folder: #preview-pane）の
// 両方を 1 つの実装で賄う。
(function() {
  // 遅延ロードした <script> を URL 単位でメモ化する。mermaid/drawio の巨大ライブラリを
  // 必要になった時に 1 度だけ読み込むためのもの。
  var cache = {};
  function loadLib(url) {
    if (cache[url]) return cache[url];
    cache[url] = new Promise(function(resolve, reject) {
      var s = document.createElement('script');
      s.src = url;
      s.onload = function() { resolve(); };
      s.onerror = function() { reject(); };
      document.head.appendChild(s);
    });
    return cache[url];
  }

  // 見出しテキストをアンカー用スラグへ変換する。記号を落とし空白をハイフン化。
  function slugify(text) {
    return (text || '')
      .toLowerCase()
      .replace(/[^\p{L}\p{N}\s-]/gu, '')
      .trim()
      .replace(/\s+/g, '-');
  }

  // 既存 id があればそれを返し、無ければスラグから一意な id を生成して付与する。
  // 同名見出しは -2 / -3 ... で一意化し、空スラグは 'h' にフォールバックする。
  function ensureHeadingId(h) {
    if (h.id) return h.id;
    var base = slugify(h.textContent);
    if (!base) base = 'h';
    var id = base;
    var n = 2;
    while (document.getElementById(id)) {
      id = base + '-' + (n++);
    }
    h.id = id;
    return id;
  }

  function ensureHeadingIds(scope) {
    var root = scope || document;
    root.querySelectorAll('h1,h2,h3,h4,h5,h6').forEach(function(h) {
      ensureHeadingId(h);
    });
  }

  // 各コードブロックを .code-wrapper で包み Copy ボタンを差し込む。多重付与は防ぐ。
  function addCopyButtons(scope) {
    var root = scope || document;
    root.querySelectorAll('pre > code').forEach(function(code) {
      var pre = code.parentNode;
      if (pre.classList.contains('mermaid')) return;
      var wrapper;
      if (pre.parentNode && pre.parentNode.classList && pre.parentNode.classList.contains('code-wrapper')) {
        wrapper = pre.parentNode;
      } else {
        wrapper = document.createElement('div');
        wrapper.className = 'code-wrapper';
        pre.parentNode.insertBefore(wrapper, pre);
        wrapper.appendChild(pre);
      }
      if (wrapper.querySelector(':scope > .copy-btn')) return;
      var btn = document.createElement('button');
      btn.className = 'copy-btn';
      btn.type = 'button';
      btn.setAttribute('aria-label', 'Copy code');
      btn.textContent = 'Copy';
      btn.addEventListener('click', function() {
        navigator.clipboard.writeText(code.innerText).then(function() {
          btn.textContent = 'Copied';
          btn.classList.add('copied');
          setTimeout(function() {
            btn.textContent = 'Copy';
            btn.classList.remove('copied');
          }, 1200);
        }).catch(function() {
          btn.textContent = 'Failed';
          setTimeout(function() { btn.textContent = 'Copy'; }, 1200);
        });
      });
      wrapper.appendChild(btn);
    });
  }

  // .source-view（.md 以外のソース表示）に行番号ガターを付ける。多重付与は防ぐ。
  // ガターは .source-main 内で <pre> の兄弟として置くので hljs のハイライトには触れない。
  function addLineNumbers(scope) {
    var root = scope || document;
    root.querySelectorAll('.source-main').forEach(function(main) {
      if (main.querySelector(':scope > .source-gutter')) return;
      var code = main.querySelector('code');
      if (!code) return;
      // 末尾の改行は「余分な空行」なので数えない（textContent は hljs 適用後も不変）。
      var text = code.textContent.replace(/\n$/, '');
      var count = text.split('\n').length;
      var nums = new Array(count);
      for (var i = 0; i < count; i++) nums[i] = i + 1;
      var gutter = document.createElement('div');
      gutter.className = 'source-gutter';
      gutter.setAttribute('aria-hidden', 'true');
      gutter.textContent = nums.join('\n');
      main.insertBefore(gutter, main.firstChild);
    });
  }

  // scope 内に mermaid 図があれば lib を遅延ロードして描画する。
  function runMermaid(scope) {
    var root = scope || document;
    var nodes = root.querySelectorAll('pre.mermaid');
    if (!nodes.length) return;
    loadLib('mdpreview://localhost/__lib/mermaid.min.js').then(function() {
      if (typeof mermaid === 'undefined') return;
      if (!mermaid.__mdInit) {
        var ap = window.MD_APPEARANCE || 'auto';
        var dark = ap === 'dark' || (ap === 'auto' && window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches);
        mermaid.initialize({ startOnLoad: false, theme: dark ? 'dark' : 'neutral', securityLevel: 'loose' });
        mermaid.__mdInit = true;
      }
      try { mermaid.run({ nodes: nodes }); } catch (e) {}
    }).catch(function() {});
  }

  // scope 内に draw.io 図があれば lib を遅延ロードして描画する。
  // GraphViewer.processElements() はページ全体を対象にするため scope は存在チェックのみに使う。
  function runDrawio(scope) {
    var root = scope || document;
    var nodes = root.querySelectorAll('.mxgraph');
    if (!nodes.length) return;
    loadLib('mdpreview://localhost/__lib/drawio-viewer.min.js').then(function() {
      if (typeof GraphViewer === 'undefined') return;
      try { GraphViewer.processElements(); } catch (e) {}
    }).catch(function() {});
  }

  // Cmd/Ctrl+A で本文（.markdown-body）だけを全選択する。ページ全体（サイドバーや
  // フロントマター外の UI）を巻き込まないようにするためのもの。
  // 入力欄や編集可能要素にフォーカスがある時は通常の全選択に任せる。
  function selectBody(e) {
    if (!(e.metaKey || e.ctrlKey) || e.key !== 'a') return;
    var t = e.target;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    var body = document.querySelector('.markdown-body');
    if (!body) return;
    e.preventDefault();
    var sel = window.getSelection();
    if (!sel) return;
    var range = document.createRange();
    range.selectNodeContents(body);
    sel.removeAllRanges();
    sel.addRange(range);
  }
  document.addEventListener('keydown', selectBody);

  // 横スクロールする表(.table-wrap)の上でホイールを回すと、横にまだ動かせる間は
  // ブラウザがホイールを横スクロールに吸ってしまい、ページの縦スクロールが止まる。
  // 縦方向主体のホイールは表に吸わせず、実際のスクロール親を自前で縦に動かして回避する。
  // 横方向主体(trackpad 横フリック / Shift+ホイール)は既定の横スクロールに任せる。
  function tableWheel(e) {
    var wrap = e.target && e.target.closest && e.target.closest('.table-wrap');
    if (!wrap) return;
    // 横あふれが無い表は問題が起きないので一切介入しない(ネイティブの慣性を保つ)。
    if (wrap.scrollWidth <= wrap.clientWidth) return;
    if (Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
    // スクロール主体は単一ファイル表示では window、folder モードでは #preview-pane。
    var sc = wrap.closest('#preview-pane') || document.scrollingElement || document.documentElement;
    var unit = e.deltaMode === 1 ? 16 : (e.deltaMode === 2 ? sc.clientHeight : 1);
    sc.scrollTop += e.deltaY * unit;
    e.preventDefault();
  }
  document.addEventListener('wheel', tableWheel, { passive: false });

  // iframe（.html-frame）で描画した html は、フォーカスが iframe 内にあると親 document
  // のショートカット（Cmd+W/R/F 等）が届かなくなる。iframe は同一オリジン（sandbox 無し）
  // なので、内部の keydown を親へ転送して既存のグローバルハンドラを効かせる。
  // 相対リンクは opts.onLinkClick に委ね、true が返れば iframe 内遷移を止める
  // （フォルダモードは親のプレビュー遷移に回してサイドバー等の状態を同期させる）。
  // "rgb(r, g, b)" の明度がしきい値超え（＝明るい色）か。パースできなければ false。
  function isLightColor(c) {
    var m = /rgba?\((\d+),\s*(\d+),\s*(\d+)/.exec(c || '');
    if (!m) return false;
    var lum = (0.299 * +m[1] + 0.587 * +m[2] + 0.114 * +m[3]) / 255;
    return lum > 0.5;
  }

  // iframe 要素の背景を、中身ドキュメントの実効キャンバス色に合わせる。
  // WebKit はサブフレームのスクロールバー・ガター（描画されない領域）を子のキャンバス色で
  // 塗らず、そこに iframe 要素の背景が透ける。固定の白だとダークページで「右端の白帯」に
  // なるため、中身の色へ追従させて帯を消す。中身は読むだけで書き換えない（忠実性を保つ）。
  function syncFrameBackground(frame) {
    // レイアウト・スタイル確定後に読むため 1 フレーム待つ。
    requestAnimationFrame(function() {
      try {
        var doc = frame.contentDocument;
        if (doc && doc.documentElement) {
          var isClear = function(c) { return !c || c === 'transparent' || c === 'rgba(0, 0, 0, 0)'; };
          // body → html の順で最初の非透明な背景色を採る（キャンバス背景の伝播順に合わせる）。
          var bg = doc.body ? getComputedStyle(doc.body).backgroundColor : '';
          if (isClear(bg)) bg = getComputedStyle(doc.documentElement).backgroundColor;
          if (isClear(bg)) {
            // 背景無指定（未装飾ページ）。中身の文字色の明暗から読みやすい地色を選ぶ
            // ＝ 明るい文字ならダーク地、暗い文字なら白地。color-scheme:dark 宣言ページで
            // 白帯が復活するのも、これで一緒に防げる。
            var probe = doc.body || doc.documentElement;
            bg = isLightColor(getComputedStyle(probe).color) ? '#1c1c1e' : '#ffffff';
          }
          frame.style.background = bg;
        }
      } catch (e) { /* cross-origin 等: 背景は CSS のフォールバックのまま */ }
      // 背景を確定させてから表示する（読み込み中の地色フラッシュを隠すため opacity:0→1）。
      frame.style.opacity = '1';
    });
  }

  function bindFrame(frame, opts) {
    var doc;
    try { doc = frame.contentDocument; } catch (e) { return; } // 外部遷移などで cross-origin
    if (!doc) return;
    // 同一文書への二重バインドを防ぐ（load と即時バインドの両方が走る窓があるため）。
    // 文書が差し替われば(iframe内遷移)新しい doc にはフラグが無いので再バインドされる。
    if (doc.__mdBound) return;
    doc.__mdBound = true;

    syncFrameBackground(frame);

    doc.addEventListener('keydown', function(e) {
      // アプリ自身の ⌘/Ctrl ショートカット(r=raw / d=diff / t=toc / w=close)だけ親へ委譲する。
      // ⌘A(全選択)・⌘F(検索)・⌘C 等は iframe のネイティブ動作に任せる。特に検索は親 DOM しか
      // 走査せず iframe 内テキストに当たらないため、転送すると 0/0 の空振りバーが出てしまう。
      var k = (e.key || '').toLowerCase();
      var isCmd = (e.metaKey || e.ctrlKey) && (k === 'r' || k === 'd' || k === 't' || k === 'w');
      // ファイル移動([/])・ペインフォーカス(Tab)はアプリ全体のナビなので、iframe にフォーカスが
      // ある時でも親へ委譲する（html 表示中でも移動が死なない）。ただし:
      //  ・iframe 内の入力欄では素キーを奪わない（文字入力・フォーム内 Tab 移動を保つ）
      //  ・検索(/)は親 DOM しか走査せず iframe 内テキストに当たらない(⌘F と同理由)ので転送しない
      //  ・縦スクロール素キー(j/k 等)は iframe 内のネイティブ・スクロールに任せ、転送しない
      var inField = isFieldEl(e.target);
      var isNav = !e.metaKey && !e.ctrlKey && !e.altKey && !inField
        && (k === '[' || k === ']' || e.key === 'Tab');
      if (!isCmd && !isNav) return;
      var ev = new KeyboardEvent('keydown', {
        key: e.key, code: e.code,
        metaKey: e.metaKey, ctrlKey: e.ctrlKey, shiftKey: e.shiftKey, altKey: e.altKey,
        bubbles: true, cancelable: true
      });
      document.dispatchEvent(ev);
      if (ev.defaultPrevented) e.preventDefault();
    });

    // 右クリック: iframe 内では WebKit 標準メニュー（"Open Frame in New Window" 等、
    // このアプリでは機能しない項目）が出てしまう。抑止して、親のカスタムメニューを
    // iframe の位置ぶんずらした座標で開く（キー転送と同じ「親へ委譲」方針）。
    doc.addEventListener('contextmenu', function(e) {
      e.preventDefault();
      var rect = frame.getBoundingClientRect();
      document.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true, cancelable: true,
        clientX: rect.left + e.clientX,
        clientY: rect.top + e.clientY
      }));
    });

    if (opts && typeof opts.onLinkClick === 'function') {
      doc.addEventListener('click', function(e) {
        var a = e.target && e.target.closest && e.target.closest('a[href]');
        if (!a) return;
        var href = a.getAttribute('href');
        if (!href) return;
        if (opts.onLinkClick(href)) e.preventDefault();
      });
    }
  }

  // scope 内の .html-frame を配線する。onload ごとに再バインドし、iframe 内遷移後も
  // ショートカット転送を効かせ続ける。要素単位でメモ化して二重登録を防ぐ。
  function wireHtmlFrames(scope, opts) {
    var root = scope || document;
    var frames = root.querySelectorAll('iframe.html-frame');
    frames.forEach(function(frame) {
      if (frame.__mdWired) return;
      frame.__mdWired = true;
      frame.addEventListener('load', function() { bindFrame(frame, opts); });
      // 保険: load が来ない/バインドに失敗しても、一定時間で必ず表示する
      // （opacity:0 のまま永久に空白、を防ぐ）。表示済みなら実質 no-op。
      setTimeout(function() { frame.style.opacity = '1'; }, 4000);
      // 既にロード済み（キャッシュ等）なら即バインドする。ただし挿入直後の iframe は
      // まだ初期 about:blank（readyState='complete'）のことがある。これを即バインドすると
      // 本物の文書が来る前に opacity を開け（＝ちらつき対策が無効化）、about:blank の背景を
      // 誤読して白箱を焼き付けてしまう。about:blank は除外し、本物の load を待つ。
      try {
        var d = frame.contentDocument;
        if (d && d.readyState !== 'loading' && d.URL !== 'about:blank') {
          bindFrame(frame, opts);
        }
      } catch (e) {}
    });
  }

  // ── キーボードナビ用の共有述語（keyscroll.js と folder.js が同じ判定を使う） ──
  // 別実装にするとフォーカス判定がズレて二重発火/無反応が出るため、必ずここに一本化する。

  // スクロール主体。folder モードは #preview-pane、単一ファイル/stdin は document。
  function getScroller() {
    return document.querySelector('#preview-pane')
      || document.scrollingElement
      || document.documentElement;
  }

  // ファイルツリー（サイドバー）にフォーカスがあるか。folder モードのツリー操作の起点判定。
  function isSidebarFocused() {
    var s = document.getElementById('sidebar');
    return !!(s && s.contains(document.activeElement));
  }

  // 検索バー / ヘルプ / 右クリックメニューのいずれかが開いているか。開いている間は素キーを止める。
  // この判定は毎 keydown（巨大ファイルの j/k 連打を含む）で走るため、getElementById による O(1)
  // 参照だけで済ませる。querySelector('.md-search-bar:not(.hidden)') は document 全走査になり、
  // オーバーレイが閉じている通常時ほど重くなるので使わない（各オーバーレイに安定 id を振ってある）。
  function isOverlayOpen() {
    if (document.getElementById('md-help-backdrop')) return true;   // 開いている間だけ存在
    if (document.getElementById('md-context-menu')) return true;    // 開いている間だけ存在
    var sb = document.getElementById('md-search-bar');              // 常在。hidden で開閉を表す
    return !!(sb && !sb.classList.contains('hidden'));
  }

  // フォーカスが文字入力欄（素キーを横取りしてはいけない要素）に乗っているか。
  // iframe 転送・スクロール素キー・フォーカス移譲の各所で同じ判定を使うため一本化する。
  function isFieldEl(el) {
    return !!(el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable));
  }

  // フォーカスが入力・操作系（Space/Enter で反応する要素）に乗っているか。素キーはそちらへ委ねる。
  function isInteractiveFocus() {
    var el = document.activeElement;
    if (!el) return false;
    if (isFieldEl(el)) return true;
    if (el.tagName === 'BUTTON') return true;
    if (el.tagName === 'A' && el.hasAttribute('href')) return true;
    return false;
  }

  window.MdCommon = {
    loadLib: loadLib,
    slugify: slugify,
    ensureHeadingId: ensureHeadingId,
    ensureHeadingIds: ensureHeadingIds,
    addCopyButtons: addCopyButtons,
    addLineNumbers: addLineNumbers,
    runMermaid: runMermaid,
    runDrawio: runDrawio,
    wireHtmlFrames: wireHtmlFrames,
    getScroller: getScroller,
    isSidebarFocused: isSidebarFocused,
    isOverlayOpen: isOverlayOpen,
    isInteractiveFocus: isInteractiveFocus,
    isFieldEl: isFieldEl
  };
})();
