// キーボードショートカット一覧のオーバーレイ。search.js / toc.js / diff.js と同じ流儀で
// window.MdHelp を公開する。`?`（Shift+/）で開閉、Esc / 背景クリックで閉じる。
// 右クリックメニューの「ショートカット一覧」からも MdHelp.open() で開ける。
//
// 配色は context-menu / diff-toggle と同じくシステムカラー（Canvas/CanvasText/…）と
// color-mix で theme-agnostic に組む。themes/*.css への追記は不要。
(function() {
  var overlay = null;

  // [キー, 説明, 表示範囲]。表示順そのまま。表示範囲は:
  //   'all'     … 全モード（stdin / single / folder）
  //   'nostdin' … stdin 以外（ファイル/git がある single・folder）
  //   'folder'  … folder モードのみ（サイドバーとファイル移動がある時）
  var SHORTCUTS = [
    ['j / k', '1 行スクロール 下 / 上', 'all'],
    ['d / u', '半ページ 下 / 上', 'all'],
    ['Space', '1 ページ送り（Shift+Space で戻る）', 'all'],
    ['g / G', '冒頭 / 末尾へ', 'all'],
    ['/', '検索（⌘F と同じ）', 'all'],
    ['⌘F', '検索', 'all'],
    ['⌘T', 'アウトライン（見出しナビ）を開閉', 'all'],
    ['⌘D', 'git 差分表示を切り替え', 'nostdin'],
    ['⌘R', 'raw（ソース）表示を切り替え', 'nostdin'],
    ['c', 'コメントモード開始/終了', 'all'],
    ['j / k / Enter', 'コメント中: 移動（Shift+j/k でレンジ）/ Enter で付与', 'all'],
    ['n / p / e / x / y', 'コメント中: 巡回 / 編集 / 削除(Delete可) / 全部コピー', 'all'],
    ['] / [', '次 / 前のファイルへ（表示中のファイルを巡回）', 'folder'],
    ['Tab', '本文 ⇄ ファイルツリー のフォーカス切替', 'folder'],
    ['ツリー内', 'j/k 移動・g/G 端・Enter/l 開く&展開・h 畳む/親へ', 'folder'],
    ['⌘A', '本文を全選択', 'all'],
    ['⌘W', 'ウィンドウを閉じる', 'all'],
    ['⌘Q', '終了', 'all'],
    ['右クリック', 'コンテキストメニュー', 'all'],
    ['?', 'このヘルプを開閉', 'all'],
    ['Esc', '検索 / メニュー / ヘルプ / コメントを閉じる', 'all']
  ];

  // 現在のモードでこの行を表示するか。
  function showFor(scope) {
    var mode = window.MD_MENU_MODE;
    if (scope === 'all') return true;
    if (scope === 'nostdin') return mode !== 'stdin';
    if (scope === 'folder') return mode === 'folder';
    return true;
  }

  function build(welcome) {
    // パネルを一度でも開いたら、初回の自動表示はもう出さない。
    markOnboarded();
    overlay = document.createElement('div');
    overlay.className = 'md-help-backdrop';
    overlay.id = 'md-help-backdrop'; // MdCommon.isOverlayOpen が O(1) で存在を見るため

    var panel = document.createElement('div');
    panel.className = 'md-help-panel';

    var title = document.createElement('div');
    title.className = 'md-help-title';
    title.textContent = 'キーボードショートカット';
    panel.appendChild(title);

    // 初回だけ、使い方の一言を添える。「?」でいつでも呼べることを伝える。
    if (welcome) {
      var intro = document.createElement('div');
      intro.className = 'md-help-welcome';
      intro.textContent = 'md へようこそ。使えるショートカットの一覧です。この一覧は「?」でいつでも開けます。';
      panel.appendChild(intro);
    }

    // 行は専用ラッパへ入れ、CSS の段組み(column-count)で上→下→次の列と流す。
    // 列単位で流れるのでスクロール系などのまとまりが崩れず、縦の高さも半分に収まる。
    var rows = document.createElement('div');
    rows.className = 'md-help-rows';
    SHORTCUTS.forEach(function(sc) {
      if (!showFor(sc[2])) return;
      var row = document.createElement('div');
      row.className = 'md-help-row';
      var key = document.createElement('kbd');
      key.className = 'md-help-key';
      key.textContent = sc[0];
      var desc = document.createElement('span');
      desc.className = 'md-help-desc';
      desc.textContent = sc[1];
      row.appendChild(key);
      row.appendChild(desc);
      rows.appendChild(row);
    });
    panel.appendChild(rows);

    var hint = document.createElement('div');
    hint.className = 'md-help-hint';
    hint.textContent = 'Esc または背景クリックで閉じる';
    panel.appendChild(hint);

    overlay.appendChild(panel);
    // 背景クリックで閉じる（パネル内クリックは無視）。
    overlay.addEventListener('mousedown', function(e) {
      if (e.target === overlay) close();
    });
    document.body.appendChild(overlay);
  }

  function open(welcome) {
    if (overlay) { close(); return; }
    build(welcome);
  }

  function close() {
    if (!overlay) return;
    overlay.remove();
    overlay = null;
  }

  // 初回オンボーディング。初めて使うときだけ、ショートカット一覧を
  // 自動で開いて使い方を提示する。一度出したら（＝パネルを一度でも開いたら）
  // 二度と自動では出さない。
  var ONBOARDED_KEY = 'md-help-onboarded';

  function lsGet(k) { try { return localStorage.getItem(k); } catch (e) { return null; } }
  function lsSet(k, v) { try { localStorage.setItem(k, v); } catch (e) {} }

  function markOnboarded() { lsSet(ONBOARDED_KEY, '1'); }

  function maybeOnboard() {
    if (lsGet(ONBOARDED_KEY) === '1') return;
    // 既にユーザーが自分で開いていたら邪魔しない（フラグだけ立てる）。
    if (overlay) { markOnboarded(); return; }
    // 描画が落ち着いてから、歓迎の一言つきで一覧を開く。
    setTimeout(function() {
      if (overlay || lsGet(ONBOARDED_KEY) === '1') { markOnboarded(); return; }
      open(true);
    }, 500);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', maybeOnboard);
  } else {
    maybeOnboard();
  }

  document.addEventListener('keydown', function(e) {
    if (e.key === 'Escape' && overlay) {
      e.preventDefault();
      close();
      return;
    }
    if (e.key !== '?' || e.metaKey || e.ctrlKey || e.altKey) return;
    // 入力欄（検索ボックス等）にフォーカス中は通常入力に委ねる。
    var t = e.target;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    e.preventDefault();
    open();
  });

  window.MdHelp = {
    open: function() {
      // 右クリックメニューから呼ばれたとき、既に開いていれば開き直さず維持。
      if (!overlay) open();
    },
    close: close
  };
})();
