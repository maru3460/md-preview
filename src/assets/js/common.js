// folder.js / toc.js などから共有するヘルパ群。<head> に inline され、
// window.MdCommon として公開する。各関数は scope（省略時は document）を受け取り、
// シェル（#preview-pane を持つアプリのページ）と、build_html で出す 1 枚もの
// （md の直開き・`--html` ダンプ）の両方を 1 つの実装で賄う。
(function() {
  // 遅延ロードした <script> を URL 単位でメモ化する。mermaid/drawio の巨大ライブラリを
  // 必要になった時に 1 度だけ読み込むためのもの。
  // URL はスキームを書かずオリジン相対にする（examples/serve.rs 越しに http で
  // 開いた時もそのまま動く）。
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
        // コメントのバッジ/埋め込みカードは <code> の中に入ることがある（ソースビューの
        // 行構造）ので、複製から取り除いてから読む。行包みは textContent を不変に保つ
        // ため、コード本文はこれで元ソースのまま取れる。
        var clone = code.cloneNode(true);
        clone.querySelectorAll('.md-cmt-badge, .md-cmt-embed, .md-cmt-badge-holder').forEach(function(n) { n.remove(); });
        copyText(clone.textContent, function() {
          btn.textContent = 'Copied';
          btn.classList.add('copied');
          setTimeout(function() {
            btn.textContent = 'Copy';
            btn.classList.remove('copied');
          }, 1200);
        }, function() {
          btn.textContent = 'Failed';
          setTimeout(function() { btn.textContent = 'Copy'; }, 1200);
        });
      });
      wrapper.appendChild(btn);
    });
  }

  // .source-view（.md 以外のソース表示）に行番号ガターを付ける。多重付与は防ぐ。
  // ガターは .source-main 内で <pre> の兄弟として置くので hljs のハイライトには触れない。
  // 行包み済み（wrapSourceLines）のソースは各行の ::before が番号を出すので付けない——
  // 行間にコメントカードが挟まると、別カラムのガターでは番号が行とずれるため。
  function addLineNumbers(scope) {
    var root = scope || document;
    root.querySelectorAll('.source-main').forEach(function(main) {
      if (main.querySelector(':scope > .source-gutter')) return;
      var code = main.querySelector('code');
      if (!code || (code.dataset && code.dataset.srcWrapped)) return;
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

  // ── ソースビューの 1 行 1 要素化 ──────────────────────────────
  // hljs は複数行にまたがる <span>（文字列・ブロックコメント）を出すので、出力を単純に
  // \n で切ると開き/閉じタグの対応が壊れる。行末で開いているタグを全部閉じ、次の行頭で
  // 同じタグを開き直す。hljs の出力は <span class="…"> / </span> / エスケープ済み
  // テキストだけの安全なサブセットなので、正規表現トークナイザ + スタックで足りる。
  function splitHighlightedLines(html) {
    var lines = [];
    var stack = [];   // 開いている <span …> の開きタグ文字列
    var buf = '';
    var re = /(<span[^>]*>)|(<\/span>)|(\n)|([^<\n]+)|(<)/g;
    var m;
    while ((m = re.exec(html)) !== null) {
      if (m[1]) { stack.push(m[1]); buf += m[1]; }
      else if (m[2]) { stack.pop(); buf += m[2]; }
      else if (m[3]) {                              // 改行 = 行境界
        buf += '</span>'.repeat(stack.length);      // 行末で全部閉じる
        lines.push(buf);
        buf = stack.join('');                       // 次の行頭で開き直す
      }
      else buf += m[0];                             // テキスト（または裸の '<'）
    }
    buf += '</span>'.repeat(stack.length);
    lines.push(buf);
    return lines;
  }

  // ソースビュー（raw トグル・非 md ファイル）の <code> を「1 行 = 1 <div>」に包み直す。
  // 行コメントの埋め込み（カードを行の直後に兄弟挿入する）を成立させるための構造で、
  // comment.js の行ユニット（[data-src-line]）がそのまま乗る。
  //
  // - 各行の末尾に実際の改行をテキストとして残す。「<code> の textContent は元ソースの
  //   まま」という既存の前提（引用・選択コピー・検索・ガターの行数え）を保つため。
  //   空行も改行 + 行番号の ::before で 1 行分の高さを持つ。
  // - 行数が閾値を超えるソースは包まない（従来の 1 本 pre + ガターのまま。行コメント
  //   不可）。閾値はサーバ（request.rs の HIGHLIGHT_MAX_LINES = 10,000）と同じ規模。
  //   バイト側の閾値はここには持ち込まない——1MB 超でも行数が少ないファイル（ログ等）は
  //   色なしで包めば行コメントできる。
  var SRC_WRAP_MAX_LINES = 10000;

  function wrapSourceLines(scope) {
    var root = scope || document;
    root.querySelectorAll('.source-main').forEach(function(main) {
      var code = main.querySelector('pre code');
      if (!code || (code.dataset && code.dataset.srcWrapped)) return;
      var text = code.textContent;
      var hadTrailingNL = /\n$/.test(text);
      // 行数の先読み。閾値超過なら innerHTML の分割ごとスキップする。
      var count = 0;
      for (var p = -1; (p = text.indexOf('\n', p + 1)) !== -1;) count++;
      if (count + (hadTrailingNL ? 0 : 1) > SRC_WRAP_MAX_LINES) return;
      var parts = splitHighlightedLines(code.innerHTML);
      // 末尾改行の後ろの空要素は行ではない（ガターと同じ数え方に揃える）。
      if (hadTrailingNL) parts.pop();
      var out = [];
      for (var i = 0; i < parts.length; i++) {
        // 最終行の改行は元ソースにあった時だけ付ける（textContent 不変を守る）。
        var nl = (i < parts.length - 1 || hadTrailingNL) ? '\n' : '';
        out.push('<div class="md-src-row" data-src-line="' + (i + 1) + '">' + parts[i] + nl + '</div>');
      }
      code.innerHTML = out.join('');
      code.dataset.srcWrapped = '1';
      // 行番号（::before）の桁数と、横スクロール時に番号の下へ敷く地色を CSS 変数で渡す。
      code.style.setProperty('--md-gutter-ch', String(String(parts.length).length));
      // 最長行の幅も配る。行の塗り・帯を全行この幅（min-width）に揃えるため——
      // CSS の 100% は可視幅（code = スクローラ）にしかならず、横スクロールすると
      // 短い行の塗りが途中で切れる。コンテンツ幅はフォント固定なのでリサイズ不変。
      code.style.setProperty('--md-src-content-w', code.scrollWidth + 'px');
      for (var n = main; n; n = n.parentElement) {
        var c = getComputedStyle(n).backgroundColor;
        if (c && c !== 'transparent' && c !== 'rgba(0, 0, 0, 0)') {
          code.style.setProperty('--md-src-gutter-bg', c);
          break;
        }
      }
    });
  }

  // scope 内のコードブロックを構文ハイライトする。hljs.highlightAll() は毎回
  // document 全体を舐めるので、差し替えた範囲だけに絞る。二重適用は hljs が付ける
  // data-highlighted で防ぐ（再適用は警告が出るうえ無駄）。
  function highlightIn(scope) {
    if (!window.hljs) return;
    var root = scope || document;
    root.querySelectorAll('pre code').forEach(function(code) {
      if (code.dataset && code.dataset.highlighted) return;
      try { hljs.highlightElement(code); } catch (e) {}
    });
  }

  // 本文（またはプレビュー枠）を差し替えた直後の後処理をまとめて行う。
  //
  // 差し替えの経路は 初回描画 / ホットリロード / ファイル切替 / raw / diff の 5 つ
  // あり、以前はそれぞれが同じ並びを書き写していた（しかも微妙に食い違っていた）。
  // 後処理を 1 つ足すときに 5 箇所を思い出す必要があったので、ここに集約する。
  //
  // 中身の種類（md / ソース / 差分）で分岐しないのは、各処理が「対象が無ければ
  // 何もしない」ように書けているため。差分に mermaid は無いし、ソースビューに見出しは
  // 無い。分岐を増やすより、無いものを黙って飛ばす方が経路ごとの差を生まない。
  //
  // opts.onLinkClick … iframe 内の相対リンクを親のプレビュー遷移へ回す（folder のみ）
  // 本文（プレビュー枠）の差し替え回数。ファイル切替・raw/diff の切替・ホットリロードの
  // どれで入れ替わっても増える。「差し替えがもう起きたか」を待ちたい側（コメントのジャンプ）が
  // 見る——currentFile() は差し替えの前に切り替わるので、名前だけでは判断できない。
  var bodySwaps = 0;

  function hydrate(scope, opts) {
    bodySwaps++;
    var o = opts || {};
    var root = scope || document;
    ensureHeadingIds(root);
    highlightIn(root);
    wrapSourceLines(root);
    addCopyButtons(root);
    addLineNumbers(root);
    runMermaid(root);
    runDrawio(root);
    wireHtmlFrames(root, o);
    // 差し替えで消えた/変わったものを、それぞれの持ち主に作り直させる。
    if (window.MdSearch) { MdSearch.reset(); MdSearch.init(root); }
    if (window.MdToc) MdToc.refresh();
    // コメントの真実は JS 配列側にあるので、マーカーを貼り直す。
    if (window.MdComment) MdComment.reanchor();
  }

  // scope 内に mermaid 図があれば lib を遅延ロードして描画する。
  function runMermaid(scope) {
    var root = scope || document;
    var nodes = root.querySelectorAll('pre.mermaid');
    if (!nodes.length) return;
    loadLib('/__lib/mermaid.min.js').then(function() {
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
    loadLib('/__lib/drawio-viewer.min.js').then(function() {
      if (typeof GraphViewer === 'undefined') return;
      try { GraphViewer.processElements(); } catch (e) {}
    }).catch(function() {});
  }

  // ⌘A で本文（.markdown-body）だけを全選択する。ページ全体（サイドバーや
  // フロントマター外の UI）を巻き込まないようにするためのもの。
  // キーの割り当てと「入力欄では譲る」判定は keymap.js 側が持つ。
  function selectBody() {
    var body = document.querySelector('.markdown-body');
    if (!body) return;
    var sel = window.getSelection();
    if (!sel) return;
    var range = document.createRange();
    range.selectNodeContents(body);
    sel.removeAllRanges();
    sel.addRange(range);
  }

  // ウィンドウを閉じる（⌘W）。Rust 側が IPC を受けて CloseRequested に流す。
  function closeWindow() {
    if (window.ipc) window.ipc.postMessage('close');
  }

  // `#id` 形式のリンクをページ内スクロールに変える。該当する形なら true を返し、
  // イベントを渡していれば preventDefault する。
  function scrollToAnchor(href, e) {
    if (!href || href.charAt(0) !== '#') return false;
    if (e) e.preventDefault();
    var target = document.getElementById(decodeURIComponent(href.slice(1)));
    if (target) target.scrollIntoView({ behavior: 'smooth' });
    return true;
  }

  // ── パスと URL ────────────────────────────────────────────────
  // 「識別子」は ?file= / ?raw= / タブが持つ文字列。root 配下なら root 相対パス、
  // root の外なら絶対パス（先頭 /）。Rust 側 urlpath.rs と同じ規則で、
  // 本文中の href / src はサーバが既に URL へ畳んである（相対パスの基準は root では
  // なく開いているファイルの場所）。ここはその逆変換と、サーバを通らない経路
  // （iframe 内の相対リンク）のための解決を持つ。

  var ABS_PREFIX = '/__abs/';

  // 絶対パスの `.` / `..` を畳む。
  function normalizeAbs(abs) {
    var parts = [];
    abs.split('/').forEach(function(seg) {
      if (seg === '' || seg === '.') return;
      if (seg === '..') { parts.pop(); return; }
      parts.push(seg);
    });
    return '/' + parts.join('/');
  }

  // 識別子 → 絶対パス。
  function idToAbs(id) {
    if (!id) return '';
    var root = window.MD_ROOT_DIR || '';
    return normalizeAbs(id.charAt(0) === '/' ? id : root + '/' + id);
  }

  // 絶対パス → 識別子。root の外なら絶対パスのまま返す（黙って root で止めない）。
  function absToId(abs) {
    var root = window.MD_ROOT_DIR || '';
    if (root && abs.indexOf(root + '/') === 0) return abs.slice(root.length + 1);
    return abs;
  }

  // 本文の href / src（サーバが畳んだ絶対 URL）を識別子へ戻す。
  function urlToId(url) {
    var p = url;
    try { p = decodeURIComponent(url); } catch (_) {}
    if (p.indexOf(ABS_PREFIX) === 0) return '/' + p.slice(ABS_PREFIX.length);
    return p.replace(/^\//, '');
  }

  // 開いているファイル（識別子）を基準に、相対パスを識別子へ解決する。
  // 先頭 `/` は「サイトルート = root」を指す URL なので urlToId と同じ扱い。
  function resolvePath(baseId, rel) {
    if (!rel) return baseId;
    if (rel.charAt(0) === '/') return urlToId(rel);
    var dir = idToAbs(baseId).replace(/\/[^/]*$/, '');
    return absToId(normalizeAbs(dir + '/' + rel));
  }

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
    // スクロール主体はシェルでは #preview-pane、1 枚ものでは document。
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
  // （親のプレビュー遷移に回してサイドバー等の状態を同期させる）。
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

  // タブから戻ってきた読み位置を iframe の中へ戻す。
  //
  // 見えるようになってから動かす。opacity:0 のうちに中の文書をスクロールさせると、
  // WKWebView は動かした先を塗らないまま合成して、そこから下が真っ白になる
  // （実機だけで出る。手でスクロールし直すと直る＝塗り直しの取りこぼし）。
  // syncFrameBackground が次のフレームで opacity を開けるので、そのさらに次の
  // フレーム——中身が一度塗られたあと——で動かす。先頭が 1 フレーム見えるが、
  // opacity のフェード中なので移動は目に付かない。
  function restoreFrameScroll(frame) {
    var top = frame.__mdScrollTo;
    if (!top) return;
    requestAnimationFrame(function() {
      requestAnimationFrame(function() {
        // 消してから戻す ＝ iframe 内リンクで別の文書へ移った先へは持ち込まない。
        frame.__mdScrollTo = 0;
        try {
          var d = frame.contentDocument;
          var sc = d && (d.scrollingElement || d.documentElement);
          if (sc) sc.scrollTop = top;
        } catch (e) { /* cross-origin / 差し替え済み: 復元は諦める */ }
      });
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

    // タブに残した読み位置へ戻す。ここまで待たないと中身の高さが決まらず、
    // clamp されて 0 に落ちる。実際に戻すのは restoreFrameScroll（数フレーム後）で、
    // 預けた値もそこで捨てる ＝ 戻し切る前に離れても readScroll がまだ拾える。
    syncFrameBackground(frame);
    restoreFrameScroll(frame);

    // iframe 内の mousedown / scroll は親 document には届かないので、contextmenu.js が
    // 「外側クリック / スクロールで閉じる」ために親へ張るリスナーが発火しない。
    // 開いたままになる右クリックメニューを、ここで明示的に閉じる（未オープン時は no-op）。
    doc.addEventListener('mousedown', function() {
      if (window.MdMenu) MdMenu.close();
    }, true);
    doc.addEventListener('scroll', function() {
      if (window.MdMenu) MdMenu.close();
    }, true);

    // 選択即コピー。iframe 内の mouseup も親には届かないので、ここで同じ口へ繋ぐ
    // （右クリックメニューやキー転送と同じ「親へ委譲」の形）。ここは親のスクリプト
    // なので、この先で叩く window.ipc / navigator はトップフレームのものになる。
    // frame も渡す。iframe 内の座標はその文書のビューポート基準なので、合図を親の
    // 画面に出すには iframe の位置ぶん足す必要がある（右クリック転送と同じ換算）。
    if (window.MdAutoCopy) MdAutoCopy.bindDoc(doc, frame);

    doc.addEventListener('keydown', function(e) {
      // アプリ自身の ⌘/Ctrl ショートカット(r=raw / d=diff / t=toc / p=ファイル検索 /
      // b=ファイルツリー開閉 / w=close / f=検索)だけ親へ委譲する。
      // ⌘A(全選択)・⌘C 等は iframe のネイティブ動作に任せる。
      // ⌘F は MdSearch が iframe の document も走査するようになったので転送する
      // （WKWebView 自体は検索 UI を持たないので、転送しないと無反応になる）。
      var k = (e.key || '').toLowerCase();
      var isCmd = (e.metaKey || e.ctrlKey) &&
        (k === 'r' || k === 'd' || k === 't' || k === 'p' || k === 'b' || k === 'w' || k === 'f');
      var inField = isFieldEl(e.target);
      var bare = !e.metaKey && !e.ctrlKey && !e.altKey && !inField;
      var overlay = isOverlayOpen();

      // 縦スクロール素キー(j/k/d/u/Space/g/G)は「親へ転送」では効かない。スクロールするのは
      // iframe 内の文書で、親は html-frame が丁度 1 画面ぶんなので動かないからである。
      // ここで中身の scroller を直接動かして、md 表示と同じ操作感にする。
      // Space/矢印はネイティブでも動くが、j/k 等はネイティブに無いので自前で賄う。
      if (bare && !overlay && applyScrollKey(e, doc.scrollingElement || doc.documentElement)) return;

      // ファイル移動([/])・ペインフォーカス(Tab)・ヘルプ(?)はアプリ全体の操作なので、iframe に
      // フォーカスがある時でも親へ委譲する（html 表示中でも移動とヘルプが死なない）。ただし:
      //  ・iframe 内の入力欄では素キーを奪わない（文字入力・フォーム内 Tab 移動を保つ）
      //  ・素キーの `/` は転送しない。rustdoc / MkDocs / Docusaurus など `/` を自前の検索
      //    ショートカットに使う生成 html は多く、ページ側のハンドラは先に登録されていて
      //    止められないので、転送すると「ページの検索 UI と親の検索バーが両方開く」二重
      //    状態になる。iframe 内から検索を開くのは ⌘F（ページ側と衝突しにくい）に絞る。
      var isNav = bare && (k === '[' || k === ']' || e.key === 'Tab' || k === '?');
      // Escape は親のオーバーレイ（? で開いたヘルプ等）が開いている時だけ転送する。
      // ? は iframe にフォーカスを残したまま開くので、これが無いと Esc で閉じられない。
      // 開いていない時は転送せず、iframe 内のページ自身の Esc 処理を邪魔しない。
      var isEsc = e.key === 'Escape' && overlay;
      if (!isCmd && !isNav && !isEsc) return;
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

  // スクロール主体。シェルは #preview-pane、1 枚もの（md の直開き）は document。
  function getScroller() {
    return document.querySelector('#preview-pane')
      || document.scrollingElement
      || document.documentElement;
  }

  // ── 読み位置（タブ / ホットリロードが持ち回る値） ──────────────
  // html 表示だけスクロールの主体が違う。.html-frame が画面 1 枚ぶんを占めて
  // 中の文書がスクロールするので、親の #preview-pane は常に 0 のまま動かない。
  // 呼ぶ側（tabs.js の saveActiveState / folder.js の loadPreview）が md と html を
  // 場合分けせずに済むよう、「いま誰がスクロールしているか」をここへ集める。

  // いま出ている html の iframe。html 表示でなければ null。
  function htmlFrame() {
    return document.querySelector('#preview-pane iframe.html-frame');
  }

  // iframe の中のスクロール要素。本物の文書がまだ来ていない（挿入直後の
  // about:blank）／ cross-origin なら null。
  function frameScroller(frame) {
    try {
      var d = frame.contentDocument;
      if (!d || d.URL === 'about:blank') return null;
      return d.scrollingElement || d.documentElement;
    } catch (e) {
      return null;
    }
  }

  // いまの読み位置。
  function readScroll() {
    var frame = htmlFrame();
    if (!frame) {
      var p = getScroller();
      return p ? p.scrollTop : 0;
    }
    // 復元待ちの値があるならそれがいまの読み位置。load 前（中身がまだ無い）と、
    // load 後 restoreFrameScroll が戻すまでの数フレームの両方をこれで覆う。
    // 素直に scrollTop を返すと、その間に別タブへ移るだけで位置が 0 に消える。
    if (frame.__mdScrollTo) return frame.__mdScrollTo;
    var sc = frameScroller(frame);
    return sc ? sc.scrollTop : 0;
  }

  // 読み位置を戻す。html は中身が load されるまで代入しても効かないので、
  // iframe へ預けて bindFrame に消費させる。
  function restoreScroll(top) {
    var frame = htmlFrame();
    if (!frame) {
      var p = getScroller();
      if (p) p.scrollTop = top || 0;
      return;
    }
    frame.__mdScrollTo = top || 0;
  }

  // ファイルツリー（サイドバー）にフォーカスがあるか。ツリー操作の起点判定。
  function isSidebarFocused() {
    var s = document.getElementById('sidebar');
    return !!(s && s.contains(document.activeElement));
  }

  // ── オーバーレイのレジストリ ──────────────────────────────────
  // 検索バー・ヘルプ・ファイル検索・右クリックメニュー・コメント入力は、互いに
  // 2 点だけ気にする必要がある:
  //   (1) 開いている間は本文の素キー（j/k・c・/ など）を止める
  //   (2) Esc は「いちばん前にある 1 つ」だけを閉じる
  //
  // 以前は (1) の判定が id のハードコード列で、(2) は各モジュールが自前で Esc を拾い
  // 「他が開いていたら譲る」を個別に書いていた。6 つ目を足すには 3 箇所を揃える必要が
  // あり、譲り合いの条件も書き写すたびにズレる形だった。ここに登録制で集約する。
  //
  // spec: {
  //   id, isOpen(), close(),
  //   priority   … 大きいほど前面。Esc はこれが最大の 1 つだけを閉じる
  //   blocksKeys … 開いている間、本文の素キーを止めるか（既定 true）
  // }
  var overlays = [];

  function registerOverlay(spec) {
    overlays.push(spec);
    overlays.sort(function(a, b) { return b.priority - a.priority; });
  }

  // 開いているオーバーレイを前面順に返す。exceptId はその 1 つを除く。
  function openOverlays(exceptId) {
    return overlays.filter(function(o) {
      if (o.id === exceptId) return false;
      try { return !!o.isOpen(); } catch (e) { return false; }
    });
  }

  // 素キーを止めるべきオーバーレイが開いているか。毎 keydown（巨大ファイルでの j/k
  // 連打を含む）で走るので、各 isOpen() は getElementById か class 参照だけで済ませる
  // （querySelector による document 全走査は使わない。各オーバーレイに安定 id がある）。
  function isOverlayOpen(exceptId) {
    return openOverlays(exceptId).some(function(o) { return o.blocksKeys !== false; });
  }

  // Esc を 1 箇所で受ける。最前面の 1 つだけ閉じてそこで止めるので、1 回の Esc で
  // 2 つ閉じることも、誰も閉じないこともない。capture で各モジュールより先に受ける。
  document.addEventListener('keydown', function(e) {
    if (e.key !== 'Escape') return;
    var open = openOverlays();
    if (!open.length) return;
    e.preventDefault();
    e.stopPropagation();
    open[0].close();
  }, true);

  // ── クリップボードとトースト ──────────────────────────────────
  // 書き込み口はここ 1 つに寄せる（コードブロックの Copy・右クリックの「コピー」・
  // コメントの全部コピー・選択即コピーが全部ここを通る）。
  //
  // navigator.clipboard を先に試し、駄目なら Rust（NSPasteboard）へ回す。
  // 以前は隠し textarea + execCommand で代替していたが、あれは textarea を select()
  // するのでユーザーの選択範囲をその場で壊す——選択即コピーでは致命的なので捨てた。
  //
  // mdpreview:// は実測で secure context 扱いになり navigator.clipboard も生えている。
  // 落ちるのは「ユーザー ジェスチャの外から呼ぶと NotAllowedError で reject」の方で、
  // IPC はその受け皿。死にコードではなく、ジェスチャの無いコピーが必ず通る道である。
  // IPC は投げっぱなしなので、その経路の done() は成功の確認ではなく送信の確認。
  function copyText(text, done, fail) {
    if (!text) { if (fail) fail(); return; }
    var viaIpc = function() {
      // wry は IPC の本文を CStr で読むので、NUL を含む文字列はそこで黙って切れる。
      // 一部だけ入れて成功と言うより、失敗として返す（NUL 入りのテキストファイルを
      // ソース表示でなぞると起こりうる）。
      if (!window.ipc || text.indexOf('\u0000') >= 0) { if (fail) fail(); return; }
      window.ipc.postMessage('copy:' + text);
      if (done) done();
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(function() { if (done) done(); }).catch(viaIpc);
    } else {
      viaIpc();
    }
  }

  // 選択中のテキスト（前後の空白だけなら空文字）。
  // .html-frame の中の選択は親の getSelection に出てこないので、親が空なら
  // iframe の文書も見る。doc を渡せばその文書だけを見る。
  //
  // フレームを覗くときは hasFocus() で「いま操作しているフレーム」に絞る。WebKit は
  // フォーカスが外れても選択を保持するので、これが無いと html を見た後に別の場所を
  // 右クリックした時、ずっと前の選択を拾って「選んでいないテキスト」をコピーする。
  // search.js の selectedText が同じ理由で同じガードを持つ。
  function selectionText(doc) {
    var pick = function(d) {
      try {
        var s = d && d.getSelection ? String(d.getSelection()) : '';
        return s && s.trim() ? s : '';
      } catch (e) { return ''; }
    };
    if (doc) return pick(doc);
    var own = pick(document);
    if (own) return own;
    var frames = document.querySelectorAll('iframe.html-frame');
    for (var i = 0; i < frames.length; i++) {
      var fd;
      try { fd = frames[i].contentDocument; } catch (e) { continue; } // 外部遷移で cross-origin
      if (!fd || !fd.hasFocus()) continue;
      var got = pick(fd);
      if (got) return got;
    }
    return '';
  }

  // ── 選択範囲をマークダウンのソース寄りに直す ────────────────────
  // レンダリング結果をなぞると、ブラウザが返すのは「見えている文字」なので記法が全部
  // 落ちる。`コード` を選んでもバッククォートが付かず、貼った先で地の文に化ける。
  //
  // 戻すのはコードの記法だけ（バッククォートとフェンス）。太字やリンクまで戻すと、
  // 文章をチャットへ貼るときに記号が邪魔になる方が多い。
  //
  // 作り方は「選択範囲の複製を画面外へ写し、そこで code を書き換えて innerText を読む」。
  // 段落・行・表の切れ目をどう文字にするかはブラウザに任せられるので、空白と改行の畳み方が
  // 素のコピーと揃う。自前で走査すると、表のタブ区切りや <br> や white-space の扱いを
  // ひとつずつ再現する話になる。
  var MIRROR_MAX = 200000;   // 写しを作るのを諦める選択の長さ（巨大ファイルの全選択対策）

  function elOf(node) {
    return node && (node.nodeType === 1 ? node : node.parentElement);
  }

  // 中身を ` で包むための区切り。中の最長の連続バッククォートより 1 本長くする。
  function ticksFor(text, min) {
    var run = 0, max = 0;
    for (var i = 0; i < text.length; i++) {
      if (text.charAt(i) === '`') { run++; if (run > max) max = run; } else run = 0;
    }
    var out = '';
    while (out.length < Math.max(min, max + 1)) out += '`';
    return out;
  }

  // インラインコード。CommonMark は両端の空白 1 文字を剥がすので、端が ` か空白のときは
  // 空白で詰めておく（詰めないと `` `x`` のように区切りと中身が繋がって読めなくなる）。
  function inlineCode(text) {
    var pad = (/^`|`$/.test(text) || (/^\s/.test(text) && /\s$/.test(text))) ? ' ' : '';
    var fence = ticksFor(text, 1);
    return fence + pad + text + pad + fence;
  }

  // コードブロック。info は fence の情報文字列（`rust` や `sh:install.sh`）。
  function fencedCode(text, info) {
    var fence = ticksFor(text, 3);
    return fence + info + '\n' + text.replace(/\n+$/, '') + '\n' + fence;
  }

  // fence の情報文字列。原文どおりの値は Rust が .code-wrapper の data-fence に置く
  // （html.rs の fence_attr）。写しが wrapper を含まない場合の保険として、hljs が
  // 混ぜた class から言語だけ拾う（先に来るのが原文の言語で、後ろは hljs の検出結果）。
  function fenceInfo(code) {
    var wrap = code.closest ? code.closest('.code-wrapper') : null;
    var info = wrap && wrap.getAttribute('data-fence');
    if (info) return info;
    var m = /(?:^|\s)language-(\S+)/.exec(code.className || '');
    // hljs は言語を判定できなかったブロックに language-undefined を付ける（indented な
    // コードブロックがこれになる）。原文の情報ではないので情報無しの fence として扱う。
    return m && m[1] !== 'undefined' ? m[1] : '';
  }

  function hasCode(body, range) {
    var codes = body.querySelectorAll('code');
    for (var i = 0; i < codes.length; i++) {
      // intersectsNode が無い/投げる環境では写しの側で判断させる（多く付くより
      // 落ちる方が困る）。
      try { if (range.intersectsNode(codes[i])) return true; } catch (e) { return true; }
    }
    return false;
  }

  function mirrorMarkdown(body, range) {
    var box = document.createElement('div');
    // 本文と同じ CSS（pre の white-space、リスト・表の箱）を継承させるため
    // .markdown-body の中へ入れる。幅も本文と揃える。
    box.style.cssText = 'position:fixed;left:-99999px;top:0;width:' + body.clientWidth + 'px;';
    box.setAttribute('aria-hidden', 'true');
    try {
      box.appendChild(range.cloneContents());
      // 本文ではないもの。Copy ボタンは user-select:none を持たないので、素のコピーだと
      // コードブロックの末尾に "Copy" の 4 文字が混ざる。
      box.querySelectorAll('.copy-btn, .md-cmt-badge, .md-cmt-badge-holder, .source-gutter')
        .forEach(function(n) { n.remove(); });
      // 画像・動画・draw.io の図は文字を持たない。写しに残すと、読み込みが走り直す分と
      // レイアウトの分だけ損をする。mermaid の svg はラベルが文字なので残す（素のコピーで
      // 入っていたものを、写しを経由したせいで落とさない）。
      box.querySelectorAll('img, canvas, video, audio, iframe, .mxgraph')
        .forEach(function(n) { n.remove(); });
      box.querySelectorAll('code').forEach(function(code) {
        if (!code.closest('pre')) {
          code.textContent = inlineCode(code.textContent);
          return;
        }
        // ファイル名バーは 1 行として混ざる。fence の情報文字列が同じ名前を持つので捨てる。
        var wrap = code.closest('.code-wrapper');
        var fname = wrap && wrap.querySelector('.code-filename');
        var info = fenceInfo(code);
        if (fname) fname.remove();
        // 前後を 1 行空ける。写しの innerText はブロックの境目に改行 1 つしか置かないので、
        // フェンスが前の段落や次のフェンスと地続きになる（下の \n{3,} で 1 行に詰まる）。
        code.textContent = '\n' + fencedCode(code.textContent, info) + '\n';
      });
      body.appendChild(box);
      var text = box.innerText;
      // ブロックの境目に開いた空行を 1 つに詰め、前後の余りは落とす（フェンスの前後に
      // 足した改行も、写しのマージン由来の空行もここで揃う）。
      return text ? text.replace(/\n{3,}/g, '\n\n').replace(/^\s+|\s+$/g, '') : '';
    } catch (e) {
      return '';
    } finally {
      if (box.parentNode) box.parentNode.removeChild(box);
    }
  }

  // doc は選択を持つ文書（selectionText と同じ引数）。マークダウン本文の選択でだけ
  // 記法を戻し、それ以外は selectionText と同じ文字列をそのまま返す。
  function selectionMarkdown(doc) {
    var plain = selectionText(doc);
    if (!plain) return '';
    // .html-frame の中は素の html。<code> はマークダウン由来ではないので触らない。
    if (doc && doc !== document) return plain;

    var sel, range;
    try {
      sel = document.getSelection ? document.getSelection() : null;
      range = sel && sel.rangeCount ? sel.getRangeAt(0) : null;
    } catch (e) { return plain; }
    if (!range) return plain;

    var host = elOf(range.commonAncestorContainer);
    if (!host || !host.closest) return plain;
    var body = host.closest('.markdown-body');
    // raw / diff 表示（.source-view）は既に原文そのもの。フェンスを足すと二重になる。
    if (!body || host.closest('.source-view')) return plain;

    // 選択が 1 つの code の内側に収まっているとき。複製すると code 要素ごと範囲の外へ
    // 落ちて（部分選択の共通祖先はテキストノード）言語も分からなくなるので、素の
    // テキストをここで包む。
    var code = host.closest('code');
    if (code) {
      return code.closest('pre') ? fencedCode(plain, fenceInfo(code)) : inlineCode(plain);
    }

    // コードが混ざらない選択（＝普通の文章）は写しを作らずに返す。ここが大半の道で、
    // 挙動もコストも従来と変わらない。
    if (!hasCode(body, range) || plain.length > MIRROR_MAX) return plain;
    return mirrorMarkdown(body, range) || plain;
  }

  // 短いフィードバックの浮遊トースト（操作の結果をボタン無しで返す）。
  // 画面下中央に 1 つだけ作り、以降は文言を差し替えて使い回す。
  var toastEl = null, toastTimer = null;
  function toast(msg) {
    if (!toastEl) {
      toastEl = document.createElement('div');
      toastEl.className = 'md-toast';
      // スクリーンリーダーにも結果（コピー/削除/巡回位置）を伝える。
      toastEl.setAttribute('role', 'status');
      toastEl.setAttribute('aria-live', 'polite');
      document.body.appendChild(toastEl);
    }
    toastEl.textContent = msg;
    toastEl.classList.add('show');
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(function() { if (toastEl) toastEl.classList.remove('show'); }, 1500);
  }

  // フォーカスが文字入力欄（素キーを横取りしてはいけない要素）に乗っているか。
  // iframe 転送・スクロール素キー・フォーカス移譲の各所で同じ判定を使うため一本化する。
  function isFieldEl(el) {
    return !!(el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable));
  }

  // 右下フロートの共有スタック。diff / raw トグルとコメントパネル/ピルを 1 つの
  // flex コンテナに入れ、どれが在っても下から詰めて並ぶ（片方だけの時に隙間が出ない）。
  // 並び順は各要素の CSS `order` で決める（DOM 追加順に依存しない）。
  function cornerStack() {
    var el = document.getElementById('md-corner');
    if (!el) {
      el = document.createElement('div');
      el.id = 'md-corner';
      document.body.appendChild(el);
    }
    return el;
  }

  // 本文スクロールの素キー（vim/less 流）を、渡された scroller 1 つに適用する。
  // 親 document（keyscroll.js）と html iframe の中身（bindFrame）の両方から呼ぶ。
  // 別実装にすると同じキーで挙動がズレるので、必ずここに一本化する。
  // 処理したら preventDefault して true、未対応キーなら何もせず false を返す。
  function applyScrollKey(e, sc) {
    if (!sc) return false;
    // ⌘/Ctrl/Alt 併用は既存ショートカット等に委ねる（Shift は Space/G で使うので許可）。
    if (e.metaKey || e.ctrlKey || e.altKey) return false;
    var LINE = 40; // 1 行ぶんのスクロール量(px)
    var page = sc.clientHeight || 600;
    switch (e.key) {
      case 'j': sc.scrollTop += LINE; break;
      case 'k': sc.scrollTop -= LINE; break;
      case 'd': sc.scrollTop += page / 2; break;
      case 'u': sc.scrollTop -= page / 2; break;
      // 1 ページ送りは 0.9 ページぶん。1 割を残して直前の文脈を画面に留め、読み位置を見失わせない。
      case ' ': sc.scrollTop += (e.shiftKey ? -1 : 1) * page * 0.9; break;
      case 'g': sc.scrollTop = 0; break;
      case 'G': sc.scrollTop = sc.scrollHeight; break; // 末尾へ（clampで下端に収まる）
      default: return false;
    }
    e.preventDefault();
    return true;
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

  // アプリ全体のキー。keymap.js は common.js の直後に評価されるので、ここで差し込める。
  if (window.MdKeymap) {
    MdKeymap.on('select-body', selectBody);
    MdKeymap.on('window-close', closeWindow);
  }

  window.MdCommon = {
    loadLib: loadLib,
    slugify: slugify,
    ensureHeadingId: ensureHeadingId,
    ensureHeadingIds: ensureHeadingIds,
    addCopyButtons: addCopyButtons,
    addLineNumbers: addLineNumbers,
    highlightIn: highlightIn,
    wrapSourceLines: wrapSourceLines,
    hydrate: hydrate,
    bodyGen: function() { return bodySwaps; },
    runMermaid: runMermaid,
    runDrawio: runDrawio,
    wireHtmlFrames: wireHtmlFrames,
    getScroller: getScroller,
    readScroll: readScroll,
    restoreScroll: restoreScroll,
    isSidebarFocused: isSidebarFocused,
    isOverlayOpen: isOverlayOpen,
    registerOverlay: registerOverlay,
    isInteractiveFocus: isInteractiveFocus,
    isFieldEl: isFieldEl,
    copyText: copyText,
    selectionText: selectionText,
    selectionMarkdown: selectionMarkdown,
    toast: toast,
    applyScrollKey: applyScrollKey,
    scrollToAnchor: scrollToAnchor,
    idToAbs: idToAbs,
    absToId: absToId,
    urlToId: urlToId,
    resolvePath: resolvePath,
    cornerStack: cornerStack,
    // ⌘W の従来の意味。タブ（tabs.js）が最後の 1 枚を閉じる時にも使う。
    closeWindow: closeWindow
  };
})();
