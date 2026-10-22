// アプリが配る部品: コピーボタンと図の拡大。<head> に inline され window.MdParts を公開する。
//
// 文書の JS は既定で止める方針なので、よく使う 2 つだけはアプリが JS を持ち、文書は目印の
// 属性を書くだけにする。
//  - `<button data-md-copy="copy-ID">` … `id="copy-ID"` の要素の文字を写す（md でも html でも）
//  - `<button data-md-copy>`（値なし）… すぐ前の兄弟の `<pre>` を写す（html だけ。md では
//    生 HTML の `<button>` が `<p>` に包まれて、`<pre>` と兄弟にならない）
//  - `data-md-zoom` を付けた `<figure>`・`<svg>`・`<img>` … 押すと本文の枠いっぱいに広げる
//  - md では目印が無くても、アプリが描いた mermaid の図と、本文の幅に収めるために縮められた
//    画像は押せば広がる（`.html` は文書が自分で大きさを決めるので、目印のあるものだけ）
//
// イベントは本文の根元に委譲して受ける。md は #preview-pane（本文はアプリの UI と同じ
// document にあるので、そこより外の目印は拾わない）、html は iframe の document。
// 親 document ぶんはここで張り、.html-frame の中は common.js の bindFrame が bindDoc() を呼ぶ。
//
// 文書の要素で名前がすり替わらないよう（`<img name=createElement>` で `doc.createElement`
// が文書の要素になる。`<form>` の中の `<input name=tagName>` で `form.tagName` も同じ。
// DOM clobbering）、document と要素のメソッド・プロパティは prototype から呼ぶ。
(function() {
  // トップのフレームでなければ何もしない（理由は html.rs の PAGE_SCRIPTS）。
  if (window.top !== window) return;
  // 文書のボタンの合図を出しておく時間。写した中身を読ませるので、トースト（1500ms）より長い。
  // 選択即コピーの合図（チェック 1 つ、620ms）より長いのは、あちらは自分で選んだ文字を
  // 見ながらなので文字が要らず、こちらは中身が見えないまま写すため。
  // 中身が長いほど読むのに時間がかかるので、出した文字数に応じて延ばす（上限あり。
  // じっくり読みたいときは、合図に乗せている間は消えない）。
  var SIGNAL_MS = 3200;
  var SIGNAL_MS_PER_CHAR = 4;
  var SIGNAL_MAX_MS = 9000;
  // コードブロックの Copy の合図は短い固定の時間。コードブロックはアプリが描いたもので、写す
  // 中身は見えているコードそのもの。合図が防ぐ「見えないものを写させる」手口が効くのは、
  // 文書が自分で書いたボタンの方。乗せている間は消えないのは同じ。
  var SIGNAL_CODE_MS = 1500;
  // 合図から外れたあと、消えるまでの時間。読み終えて離したところなので短くてよい。
  var SIGNAL_LEAVE_MS = 1500;
  // 合図に出す中身の上限（文字数と行数）。超えたら頭と尻の行を残して真ん中の行を省く。
  // 1 行だけが長いときは、行では切れないので文字で省く。
  var SIGNAL_CHARS = 1600;
  var SIGNAL_LINES = 40;
  var SIGNAL_EDGE_LINES = 15;
  // 拾う id の頭。md の本文はアプリの UI と同じ document にあり、見出しの自動 id や脚注の
  // `id="1"` とぶつかるので、決まった頭のものだけを探す。
  var COPY_ID_PREFIX = 'copy-';
  var ZOOM_TAGS = { FIGURE: 1, SVG: 1, IMG: 1 };
  // 拡大の内側の余白（px）。スタイルシートの padding と揃える。
  var ZOOM_PAD = 24;
  // 写しから外す、文字を持たない要素。読み込みが走り直す分を避ける（mirrorMarkdown と同じ）。
  var MEDIA = 'img, canvas, video, audio, iframe, object, embed';
  // svg の中のリンクは xlink:href のこともある。
  var LINK = 'a[href], a[*|href]';

  function getter(proto, name) { return Object.getOwnPropertyDescriptor(proto, name).get; }
  var D = Document.prototype, E = Element.prototype, N = Node.prototype;
  var createEl = D.createElement;
  var createText = D.createTextNode;
  var docQuerySel = D.querySelector;
  var docQuerySelAll = D.querySelectorAll;
  var bodyOf = getter(D, 'body');
  var headOf = getter(D, 'head');
  var rootOf = getter(D, 'documentElement');
  var scrollerOf = getter(D, 'scrollingElement');
  var viewOf = getter(D, 'defaultView');
  var activeOf = getter(D, 'activeElement');
  var tagOf = getter(E, 'tagName');
  var prevElOf = getter(E, 'previousElementSibling');
  var closestOf = E.closest;
  var querySel = E.querySelector;
  var querySelAll = E.querySelectorAll;
  var getAttr = E.getAttribute;
  var hasAttr = E.hasAttribute;
  var setAttr = E.setAttribute;
  var removeAttr = E.removeAttribute;
  var rectOf = E.getBoundingClientRect;
  var removeEl = E.remove;
  var contains = N.contains;
  var appendTo = N.appendChild;
  var insertBefore = N.insertBefore;
  var replaceChild = N.replaceChild;
  var removeChild = N.removeChild;
  var cloneNode = N.cloneNode;
  var parentOf = getter(N, 'parentNode');
  var nextOf = getter(N, 'nextSibling');
  var styleOf = getter(HTMLElement.prototype, 'style');
  var connected = getter(N, 'isConnected');
  var ownerOf = getter(N, 'ownerDocument');
  var innerTextOf = getter(HTMLElement.prototype, 'innerText');
  var textContentOf = getter(N, 'textContent');
  var listen = EventTarget.prototype.addEventListener;
  var unlisten = EventTarget.prototype.removeEventListener;

  function tag(el) { return String(tagOf.call(el)).toUpperCase(); }

  // 本文の根元。common.js が本文の入る前に取ったもの（1 枚ページには無い）。
  var pane = null;
  // 配線済みの document。文書が書ける場所（`doc.__mdParts` など）に印を置くと、
  // `<img name=__mdParts>` で配線済みに見せかけられる。
  var boundDocs = new WeakSet();

  function capturePane() {
    pane = (window.MdCommon && MdCommon.regions) ? MdCommon.regions().pane : null;
    if (!pane) return;
    boundDocs.add(document);
    injectSheet(document, true);
    bindRoot(pane, document, null);
  }

  // ── 合図 ─────────────────────────────────────────────────────
  // 文書の CSS が届かない場所に出す。閉じた shadow root の中なので本文の CSS が届かず、
  // :host の all:initial!important は外からの display:none!important にも勝つ（shadow 側の
  // !important が先に立つ）。popover はトップレイヤーに出るので、z-index を最大にした
  // 重ねものにも隠されない。
  // これは文書の JS が止まっている（既定）ことが前提の守り。文書の JS を動かす設定では、
  // 文書の JS がこのファイルより先に動くことはないが、MdCommon の口や DOM を後から
  // 好きにいじれるので、合図が本物だとは請け負わない（その設定にした文書は信じたもの）。
  var signal = null, signalTimer = null, signalHover = false;
  // 窓の上端から合図までの距離（本文の枠が無い 1 枚ページのとき）と、枠の上端からの距離。
  var SIGNAL_TOP = 12, SIGNAL_PANE_GAP = 8;
  // 「全部見る」で出す上限の文字数。写したものが巨大でも、合図の描画で窓を固めない。
  var SIGNAL_FULL_CHARS = 200000;

  function ensureSignal() {
    if (signal) return signal;
    var host = createEl.call(document, 'div');
    var root = host.attachShadow({ mode: 'closed' });
    var style = createEl.call(document, 'style');
    style.textContent =
      ':host{all:initial!important;}' +
      // 上の真ん中に、中身の幅で出す。下はトーストと右下の Raw / 差分の釦の置き場所。
      // 高さの上限は窓の下端まで（--avail。placeSignal が入れる）。中身は .body の中で
      // 縦にも横にもスクロールする。
      '.box{all:initial;position:fixed;top:' + SIGNAL_TOP + 'px;left:50%;transform:translateX(-50%);margin:0;' +
      'box-sizing:border-box;width:max-content;max-width:min(640px,calc(100vw - 32px));' +
      'max-height:min(60vh,420px,var(--avail,100vh));padding:10px 14px;border-radius:8px;border:0;' +
      'flex-direction:column;' +
      // all:initial は direction と unicode-bidi を戻さないので、ここで決める。外の文書の
      // `dir=rtl` や direction を受け継ぐと、合図の並びも写した行の見え方も裏返る。
      'direction:ltr;unicode-bidi:isolate;' +
      'background:#1f2328;color:#f0f3f6;font:13px/1.5 -apple-system,BlinkMacSystemFont,sans-serif;' +
      'box-shadow:0 6px 24px rgba(0,0,0,.35);}' +
      '.box.full{max-width:min(960px,calc(100vw - 32px));max-height:min(80vh,var(--avail,100vh));}' +
      // 開閉は自前のクラスで持つ。:popover-open に頼ると、popover の無いエンジンで
      // 規則ごと捨てられて出しっぱなしになる。
      '.box:not(.open){display:none;}.box.open{display:flex;}' +
      '.head{font-weight:600;display:flex;gap:12px;align-items:baseline;flex:none;}' +
      '.title{flex:1;}' +
      // 操作の文言は ::after の content で出す。textContent に入らないので、選択にも
      // 合図の中身の読み取りにも混ざらない。
      '.act{font-weight:400;color:#8ab4f8;cursor:pointer;user-select:none;-webkit-user-select:none;' +
      'white-space:nowrap;}' +
      '.act:hover{text-decoration:underline;}' +
      '.act::after{content:attr(data-label);}' +
      '.act[hidden]{display:none;}' +
      // 行は折り返さない（white-space:pre）。折り返すと、画面の 1 行が写した 1 行なのか、
      // 長い行の続きなのかが見分けられない。はみ出した分は横にスクロールして見る。
      '.body{display:block;white-space:pre;overflow:auto;min-height:0;flex:0 1 auto;' +
      'font-family:ui-monospace,SFMono-Regular,Menlo,monospace;color:#c9d1d9;margin-top:4px;' +
      'tab-size:4;overscroll-behavior:contain;' +
      // 写した行は、行ごとに中の最初の強い文字で向きを決める（貼り付けた先のテキスト
      // エディタと同じ）。合図の ltr に揃えると、アラビア文字やヘブライ文字で始まる行の
      // 語順が貼った先と違って見える。
      'unicode-bidi:plaintext;}' +
      '.box.full .body{user-select:text;-webkit-user-select:text;cursor:text;}' +
      '.body[hidden]{display:none;}' +
      '.gap{color:#8b949e;font-style:italic;}';
    var box = createEl.call(document, 'div');
    box.className = 'box';
    box.setAttribute('role', 'status');
    box.setAttribute('aria-live', 'polite');
    if ('popover' in box) box.setAttribute('popover', 'manual');
    var head = createEl.call(document, 'div');
    head.className = 'head';
    var title = createEl.call(document, 'span');
    title.className = 'title';
    var more = createEl.call(document, 'span');
    more.className = 'act';
    more.setAttribute('data-act', 'more');
    more.setAttribute('data-label', '全部見る');
    more.setAttribute('role', 'button');
    var close = createEl.call(document, 'span');
    close.className = 'act';
    close.setAttribute('data-act', 'close');
    close.setAttribute('data-label', '閉じる');
    close.setAttribute('role', 'button');
    close.setAttribute('aria-label', '閉じる');
    head.appendChild(title);
    head.appendChild(more);
    head.appendChild(close);
    var body = createEl.call(document, 'div');
    body.className = 'body';
    box.appendChild(head);
    box.appendChild(body);
    root.appendChild(style);
    root.appendChild(box);
    appendTo.call(rootOf.call(document), host);
    // 乗せている間は消さない（長い中身を読み切る前に消えないように）。
    box.addEventListener('mouseenter', function() {
      signalHover = true;
      if (signalTimer) { clearTimeout(signalTimer); signalTimer = null; }
    });
    box.addEventListener('mouseleave', function() {
      signalHover = false;
      if (isSignalOpen()) scheduleHide(SIGNAL_LEAVE_MS);
    });
    // 押したとき: 「閉じる」は閉じる。真ん中を省いている間は、どこを押しても全部を出す
    // （省いた所に何があるかを確かめる手段がほかに無い）。全部を出したあとは押しても
    // 閉じない（中身を選んだりスクロールしたりするため）。省いていない合図は、押せば閉じる。
    box.addEventListener('click', function(e) {
      var t = e.target;
      var act = t && t.closest ? t.closest('[data-act]') : null;
      if (act && act.getAttribute('data-act') === 'close') { hideSignal(); return; }
      if (!signal.copied) { hideSignal(); return; }
      if (signal.elided && !signal.full) { expandSignal(); return; }
      if (act && act.getAttribute('data-act') === 'more') { collapseSignal(); return; }
      if (!signal.elided) hideSignal();
    });
    signal = { host: host, box: box, title: title, more: more, body: body,
               copied: null, elided: false, full: false };
    return signal;
  }

  function isSignalOpen() { return !!signal && signal.box.classList.contains('open'); }

  function scheduleHide(ms) {
    if (signalTimer) clearTimeout(signalTimer);
    signalTimer = signalHover ? null : setTimeout(hideSignal, ms);
  }

  // 本文の枠（#preview-pane）を基準に置く。横は枠の真ん中（窓の真ん中だと、サイドバーの
  // ぶん本文から左へずれて見える）、縦は枠の上端から少し下（タブ帯にかぶせない）。
  // 横に置くと窓からはみ出すなら、窓の端から 16px までに収める。枠の無い 1 枚ページは窓が基準。
  function placeSignal(box) {
    var de = rootOf.call(document);
    var vw = de.clientWidth || window.innerWidth;
    var vh = de.clientHeight || window.innerHeight;
    var x = vw / 2, top = SIGNAL_TOP;
    if (pane && connected.call(pane)) {
      var r = rectOf.call(pane);
      if (r.width > 0) {
        x = r.left + r.width / 2;
        top = Math.max(0, r.top) + SIGNAL_PANE_GAP;
      }
    }
    box.style.setProperty('top', top + 'px');
    box.style.setProperty('--avail', Math.max(80, vh - top - 16) + 'px');
    var half = rectOf.call(box).width / 2;
    x = Math.max(half + 16, Math.min(vw - half - 16, x));
    box.style.setProperty('left', x + 'px');
  }

  // 中身（segments は [{ text, gap? }]）を描き直す。文字は textContent で入れる。
  function fillSignalBody(segments) {
    var s = signal;
    s.body.textContent = '';
    segments.forEach(function(seg) {
      if (seg.gap) {
        var g = createEl.call(document, 'span');
        g.className = 'gap';
        g.textContent = seg.text;
        s.body.appendChild(g);
      } else {
        s.body.appendChild(createText.call(document, seg.text));
      }
    });
    s.body.scrollTop = 0;
    s.body.scrollLeft = 0;
  }

  function openSignal(ms) {
    var s = signal;
    s.box.classList.add('open');
    // 出し直して、後から開いた重ねものより前へ出す。
    if (s.box.showPopover) {
      try { s.box.hidePopover(); } catch (e) {}
      try { s.box.showPopover(); } catch (e) {}
    }
    // 同じ場所に出ていたトーストは下げる（重なると、どちらの文言か読めない）。
    var t = docQuerySel.call(document, '.md-toast.show');
    if (t) t.classList.remove('show');
    placeSignal(s.box);
    scheduleHide(ms || SIGNAL_MS);
  }

  // 一言だけの合図（見つからない・失敗など）。中身の欄は出さない。
  function showSignal(message) {
    var s = ensureSignal();
    s.copied = null;
    s.elided = false;
    s.full = false;
    s.box.classList.remove('full');
    s.more.hidden = true;
    s.title.textContent = message;
    fillSignalBody([]);
    s.body.hidden = true;
    openSignal();
  }

  function hideSignal() {
    if (!signal) return;
    if (signalTimer) { clearTimeout(signalTimer); signalTimer = null; }
    signalHover = false;
    signal.box.classList.remove('open');
    if (signal.box.hidePopover) {
      try { signal.box.hidePopover(); } catch (e) {}
    }
  }

  // 写した中身をそのまま見せる。画面で見えていた文字と違うものが写っていたら、ここで
  // 気づける。見えない文字の作り方は数が多すぎるので、取り出す側では除かない。
  // Why not 先頭と末尾の行だけ: 間の行に何が入っていても見えない。省いた真ん中は、
  // 押せば全部出る。
  function signalCopied(text, ms) {
    var s = ensureSignal();
    // コードブロックは末尾の改行ごと写すので、1 つだけ外して数える（3 行のコードを 4 行と
    // 言わない）。写したもの自体はそのまま。
    var body = text.replace(/\n$/, '');
    var chars = Array.from(body).length;
    var lines = body.split('\n').length;
    var segs = elide(body);
    s.copied = body;
    s.elided = segs.length > 1;
    s.full = false;
    s.box.classList.remove('full');
    s.title.textContent = 'コピーしました（' + (lines > 1 ? lines + ' 行・' : '') + chars + ' 文字）';
    s.more.hidden = !s.elided;
    s.more.setAttribute('data-label', '全部見る');
    s.body.hidden = false;
    fillSignalBody(segs);
    var shown = segs.reduce(function(a, g) { return a + Array.from(g.text).length; }, 0);
    openSignal(ms || Math.min(SIGNAL_MAX_MS, SIGNAL_MS + shown * SIGNAL_MS_PER_CHAR));
  }

  function expandSignal() {
    var s = signal;
    var cs = Array.from(s.copied);
    fillSignalBody(cs.length <= SIGNAL_FULL_CHARS ? [{ text: s.copied }] : [
      { text: cs.slice(0, SIGNAL_FULL_CHARS).join('') },
      { gap: true, text: '\n…（残り ' + (cs.length - SIGNAL_FULL_CHARS) + ' 文字は表示しません）…' }
    ]);
    s.full = true;
    s.box.classList.add('full');
    s.more.setAttribute('data-label', '省いて見る');
    placeSignal(s.box);
    // 読み始めたところなので、乗せていなくても時間切れでは消さない（Esc・閉じる・外して
    // しばらくで消える）。
    if (signalTimer) { clearTimeout(signalTimer); signalTimer = null; }
  }

  function collapseSignal() {
    var s = signal;
    fillSignalBody(elide(s.copied));
    s.full = false;
    s.box.classList.remove('full');
    s.more.setAttribute('data-label', '全部見る');
    placeSignal(s.box);
  }

  // 合図に出す形（[{ text, gap? }]。省かなければ 1 つ）。省くなら行の境目で省き、残す行は
  // 途中で切らない（行の途中で切ると、どこまでが 1 行かが読めない）。頭と尻の行がそれぞれ
  // 文字数の半分に収まらないとき（1 行だけが長い）だけ、文字で省く。
  function elide(text) {
    var cs = Array.from(text);
    var rows = text.split('\n');
    if (cs.length <= SIGNAL_CHARS && rows.length <= SIGNAL_LINES) return [{ text: text }];
    var half = SIGNAL_CHARS / 2;
    var head = edgeRows(rows, half);
    var tail = edgeRows(rows.slice().reverse(), half).reverse();
    if (head.length && tail.length && head.length + tail.length < rows.length) {
      return [
        { text: head.join('\n') + '\n' },
        { gap: true, text: '…（' + (rows.length - head.length - tail.length) + ' 行を省略）…' },
        { text: '\n' + tail.join('\n') }
      ];
    }
    return [
      { text: cs.slice(0, half).join('') + '\n' },
      { gap: true, text: '…（' + (cs.length - SIGNAL_CHARS) + ' 文字を省略）…' },
      { text: '\n' + cs.slice(cs.length - half).join('') }
    ];
  }

  // 端から、SIGNAL_EDGE_LINES 行まで・合わせて budget 文字までの行。
  function edgeRows(rows, budget) {
    var out = [], used = 0;
    for (var i = 0; i < rows.length && out.length < SIGNAL_EDGE_LINES; i++) {
      used += Array.from(rows[i]).length + 1;
      if (used > budget) break;
      out.push(rows[i]);
    }
    return out;
  }

  // Esc で閉じる。重ねもの（registerOverlay）には入れない。入れると開いている間は素キーが
  // 止まったことになり、iframe から j / k が渡らなくなる。common.js の Esc（capture）が
  // 重ねものを閉じたときは伝播を止めるので、ここへは来ない（重ねものが先に閉じる）。
  // 文書自身の Esc の処理も邪魔しないよう、preventDefault はしない。
  function onSignalKey(e) {
    if (e.key !== 'Escape' || !isSignalOpen()) return;
    if (window.MdCommon && MdCommon.isOverlayOpen && MdCommon.isOverlayOpen()) return;
    hideSignal();
  }
  listen.call(document, 'keydown', onSignalKey);
  // コードブロックの Copy も同じ合図を、短い時間で出す（受け口は最初の 1 回だけ取れる）。
  if (window.MdCommon && MdCommon.claimCopySignal) {
    MdCommon.claimCopySignal(function(text) { signalCopied(text, SIGNAL_CODE_MS); });
  }

  // ── コピー ───────────────────────────────────────────────────
  // ボタンが写す相手。見つからなければ null。
  function copySource(btn, root, frame) {
    var id = getAttr.call(btn, 'data-md-copy');
    if (id) {
      var q = '#' + CSS.escape(id);
      // getElementById は文書全体（md ではアプリの UI まで）を見るので使わない。
      var all = root.nodeType === 9 ? docQuerySelAll.call(root, q) : querySelAll.call(root, q);
      // 見出しに自動で付いた id（「Copy settings」→ copy-settings）は書き手が付けたもの
      // ではないので飛ばす。同じ id の要素を書き手が置いていれば、そちらを写す。
      var autoAttr = (window.MdCommon && MdCommon.AUTO_ID_ATTR) || 'data-md-auto-id';
      for (var i = 0; i < all.length; i++) {
        if (!hasAttr.call(all[i], autoAttr)) return all[i];
      }
      return null;
    }
    if (!frame) return null;
    var prev = prevElOf.call(btn);
    return prev && tag(prev) === 'PRE' ? prev : null;
  }

  // 要素の文字。入力欄は値、それ以外は表示どおりの文字（innerText）から、アプリが本文に
  // 足した UI を外して取る。innerText は描画中の要素でしか改行を正しく返さないので、
  // 写しを画面の外に置いてから読む（mirrorMarkdown と同じ）。
  //
  // 写しは元の要素のすぐ後ろ（同じ親の中）に置く。文書の CSS が写しにも元と同じように
  // 当たり、CSS で隠した文字は写さない（innerText は描かれない文字を返さない）。md の文書の
  // CSS は本文（.markdown-body）の中にしか効かないので、本文の外へ置くと隠した文字まで写る。
  // Why not mirrorMarkdown と同じく .markdown-body の直下に置く: 親や祖先を辿るセレクタ
  // （`.note > span` の `>` や、`:scope > p` に組み直された `body > p`）が写しに当たらない。
  function textOf(el) {
    var t = tag(el);
    if (t === 'TEXTAREA' || t === 'INPUT') return el.value;
    var doc = ownerOf.call(el);
    var clone = cloneNode.call(el, true);
    querySelAll.call(clone, MdCommon.NOT_BODY_TEXT + ', .code-filename, ' + MEDIA)
      .forEach(function(n) { removeEl.call(n); });
    // svg などは innerText を持たない。
    var win = viewOf.call(doc);
    var parent = parentOf.call(el);
    if (!(win && clone instanceof win.HTMLElement) || !parent) return textContentOf.call(clone);
    setAttr.call(clone, 'aria-hidden', 'true');
    var cs = styleOf.call(clone);
    // 位置と幅だけ外から決める。表示（display や visibility）は文書の CSS に任せる。
    // 幅は描かれている幅（行内の要素は clientWidth が 0）。innerText の改行は幅に
    // よらないが、幅で切り替わる CSS（コンテナクエリなど）を元と揃える。
    cs.setProperty('position', 'fixed', 'important');
    cs.setProperty('left', '-99999px', 'important');
    cs.setProperty('top', '0', 'important');
    cs.setProperty('width', Math.max(rectOf.call(el).width, 1) + 'px', 'important');
    cs.setProperty('pointer-events', 'none', 'important');
    insertBefore.call(parent, clone, nextOf.call(el));
    try {
      return innerTextOf.call(clone);
    } finally {
      var p = parentOf.call(clone);
      if (p) removeChild.call(p, clone);
    }
  }

  function onCopy(btn, root, frame) {
    var id = getAttr.call(btn, 'data-md-copy') || '';
    // 文書のボタンの中身は書き換えない。止めたことを知らせる印を出さない方針なので、
    // 合図の文言が書き手が直すための唯一の手がかりになる。
    if (id && id.indexOf(COPY_ID_PREFIX) !== 0) {
      showSignal('data-md-copy の値は copy- で始めてください（' + id + '）');
      return;
    }
    var src = copySource(btn, root, frame);
    if (!src) {
      showSignal('コピー先が見つかりません（' + (id || '直前の <pre>') + '）');
      return;
    }
    var empty = function() {
      showSignal('コピーする文字がありません（' + (id || '直前の <pre>') + '）');
    };
    // 見えない制御の文字は copyText が落とす。合図には落とした後の文字が来る。
    var text = textOf(src).replace(/^\n+|\n+$/g, '');
    if (!text) { empty(); return; }
    MdCommon.copyText(text, signalCopied, function(why) {
      if (why === 'empty') empty();
      else showSignal('コピーに失敗しました');
    });
  }

  // ── 図の拡大 ─────────────────────────────────────────────────
  // 図は複製しない。mermaid や drawio の SVG を複製すると id が重なり、グラデーションや
  // 矢印が壊れる。要素そのものを重ねものの中へ移し、閉じたら元の場所（置き札）へ戻す。
  // 開いているかは、この変数で持つ（クラスで判定すると、文書が先にクラスを書いておくだけで
  // Esc の順番やキーの抑止が狂う）。
  //
  // 重ねものの置き場所: md は本文（#preview-pane 直下の .markdown-body）の中、html は
  // iframe の body の直下。
  // md で .markdown-body の中に置くのは、文書の CSS（`@scope (.markdown-body)` に組み直した
  // もの）とテーマの `.markdown-body …` の規則を、広げた図にも当て続けるため。外へ出すと
  // それが外れ、mermaid のラベルは字の大きさが変わって枠からはみ出して切れ、クラスで
  // 塗っていた svg は色を失う。
  // 引き換えに、文書の CSS が重ねものと手がかりにも当たる（!important を重ねても、文書の
  // 側がもっと強いセレクタの !important を書けば勝つ）。崩せるのは見た目だけで、置き場所は
  // 本文の枠の中のまま。枠は paint の封じ込め（common.js の confinePane）で外に描けない
  // ので、拡大を崩されてもサイドバーやタブにはかぶらない。
  // Why not 窓全体（親の body）を覆う: 本文の外へ文書の要素を出すことになる。
  // z-index は検索バー・⌘P・ヘルプ・右クリックメニュー・トーストより下（900）にして、
  // 拡大中に開いたアプリの重ねものが図の下に潜らないようにする。
  var zoomed = null;

  var SHEET =
    '.md-zoom-ov{box-sizing:border-box!important;margin:0!important;border:0!important;' +
    'padding:' + ZOOM_PAD + 'px!important;overflow:auto!important;display:flex!important;' +
    'z-index:900!important;cursor:zoom-out!important;outline:none!important;' +
    'transform:none!important;opacity:1!important;visibility:visible!important;' +
    // 図の端までスクロールしたあとのホイールで、裏（本文の枠や窓）を動かさない。
    'overscroll-behavior:contain!important;}' +
    // md: 枠の見えている範囲にぴったり置く（top・left・width・height は開くときに入れる）。
    '.md-zoom-ov.md-zoom-pane{position:absolute!important;right:auto!important;bottom:auto!important;}' +
    '.md-zoom-ov.md-zoom-frame{position:fixed!important;inset:0!important;width:auto!important;height:auto!important;}' +
    // 真ん中寄せは justify/align ではなく auto の margin で取る。前者だと、はみ出した図の
    // 上と左がスクロールで届かない所へ切れる。
    '.md-zoom-ov>*{margin:auto!important;flex:none!important;float:none!important;' +
    'position:static!important;transform:none!important;max-width:none!important;' +
    'max-height:none!important;cursor:zoom-in!important;}' +
    '.md-zoom-ov>figure{width:auto!important;height:auto!important;max-width:100%!important;}' +
    '.md-zoom-ov.md-zoom-actual>*{cursor:zoom-out!important;}' +
    // 画面に合わせても 1:1 でも大きさが変わらない図は、押しても切り替えない。
    '.md-zoom-ov.md-zoom-fixed>*{cursor:default!important;}' +
    '.md-zoom-ov.md-zoom-actual>figure{max-width:none!important;}' +
    // 操作の手がかり。文字は ::after の content で出すので、選択にもコピーにも入らない。
    // 押しても下（重ねもの）へ抜ける。
    '.md-zoom-hint{all:initial!important;position:absolute!important;z-index:901!important;' +
    'pointer-events:none!important;user-select:none!important;-webkit-user-select:none!important;' +
    'padding:3px 10px!important;border-radius:999px!important;background:rgba(31,35,40,.78)!important;' +
    'color:#f0f3f6!important;font:12px/1.5 -apple-system,BlinkMacSystemFont,sans-serif!important;' +
    'white-space:nowrap!important;}' +
    '.md-zoom-hint.md-zoom-frame{position:fixed!important;}' +
    '.md-zoom-hint::after{content:attr(data-hint)!important;}';

  // 拡大中の手がかりの文言。押したときに切り替わる先を出す。
  var HINT_FIT = 'Esc で閉じる · クリックで 1:1';
  var HINT_ACTUAL = 'Esc で閉じる · クリックで全体';
  var HINT_CLOSE = 'Esc で閉じる';
  // 画面に合わせた大きさと 1:1 の大きさがこの割合より近ければ、切り替えても見た目が
  // 変わらないので切り替えを出さない。
  var TOGGLE_MIN_DIFF = 0.05;

  // 文書ごとに 1 回だけ入れる（親は capturePane、iframe は bindDoc）。
  function injectSheet(doc, inPage) {
    var st = createEl.call(doc, 'style');
    // 広げても大きくならない図（data-md-zoom-off。覗いたときに付け外しする）には出さない。
    // 目印の無い図（mermaid・縮められた画像）は、覗いたときに付ける印（data-md-zoom-auto）で出す。
    st.textContent = SHEET + (inPage
      ? '#preview-pane .markdown-body [data-md-zoom]:not([data-md-zoom-off]),' +
        '#preview-pane .markdown-body pre.mermaid>svg[data-md-zoom-auto],' +
        '#preview-pane .markdown-body img[data-md-zoom-auto]{cursor:zoom-in;}'
      : '[data-md-zoom]:not([data-md-zoom-off]){cursor:zoom-in;}');
    appendTo.call(headOf.call(doc) || rootOf.call(doc), st);
  }

  function restoreStyle(el, style) {
    if (style === null) removeAttr.call(el, 'style');
    else setAttr.call(el, 'style', style);
  }

  // 寸法の属性の px の値。% や em など、それだけで大きさが決まらないものは NaN。
  function pxAttr(el, name) {
    var v = getAttr.call(el, name);
    var m = v && /^\s*([\d.]+)\s*(px)?\s*$/.exec(v);
    return m ? parseFloat(m[1]) : NaN;
  }

  // 広げる中身（画像と svg）の、もとの大きさ。開く前（本文の中にいるうち）に測る。
  //   w, h   … 1:1 で出す大きさ
  //   cur    … 本文の中で出ている倍率（1:1 に対して）。画面に合わせても、これより小さくしない
  //   vector … 画面に合わせるとき、もとより大きくしてよいか
  function measureMedia(m) {
    var r = rectOf.call(m);
    var cur = function(w) { return w > 0 && r.width > 0 ? r.width / w : 1; };
    if (tag(m) === 'IMG') {
      var src = getAttr.call(m, 'src') || '';
      var vector = /^data:image\/svg\+xml/i.test(src) || /\.svg(?:[?#]|$)/i.test(src);
      var nw = m.naturalWidth || r.width, nh = m.naturalHeight || r.height;
      return { el: m, w: nw, h: nh, cur: cur(nw), vector: vector, viewBox: false };
    }
    var vb = (getAttr.call(m, 'viewBox') || '').trim().split(/[\s,]+/).map(parseFloat);
    var hasVb = vb.length === 4 && vb[2] > 0 && vb[3] > 0;
    var w = pxAttr(m, 'width'), h = pxAttr(m, 'height');
    if (!(w > 0 && h > 0)) {
      // mermaid は width="100%" で、本来の大きさは viewBox にある。
      if (hasVb) { w = vb[2]; h = vb[3]; } else { w = r.width; h = r.height; }
    }
    // viewBox の無い svg は、箱を大きくしても中身が伸びない。開いている間だけ足す。
    return { el: m, w: w, h: h, cur: cur(w), vector: true, viewBox: !hasVb };
  }

  // 画面に合わせる倍率。縦と横の両方を収めると、本文の中より小さくなることがある（縦に
  // 長い mermaid は、枠の高さに収めると線も字も読めない大きさになる）。そのときは片方の辺
  // だけ合わせてもう片方はスクロールさせ、本文の中の大きさを下回らない方を採る（縦長なら
  // 幅に、横長なら高さに合う）。どちらでも下回るなら、本文の中の大きさのまま出す。
  // ラスター画像は実寸より大きくしない（ぼやけるだけ）が、本文の中で実寸より大きく
  // 出ていたなら、その大きさは保つ。
  function fitScale(m, W, H) {
    var cap = function(x) { return m.vector ? x : Math.min(x, 1); };
    var both = cap(Math.min(W / m.w, H / m.h));
    if (both >= m.cur) return both;
    var one = [cap(W / m.w), cap(H / m.h)]
      .filter(function(x) { return x >= m.cur; })
      .sort(function(a, b) { return a - b; });
    return one.length ? one[0] : m.cur;
  }

  // 広げて得るものがあるか。中身がラスター画像だけで、どれも本文の中で実寸以上に出て
  // いるなら、画面に合わせても（実寸より大きくしない）1:1 でも大きさは変わらない。
  // そういう図は押しても開かず、拡大のカーソルも出さない。
  // Why not 小さなラスターを image-rendering:pixelated で引き伸ばす: 写真やスクリーン
  // ショットの文字がギザギザに崩れるだけで、読みやすくはならない。アイコンのような絵を
  // 大きく見たいときは、書き手が SVG で置ける。
  function zoomOffers(el) {
    var media = mediaOf(el).map(measureMedia);
    if (!media.length) return true;
    return media.some(function(m) { return m.vector || !(m.w > 0) || m.cur < 0.99; });
  }

  function mediaOf(el) {
    var t = tag(el);
    if (t === 'IMG' || t === 'SVG') return [el];
    return Array.prototype.filter.call(querySelAll.call(el, 'img, svg'), function(m) {
      var outer = closestOf.call(parentOf.call(m), 'svg');
      return !(outer && contains.call(el, outer));
    });
  }

  function sizeMedia(z) {
    var W = Math.max(z.overlay.clientWidth - ZOOM_PAD * 2 - z.chromeW, 1);
    var H = Math.max(z.overlay.clientHeight - ZOOM_PAD * 2 - z.chromeH, 1);
    z.media.forEach(function(m) {
      if (!(m.w > 0 && m.h > 0)) return;
      var s = z.actual ? 1 : fitScale(m, W, H);
      var st = m.el.style;
      st.setProperty('width', (m.w * s) + 'px', 'important');
      st.setProperty('height', (m.h * s) + 'px', 'important');
      st.setProperty('max-width', 'none', 'important');
      st.setProperty('max-height', 'none', 'important');
    });
  }

  // 重ねものの地色。図の親から上へ、最初に色の付いた背景を採る。暗い配色で mermaid の
  // 線や文字が地の色に合わせて描かれているので、白で塗ると読めなくなる。
  function backgroundOf(win, start) {
    for (var n = start; n && n.nodeType === 1; n = parentOf.call(n)) {
      var c = win.getComputedStyle(n).backgroundColor;
      if (c && c !== 'transparent' && !/^rgba\(.*,\s*0\)$/.test(c)) return c;
    }
    return 'Canvas';
  }

  // 拡大中は裏をスクロールさせない（重ねものは開いた位置に置いたので、裏が動くとずれる）。
  // 触るのは overflow だけで、閉じたらもとのインラインの値へ戻す。
  function lockScroll(sc) {
    var s = sc.style;
    var saved = {
      el: sc,
      hadStyle: getAttr.call(sc, 'style') !== null,
      x: [s.getPropertyValue('overflow-x'), s.getPropertyPriority('overflow-x')],
      y: [s.getPropertyValue('overflow-y'), s.getPropertyPriority('overflow-y')]
    };
    s.setProperty('overflow', 'hidden', 'important');
    return saved;
  }

  function unlockScroll(saved) {
    var s = saved.el.style;
    [['overflow-x', saved.x], ['overflow-y', saved.y]].forEach(function(p) {
      if (p[1][0]) s.setProperty(p[0], p[1][0], p[1][1]);
      else s.removeProperty(p[0]);
    });
    if (!saved.hadStyle && !s.cssText) removeAttr.call(saved.el, 'style');
  }

  function frameDoc(frame) {
    try { return frame.contentDocument; } catch (e) { return null; }
  }

  // 開いた拡大がまだ生きているか。本文の差し替えには hydrate を通らないもの（すべて閉じる・
  // 設定タブ・読み込みの失敗の表示）や iframe の遷移もあるので、置き札と重ねものが
  // document に繋がっているかで見る。切れていたら状態を捨てる（素キーを止めたままにしない）。
  function alive() {
    var z = zoomed;
    if (!z) return false;
    if (connected.call(z.overlay) && connected.call(z.placeholder) &&
        (!z.frame || (connected.call(z.frame) && frameDoc(z.frame) === z.doc))) return true;
    closeZoom();
    return false;
  }

  function openZoom(el, frame) {
    if (zoomed) closeZoom();
    var doc = ownerOf.call(el);
    var win = viewOf.call(doc);
    var host = frame ? bodyOf.call(doc) : paneBody(el);
    if (!host || !win) return;
    var parent = parentOf.call(el);
    var cs = win.getComputedStyle(el);
    var rect = rectOf.call(el);
    var media = mediaOf(el).map(measureMedia);
    var chromeW = 0, chromeH = 0;
    if (tag(el) === 'FIGURE') {
      chromeW = ['paddingLeft', 'paddingRight', 'borderLeftWidth', 'borderRightWidth']
        .reduce(function(a, k) { return a + (parseFloat(cs[k]) || 0); }, 0);
      // 図の説明（figcaption）などのぶん。中身が 1 つのときだけ測れる。
      if (media.length === 1) chromeH = Math.max(0, rect.height - rectOf.call(media[0].el).height);
    }

    // 置き札は、図と同じ並び方（ブロックか行内か）と大きさで、本文がずれないようにする。
    var placeholder = createEl.call(doc, 'span');
    setAttr.call(placeholder, 'data-md-zoom-placeholder', '');
    var ps = placeholder.style;
    ps.setProperty('display', /^inline/.test(cs.display) ? 'inline-block' : 'block');
    ps.setProperty('box-sizing', 'border-box');
    ps.setProperty('width', rect.width + 'px');
    ps.setProperty('height', rect.height + 'px');
    ps.setProperty('margin', cs.marginTop + ' ' + cs.marginRight + ' ' + cs.marginBottom + ' ' + cs.marginLeft);
    ps.setProperty('float', cs.cssFloat || 'none');
    ps.setProperty('vertical-align', cs.verticalAlign);

    var overlay = createEl.call(doc, 'div');
    overlay.className = 'md-zoom-ov ' + (frame ? 'md-zoom-frame' : 'md-zoom-pane');
    setAttr.call(overlay, 'role', 'dialog');
    setAttr.call(overlay, 'tabindex', '-1');
    setAttr.call(overlay, 'aria-label', '図の拡大（Esc か背景のクリックで閉じる。図のクリックで 1:1 と切り替え）');
    var os = overlay.style;
    os.setProperty('background', backgroundOf(win, parent), 'important');

    var styles = [el].concat(media.map(function(m) { return m.el; }))
      .filter(function(n, i, a) { return a.indexOf(n) === i; })
      .map(function(n) { return { el: n, style: getAttr.call(n, 'style') }; });
    var prevFocus = activeOf.call(document);

    // 手がかりは重ねものの右上に、重ねものの兄弟として置く。中に入れると、1:1 で
    // 図をスクロールしたときに一緒に流れて見えなくなる。md は枠のスクロールを止めて
    // あるので、absolute で重ねものの上端に留まる。
    var hint = createEl.call(doc, 'span');
    hint.className = 'md-zoom-hint' + (frame ? ' md-zoom-frame' : '');
    setAttr.call(hint, 'aria-hidden', 'true');
    setAttr.call(hint, 'data-hint', HINT_FIT);
    hint.style.setProperty('top', '8px', 'important');
    hint.style.setProperty('right', '8px', 'important');

    insertBefore.call(parent, placeholder, el);
    appendTo.call(overlay, el);
    appendTo.call(host, overlay);
    appendTo.call(host, hint);
    var sc = frame ? (scrollerOf.call(doc) || rootOf.call(doc)) : pane;
    // 裏のスクロールを止めてから置く（止めるとスクロールバーが消えて、枠の幅が変わる）。
    var lock = lockScroll(sc);
    if (!frame) placeOnPane(overlay, hint);

    zoomed = {
      el: el, doc: doc, frame: frame, overlay: overlay, placeholder: placeholder, hint: hint,
      styles: styles, media: media, chromeW: chromeW, chromeH: chromeH, actual: false,
      lock: lock, prevFocus: prevFocus, observer: null, toggles: true, unwatch: null
    };
    media.forEach(function(m) {
      if (m.viewBox && m.w > 0 && m.h > 0) setAttr.call(m.el, 'viewBox', '0 0 ' + m.w + ' ' + m.h);
    });
    sizeMedia(zoomed);
    if (!togglesSize(zoomed)) {
      zoomed.toggles = false;
      overlay.classList.add('md-zoom-fixed');
      setAttr.call(hint, 'data-hint', HINT_CLOSE);
    }

    // 中身の差し替えで重ねものや置き札が外れたら、次のキー入力を待たずに畳む（裏の
    // スクロールの固定が残ると、差し替わった本文がホイールで動かない）。
    if (window.MutationObserver) {
      var mo = new MutationObserver(alive);
      mo.observe(host, { childList: true });
      if (parent !== host) mo.observe(parent, { childList: true });
      // md は .markdown-body ごと差し替わることがある（その中だけ見ていると気づかない）。
      if (!frame && pane && host !== pane) mo.observe(pane, { childList: true });
      zoomed.observer = mo;
    }

    zoomed.unwatch = watchScroll(frame ? [[doc, sc], [pane, pane]] : [[sc, sc]], overlay);

    listen.call(overlay, 'click', function(e) {
      if (!zoomed || zoomed.overlay !== overlay) return;
      var t = e.target;
      // 図の中のリンクは、リンクとして働かせる。開いたままだと飛んだ先が裏に隠れる。
      if (t && t.nodeType === 1 && closestOf.call(t, LINK)) { closeZoom(); return; }
      e.preventDefault();
      e.stopPropagation();
      if (t !== overlay && contains.call(zoomed.el, t)) { if (zoomed.toggles) toggleActual(); }
      else closeZoom();
    });
    // 1:1 で画面からはみ出した図を、矢印や Space で動かせるように。
    try { overlay.focus({ preventScroll: true }); } catch (e) {}
  }

  // 裏のスクロールが動いたら畳む。overflow を止めても、⌘F の検索の飛び先・目次・アンカー・
  // コメントへのジャンプはコードから scrollTop を動かすので、重ねもの（開いた位置に置いた
  // absolute）が画面の外へ流れ、素キーだけ止まったままになる。html の中の拡大は、iframe を
  // 載せた本文の枠が動いても iframe ごと流れるので、そちらも見る。
  // 開いた直後の位置と比べるのは、止めたときにスクロールバーが消えて位置が詰められた分を
  // 動いたと数えないため（置く間の測り直しで、その分はもう済んでいる）。
  // Why not 検索バーやパレットが開いたら畳む: それらは拡大より上に重なり、Esc で先に閉じる
  // 作り（registerOverlay の priority）。重なるだけなら位置はずれず、飛び先へ動けばここで畳む。
  function watchScroll(pairs, overlay) {
    var subs = pairs.filter(function(p) { return p[0] && p[1]; }).map(function(p) {
      var target = p[0], sc = p[1];
      var x = sc.scrollLeft, y = sc.scrollTop;
      var fn = function(e) {
        // 1:1 の重ねもの自身のスクロールなど、中の要素のスクロールは数えない。
        if (e.target !== target || !zoomed || zoomed.overlay !== overlay) return;
        if (Math.abs(sc.scrollLeft - x) > 1 || Math.abs(sc.scrollTop - y) > 1) closeZoom();
      };
      listen.call(target, 'scroll', fn);
      return function() { unlisten.call(target, 'scroll', fn); };
    });
    return function() { subs.forEach(function(f) { f(); }); };
  }

  // md の重ねものを置く本文。図から上へ辿って、枠の直下の .markdown-body を採る（文書が
  // 中に書いた .markdown-body ではなく、アプリが置いた方）。見つからなければ枠そのもの。
  function paneBody(el) {
    if (!pane) return null;
    for (var n = el; n && n !== pane; n = parentOf.call(n)) {
      if (parentOf.call(n) === pane) {
        return n.nodeType === 1 && n.classList.contains('markdown-body') ? n : pane;
      }
    }
    return pane;
  }

  // 重ねものを枠の見えている範囲（スクロールバーと枠線を除いた内側）へ合わせ、手がかりを
  // その右上に置く。absolute の基準は、ふつうは paint の封じ込めを持つ枠だが、文書の CSS が
  // .markdown-body に position や余白を足すと基準がずれる。値を決め打ちせず、置いてから
  // 測ってずれた分を足し引きする。
  function placeOnPane(overlay, hint) {
    var os = overlay.style;
    var pr = rectOf.call(pane);
    var x = pr.left + pane.clientLeft, y = pr.top + pane.clientTop;
    os.setProperty('left', '0px', 'important');
    os.setProperty('top', '0px', 'important');
    os.setProperty('width', pane.clientWidth + 'px', 'important');
    os.setProperty('height', pane.clientHeight + 'px', 'important');
    var r = rectOf.call(overlay);
    os.setProperty('left', (x - r.left) + 'px', 'important');
    os.setProperty('top', (y - r.top) + 'px', 'important');
    placeHint(overlay, hint);
  }

  function placeHint(overlay, hint) {
    var hs = hint.style;
    hs.setProperty('top', '0px', 'important');
    hs.setProperty('right', '0px', 'important');
    var o = rectOf.call(overlay), h = rectOf.call(hint);
    hs.setProperty('top', (o.top + 8 - h.top) + 'px', 'important');
    // right は基準の右端からの距離なので、右へずらすには減らす。
    hs.setProperty('right', (h.right - (o.right - 8)) + 'px', 'important');
  }

  // 画面に合わせた大きさと 1:1 の大きさが見て分かるほど違うか。実際に両方で描いて比べる
  // （図の中身が画像か svg か、figure の幅の決まり方かで、計算では揃えきれない）。
  function togglesSize(z) {
    var a = rectOf.call(z.el);
    z.actual = true;
    z.overlay.classList.add('md-zoom-actual');
    sizeMedia(z);
    var b = rectOf.call(z.el);
    z.actual = false;
    z.overlay.classList.remove('md-zoom-actual');
    sizeMedia(z);
    var near = function(p, q) { return Math.abs(p - q) <= TOGGLE_MIN_DIFF * Math.max(p, q, 1); };
    return !(near(a.width, b.width) && near(a.height, b.height));
  }

  function toggleActual() {
    var z = zoomed;
    z.actual = !z.actual;
    z.overlay.classList.toggle('md-zoom-actual', z.actual);
    setAttr.call(z.hint, 'data-hint', z.actual ? HINT_ACTUAL : HINT_FIT);
    // 文言の長さで手がかりの幅が変わる。md は右端を測り直す。
    if (!z.frame) placeHint(z.overlay, z.hint);
    sizeMedia(z);
    z.overlay.scrollTop = 0;
    z.overlay.scrollLeft = 0;
  }

  function closeZoom() {
    var z = zoomed;
    if (!z) return;
    zoomed = null;
    if (z.observer) z.observer.disconnect();
    if (z.unwatch) z.unwatch();
    z.media.forEach(function(m) { if (m.viewBox) removeAttr.call(m.el, 'viewBox'); });
    z.styles.forEach(function(s) { restoreStyle(s.el, s.style); });
    var home = parentOf.call(z.placeholder);
    if (home) replaceChild.call(home, z.el, z.placeholder);
    var op = parentOf.call(z.overlay);
    if (op) removeChild.call(op, z.overlay);
    var hp = parentOf.call(z.hint);
    if (hp) removeChild.call(hp, z.hint);
    unlockScroll(z.lock);
    // 重ねものに移っていたフォーカスを戻す。外へ移っていたら触らない。
    var a = activeOf.call(document);
    if (z.prevFocus && connected.call(z.prevFocus) && (!a || a === bodyOf.call(document) || (z.frame && a === z.frame))) {
      try { z.prevFocus.focus({ preventScroll: true }); } catch (e) {}
    }
  }

  if (window.MdCommon && MdCommon.registerOverlay) {
    // html の中で拡大していても、ここで開いていると答える。bindFrame はそのとき Esc を親へ
    // 渡し、c やスクロールの素キーを止める（? は渡る。ヘルプは拡大より前に出る）。
    // isOpen は毎回のキー入力で呼ばれるので、文書を探さず変数と isConnected で答える。
    // priority は検索バー（20）より下。拡大の上に ⌘F・⌘P・? で開いたものを、Esc で先に閉じる。
    MdCommon.registerOverlay({
      id: 'md-zoom',
      isOpen: alive,
      close: closeZoom,
      priority: 15
    });
  }

  // 置いた位置と大きさは開いたときの窓で決めたので、窓の大きさが変わったら畳む。
  listen.call(window, 'resize', function() { if (zoomed) closeZoom(); });

  // 目印（data-md-zoom）の無い、押せば広がるもの。md の本文だけ。
  //  - アプリが描いた mermaid の図（`pre.mermaid` の直下の svg）。mermaid は本文の幅に
  //    収めて描くので、大きな図ほど読めない
  //  - 本文の幅に収めるために縮められた画像（実寸より小さく出ている img）。実寸のまま
  //    出ている画像は広げても大きくならないので、今までどおり押しても開かない
  // 文書が目印を書いた要素は、こちらでは拾わない（目印の側の判定に任せる）。
  // Why not `.html` の中でも: 文書が自分の CSS で画像の大きさを決めている。縮めたのが
  // 文書の意図なら、押して広がるのは文書の作りと食い違う。md の本文の大きさはアプリが
  // 決めている（画像は `max-width:100%`）ので、書き手が `width` や style で決めたもの
  // （autoZoomOffers が外す）を除けば、縮められたのは幅が足りなかっただけ。
  function autoZoomTarget(t, root, frame) {
    if (frame) return null;
    var z = closestOf.call(t, 'pre.mermaid > svg, img');
    if (!z || !contains.call(root, z) || hasAttr.call(z, 'data-md-zoom')) return null;
    if (closestOf.call(z, '[data-md-zoom]')) return null;
    return z;
  }

  // 目印の無いものを広げる意味があるか。mermaid の svg はベクタなのでいつでも。画像は、幅が
  // 足りずに縮められているときだけ（zoomOffers は実寸より大きく出せる svg の画像も通すが、
  // ここでは「縮められた」だけを拾う。実寸で出ている小さなアイコンが、押すたびに広がらないように）。
  // 書き手が `width` 属性やインラインの style で大きさを決めた画像（Retina のスクリーンショットを
  // 半分の幅で置くなど）は、縮んでいても書き手の意図なので拾わない。目印を書けば広げられる。
  function autoZoomOffers(z) {
    if (tag(z) !== 'IMG') return zoomOffers(z);
    if (hasAttr.call(z, 'width') || hasAttr.call(z, 'height')) return false;
    var st = styleOf.call(z);
    if (st.getPropertyValue('width') || st.getPropertyValue('height')) return false;
    var m = measureMedia(z);
    return m.w > 0 && m.cur < 0.99;
  }

  // ── 配線 ─────────────────────────────────────────────────────
  function bindRoot(root, doc, frame) {
    listen.call(root, 'click', function(e) {
      // 文書の JS を動かす設定のとき、文書自身がこのクリックを処理していれば譲る（二重に
      // 開かないため）。
      if (e.defaultPrevented) return;
      if (window.MdComment && MdComment.isMode && MdComment.isMode()) return;
      var t = e.target;
      if (!t || t.nodeType !== 1 || !contains.call(root, t)) return;
      if (zoomed && contains.call(zoomed.overlay, t)) return;

      var btn = closestOf.call(t, '[data-md-copy]');
      if (btn && contains.call(root, btn)) {
        e.preventDefault();
        onCopy(btn, root, frame);
        return;
      }
      var z = closestOf.call(t, '[data-md-zoom]');
      if (z && contains.call(root, z) && ZOOM_TAGS[tag(z)]) {
        // drawio は元からクリックで全画面にするので、二重に開かない。
        if (closestOf.call(z, '.drawio-wrap') || closestOf.call(t, '.drawio-wrap')) return;
        // リンクで包んだ画像・図の中のリンクは、リンクとして押されたものとして扱う。
        if (closestOf.call(t, LINK)) return;
        if (!zoomOffers(z)) return;
        e.preventDefault();
        openZoom(z, frame);
        return;
      }
      var a = autoZoomTarget(t, root, frame);
      if (a) {
        if (closestOf.call(t, '.drawio-wrap') || closestOf.call(t, LINK)) return;
        if (!autoZoomOffers(a)) return;
        e.preventDefault();
        openZoom(a, frame);
      }
    });
    // 拡大のカーソルを出すかは、覗いたときに決める。大きさは窓や画像の読み込みで
    // 変わるので、印は毎回付け直す。
    listen.call(root, 'mouseover', function(e) {
      var t = e.target;
      if (!t || t.nodeType !== 1) return;
      var z = closestOf.call(t, '[data-md-zoom]');
      if (z && contains.call(root, z) && ZOOM_TAGS[tag(z)]) {
        if (zoomed && zoomed.el === z) return;
        if (zoomOffers(z)) removeAttr.call(z, 'data-md-zoom-off');
        else setAttr.call(z, 'data-md-zoom-off', '');
        return;
      }
      var a = autoZoomTarget(t, root, frame);
      if (!a || (zoomed && zoomed.el === a) || closestOf.call(t, '.drawio-wrap') || closestOf.call(t, LINK)) return;
      if (autoZoomOffers(a)) setAttr.call(a, 'data-md-zoom-auto', '');
      else removeAttr.call(a, 'data-md-zoom-auto');
    });
  }

  // common.js の bindFrame が .html-frame の中身に対して呼ぶ。
  function bindDoc(doc, frame) {
    if (!doc || boundDocs.has(doc)) return;
    boundDocs.add(doc);
    injectSheet(doc, false);
    bindRoot(doc, doc, frame);
    // 合図は親に出るが、html を読んでいる間はフォーカスが iframe の中にあり、Esc は
    // 親のオーバーレイが開いているときしか親へ渡らない（common.js の bindFrame）。
    listen.call(doc, 'keydown', onSignalKey);
    // iframe の中で遷移・再読み込みしたら、その文書の拡大は畳む。
    var win = viewOf.call(doc);
    if (win) listen.call(win, 'pagehide', function() { if (zoomed && zoomed.doc === doc) closeZoom(); });
  }

  // 本文が差し替わるとき（ファイル切替）に、拡大を畳む。図ごと置き札が消えるので、
  // 残すと前のファイルの図が重ねものに居残る。
  // 合図は閉じない。コードブロックの Copy の直後に本文が描き直される（ホットリロード・
  // ファイル切替）と、出したばかりの合図が読む前に消える。合図は本文の外にあり、写した
  // 中身は前のファイルのものでも嘘にはならない。
  function reset() { closeZoom(); }

  if (document.readyState === 'loading') {
    listen.call(document, 'DOMContentLoaded', capturePane);
  } else {
    capturePane();
  }

  window.MdParts = {
    bindDoc: bindDoc,
    reset: reset,
    // テスト用の覗き口。合図の要素そのものは渡さない（渡すと文書の JS から書き換えられる）。
    // 返す文字は写した中身で、文書がもともと持っているものなので、見せても漏れない。
    __test: {
      isZoomed: function() { return !!zoomed; },
      signalText: function() { return signal ? signal.box.textContent : ''; },
      signalRect: function() {
        if (!signal) return null;
        var r = signal.box.getBoundingClientRect();
        return { x: r.left, y: r.top, w: r.width, h: r.height };
      },
      // 合図の操作（'more' か 'close'）の位置と文言。出ていなければ null。
      signalAct: function(name) {
        if (!signal) return null;
        var a = signal.box.querySelector('[data-act="' + name + '"]');
        if (!a || a.hidden) return null;
        var r = a.getBoundingClientRect();
        return { x: r.left, y: r.top, w: r.width, h: r.height, label: a.getAttribute('data-label') };
      },
      signalFull: function() { return !!signal && signal.full; },
      // 合図の枠と本文の書字方向（direction / unicode-bidi の計算値）。
      signalBidi: function() {
        if (!signal) return null;
        var b = getComputedStyle(signal.box), t = getComputedStyle(signal.body);
        return { box: b.direction + ' ' + b.unicodeBidi, body: t.unicodeBidi };
      },
      hintText: function() { return zoomed ? getAttr.call(zoomed.hint, 'data-hint') : ''; },
      signalShown: function() {
        if (!signal || !signal.box.classList.contains('open')) return false;
        var r = signal.box.getBoundingClientRect();
        return getComputedStyle(signal.host).display !== 'none' && r.width > 0 && r.height > 0;
      }
    }
  };
})();
