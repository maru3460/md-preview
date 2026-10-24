// md の本文を本文の枠の中に閉じ込める。<head> に inline され window.MdDocGuard を公開する。
//
// md の本文は innerHTML でアプリの UI（サイドバー・タブ）と同じ document に入るので、
// 文書の `<style>` や `<link>`・`<base>`・popover はそのままだと枠の外まで届く。
// 本文を入れる経路（folder.js のプレビュー・viewmode.js の raw / diff）は fill() で入れ、
// document に繋ぐ前に塞ぐ。繋いでから外すのでは遅いものがある（`<meta http-equiv=refresh>`
// は繋いだ時点で遷移を予約し、`<link>` は読み込みを飛ばし、`<base>` はその間のアプリの
// 相対 URL を変える）。hydrate も guard() を呼ぶので、fill() を通らない経路でも残りは塞がる。
// `.html` は iframe の中の別の document で描くので、ここでは中身に触らない。
//
// `<style>` は自前で字句解析せず、ブラウザの CSS パーサ（CSSStyleSheet.replaceSync）に
// 読ませた規則から組み直す。自前の解析はブラウザと食い違う（エスケープ・閉じ忘れ・
// コメントの中の `}`）と、普通の文書を壊すか、ずれを突いて枠の外へ出られる。
// 組み直した CSS はもう一度パーサに通し、`@scope` 1 つだけになっているかを確かめる。
//
// 文書の要素で名前がすり替わらないよう（`<form>` の中の `<input name=remove>` で
// `form.remove` が文書の要素になる。DOM clobbering）、要素のメソッドは prototype から呼ぶ。
(function() {
  // トップのフレームでなければ何もしない（理由は html.rs の PAGE_SCRIPTS）。
  if (window.top !== window) return;
  var SCOPE_ROOT = '.markdown-body';
  var XHTML = 'http://www.w3.org/1999/xhtml';

  var elProto = Element.prototype;
  var removeAttr = elProto.removeAttribute;
  var removeEl = elProto.remove;
  var qsa = elProto.querySelectorAll;
  var replaceKids = elProto.replaceChildren;
  var appendKids = elProto.append;
  var prependKids = elProto.prepend;
  var firstElGet = Object.getOwnPropertyDescriptor(elProto, 'firstElementChild').get;
  var textSet = Object.getOwnPropertyDescriptor(Node.prototype, 'textContent').set;
  var textGet = Object.getOwnPropertyDescriptor(Node.prototype, 'textContent').get;
  var nsGet = Object.getOwnPropertyDescriptor(elProto, 'namespaceURI').get;

  // CSSOM の型は読み込み時に取っておく。未実装の型を後から window で引くと、本文の
  // `<div id=CSSStartingStyleRule>` を拾う（window の名前付きアクセス）。
  var T = {};
  ['CSSStyleRule', 'CSSMediaRule', 'CSSSupportsRule', 'CSSContainerRule', 'CSSLayerBlockRule',
   'CSSLayerStatementRule', 'CSSStartingStyleRule', 'CSSScopeRule', 'CSSKeyframesRule',
   'CSSCounterStyleRule', 'CSSFontPaletteValuesRule', 'CSSNestedDeclarations', 'CSSStyleSheet'
  ].forEach(function(n) { if (typeof window[n] === 'function') T[n] = window[n]; });
  function is(rule, name) { return !!T[name] && rule instanceof T[name]; }

  // 中身を辿って残す規則（中の規則も同じ表で選ぶ）。
  var GROUPS = ['CSSMediaRule', 'CSSSupportsRule', 'CSSContainerRule', 'CSSLayerBlockRule',
                'CSSStartingStyleRule', 'CSSScopeRule', 'CSSStyleRule'];
  // そのまま残す規則。@keyframes と @counter-style は名前を document 全体に定義するが、
  // アプリの CSS はどちらも定義していないので、同じ名前でアプリの動きを乗っ取られることは
  // ない（効くのは文書の側だけ）。この前提は tests/docguard_premises.rs が見張っていて、
  // アプリの CSS に足すとそこで落ちる。
  // Why not 文書の名前に頭を付けて分ける: animation・animation-name・list-style・
  // list-style-type・counter の system の中の参照まで書き換えることになり、ショートハンドの
  // 中から名前を見分ける解析が要る。前提を見張る方が小さく済む。
  var LEAVES = ['CSSLayerStatementRule', 'CSSKeyframesRule', 'CSSCounterStyleRule',
                'CSSFontPaletteValuesRule', 'CSSNestedDeclarations'];
  // ここに無いものは落とす。@import は枠の外を塗れる別のシートを読み込む。@property は
  // `@scope` の中でも document 全体に効き、アプリのレイアウトが読む `--md-*` の初期値や
  // 継承を変えられる。@font-face も `@scope` の中から全体に効き、アプリが使う
  // ファミリ名（Menlo・-apple-system など）を別の字形に差し替えられる。@page・
  // @font-feature-values・@namespace と未知の at-rule も、許してよいと分かるまで落とす。
  function kept(rule) {
    return GROUPS.some(function(n) { return is(rule, n); }) ||
           LEAVES.some(function(n) { return is(rule, n); });
  }

  // `@scope` の中のセレクタには WebKit が暗黙の `:scope ` を前に足すので、枠の根を指す
  // 書き方（`:root` / `html` / `body` と、枠の根そのもののセレクタ）が何にも当たらなくなる。
  // 先頭に連なるそれらを `:scope` へ置き換える。外れても `@scope` の中なので枠の外には
  // 届かず、効かない規則が残るだけ。
  // 枠は `sel`（`@scope` の根）・`lead`（置き換える先頭）・`self`（根そのものの書き方。
  // これを名指しした規則は rootish にしない）の組で持つ。
  function makeRoot(sel) {
    var esc = sel.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    return { sel: sel, lead: new RegExp('^(?::root|html|body|' + esc + ')(?=$|[\\s>])', 'i'), self: sel.toLowerCase() };
  }
  var BODY_ROOT = makeRoot(SCOPE_ROOT);
  function rewriteSelector(sel) { return rewriteLead(sel, BODY_ROOT).sel; }
  // 書き換えた結果と、それが枠の根そのもの（`:scope` だけ）を指すようになったのが
  // `html` / `body` / `:root` からか（rootish）。根を名指ししたものは含めない。
  function rewriteLead(sel, root) {
    root = root || BODY_ROOT;
    var rest = sel.trim();
    var comb = null;
    var last = '';
    var m;
    while ((m = root.lead.exec(rest))) {
      last = m[0].toLowerCase();
      rest = rest.slice(m[0].length);
      var c = /^\s*(>?)\s*/.exec(rest);
      comb = c[1] ? ' > ' : ' ';
      rest = rest.slice(c[0].length);
    }
    if (comb === null) return { sel: sel, rootish: false };
    if (rest) return { sel: ':scope' + comb + rest, rootish: false };
    return { sel: ':scope', rootish: last !== root.self };
  }

  // `html` / `body` / `:root` を本文に置き換えた規則に残す宣言。ページ全体に書いたつもりの
  // 字・色・行の高さなどの受け継がれる性質と、変数（`--*`）と、背景（`background*`）。
  // 背景は受け継がれないが、字の色と組で書かれる（`body { background: #fff; color: #222 }`）。
  // 色だけ通すと、暗いテーマの背景の上に暗い字が載って読めなくなる。
  // Why not 全部残す: `body { margin: 0; max-width: none; padding: … }` のような
  // ページ全体の箱の指定が本文の列に当たり、テーマが組んだ本文の幅と余白が崩れる。
  // 本文の箱を変えたい書き手は `.markdown-body { … }` と名指しすれば、そちらは全部効く。
  var INHERITED = /^(?:--|background|font|color$|line-height$|letter-spacing$|word-spacing$|text-|white-space|direction$|quotes$|list-style|cursor$|visibility$|tab-size$|hyphens$|word-break$|overflow-wrap$|word-wrap$|line-break$|-webkit-(?:text-|font-|hyphens$|line-break$|locale$|nbsp-mode$))/i;
  function keepInherited(style) {
    var drop = [];
    for (var i = 0; i < style.length; i++) {
      if (!INHERITED.test(style[i])) drop.push(style[i]);
    }
    drop.forEach(function(n) { style.removeProperty(n); });
  }
  // セレクタの並びを最上位のカンマで割る。読むのはブラウザが正規化した selectorText。
  function splitSelectors(text) {
    var out = [];
    var depth = 0;
    var quote = '';
    var start = 0;
    for (var i = 0; i < text.length; i++) {
      var ch = text[i];
      if (ch === '\\') { i++; continue; }
      if (quote) { if (ch === quote) quote = ''; continue; }
      if (ch === '"' || ch === "'") quote = ch;
      else if (ch === '(' || ch === '[') depth++;
      else if (ch === ')' || ch === ']') depth--;
      else if (ch === ',' && depth === 0) { out.push(text.slice(start, i)); start = i + 1; }
    }
    out.push(text.slice(start));
    return out;
  }

  // 規則の並びから許さないものを抜き、本文全体を指すセレクタを書き換える。
  // `topLevel` は「セレクタが `@scope` の根を基準に読まれる位置か」。入れ子の style rule
  // （`&` 基準）と文書自身の `@scope` の中は書き換えない。
  // 入れ子の並び（@media などの中）は deleteRule / insertRule でその場で直す。最上位は
  // cleanTop が組み直す（シートの deleteRule は @namespace を消すときに投げる）。
  function cleanRules(list, owner, topLevel, root) {
    for (var i = list.length - 1; i >= 0; i--) {
      var rule = list[i];
      if (!kept(rule)) { owner.deleteRule(i); continue; }
      var extra = cleanRule(rule, topLevel, root);
      if (extra) owner.insertRule(extra, i + 1);
    }
  }

  // 1 つの規則を直す。`html, .x { … }` のように本文そのものを指す書き方と他が混ざって
  // いたら、本文そのものの分を受け継がれる性質だけの別の規則に分け、その cssText を返す
  // （呼ぶ側がすぐ後ろへ足す）。分けなければ null。
  function cleanRule(rule, topLevel, root) {
    if (is(rule, 'CSSStyleRule')) {
      var extra = null;
      if (topLevel) {
        var parts = splitSelectors(rule.selectorText).map(function(s) { return rewriteLead(s, root); });
        var rootish = parts.filter(function(p) { return p.rootish; });
        var others = parts.filter(function(p) { return !p.rootish; })
          .map(function(p) { return p.sel; });
        if (!rootish.length) {
          var next = others.join(', ');
          if (next !== rule.selectorText) rule.selectorText = next;
        } else if (!others.length) {
          rule.selectorText = ':scope';
          keepInherited(rule.style);
        } else {
          // 写しを作って本文そのものの分にし、元の規則は残りのセレクタだけにする。
          var copy = parse(rule.cssText).cssRules[0];
          copy.selectorText = ':scope';
          keepInherited(copy.style);
          if (copy.cssRules) cleanRules(copy.cssRules, copy, false, root);
          extra = copy.cssText;
          rule.selectorText = others.join(', ');
        }
      }
      if (rule.cssRules) cleanRules(rule.cssRules, rule, false, root);
      return extra;
    }
    if (GROUPS.some(function(n) { return is(rule, n); })) {
      cleanRules(rule.cssRules, rule, topLevel && !is(rule, 'CSSScopeRule'), root);
    }
    return null;
  }

  // 最上位の並びを直し、残す規則の cssText を順に返す。シートからは消さずに読み飛ばす。
  // `@namespace` はほかの規則があるとシートから消せない（deleteRule が投げる）ので、
  // 消しにいくと同じ `<style>` の残りまで道連れになる。
  function cleanTop(sheet, root) {
    var out = [];
    for (var i = 0; i < sheet.cssRules.length; i++) {
      var rule = sheet.cssRules[i];
      if (!kept(rule)) continue;
      var extra = cleanRule(rule, true, root);
      out.push(rule.cssText);
      if (extra) out.push(extra);
    }
    return out;
  }

  function parse(css) {
    var sheet = new T.CSSStyleSheet();
    sheet.replaceSync(css);
    return sheet;
  }

  // 文書の CSS を、枠（既定は本文）の中にだけ効く CSS にして返す。組めなければ空文字
  // （何も効かせない）。
  // `@scope` の無いエンジン（Safari 17.4 より前）では、文書の `<style>` も図の `<style>` も
  // 黙って空になる。枠の外へ漏らすより、見た目が素になる方を選んでいる。
  function scopeCss(css, root) {
    root = root || BODY_ROOT;
    if (!T.CSSStyleSheet || !T.CSSScopeRule) return '';
    try {
      var inner = cleanTop(parse(css), root);
      if (!inner.length) return '';
      var out = '@scope (' + root.sel + ') {\n' + inner.join('\n') + '\n}';
      // 組み直した文字列をもう一度読ませ、最上位が自分の `@scope` 1 つで、中身の数も
      // 変わっていないことを確かめる。直列化の癖で括弧がずれていれば、ここで外れる。
      var check = parse(out);
      var top = check.cssRules;
      if (top.length !== 1 || !is(top[0], 'CSSScopeRule') || top[0].cssRules.length !== inner.length) return '';
      return out;
    } catch (e) {
      return '';
    }
  }

  // 組み直した `<style>`。hydrate からもう一度呼ばれても二重に包まない。
  var scoped = new WeakSet();

  // 本文の外に効く要素と属性。`<title>` は HTML のものだけ（SVG の title はツールチップ）。
  var DROP_ELEMENTS = 'link, base, meta, title';
  // popover と invoker（command / commandfor・interestfor）はトップレイヤーに出るので、
  // `contain: paint` の枠を無視してアプリの UI の上にかぶさる。
  var DROP_ATTRS = ['popover', 'popovertarget', 'popovertargetaction', 'command', 'commandfor', 'interestfor'];
  // document の名前付きプロパティになる要素（`<img name=getElementById>` で
  // `document.getElementById` が文書の要素になる）。本文のアンカーは id で足り、
  // プレビューで form や iframe の name が要る場面は無い。
  var NAMED = 'img[name], form[name], embed[name], object[name], iframe[name], frame[name]';

  function guard(root) {
    if (!root) return;
    qsa.call(root, DROP_ELEMENTS).forEach(function(el) {
      if (el.localName === 'title' && nsGet.call(el) !== XHTML) return;
      removeEl.call(el);
    });
    qsa.call(root, DROP_ATTRS.map(function(a) { return '[' + a + ']'; }).join(', ')).forEach(function(el) {
      DROP_ATTRS.forEach(function(a) { removeAttr.call(el, a); });
    });
    qsa.call(root, NAMED).forEach(function(el) { removeAttr.call(el, 'name'); });
    // `<object id>` も document の名前付きプロパティになる。
    qsa.call(root, 'object[id]').forEach(function(el) { removeAttr.call(el, 'id'); });
    qsa.call(root, 'style').forEach(function(el) {
      if (scoped.has(el)) return;
      scoped.add(el);
      textSet.call(el, scopeCss(textGet.call(el) || ''));
    });
  }

  // 本文の外から入ってきた `<style>`（mermaid が描いた図の SVG の中のものなど）を、
  // `scopeSelector` の要素の中にだけ効くように組み直す。図の `<style>` は本文の組み直しより
  // 後に描かれ、`classDef` / `style` の文字列が `&` を通じて図の外を指す規則になりうる。
  // 根そのもの（`#mermaid-1 .node` の `#mermaid-1`）は本文と同じく `:scope` へ置き換えるので、
  // 図の側の規則はそのまま効く。`scopeSelector` は CSS として正しい 1 つのセレクタを渡す。
  function scopeStyleTo(el, scopeSelector) {
    if (!el || scoped.has(el)) return;
    scoped.add(el);
    var root = null;
    try {
      // ブラウザが直列化した形にそろえる（規則の selectorText と突き合わせるため）。
      root = makeRoot(parse(scopeSelector + ' {}').cssRules[0].selectorText);
    } catch (e) {}
    textSet.call(el, root ? scopeCss(textGet.call(el) || '', root) : '');
  }

  // 本文の HTML を el の中身にする。繋がっていない要素で解析して塞いでから入れ替えるので、
  // 文書の `<style>` や `<meta>` は一度も document に繋がらない。解析は el.innerHTML と
  // 同じ文脈（同じ document の div の中）になる。
  //
  // サーバの応答は `<div class="markdown-body …">本文</div>` の 1 要素。本文に余った
  // `</div>` があると包みがそこで閉じ、続きの本文が包みの兄弟として el の直下に並ぶ
  // （`.markdown-body.html-page > iframe.html-frame` を el の直下に作れてしまう）。
  // 先頭の要素を包みとみなし、それ以外のノードはすべて包みの中へ移して、el の直下を
  // 常に 1 要素にする。
  // Why not 余りを捨てる: 閉じ忘れや書き損じの文書でも、本文の続きが消えずに読める方を取る。
  function fill(el, html) {
    var box = document.createElement('div');
    box.innerHTML = html;
    guard(box);
    var nodes = Array.prototype.slice.call(box.childNodes);
    var wrap = firstElGet.call(box);
    if (wrap) {
      var at = nodes.indexOf(wrap);
      var before = nodes.slice(0, at), after = nodes.slice(at + 1);
      // 包みの前に来るのは（応答の形からして）空白くらいだが、落とさずに先頭へ入れる。
      if (before.length) prependKids.apply(wrap, before);
      if (after.length) appendKids.apply(wrap, after);
      nodes = [wrap];
    }
    replaceKids.apply(el, nodes);
  }

  window.MdDocGuard = {
    fill: fill,
    guard: guard,
    scopeStyleTo: scopeStyleTo,
    // 外す属性の一覧（図の入れ物を見張る側が、変わったら掃除する属性として使う）。
    DROP_ATTRS: DROP_ATTRS.slice(),
    __test: { scopeCss: scopeCss, rewriteSelector: rewriteSelector, rewriteLead: rewriteLead }
  };
})();
