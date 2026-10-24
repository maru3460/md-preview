// 外部の読み込みを止めているとき（設定「文書を信頼する」が OFF）、.html の中の外部の
// 画像・iframe・動画・音声を、小さな「開く」リンクに置き換える。<head> に inline され
// window.MdExtMedia を公開する。common.js の bindFrame が、読み込み後の文書に bindDoc() を呼ぶ。
//
// 守っているのはヘッダの CSP で、ここは見た目のためだけ。止めた画像が壊れた画像の印に、
// iframe が空の枠になるのを、何があったか分かる札にする。md の本文は Rust が描くときに同じ
// ことをしている（extmedia.rs）。
//  - http(s) の URL は「開く」リンクに。`<a href>` の中（README のバッジ）では入れ子にせず、
//    文字だけのチップにして外側のリンクを活かす
//  - 手元のもの（文書と同じ origin）・data:・blob: はそのまま
//  - それ以外のスキーム（file:・vscode:・javascript: など）は置き換えずに消す
//  - `//host/…` は、文書が http(s) でなくても https の外部として扱う（書いた人の意図はウェブ）
// あちらと違うところ（わざと。URL の読み方を WebView に任せられるのがこちらだけなので）:
//  - about: は残す（about:blank の iframe はよくある。md の本文には出てこない）
//  - URL として読めないものは消す（WebView も読み込めない。Rust は自前の緩い読み方で分ける）
//  - 文書と同じ origin の http(s) は手元として残す（開発用サーバの `.html` は http で配る。
//    md の本文の手元の参照は Rust が `/…` のパスに畳むので、http(s) は常に外部）
//  - mdpreview: は文書と同じホストのときだけ手元（Rust は localhost のときだけ）
//
// Why not 違反のイベント（securitypolicyviolation）で拾う: 画像の違反は bindFrame が配線する
// より前に飛ぶことがあり、取りこぼす。読み込み後の DOM の src / srcset を見る。
//
// 文書の要素で名前がすり替わらないよう、document と要素のメソッドは prototype から呼ぶ
// （DOM clobbering）。文字は textContent で入れ、リンクは本物の
// `<a href>` にする（押したときの移動は、本文のほかのリンクと同じ判定を通る）。
(function() {
  // トップのフレームでなければ何もしない（理由は html.rs の PAGE_SCRIPTS）。
  if (window.top !== window) return;
  // 起動時に決めた CSP（page_globals が必ず定義する）。評価した時点で写しておく。
  var EXTERNAL_ALLOWED = window.MD_TRUSTED === true;

  var SVG_NS = 'http://www.w3.org/2000/svg';
  var XLINK_NS = 'http://www.w3.org/1999/xlink';
  var LABEL = { image: '外部の画像', video: '外部の動画', audio: '外部の音声', frame: '外部の埋め込み' };
  // .html の中はアプリの CSS が届かないので、base.css の .md-ext-media と同じ形を直に付ける。
  // 色は周りの文字の色（color: inherit）。文書のリンクの色は地の色と合っている保証が無い。
  // 書体を指定するのは、文書の書体が等幅や装飾体のこともあるため。
  var BOX_STYLE = 'display:inline-block;max-width:100%;color:inherit;' +
    'border:1px dashed color-mix(in srgb, currentColor 45%, transparent);border-radius:4px;' +
    'overflow-wrap:anywhere;vertical-align:middle;';
  var LINK_STYLE = BOX_STYLE + 'padding:1px 8px;font:0.85em/1.6 -apple-system,BlinkMacSystemFont,sans-serif;' +
    'text-decoration:underline;';
  var CHIP_STYLE = BOX_STYLE + 'padding:0 6px;font:0.8em/1.6 -apple-system,BlinkMacSystemFont,sans-serif;';

  function getter(proto, name) { return Object.getOwnPropertyDescriptor(proto, name).get; }
  var D = Document.prototype, E = Element.prototype, N = Node.prototype;
  var createEl = D.createElement;
  var createElNS = D.createElementNS;
  var docQuerySelAll = D.querySelectorAll;
  var urlOf = getter(D, 'URL');
  var localNameOf = getter(E, 'localName');
  var nsOf = getter(E, 'namespaceURI');
  var getAttr = E.getAttribute;
  var getAttrNS = E.getAttributeNS;
  var hasAttr = E.hasAttribute;
  var setAttr = E.setAttribute;
  var removeAttr = E.removeAttribute;
  var closestOf = E.closest;
  var querySelAll = E.querySelectorAll;
  var removeEl = E.remove;
  var appendTo = N.appendChild;
  var replaceChild = N.replaceChild;
  var parentOf = getter(N, 'parentNode');
  var baseOf = getter(N, 'baseURI');
  var connected = getter(N, 'isConnected');
  var setText = Object.getOwnPropertyDescriptor(N, 'textContent').set;
  var each = function(list, fn) { Array.prototype.forEach.call(list, fn); };

  // 置き換え済みの document。文書が書ける場所に印を置くと、同じ名前の要素で配線済みに見せかけられる。
  var doneDocs = new WeakSet();

  function isWeb(p) { return p === 'http:' || p === 'https:'; }

  // URL を分ける。{ kind: 'none' | 'local' | 'external' | 'blocked', url, host }。
  // base は解決の基準（文書の baseURI）、ownUrl は文書自身の URL（読めなければ null）。
  function classifyUrl(raw, base, ownUrl) {
    if (raw === null || raw === undefined) return { kind: 'none' };
    var u;
    try {
      // `//host/…` は、文書が mdpreview: だと mdpreview://host/… に解かれ、外部と分からない。
      // 前後の空白と途中のタブ・改行は URL の読み方でも落ちるので、落としてから見る。
      var cleaned = String(raw).replace(/^[\u0000-\u0020]+|[\u0000-\u0020]+$/g, '').replace(/[\t\n\r]/g, '');
      if (/^[\/\\]{2}/.test(cleaned) && !(ownUrl && isWeb(ownUrl.protocol))) u = new URL('https:' + cleaned);
      else u = new URL(raw, base);
    } catch (e) { return { kind: 'blocked' }; }
    var p = u.protocol;
    if (p === 'data:' || p === 'blob:' || p === 'about:') return { kind: 'local' };
    var web = isWeb(p);
    // 手元のものは文書と同じ住所（本番は mdpreview://localhost、開発用サーバは
    // http://127.0.0.1:<port>）。mdpreview: は特別なスキームではないので origin が "null" に
    // なり、比べられない。スキームとホストを並べて比べる。
    if (ownUrl && p === ownUrl.protocol && u.host === ownUrl.host) return { kind: 'local' };
    if (web && u.host) return { kind: 'external', url: u.href, host: u.host };
    return { kind: 'blocked' };
  }

  function classify(raw, doc, ownUrl) { return classifyUrl(raw, baseOf.call(doc), ownUrl); }

  // srcset の候補（url と、記述子込みの元の文字列）。extmedia.rs の srcset_candidates と同じ読み方。
  function srcsetCandidates(value) {
    var out = [], i = 0, n = value.length;
    var ws = function(c) { return c === ' ' || c === '\t' || c === '\n' || c === '\r' || c === '\f'; };
    while (i < n) {
      while (i < n && (ws(value[i]) || value[i] === ',')) i++;
      if (i >= n) break;
      var start = i;
      while (i < n && !ws(value[i])) i++;
      var url = value.slice(start, i);
      if (/,$/.test(url)) {
        url = url.replace(/,+$/, '');
        out.push({ url: url, text: url });
        continue;
      }
      while (i < n && value[i] !== ',') i++;
      out.push({ url: url, text: value.slice(start, i).trim() });
    }
    return out;
  }

  // srcset から手元の候補だけを残す。{ kept, external, dropped }。
  function filterSrcset(value, doc, ownUrl) {
    var kept = [], external = null, dropped = false;
    srcsetCandidates(value).forEach(function(c) {
      var r = classify(c.url, doc, ownUrl);
      if (r.kind === 'local' || r.kind === 'none') kept.push(c.text);
      else {
        dropped = true;
        if (r.kind === 'external' && !external) external = r;
      }
    });
    return { kept: kept.join(', '), external: external, dropped: dropped };
  }

  function caption(el) {
    var names = ['alt', 'title', 'aria-label'];
    for (var i = 0; i < names.length; i++) {
      var v = getAttr.call(el, names[i]);
      if (v && v.trim()) return v.trim();
    }
    return '';
  }

  function linkText(kind, host, cap) {
    return LABEL[kind] + ': ' + (cap ? cap + '（' + host + '）' : host);
  }

  // チップの文字。alt → title → URL の最後のパス（拡張子を落とす）→ ホスト名。
  function chipText(ext, cap) {
    if (cap) return cap;
    var last = '';
    try {
      var segs = new URL(ext.url).pathname.split('/').filter(Boolean);
      last = segs.length ? decodeURIComponent(segs[segs.length - 1]) : '';
    } catch (e) {}
    var dot = last.lastIndexOf('.');
    var stem = dot > 0 ? last.slice(0, dot) : last;
    return stem || ext.host;
  }

  // el を札に差し替える。svgPos があれば SVG の中なので、SVG の文字で出す。
  function replaceWith(doc, el, kind, ext, cap, svgPos) {
    var parent = parentOf.call(el);
    if (!parent) return;
    // `<a name>` などは押しても移動しないので、リンクの中に数えない。
    var inLink = !!closestOf.call(el, 'a[href]');
    var node;
    if (svgPos) {
      var text = createElNS.call(doc, SVG_NS, 'text');
      setAttr.call(text, 'x', svgPos.x);
      setAttr.call(text, 'y', svgPos.y);
      setAttr.call(text, 'dominant-baseline', 'hanging');
      setAttr.call(text, 'style', 'font:12px sans-serif;fill:currentColor;text-decoration:underline;');
      setText.call(text, linkText(kind, ext.host, cap));
      if (inLink) {
        node = text;
      } else {
        node = createElNS.call(doc, SVG_NS, 'a');
        setAttr.call(node, 'href', ext.url);
        appendTo.call(node, text);
      }
    } else if (inLink) {
      node = createEl.call(doc, 'span');
      setAttr.call(node, 'data-md-ext-chip', '');
      setAttr.call(node, 'title', ext.host);
      setAttr.call(node, 'style', CHIP_STYLE);
      setText.call(node, chipText(ext, cap));
    } else {
      node = createEl.call(doc, 'a');
      setAttr.call(node, 'data-md-ext-media', '');
      setAttr.call(node, 'href', ext.url);
      setAttr.call(node, 'title', ext.url);
      setAttr.call(node, 'style', LINK_STYLE);
      setText.call(node, linkText(kind, ext.host, cap));
    }
    replaceChild.call(parent, node, el);
  }

  function firstExternal(tags, doc, ownUrl) {
    for (var i = 0; i < tags.length; i++) {
      var r = classify(getAttr.call(tags[i], 'src'), doc, ownUrl);
      if (r.kind === 'external') return r;
      var set = getAttr.call(tags[i], 'srcset');
      if (set !== null) {
        var f = filterSrcset(set, doc, ownUrl);
        if (f.external) return f.external;
      }
    }
    return null;
  }

  function onImg(doc, ownUrl, img) {
    if (!connected.call(img)) return;
    var r = classify(getAttr.call(img, 'src'), doc, ownUrl);
    if (r.kind === 'external') return replaceWith(doc, img, 'image', r, caption(img));
    if (r.kind === 'blocked') return removeEl.call(img);
    var set = getAttr.call(img, 'srcset');
    if (set === null) return;
    var f = filterSrcset(set, doc, ownUrl);
    if (r.kind === 'none' && !f.kept) {
      if (f.external) replaceWith(doc, img, 'image', f.external, caption(img));
      else removeEl.call(img);
    } else if (f.dropped) {
      if (f.kept) setAttr.call(img, 'srcset', f.kept);
      else removeAttr.call(img, 'srcset');
    }
  }

  // picture・video・audio の中の source。外の候補を外し、残らなければ消す。
  function onSource(doc, ownUrl, s) {
    if (!connected.call(s)) return;
    var r = classify(getAttr.call(s, 'src'), doc, ownUrl);
    if (r.kind === 'external' || r.kind === 'blocked') return removeEl.call(s);
    var set = getAttr.call(s, 'srcset');
    if (set === null) return;
    var f = filterSrcset(set, doc, ownUrl);
    if (!f.kept) removeEl.call(s);
    else if (f.dropped) setAttr.call(s, 'srcset', f.kept);
  }

  // 中の source がいくつあっても 1 個に置き換える。手元の img が控えにあれば置き換えない
  // （外部の source は onSource が外し、手元の画像が出る）。
  function onPicture(doc, ownUrl, pic) {
    if (!connected.call(pic)) return;
    var img = querySelAll.call(pic, 'img')[0] || null;
    if (img && classify(getAttr.call(img, 'src'), doc, ownUrl).kind === 'local') return;
    var ext = firstExternal(querySelAll.call(pic, 'source, img'), doc, ownUrl);
    if (ext) replaceWith(doc, pic, 'image', ext, img ? caption(img) : '');
  }

  function onMedia(doc, ownUrl, m) {
    if (!connected.call(m)) return;
    var kind = localNameOf.call(m) === 'audio' ? 'audio' : 'video';
    var r = classify(getAttr.call(m, 'src'), doc, ownUrl);
    if (r.kind === 'external') return replaceWith(doc, m, kind, r, caption(m));
    if (r.kind === 'blocked') return removeEl.call(m);
    if (r.kind === 'none') {
      var sources = querySelAll.call(m, 'source');
      var anyLocal = Array.prototype.some.call(sources, function(s) {
        return classify(getAttr.call(s, 'src'), doc, ownUrl).kind === 'local';
      });
      if (sources.length && !anyLocal) {
        var ext = firstExternal(sources, doc, ownUrl);
        if (ext) replaceWith(doc, m, kind, ext, caption(m));
        else removeEl.call(m);
        return;
      }
    }
    // 本体は手元のもの。外の poster だけ外す。
    var poster = getAttr.call(m, 'poster');
    if (poster !== null && classify(poster, doc, ownUrl).kind !== 'local') removeAttr.call(m, 'poster');
  }

  function onFrame(doc, ownUrl, f) {
    if (!connected.call(f) || !hasAttr.call(f, 'src')) return;
    var r = classify(getAttr.call(f, 'src'), doc, ownUrl);
    if (r.kind === 'external') replaceWith(doc, f, 'frame', r, caption(f));
    else if (r.kind === 'blocked') removeEl.call(f);
  }

  // SVG の <image>。HTML の中に書いた <image> はパーサが <img> にするので、ここに来るのは SVG だけ。
  function onSvgImage(doc, ownUrl, im) {
    if (!connected.call(im) || nsOf.call(im) !== SVG_NS) return;
    var href = getAttr.call(im, 'href');
    if (href === null) href = getAttrNS.call(im, XLINK_NS, 'href');
    var r = classify(href, doc, ownUrl);
    if (r.kind === 'external') {
      replaceWith(doc, im, 'image', r, caption(im), {
        x: getAttr.call(im, 'x') || '0',
        y: getAttr.call(im, 'y') || '0'
      });
    } else if (r.kind === 'blocked') {
      removeEl.call(im);
    }
  }

  function bindDoc(doc) {
    if (EXTERNAL_ALLOWED || !doc || doneDocs.has(doc)) return;
    doneDocs.add(doc);
    var ownUrl = null;
    try { ownUrl = new URL(urlOf.call(doc)); } catch (e) {}
    var all = function(sel) { return docQuerySelAll.call(doc, sel); };
    // picture と video / audio を先に見る。中の source を丸ごと 1 個にまとめるため。
    each(all('picture'), function(el) { onPicture(doc, ownUrl, el); });
    each(all('video, audio'), function(el) { onMedia(doc, ownUrl, el); });
    each(all('img'), function(el) { onImg(doc, ownUrl, el); });
    each(all('source'), function(el) { onSource(doc, ownUrl, el); });
    each(all('iframe'), function(el) { onFrame(doc, ownUrl, el); });
    each(all('image'), function(el) { onSvgImage(doc, ownUrl, el); });
  }

  // classifyUrl はテストから URL の分け方を直に確かめるため（本番の文書の URL は
  // mdpreview: で、開発用サーバの上では作れない）。
  window.MdExtMedia = { bindDoc: bindDoc, classifyUrl: function(raw, docUrl) {
    var own = null;
    try { own = new URL(docUrl); } catch (e) {}
    return classifyUrl(raw, docUrl, own);
  } };
})();
