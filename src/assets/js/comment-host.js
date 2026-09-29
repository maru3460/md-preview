// コメント機能が DOM に触る口。comment.js はコメントの意味論（行の算術・レンジ・
// 引用・巡回・表示の切替・非同期の着地）だけを持ち、document / 矩形 / リスナ /
// classList を触るところは全部ここを通す。
//
// 実装は 2 つ。本文 DOM を直に持つ DomHost（md のプレビュー・raw・非 md のソースビュー）と、
// iframe の中を錨る FrameHost（html のレンダリング表示）。
//
// 線引きの基準は「その関数は document / getBoundingClientRect / addEventListener /
// classList を触るか？」。コメントの意味論は 1 つも渡さない——渡すと両実装に同じ
// 分岐が生えて、直すときに 2 箇所を見ることになる。
(function() {
  // 塗りの状態と、DomHost が使う class。呼ぶ側は class 名を知らない——FrameHost は
  // 他人のページを相手にするので DOM を書き換えられず、同じ状態を ::highlight() と
  // 親のオーバーレイへ振り分ける。キーの並びが「塗りの状態」の定義でもある。
  var PAINT_CLASS = {
    marked: 'md-cmt-marked',        // コメントが付いているユニット
    kbcursor: 'md-cmt-kbcursor',    // キーボード・カーソル
    selecting: 'md-cmt-selecting',  // レンジ選択の範囲
    anchor: 'md-cmt-anchor',        // レンジの掴んだ側の端
    flash: 'md-cmt-flash'           // 着地の点滅・一覧ホバー
  };

  // place が本文へ挿し込む派生ノード。真実は comments[] が持つので、これらは
  // まとめて剥がして貼り直してよい。
  var PLACED_SEL = '.md-cmt-badge, .md-cmt-embed, .md-cmt-badge-holder';

  function createDomHost(o) {
    var getContainer = (o && o.getContainer) || null;
    var gutterTick = false;

    // ユニット走査の起点。単一 = .markdown-body / フォルダ = #preview-pane。
    function root() { return getContainer ? getContainer() : null; }

    // 行ユニットが 1 個も無い表示のとき、その理由を返す（錨れるなら null）。
    // トーストではなく居座るヒントで出す文言——「なぜ付けられないか」はモードに
    // 居る間ずっと効いている事情なので、1.5 秒で消えるものより常設の方が合う。
    function reason() {
      var r = root();
      if (r && r.querySelector('[data-src-line]')) return null;
      if (r) {
        // html（iframe）の分岐はここに置かない。本文に iframe が居れば pickHost が
        // 必ず FrameHost へ回すので、DomHost がその本文を見ることは無い。
        if (r.querySelector('.diff-source')) return 'git 差分にコメントはできません';
        if (r.querySelector('.source-main')) return '大きなファイルなので行コメントはできません';
      }
      // バイナリの案内・読み込み失敗・空の差分・中身の無い md など、上の 2 つに当たらない
      // 錨無しの本文がまだある。表示の種類を数え上げ切るのは無理なので、最後はここへ
      // 落として総称で言う——件数ベースの案内を出すと、また嘘になる。
      return 'この表示にはコメントできません';
    }

    function paint(el, state, on) {
      var cls = PAINT_CLASS[state];
      if (!el || !cls) return;
      el.classList.toggle(cls, !!on);
    }

    // 渡した状態の塗りを本文から落とす。状態を複数受けるのは、1 回の走査で
    // 済ませるため（レンジの解除は selecting と anchor が必ず対で外れる。
    // ドラッグ中の mousemove ごとに走るので、走査を 2 周に割らない）。
    function clearPaint() {
      var r = root();
      if (!r) return;
      var classes = [];
      for (var i = 0; i < arguments.length; i++) {
        var c = PAINT_CLASS[arguments[i]];
        if (c) classes.push(c);
      }
      if (!classes.length) return;
      var sel = classes.map(function(c) { return '.' + c; }).join(', ');
      r.querySelectorAll(sel).forEach(function(el) {
        classes.forEach(function(c) { el.classList.remove(c); });
      });
    }

    // place した派生ノードを全部剥がす。塗り（clearPaint）とは別の口にしてある
    // ——redraw は marked を塗り直すだけで、カーソルとレンジの枠は残したい。
    // 「host が描いたものを全部消す」という 1 つの口にすると、そこが分けられない。
    function clearPlaced() {
      var r = root();
      if (!r) return;
      r.querySelectorAll(PLACED_SEL).forEach(function(n) { n.remove(); });
      scheduleGutterSync();
    }

    // 派生ノード（💬 バッジ・インライン埋め込み）を本文へ置く。どこへ挿すかは
    // 本文の作りしだいなので、呼ぶ側は「このユニットに、この kind で」とだけ言う。
    function place(el, node, kind) {
      if (!el || !node) return;
      if (kind === 'embed') {
        // ユニットの「兄弟」として直後に差し込む（子に入れるとユニット自身の
        // レイアウトを壊す）。
        el.parentNode.insertBefore(node, el.nextSibling);
        scheduleGutterSync();
        return;
      }
      // ソースの行は、番号のセル（横スクローラの外のガター）へ載せる。行の中に
      // 置くとコードの先頭文字に重なる——番号が外へ出たぶん、行頭 = コードの頭。
      var cell = el.classList.contains('md-src-row') && MdCommon.srcGutterCell(el);
      if (cell) { cell.appendChild(node); return; }
      // mermaid はユニットの textContent がそのまま図のソースで、ホットリロード時は
      // バッジ貼り(reanchor・同期)の後に mermaid.run(非同期)が走るため、中に置くと
      // 「💬」が混ざって構文エラーになる。0 高さのホルダーを直前に挟んでそこへ載せる。
      if (el.classList.contains('mermaid')) {
        var holder = document.createElement('div');
        holder.className = 'md-cmt-badge-holder';
        holder.setAttribute('contenteditable', 'false');
        holder.appendChild(node);
        el.parentNode.insertBefore(holder, el);
        return;
      }
      el.appendChild(node);
    }

    // 親ビューポート座標に正規化した矩形。DomHost は本文が親と同じ座標系に居るので
    // そのまま返す（FrameHost は iframe の位置を足して返す）。ここが要石で、
    // in-view ハイライト・ポップオーバー配置・ホバープレビュー・「+」ハンドル・
    // カーソル初期化が全部この 1 本に集約される。
    function rectOf(el) {
      return el ? el.getBoundingClientRect() : null;
    }

    // 本文のスクロール主体を dy だけ動かす。埋め込みの出し入れで本文の高さが変わった
    // ぶんを打ち消すのに使う。主体は単一 = window、フォルダ = #preview-pane なので、
    // root からいちばん近いスクロール可能な祖先を探す（root 自身も含む）。
    function scrollBy(dy) {
      if (!dy) return;
      var el = root();
      while (el && el !== document.body && el !== document.documentElement) {
        var st = getComputedStyle(el);
        if ((st.overflowY === 'auto' || st.overflowY === 'scroll') && el.scrollHeight > el.clientHeight) {
          el.scrollBy(0, dy);
          return;
        }
        el = el.parentElement;
      }
      window.scrollBy(0, dy);
    }

    // ポインタ操作を親座標で通知する。unit（最寄りの行ユニット）と inBody（本文の上か）は
    // host が引く——FrameHost では iframe の contentDocument を見ることになるので、
    // 呼ぶ側がそこを知っていると両実装に同じ分岐が生える。
    function onPointer(h) {
      if (!h) return;
      bind('mousedown', h.down, false);
      // click だけ capture。本文のリンクやボタン自身のリスナへ届く前に止めるため。
      bind('click', h.click, true);
      bind('mousemove', h.move, false);
      bind('mouseup', h.up, false);
      bind('mouseover', h.over, false);
      bind('mouseout', h.out, false);
    }

    function bind(type, fn, capture) {
      if (typeof fn !== 'function') return;
      document.addEventListener(type, function(e) { fn(normalize(e)); }, capture);
    }

    // 生のイベントを親座標の形へ均す。unit と inBody を遅延で引くのは mousemove の
    // ため——モード外でもマウスを動かすだけで飛んでくるので、使わない回で closest を
    // 走らせない。
    function normalize(e) {
      var t = e.target;
      var unit;
      var inBody;
      return {
        target: t,
        relatedTarget: e.relatedTarget,
        x: e.clientX,
        y: e.clientY,
        button: e.button,
        get unit() {
          if (unit === undefined) unit = (t && t.closest) ? t.closest('[data-src-line]') : null;
          return unit;
        },
        get inBody() {
          if (inBody === undefined) {
            var r = root();
            inBody = !!(r && t && r.contains(t));
          }
          return inBody;
        },
        preventDefault: function() { e.preventDefault(); },
        stopPropagation: function() { e.stopPropagation(); }
      };
    }

    // モードの出入り。本文のカーソルと選択の扱いは親の `body.md-cmt-mode` に対する
    // CSS（base.css）が持っているので、ここですることは無い。
    function setMode() {}

    // ビューポートが動いた（スクロール・リサイズ）ことを伝える。フォルダのスクロール
    // 主体は #preview-pane なので、capture で拾って window / preview-pane 双方に効かせる。
    function onViewportChange(fn) {
      if (typeof fn !== 'function') return;
      window.addEventListener('resize', fn);
      document.addEventListener('scroll', fn, true);
    }

    // ソース表示では、挟まった派生ノードのぶん行番号のガターに隙間を空け直す。
    // 挿さった直後のノードはまだ高さを持たないので、1 フレーム待ってから測る。
    function scheduleGutterSync() {
      if (gutterTick) return;
      gutterTick = true;
      requestAnimationFrame(function() {
        gutterTick = false;
        var r = root();
        if (r) MdCommon.syncSrcGutter(r);
      });
    }

    return {
      root: root,
      reason: reason,
      rectOf: rectOf,
      scrollBy: scrollBy,
      paint: paint,
      clearPaint: clearPaint,
      clearPlaced: clearPlaced,
      place: place,
      onPointer: onPointer,
      onViewportChange: onViewportChange,
      setMode: setMode
    };
  }

  // ── FrameHost（html のレンダリング表示。本文は iframe の中） ──────────────
  //
  // DomHost との違いは 2 つだけで、どちらも「他人のページを預かっている」ことから来る。
  //
  //  ・塗りは DOM を書き換えず CSS Custom Highlight API で入れる。`<mark>` の挿入は
  //    テキストノードを割り兄弟を増やすので、相手の `:nth-child` / `p + p` を動かす。
  //  ・枠（カーソル・レンジの端・着地の点滅）は親のオーバーレイに重ねる。Highlight は
  //    色しか指定できないので枠を描けない。
  //
  // 前例は `search.js` の ⌘F。同一オリジンの iframe へ `adoptedStyleSheets` で
  // `::highlight()` の規則を入れて塗るところまで同じ手順を踏む。

  // ::highlight() の名前。文書内で一意ならよく、search.js（md-search-hit 系）とは別物。
  var HL_NAME = { marked: 'md-cmt-marked', selecting: 'md-cmt-selecting' };

  function createFrameHost(o) {
    var getContainer = (o && o.getContainer) || null;
    var onReady = (o && o.onReady) || null;
    var pointerHandlers = null;
    var viewportHandler = null;
    var modeOn = false;
    var placed = [];        // オーバーレイへ置いた派生ノードと、その錨
    var painted = {};       // state -> 塗って／指している要素
    var overlay = null;     // 枠を置く親のレイヤ
    var marks = {};         // state -> 枠の要素
    var pending = {};       // 次のフレームで applyNow する state
    var applyTick = false;
    var syncTick = false;

    function frameEl() {
      var c = getContainer ? getContainer() : null;
      return c ? c.querySelector('iframe.html-frame') : null;
    }
    // 外部サイトへ遷移した iframe は cross-origin で読めない。
    function doc() {
      var f = frameEl();
      try { return f ? f.contentDocument : null; } catch (e) { return null; }
    }
    function win() {
      var f = frameEl();
      try { return f ? f.contentWindow : null; } catch (e) { return null; }
    }

    function root() {
      var f = frameEl();
      if (f) bindFrameEl(f);
      var d = doc();
      if (!d) return null;
      bindDoc(d);
      return d.body;
    }

    function reason() {
      var f = frameEl();
      if (!f) return 'この表示にはコメントできません';
      bindFrameEl(f);
      var d = doc();
      if (!d || !d.body) return '外部ページにはコメントできません';
      bindDoc(d);
      if (navigatedAway(f, d)) return 'リンク先へ移っています（戻るとコメントできます）';
      if (d.querySelector('[data-src-line]')) return null;
      // 掴める行が無い html。1MB 超で刻まなかった（request.rs の STAMP_MAX_BYTES）ほか、
      // ブロック要素を 1 つも持たないページ（div だけで組んだ SPA、DOM を JS で作るページ）
      // もここへ来る。どちらかを名指しすると片方で嘘になるので総称で言う。
      return 'この HTML には行コメントを付けられません';
    }

    // 中身が動いたことを呼び出し側へも伝える。枠は自分で置き直せるが、ポップオーバーの
    // 追従・「+」の畳み・サイドバーの in-view ハイライトは comment.js が持っていて、
    // iframe の中のスクロールは親の document には届かない。
    function onViewportChange(fn) {
      viewportHandler = fn || null;
      bindDoc(doc());
    }

    function onFrameViewport() {
      syncMarks();
      if (viewportHandler) viewportHandler();
    }

    // 親ビューポート座標へ直した矩形。iframe の中の座標はその文書のビューポート基準なので、
    // iframe 自身の位置ぶんを足す（`autocopy.js` / `contextmenu.js` と同じ換算）。
    //
    // 返すのは親の画面座標なので、呼ぶ側が `bottom > 0` で「見えている」を判定すると、
    // iframe の上端（フォルダ表示では 42px）より上に隠れたぶんを見えている扱いにする。
    // md 側も `#preview-pane` の top ぶん同じ甘さを持っていて、ずれるのは 1 ユニット
    // 以内。本文の可視領域を契約に足すほどの実害が出てから直す。
    function rectOf(el) {
      var f = frameEl();
      if (!el || !f) return null;
      var r = el.getBoundingClientRect();
      var fr = f.getBoundingClientRect();
      return {
        left: r.left + fr.left, top: r.top + fr.top,
        right: r.right + fr.left, bottom: r.bottom + fr.top,
        width: r.width, height: r.height,
        x: r.left + fr.left, y: r.top + fr.top
      };
    }

    // html 表示は埋め込みカードを出さない（本文はサイドバー一覧で読む）ので、本文の高さが
    // 変わらない。補正すると逆にズレる。
    function scrollBy() {}

    // 💬 バッジは親のオーバーレイへ置く。他人のレイアウト（flex / grid）へ兄弟を挿すと
    // `:nth-child` や `p + p` が動いて崩れるので、本文には入れない。
    //
    // 埋め込みカード（GitHub 風の本文表示）は html では出さない。モードに入ると右サイド
    // バーが開いて iframe は 850 → 550 に縮んでおり、カードを置く余白がもう無い。本文の
    // 上に重ねれば本文が読めなくなる。html 表示に出るのは塗りとバッジだけで、コメントの
    // 本文はサイドバー一覧で読む（#23）。
    function place(el, node, kind) {
      if (!el || !node || kind === 'embed') return;
      ensureOverlay().appendChild(node);
      placed.push({ node: node, el: el });
      placeBadge(node, el);
    }

    function clearPlaced() {
      placed.forEach(function(p) { p.node.remove(); });
      placed = [];
    }

    // 右上へ寄せる。幅ではなく右端からの距離で置くのは、中身が 💬 と 💬N で変わって
    // 幅が読めないため（オーバーレイは画面いっぱいなので right がそのまま使える）。
    function placeBadge(node, el) {
      var box = clippedRect(el);
      if (!box) { node.hidden = true; return; }
      node.hidden = false;
      node.style.left = 'auto';
      node.style.right = Math.max(0, window.innerWidth - (box.left + box.width) + 2) + 'px';
      node.style.top = box.top + 'px';
    }

    // iframe の中のポインタは親の document には届かないので、文書ごとに張る
    // （右クリックメニューやキー転送と同じ「親へ委譲」の形）。文書は iframe 内遷移や
    // ファイル切替で差し替わるので、handlers を覚えておいて bindDoc から張り直す。
    function onPointer(h) {
      pointerHandlers = h || null;
      bindDoc(doc());
    }

    function bindPointer(d, type, fn, capture) {
      if (typeof fn !== 'function') return;
      d.addEventListener(type, function(e) { fn(normalizeIn(e)); }, capture);
    }

    // iframe の中のイベントを親座標へ均す。座標に iframe の位置を足すのは
    // `autocopy.js` / `contextmenu.js` と同じ換算。
    function normalizeIn(e) {
      var f = frameEl();
      var fr = f ? f.getBoundingClientRect() : { left: 0, top: 0 };
      var t = e.target;
      var unit;
      return {
        target: t,
        relatedTarget: e.relatedTarget,
        x: e.clientX + fr.left,
        y: e.clientY + fr.top,
        button: e.button,
        get unit() {
          if (unit === undefined) unit = (t && t.closest) ? t.closest('[data-src-line]') : null;
          return unit;
        },
        // iframe の中は丸ごと本文。どこで起きても「本文の上」でよい。
        inBody: true,
        preventDefault: function() { e.preventDefault(); },
        stopPropagation: function() { e.stopPropagation(); }
      };
    }

    function paint(el, state, on) {
      if (!el || !PAINT_CLASS[state]) return;
      var list = painted[state] || (painted[state] = []);
      var i = list.indexOf(el);
      if (on) {
        if (i >= 0) return;
        list.push(el);
      } else {
        if (i < 0) return;
        list.splice(i, 1);
      }
      schedule(state);
    }

    function clearPaint() {
      for (var i = 0; i < arguments.length; i++) {
        var state = arguments[i];
        if (!PAINT_CLASS[state] || !(painted[state] || []).length) continue;
        painted[state] = [];
        schedule(state);
      }
    }

    // 1 回の redraw は marked を要素ごとに塗るので、そのたびに Highlight を作り直すと
    // ユニットの数だけ組み直すことになる。次のフレームまで畳んで 1 回にする。
    function schedule(state) {
      pending[state] = true;
      if (applyTick) return;
      applyTick = true;
      requestAnimationFrame(function() {
        applyTick = false;
        var states = Object.keys(pending);
        pending = {};
        states.forEach(applyNow);
      });
    }

    function applyNow(state) {
      if (HL_NAME[state]) {
        applyHighlight(state);
      } else {
        applyMark(state);
      }
    }

    // ── 塗り（iframe の中の ::highlight()） ──

    // WebKit は Safari 17.2 以降。使えないフレームでは塗らない——`<mark>` を挿す方式へは
    // 落とさない（他人の文書のテキストノードを割ることになる）。`search.js` と同じ割り切り。
    function supportsHighlight(w) {
      try {
        return !!(w && w.CSS && w.CSS.highlights && typeof w.Highlight === 'function');
      } catch (e) {
        return false;
      }
    }

    // テーマ CSS は親にしか無いので、iframe へ持ち込む色を親から読む。プローブ要素を
    // 挿して getComputedStyle で測る手もあるが（`search.js` はそうしている）、読むのが
    // アクセント 1 色で済むならこちらが軽く、しかも**キャッシュが要らない**
    // ——毎回読むのでテーマを切り替えても次の塗りから追随する。
    function accentColor() {
      var v = getComputedStyle(document.body).getPropertyValue('--md-accent');
      return (v || '').trim() || 'Highlight';
    }

    // 混色の比率は base.css の .md-cmt-marked / .md-cmt-selecting と揃える（片方だけ
    // 変えると md と html で濃さが食い違う）。帯（inset box-shadow）は ::highlight() が
    // 色しか受け付けないので出せない。
    //
    // モード中はカーソルと選択の扱いも届ける。親の CSS は iframe の中まで届かないので、
    // これが無いと html 表示だけ「掴む画面に入った」合図が出ず、普段どおりの I ビームの
    // まま指すことになる。`html` に置いて継承させるのは md 側（`.markdown-body` へ 1 回
    // 指定）と同じ形で、ページが `a { cursor: pointer }` のように子で上書きしていれば
    // そちらが勝つ——他人のページの指定を `!important` で踏み潰さない。
    //
    // ここは文字列連結で CSS を組むが、差し込む値は自分たちのテーマ CSS が定義する
    // `--md-accent` だけ。他人のページから来た文字列を同じ経路に通すと、その時点で
    // iframe への任意 CSS 注入になる。増やすときは注意すること。
    function styleCss() {
      var a = accentColor();
      var css = '::highlight(' + HL_NAME.marked + '){background-color:color-mix(in srgb,' + a + ' 12%,transparent);}' +
                '::highlight(' + HL_NAME.selecting + '){background-color:color-mix(in srgb,' + a + ' 24%,transparent);}';
      if (modeOn) {
        css += 'html{cursor:crosshair;user-select:none;-webkit-user-select:none;}';
      }
      return css;
    }

    // モードの出入り。塗りが 1 件も無くてもカーソルは変わるので、`applyHighlight` 任せに
    // せずここから直に届ける（Highlight API が使えない環境でもカーソルは効く）。
    function setMode(on) {
      modeOn = !!on;
      var d = doc(), w = win();
      if (d && w) ensureSheet(d, w);
    }

    function sheetAdopted(d, sheet) {
      return Array.prototype.indexOf.call(d.adoptedStyleSheets || [], sheet) >= 0;
    }

    // 「差した覚えがあるか」ではなく「いま実際に adopt されているか」を見る。
    // adoptedStyleSheets は FrozenArray なので、ページ側は代入で丸ごと差し替える
    // （Lit 等がそう書く）。そのとき黙って外れるので、覚えているだけだと
    // 「塗ったつもりで色が一切付かない」という気付けない壊れ方をする。
    function ensureSheet(d, w) {
      var css = styleCss();
      // 中身が同じなら触らない。`reason()` から毎 redraw で通るので、ここで毎回
      // replaceSync するとその都度 CSS のパースが走る。
      if (d.__mdCmtCss === css && d.__mdCmtSheet && sheetAdopted(d, d.__mdCmtSheet)) return;
      d.__mdCmtCss = css;
      try {
        var kept = d.__mdCmtSheet;
        if (kept && sheetAdopted(d, kept)) { kept.replaceSync(css); return; }
        var sheet = kept || new w.CSSStyleSheet();
        sheet.replaceSync(css);
        d.adoptedStyleSheets = Array.prototype.slice.call(d.adoptedStyleSheets || []).concat([sheet]);
        d.__mdCmtSheet = sheet;
        return;
      } catch (e) { /* 構築済みスタイルシートが使えない環境 */ }
      try {
        var el = d.__mdCmtStyleEl;
        if (!el || !el.isConnected) {
          el = d.createElement('style');
          (d.head || d.documentElement).appendChild(el);
          d.__mdCmtStyleEl = el;
        }
        el.textContent = css;
      } catch (e2) { /* 色が付かないだけで、巡回とカーソルは動く */ }
    }

    function applyHighlight(state) {
      var d = doc(), w = win();
      if (!d || !w || !supportsHighlight(w)) return;
      try {
        var list = (painted[state] || []).filter(function(el) { return el.isConnected; });
        if (!list.length) { w.CSS.highlights.delete(HL_NAME[state]); return; }
        ensureSheet(d, w);
        var h = new w.Highlight();
        list.forEach(function(el) {
          var r = d.createRange();
          r.selectNodeContents(el);
          h.add(r);
        });
        // 選択中はコメント済みの上に出す（掴んでいる範囲が読めなくなるのを防ぐ）。
        h.priority = state === 'selecting' ? 1 : 0;
        w.CSS.highlights.set(HL_NAME[state], h);
      } catch (e) {
        // 文書の差し替え中くらいしか来ない。次の redraw でやり直せば直る。
      }
    }

    // ── 枠（親のオーバーレイ） ──

    function ensureOverlay() {
      if (overlay && overlay.isConnected) return overlay;
      overlay = document.createElement('div');
      overlay.className = 'md-cmt-frame-overlay';
      document.body.appendChild(overlay);
      return overlay;
    }

    function applyMark(state) {
      // 枠が指すのは常に 1 つ（カーソル・レンジの端・着地の点滅）。後から来たものを採る。
      var list = painted[state] || [];
      var el = list.length ? list[list.length - 1] : null;
      var mark = marks[state];
      if (!el) {
        if (mark) mark.hidden = true;
        return;
      }
      if (!mark || !mark.isConnected) {
        mark = document.createElement('div');
        mark.className = 'md-cmt-frame-mark';
        mark.dataset.state = state;
        ensureOverlay().appendChild(mark);
        marks[state] = mark;
      }
      placeMark(mark, el);
    }

    function placeMark(mark, el) {
      var box = clippedRect(el);
      if (!box) { mark.hidden = true; return; }
      mark.hidden = false;
      mark.style.left = box.left + 'px';
      mark.style.top = box.top + 'px';
      mark.style.width = box.width + 'px';
      mark.style.height = box.height + 'px';
    }

    // 要素の矩形を、祖先の overflow と iframe のビューポートで切った結果（親座標）。
    // 切らないと、内部スクロールする div の外へ出た枠が iframe の外まで漏れてタブバーの
    // 上に描かれる。親のレイヤはページのペイント順を知りようがないので、sticky ヘッダに
    // 隠れた要素までは救えない——そちらは塗り（Highlight）に任せる。
    function clippedRect(el) {
      var d = doc(), w = win(), f = frameEl();
      if (!d || !w || !f || !el.isConnected) return null;
      var r = el.getBoundingClientRect();
      var box = { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
      var p = el.parentElement;
      while (p && p !== d.documentElement) {
        var st = w.getComputedStyle(p);
        if (st.overflowX !== 'visible' || st.overflowY !== 'visible') {
          box = intersect(box, p.getBoundingClientRect());
          if (!box) return null;
        }
        p = p.parentElement;
      }
      var fr = f.getBoundingClientRect();
      box = intersect(box, { left: 0, top: 0, right: fr.width, bottom: fr.height });
      if (!box) return null;
      return {
        left: box.left + fr.left, top: box.top + fr.top,
        width: box.right - box.left, height: box.bottom - box.top
      };
    }

    function intersect(a, b) {
      var out = {
        left: Math.max(a.left, b.left),
        top: Math.max(a.top, b.top),
        right: Math.min(a.right, b.right),
        bottom: Math.min(a.bottom, b.bottom)
      };
      return (out.right > out.left && out.bottom > out.top) ? out : null;
    }

    // ページ自身の JS が別の html へ移ったか。タブは元のファイルを指したままなので、
    // ここで錨ると**遷移先の行番号を元のファイルの識別子で**保存してしまう。
    //
    // 本当は遷移先を `doc.URL` から識別子へ引き直して `file` を差し替えたいが、URL から
    // 識別子への変換（root 相対と `/__abs/` の出し分け）は配信側の規則で、いま JS 側に
    // 口が無い。**間違った場所に付けるより、付けられないほうがまし**なので、錨らない。
    //
    // 見るのは**パスだけ**。クエリとハッシュは、ページが `history.pushState` で
    // `?tab=2` のように書き換えることがあり（Document は作り直されないので刻んだ行は
    // そのまま生きている）、これを「移った」と数えると SPA で錨れなくなる。
    // 裏を返すと、pushState でパスごと書き換える作りのページでは錨れなくなる
    // ——そちらは「本当に別ファイルへ移った」と区別が付かないので、安全側に倒す。
    function navigatedAway(f, d) {
      var src = f.getAttribute('src');
      if (!src) return false;
      try {
        return new URL(src, d.URL).pathname !== new URL(d.URL).pathname;
      } catch (e) {
        return false;
      }
    }

    // iframe の中身は本文の差し替えより遅れて届く（直後はまだ about:blank）。届いた時点で
    // 呼び出し側へ塗り直しを促さないと、錨が 1 つも無い表示として案内が出たまま止まる。
    // 要素ごとに 1 回だけ張る（frame 要素は本文の差し替えで作り直されるので漏れない）。
    function bindFrameEl(f) {
      if (!f || f.__mdCmtWired) return;
      f.__mdCmtWired = true;
      f.addEventListener('load', function() { if (onReady) onReady(); });
    }

    // 枠は本文の上に「浮いている」だけなので、中身が動いたら置き直す。文書スクロールも
    // 内部スクロールコンテナも、frame doc の scroll を capture で 1 本拾えば両方届く。
    // フラグを用途ごとに分ける。1 つにまとめると、`onPointer` より先に `root()` /
    // `reason()` が呼ばれた文書へはポインタが二度と張られなくなり、配線の順序に
    // 依存する（いまは init が守っているだけで、契約としては約束していない）。
    function bindDoc(d) {
      if (!d) return;
      if (!d.__mdCmtViewport) {
        d.__mdCmtViewport = true;
        d.addEventListener('scroll', onFrameViewport, true);
        var w = win();
        if (w) w.addEventListener('resize', onFrameViewport);
      }
      // モード中に文書が差し替わった（ファイル切替・iframe 内遷移）なら、新しい文書にも
      // 規則を入れ直す。モードは文書より長生きする。
      if (modeOn) {
        var mw = win();
        if (mw) ensureSheet(d, mw);
      }
      if (!pointerHandlers || d.__mdCmtPointer) return;
      d.__mdCmtPointer = true;
      bindPointer(d, 'mousedown', pointerHandlers.down, false);
      // click だけ capture。ページ自身のリンク遷移へ届く前に止めるため。
      bindPointer(d, 'click', pointerHandlers.click, true);
      bindPointer(d, 'mousemove', pointerHandlers.move, false);
      bindPointer(d, 'mouseup', pointerHandlers.up, false);
      bindPointer(d, 'mouseover', pointerHandlers.over, false);
      bindPointer(d, 'mouseout', pointerHandlers.out, false);
    }

    function syncMarks() {
      if (syncTick) return;
      syncTick = true;
      requestAnimationFrame(function() {
        syncTick = false;
        Object.keys(marks).forEach(function(state) {
          var list = painted[state] || [];
          var el = list.length ? list[list.length - 1] : null;
          if (el) placeMark(marks[state], el);
          else marks[state].hidden = true;
        });
        placed.forEach(function(p) { placeBadge(p.node, p.el); });
      });
    }

    return {
      root: root,
      reason: reason,
      rectOf: rectOf,
      scrollBy: scrollBy,
      paint: paint,
      clearPaint: clearPaint,
      clearPlaced: clearPlaced,
      place: place,
      onPointer: onPointer,
      onViewportChange: onViewportChange,
      setMode: setMode
    };
  }

  window.MdCommentHost = { dom: createDomHost, frame: createFrameHost };
})();
