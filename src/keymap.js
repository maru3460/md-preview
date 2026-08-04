// キーボードショートカットの単一定義（Single Source of Truth）。
// 「キー・説明・カテゴリ・表示範囲」をここだけに置き、表示側（help.js の `?` 一覧）は
// すべてこの表から導出する。文言を表示側に持たせると実装との乖離が必ず起きるため、
// 表示に使うメタデータは必ずこのファイルへ足す。
//
// 【重要】ここは「表示のための定義」であって、キーを実際に処理するのはこの表ではない。
// 実処理は各モジュールに分散した keydown ハンドラが担う:
//   keyscroll.js   j k d u Space g G /
//   comment.js     c / コメントモード中の j k Enter n p e x y
//   folder.js      ツリー内移動・] [ Tab・⌘W
//   search.js ⌘F / toc.js ⌘T / diff.js ⌘D / raw.js ⌘R / common.js ⌘A / help.js ? Esc
//   palette.js     ファイル検索（folder モードのみ。パレット内の ↑↓ Enter Esc も palette.js）
// 排他は stopPropagation ではなく「フォーカスの所在」と「モードの有無」で取っている。
// よって **キー自体を変える時はハンドラ側とこの表の両方を直す**必要がある。
// （ハンドラをこの表から駆動する案は将来の課題。Esc の譲り合いやフォーカス排他を
//   全面的に作り替えることになるため、表示の一元化とは分けて考える。）
(function() {
  // カテゴリ。BINDS の並びもこの順に揃える。`?` 一覧は見出しを出さず平坦に流すので、
  // ここでの順序がそのまま表示順になる。
  var CATEGORIES = [
    { id: 'scroll',  label: '読み進める' },
    { id: 'find',    label: '探す・飛ぶ' },
    { id: 'view',    label: '表示を切り替える' },
    { id: 'comment', label: 'コメント' },
    { id: 'files',   label: 'ファイル移動' },
    { id: 'tree',    label: 'ファイルツリー' },
    { id: 'app',     label: 'ウィンドウ・ヘルプ' }
  ];

  // 表示範囲（scope）は:
  //   'all'     … 全モード（stdin / single / folder）
  //   'nostdin' … stdin 以外（ファイル/git がある single・folder）
  //   'folder'  … folder モードのみ（サイドバーとファイル移動がある時）
  var BINDS = [
    // ── 読み進める（keyscroll.js） ──
    { cat: 'scroll', keys: 'j / k',   desc: '1 行スクロール 下 / 上', scope: 'all' },
    { cat: 'scroll', keys: 'd / u',   desc: '半ページ 下 / 上', scope: 'all' },
    { cat: 'scroll', keys: 'Space',   desc: '1 ページ送り（Shift+Space で戻る）', scope: 'all' },
    { cat: 'scroll', keys: 'g / G',   desc: '冒頭 / 末尾へ', scope: 'all' },

    // ── 探す・飛ぶ（search.js / toc.js） ──
    { cat: 'find',   keys: '/',       desc: '検索（⌘F と同じ）', scope: 'all' },
    { cat: 'find',   keys: '⌘F',      desc: '検索', scope: 'all' },
    { cat: 'find',   keys: '⌘T',      desc: 'アウトライン（見出しナビ）を開閉', scope: 'all' },
    { cat: 'find',   keys: '⌘P',      desc: 'ファイル検索（あいまい検索。未入力なら git 変更ファイルが先頭）', scope: 'folder' },

    // ── 表示を切り替える（diff.js / raw.js） ──
    { cat: 'view',   keys: '⌘D',      desc: 'git 差分表示を切り替え', scope: 'nostdin' },
    { cat: 'view',   keys: '⌘R',      desc: 'raw（ソース）表示を切り替え', scope: 'nostdin' },

    // ── コメント（comment.js） ──
    { cat: 'comment', keys: 'c',                 desc: 'コメントモード開始/終了', scope: 'all' },
    { cat: 'comment', keys: 'j / k / Enter',     desc: 'コメント中: 移動（Shift+j/k でレンジ）/ Enter で付与', scope: 'all' },
    { cat: 'comment', keys: 'n / p / e / x / y', desc: 'コメント中: 巡回 / 編集 / 削除(Delete可) / 全部コピー', scope: 'all' },

    // ── ファイル移動（folder.js） ──
    { cat: 'files',  keys: '] / [',   desc: '次 / 前のファイルへ（表示中のファイルを巡回）', scope: 'folder' },
    { cat: 'files',  keys: 'Tab',     desc: '本文 ⇄ ファイルツリー のフォーカス切替', scope: 'folder' },

    // ── ファイルツリー（folder.js。ツリーにフォーカスがある時） ──
    { cat: 'tree',   keys: 'ツリー内', desc: 'j/k 移動・g/G 端・Enter/l 開く&展開・h 畳む/親へ', scope: 'folder' },

    // ── ウィンドウ・ヘルプ（common.js / init.js / contextmenu.js / help.js） ──
    { cat: 'app',    keys: '⌘A',       desc: '本文を全選択', scope: 'all' },
    { cat: 'app',    keys: '⌘W',       desc: 'ウィンドウを閉じる', scope: 'all' },
    { cat: 'app',    keys: '⌘Q',       desc: '終了', scope: 'all' },
    { cat: 'app',    keys: '右クリック', desc: 'コンテキストメニュー', scope: 'all' },
    { cat: 'app',    keys: '?',         desc: 'このヘルプを開閉', scope: 'all' },
    { cat: 'app',    keys: 'Esc',       desc: '検索 / メニュー / ヘルプ / コメントを閉じる', scope: 'all' }
  ];

  // 現在のモードでこの scope を表示するか。MD_MENU_MODE は起動時に決まるが、
  // 参照は呼び出し時に行う（スクリプトの評価順に依存しないため）。
  function inScope(scope) {
    var mode = window.MD_MENU_MODE;
    if (scope === 'nostdin') return mode !== 'stdin';
    if (scope === 'folder') return mode === 'folder';
    return true; // 'all' と未指定
  }

  // 現在のモードで表示すべきバインドを、定義順（＝カテゴリ順）で返す。
  function visible() {
    return BINDS.filter(function(b) { return inScope(b.scope); });
  }

  window.MdKeymap = {
    categories: CATEGORIES,
    binds: BINDS,
    inScope: inScope,
    visible: visible
  };
})();
