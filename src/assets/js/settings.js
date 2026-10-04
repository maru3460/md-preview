// 設定タブ（#38）。⌘, とタブバー右端の歯車で開く、ファイルではないタブ。
//
// **設定を持っているのは Rust 側**（`~/.config/md-preview/settings`）。ここは
// 渡された写しを描いて、変えたいことを `settings:` で頼むだけ。タブを前に出す
// たびに `settings:get` で取り直すので、`md theme` や手で書き換えたぶんもそこで拾う。
//
// 保存ボタンは無い。変えた時点で Rust が保存し、テーマなら塗り直しを送ってくる
// （`applyTheme`。`md theme` を打ったときも同じ口に来る）。
//
// タブとしての出入りは folder.js の `loadPreview` が持つ。設定タブを開くときは
// 「何も開いていない状態」にしてから、本文ペインにここが描く——raw / diff・
// ホットリロード・コメントの付け先・パスのコピーは、どれもファイルを前提に
// しているので、ファイルが無い経路に乗せて素通りさせる。
//
// Why not 別の窓: md-preview は窓 1 枚にタブを並べる形で、設定もその 1 枚に乗せる。
// 窓を増やすと、隠す・全画面・Space の扱いを設定の窓にも決め直すことになる。
(function() {
  // タブの識別子。ファイルの識別子は必ず `/` で始まる（#33）ので、ぶつからない。
  var TAB_ID = 'md:settings';

  // Rust から渡された写し。
  // { theme, defaultTheme, autoDarkBg, themes:[{name, group, swatch}], windowSize, defaultWindowSize }
  var state = null;
  var pane = 'theme';
  var host = null;    // 描いている先（本文ペイン）
  var opts = null;    // { openFile(id) }
  var gearEl = null;

  var PANES = [
    { key: 'theme', title: 'テーマ',
      icon: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><circle cx="8" cy="8" r="6"/><path d="M8 2a6 6 0 0 1 0 12z" fill="currentColor"/></svg>' },
    { key: 'window', title: 'ウィンドウ',
      icon: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="1.8" y="2.8" width="12.4" height="10.4" rx="1.6"/><path d="M1.8 5.8h12.4"/></svg>' }
  ];

  var GROUPS = [
    { key: 'light', label: 'ライト' },
    { key: 'dark', label: 'ダーク' },
    { key: 'auto', label: 'OS の設定に追従' },
    { key: 'user', label: 'ユーザー' }
  ];


  function send(verb) {
    if (window.ipc) window.ipc.postMessage('settings:' + verb);
  }

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  // ── 全体 ────────────────────────────────────────────────────
  // 描いている先。設定タブが前に出ている間だけ在る（別のタブへ移ると、本文ペインの
  // 中身ごと folder.js が差し替えるので、ここに残るのは外れた要素になる）。
  function mounted() {
    return host && host.isConnected && host.querySelector('.st-layout') ? host : null;
  }

  function render() {
    var root = mounted();
    if (!root) return;
    // 入力欄に打ちかけの値があるとき、描き直しで消さない（届いた写しより
    // 打っている途中の方が新しい）。読み位置は本文ペイン（スクローラ）が持つので
    // 中身を差し替えても動かない。
    var active = document.activeElement;
    var typing = active && active.tagName === 'INPUT' && root.contains(active)
      ? { id: active.id, value: active.value, start: active.selectionStart, end: active.selectionEnd }
      : null;

    var layout = el('div', 'st-layout');
    layout.appendChild(renderNav());
    var p = el('div', 'st-pane');
    if (state) {
      if (pane === 'theme') renderTheme(p);
      else renderWindow(p);
    }
    layout.appendChild(p);
    root.innerHTML = '';
    root.appendChild(layout);

    if (typing && typing.id) {
      var again = document.getElementById(typing.id);
      if (again) {
        again.value = typing.value;
        again.focus();
        // カーソルの位置も戻す。戻さないと末尾へ飛び、続けて打った数字が後ろに付く。
        again.setSelectionRange(typing.start, typing.end);
      }
    }
  }

  function renderNav() {
    var nav = el('nav', 'st-nav');
    PANES.forEach(function(item) {
      var b = el('button', item.key === pane ? 'is-on' : '');
      b.type = 'button';
      b.dataset.pane = item.key;
      b.innerHTML = item.icon;
      b.appendChild(el('span', null, item.title));
      b.addEventListener('click', function() {
        if (pane === item.key) return;
        pane = item.key;
        render();
        // ペインを替えたら先頭から見せる。
        if (host) host.scrollTop = 0;
      });
      nav.appendChild(b);
    });
    return nav;
  }

  // ── テーマ ──────────────────────────────────────────────────
  function themeKnown() {
    return state.themes.some(function(t) { return t.name === state.theme; });
  }

  function renderTheme(p) {
    // 見出し要素にしない。⌘T のアウトラインは本文ペインの h1〜h6 を拾うので、
    // h2 にすると設定の項目名が本文の見出しとして並ぶ。
    p.appendChild(el('div', 'st-title', 'テーマ'));
    var item = el('div', 'st-item');
    item.appendChild(el('p', 'st-desc', '押すとすぐに窓全体が切り替わります。'));
    if (!themeKnown()) {
      // 手で書いた名前・消したユーザーテーマ。実際に塗っているのは既定のテーマ。
      item.appendChild(el('p', 'st-note',
        '設定のテーマ「' + state.theme + '」が見つからないので、' + state.defaultTheme + ' で表示しています。'));
    }
    var shown = themeKnown() ? state.theme : state.defaultTheme;

    GROUPS.forEach(function(g) {
      var list = state.themes.filter(function(t) { return t.group === g.key; });
      if (!list.length) return;
      item.appendChild(el('div', 'st-group', g.label));
      var grid = el('div', 'st-cards');
      list.forEach(function(t) { grid.appendChild(card(t, t.name === shown)); });
      item.appendChild(grid);
    });


    p.appendChild(item);
  }

  function card(t, on) {
    var b = el('button', 'st-card' + (on ? ' is-on' : ''));
    b.type = 'button';
    b.dataset.theme = t.name;
    b.title = t.name;
    b.appendChild(swatch(t));
    b.appendChild(el('span', 'chk', '✓'));
    b.appendChild(el('div', 'name', t.name));
    b.addEventListener('click', function() { pickTheme(t.name); });
    return b;
  }

  // 見本。色の出所は theme.rs の swatch（[bg, fg, accent, accent2, accent3]）。
  function swatch(t) {
    var sw = el('div', 'st-sw');
    if (!t.swatch) {
      sw.className += ' is-unknown';
      sw.textContent = '見本なし';
      return sw;
    }
    var c = t.swatch;
    // OS 追従のテーマは明暗どちらにもなるので、地を斜めに割って両方見せる。
    // 暗い側は窓の下地と同じ色（Rust の theme.rs が持っている）。
    sw.style.background = t.group === 'auto'
      ? 'linear-gradient(135deg, ' + c[0] + ' 50%, ' + state.autoDarkBg + ' 50%)'
      : c[0];
    var aa = el('span', 'aa', 'Aa');
    aa.style.color = c[1];
    sw.appendChild(aa);
    var dots = el('span', 'dots');
    [c[2], c[3], c[4]].forEach(function(color) {
      var i = el('i');
      i.style.background = color;
      dots.appendChild(i);
    });
    sw.appendChild(dots);
    return sw;
  }

  function pickTheme(name) {
    if (name === state.theme) return;
    // 写しは先に書き換える。Rust は切り替えの返事に状態を返さない（返すと続けて
    // 押したときに 1 つ前へ引き戻される）ので、ここが書かないと ✓ が動かない。
    state.theme = name;
    render();
    send('theme:' + name);
  }

  // ── ウィンドウ ──────────────────────────────────────────────
  function renderWindow(p) {
    p.appendChild(el('div', 'st-title', 'ウィンドウ'));

    var item = el('div', 'st-item');
    item.appendChild(el('span', 'st-label', '初期サイズ'));

    var size = state.windowSize || state.defaultWindowSize;
    var dims = el('div', 'st-dims');
    dims.appendChild(numField('st-width', '幅', size.w));
    dims.appendChild(el('div', 'st-times', '×'));
    dims.appendChild(numField('st-height', '高さ', size.h));
    var use = el('button', 'st-btn', 'いまの大きさに設定する');
    use.type = 'button';
    use.addEventListener('click', useCurrentSize);
    dims.appendChild(use);
    item.appendChild(dims);

    var d = state.defaultWindowSize;
    item.appendChild(resetLine(d.w + ' × ' + d.h, function() {
      settle();
      send('window-size:reset');
    }));

    // ⌘W で閉じた窓は大きさごと隠して出し直す（#49）ので、効くのはプロセスが
    // 立ち上がり直したときだけ。
    item.appendChild(el('div', 'st-when', 'プロセスを終了した後から効きます。'));
    p.appendChild(item);
  }

  // 窓の中身の大きさ。webview は窓の中身いっぱいなので、窓を作るときに渡す大きさと
  // 同じものになる（Rust で測らない理由は main.rs の SettingsRequest::WindowSize）。
  function useCurrentSize() {
    settle();
    send('window-size:' + Math.round(window.innerWidth) + 'x' + Math.round(window.innerHeight));
  }


  // 「既定に戻す」。いつも出す——既定のときだけ消すと、「いまの大きさに設定する」で
  // たまたま既定と同じ値になったときに出たり消えたりして落ち着かない。
  // 戻し先の値は文に書かず、ツールチップに置く。
  function resetLine(shipped, onReset) {
    var line = el('div', 'st-reset');
    var back = el('button', 'st-link', '既定に戻す');
    back.type = 'button';
    back.title = '既定は ' + shipped;
    back.addEventListener('click', onReset);
    line.appendChild(back);
    return line;
  }

  // 入力欄からフォーカスを外す。打ちかけの欄は描き直しで守る（render）ので、
  // ボタンで値を入れ替えるときは先に外しておかないと、届いた値が欄に入らない。
  // 打ちかけに変更があれば、外した時点の `change` で先に送られる。
  //
  // 外したフォーカスは本文ペインへ渡す。body に落とすと、キーの行き先が見えなくなる。
  function settle() {
    var a = document.activeElement;
    if (!(a && a.tagName === 'INPUT' && host && host.contains(a))) return;
    a.blur();
    host.focus({ preventScroll: true });
  }

  // 打ちかけの初期サイズを確定させる。設定タブを離れるとき（別のタブへ移る・転送で
  // 開く）に folder.js が呼ぶ。
  //
  // Why not `change` に任せる: タブは mousedown で preventDefault するので、タブを
  // 押しても欄からフォーカスが外れず、`change` が起きないまま本文が差し替わる。
  // 保存ボタンの無い画面で、打った値が黙って消えることになる。
  function flush() {
    if (!mounted() || !state) return;
    var w = document.getElementById('st-width');
    var h = document.getElementById('st-height');
    if (!w || !h) return;
    var saved = state.windowSize || state.defaultWindowSize;
    if (w.value !== String(saved.w) || h.value !== String(saved.h)) commitSize();
  }

  // 半角の数字だけにする。全角の数字（０〜９）は半角へ直し、それ以外は捨てる。
  function digitsOnly(s) {
    return s.replace(/[０-９]/g, function(c) {
      return String.fromCharCode(c.charCodeAt(0) - 0xFEE0);
    }).replace(/[^0-9]/g, '');
  }

  function numField(id, label, value) {
    var f = el('div', 'st-field');
    var l = el('label', null, label);
    l.htmlFor = id;
    f.appendChild(l);
    var box = el('div', 'st-num');
    var input = el('input');
    input.id = id;
    input.inputMode = 'numeric';
    // 数字しか受けない。`12e80` のような綴りを打たせない（読めるかの最終判定は
    // Rust の parse_window_size。上限 16384 の 5 桁で切る）。
    input.maxLength = 5;
    //
    // 全角の数字（IME で打ったもの）は消さずに半角へ直す。変換中は触らない——
    // 書き換えると変換の途中の文字ごと消える。確定したら compositionend で直す。
    function normalize() {
      var before = input.value;
      var pos = input.selectionStart;
      var head = digitsOnly(before.slice(0, pos));
      var all = digitsOnly(before).slice(0, input.maxLength);
      if (all === before) return;
      input.value = all;
      // 消した文字のぶんだけカーソルを戻す。そのままだと末尾へ飛ぶ。
      var at = Math.min(head.length, all.length);
      input.setSelectionRange(at, at);
    }
    input.addEventListener('input', function(e) {
      if (e.isComposing) return;
      normalize();
    });
    input.addEventListener('compositionend', normalize);
    // Esc は打ちかけを捨てて、保存してある値へ戻す。
    input.addEventListener('keydown', function(e) {
      if (e.key !== 'Escape' || e.isComposing) return;
      e.preventDefault();
      var saved = state.windowSize || state.defaultWindowSize;
      input.value = String(id === 'st-width' ? saved.w : saved.h);
      settle();
    });
    input.value = String(value);
    // 1 打鍵ごとには送らない。`change`（Enter・フォーカスが外れたとき）で確定する。
    // 打っている途中の「14」を保存すると、下限まで持ち上がった窓で次に開くことになる。
    input.addEventListener('change', commitSize);
    box.appendChild(input);
    box.appendChild(el('span', null, 'pt'));
    f.appendChild(box);
    return f;
  }

  // 打ったままの綴りを送る。読めるかどうか（数字か・上限を超えていないか）の関門は
  // Rust の `parse_window_size` 1 つだけで、読めなければ保存せずに今の状態を送り返して
  // くるので、欄は保存してある値へ戻る。
  //
  // Why not ここで parseInt して送る: `12abc` が 12 に、`1e4` が 1 に化けて保存される。
  // 上限もここに写すと、Rust と食い違ったときに「保存されたように見えて保存されて
  // いない」が起きる。
  var committing = false;
  function commitSize() {
    // 下の settle() が外したフォーカスで `change` がもう一度届く。入れ子で送らない。
    if (committing) return;
    committing = true;
    try {
      var w = document.getElementById('st-width').value.trim();
      var h = document.getElementById('st-height').value.trim();
      // 確定したらフォーカスを外す。外さないと、Rust が断って送り返してきた値を
      // 「打ちかけを守る」描き直しが弾いて、断られた綴りが欄に残る。
      settle();
      send('window-size:' + w + 'x' + h);
    } finally {
      committing = false;
    }
  }

  // ── 入口 ────────────────────────────────────────────────────
  // 開く＝設定タブを前に出す。もうあればそこへ移るだけ（タブは増えない）。
  function open() {
    if (opts && opts.openFile) opts.openFile(TAB_ID);
  }

  // ── 歯車 ────────────────────────────────────────────────────
  function buildGear() {
    var icons = document.getElementById('tabbar-icons');
    if (!icons || gearEl) return;
    gearEl = document.createElement('button');
    gearEl.type = 'button';
    gearEl.id = 'tabbar-gear';
    gearEl.title = '設定 (⌘,)';
    gearEl.setAttribute('aria-label', '設定');
    gearEl.innerHTML =
      '<svg viewBox="0 0 16 16" width="15" height="15" fill="currentColor" aria-hidden="true">' +
      '<path d="M8 0a8.2 8.2 0 0 1 .701.031C9.444.095 9.99.645 10.16 1.29l.288 1.107c.018.066.079.158.212.224.231.114.454.243.668.386.123.082.233.09.299.071l1.103-.303c.644-.176 1.392.021 1.82.63.27.385.506.792.704 1.218.315.675.111 1.422-.364 1.891l-.814.806c-.049.048-.098.147-.088.294.016.257.016.515 0 .772-.01.147.038.246.088.294l.814.806c.475.469.679 1.216.364 1.891a7.977 7.977 0 0 1-.704 1.217c-.428.61-1.176.807-1.82.63l-1.102-.302c-.067-.019-.177-.011-.3.071a5.909 5.909 0 0 1-.668.386c-.133.066-.194.158-.211.224l-.29 1.106c-.168.646-.715 1.196-1.458 1.26a8.006 8.006 0 0 1-1.402 0c-.743-.064-1.289-.614-1.458-1.26l-.289-1.106c-.018-.066-.079-.158-.212-.224a5.738 5.738 0 0 1-.668-.386c-.123-.082-.233-.09-.299-.071l-1.103.303c-.644.176-1.392-.021-1.82-.63a8.12 8.12 0 0 1-.704-1.218c-.315-.675-.111-1.422.363-1.891l.815-.806c.05-.048.098-.147.088-.294a6.214 6.214 0 0 1 0-.772c.01-.147-.038-.246-.088-.294l-.815-.806C.635 6.045.431 5.298.746 4.623a7.92 7.92 0 0 1 .704-1.217c.428-.61 1.176-.807 1.82-.63l1.102.302c.067.019.177.011.3-.071.214-.143.437-.272.668-.386.133-.066.194-.158.211-.224l.29-1.106C6.009.645 6.556.095 7.299.03 7.53.01 7.764 0 8 0Zm-.571 1.525c-.036.003-.108.036-.137.146l-.289 1.105c-.147.561-.549.967-.998 1.189-.173.086-.34.183-.5.29-.417.278-.97.423-1.529.27l-1.103-.303c-.109-.03-.175.016-.195.045-.22.312-.412.644-.573.99-.014.031-.021.11.059.19l.815.806c.411.406.562.957.53 1.456a4.709 4.709 0 0 0 0 .582c.032.499-.119 1.05-.53 1.456l-.815.806c-.081.08-.073.159-.059.19.162.346.353.677.573.989.02.03.085.076.195.046l1.102-.303c.56-.153 1.113-.008 1.53.27.161.107.328.204.501.29.447.222.85.629.997 1.189l.289 1.105c.029.109.101.143.137.146a6.6 6.6 0 0 0 1.142 0c.036-.003.108-.036.137-.146l.289-1.105c.147-.561.549-.967.998-1.189.173-.086.34-.183.5-.29.417-.278.97-.423 1.529-.27l1.103.303c.109.029.175-.016.195-.045.22-.313.411-.644.573-.99.014-.031.021-.11-.059-.19l-.815-.806c-.411-.406-.562-.957-.53-1.456a4.709 4.709 0 0 0 0-.582c-.032-.499.119-1.05.53-1.456l.815-.806c.081-.08.073-.159.059-.19a6.464 6.464 0 0 0-.573-.989c-.02-.03-.085-.076-.195-.046l-1.102.303c-.56.153-1.113.008-1.53-.27a4.44 4.44 0 0 0-.501-.29c-.447-.222-.85-.629-.997-1.189l-.289-1.105c-.029-.11-.101-.143-.137-.146a6.6 6.6 0 0 0-1.142 0ZM11 8a3 3 0 1 1-6 0 3 3 0 0 1 6 0ZM9.5 8a1.5 1.5 0 1 0-3.001.001A1.5 1.5 0 0 0 9.5 8Z"/>' +
      '</svg>';
    gearEl.addEventListener('click', open);
    // ベルより右（窓の縁の側）に置く。設定は一番奥の操作なので端に寄せる。
    icons.appendChild(gearEl);
  }

  // ── テーマの差し替え ────────────────────────────────────────
  // Rust から呼ばれる。設定画面からの切り替えでも `md theme` からでも同じ口。
  function applyTheme(css, appearance) {
    var style = document.getElementById('md-theme');
    if (style) style.textContent = css;
    window.MD_APPEARANCE = appearance;
    // mermaid は初期化のときに配色を決める。初期化をやり直させてから本文を
    // 読み直さないと、描いてある図だけが前のテーマの配色で残る。
    if (window.mermaid && window.mermaid.__mdInit) {
      window.mermaid.__mdInit = false;
      if (document.querySelector('.markdown-body pre.mermaid') && window.MdRerender) {
        window.MdRerender();
      }
    }
  }

  window.MdSettings = {
    // o: { openFile(id) } … タブを開く入口（folder.js の loadPreview）。
    init: function(o) {
      opts = o;
      buildGear();
      if (window.MdKeymap) MdKeymap.on('settings-open', open);
    },
    isTab: function(id) { return id === TAB_ID; },
    flush: flush,
    // 設定タブを `el`（本文ペイン）に描く。folder.js の loadPreview から呼ばれる。
    mount: function(paneEl) {
      host = paneEl;
      host.innerHTML = '';
      host.appendChild(el('div', 'st-layout'));
      render();
      // 写しは前に出すたびに取り直す。届けば push() が描き直す。
      send('get');
    },
    // Rust から状態を渡される唯一の口。差分ではないので、毎回まるごと置き換える。
    push: function(next) {
      state = next;
      render();
    },
    applyTheme: applyTheme
  };
})();
