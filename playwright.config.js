// UI スモークテストの設定。
//
// 実物のページを `examples/serve.rs`（handle_request を HTTP で公開する開発用サーバ）
// 越しに開いて叩く。ブラウザは WebKit を使う——本番の実行環境が WKWebView なので、
// キーイベントの扱いや CSS Custom Highlight API の有無を一番忠実に再現できる。
//
// 起動モードはフォルダ 1 本だが、「起動時に何が開いているか」で通る道が違うので
// サーバは 3 つ立てる。`md <dir>`（タブ 0 枚）・`md <cwd 外の 1 ファイル>`（タブ 1 枚。
// root はそのファイルの親フォルダ）・`md a.md b.md`（タブ 2 枚）の 3 つ。
//
// ── このスイートが守れないもの ──────────────────────────────────
// Playwright の WebKit は本番の WKWebView とは別のビルドで、macOS には WKWebView
// 用の WebDriver が無いため実機を直接は叩けない。つまり **ここが全部緑でも実機が
// 壊れている領域がある**。以下は原理的に守備範囲の外なので、実機で確かめること:
//
//   ・IME（変換候補ウィンドウ、変換中の Enter / ↑ / ↓）
//     `keyboard.press` では composition が起きない。isComposing のガードだけなら
//     compositionstart / update / end を dispatchEvent で合成すれば触れる。
//   ・macOS のメニューが処理するキー（⌃⌘F / ⌘Q / ⌘Z ⌘X ⌘C ⌘V）
//     keydown が WebView に届かないので、keymap.js では表示専用の行になっている。
//   ・実機のマウス・トラックパッド挙動
//     アプリ内で NSEvent を合成して確かめる（ブラウザ側は blur の扱いが違い、
//     バグを隠してしまうことがある）。
//   ・CSS Custom Highlight API など、エンジンのバージョン差が出る API の可否
//   ・ウィンドウ・メニュー・ホットリロードの監視（IPC は serve.rs でスタブ）
// ──────────────────────────────────────────────────────────────
const path = require('path');
const os = require('os');
const { defineConfig, devices } = require('@playwright/test');

const FOLDER_PORT = 7878;
const ONE_FILE_PORT = 7879;
const MULTI_PORT = 7880;

const ROOT = __dirname;
// ファイルを渡す 2 つのサーバは cwd を一時ディレクトリにして起動する。cwd 配下の
// ファイルを渡すと root が cwd（＝リポジトリ全体）になってツリーが巨大になるため、
// 「cwd の外のファイル」として渡して root をそのファイルの居場所へ寄せる。
// cargo は --manifest-path でどこからでも動かせる。
const CARGO = ['cargo', 'run', '-q', '--manifest-path', path.join(ROOT, 'Cargo.toml'), '--example', 'serve'];

module.exports = defineConfig({
  testDir: './tests/ui',
  // 1 つのサーバを共有するので直列に走らせる。
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? 'list' : [['list']],
  use: {
    baseURL: `http://127.0.0.1:${FOLDER_PORT}`,
    // 失敗時だけ痕跡を残す（成功時に容量を食わない）。
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'webkit', use: { ...devices['Desktop Safari'] } }],
  webServer: [
    {
      command: `cargo run -q --example serve -- --port ${FOLDER_PORT} tests/ui-fixtures`,
      url: `http://127.0.0.1:${FOLDER_PORT}/`,
      reuseExistingServer: !process.env.CI,
      timeout: 180000,
    },
    {
      // 1 ファイル起動。root はそのファイルの親（tests/ui-fixtures）で、タブが 1 枚
      // 開いた状態で始まる。かつての「単一ファイルモード」がここへ畳まれた。
      command: `${CARGO.join(' ')} -- --port ${ONE_FILE_PORT} ${path.join(ROOT, 'tests/ui-fixtures/a.md')}`,
      cwd: os.tmpdir(),
      url: `http://127.0.0.1:${ONE_FILE_PORT}/`,
      reuseExistingServer: !process.env.CI,
      timeout: 180000,
    },
    {
      // 複数ファイル起動。cwd が一時ディレクトリなので root は 2 つの共通の親
      // （tests/ui-fixtures）になる——リポジトリ全体をツリーに出さないため。
      command: `${CARGO.join(' ')} -- --port ${MULTI_PORT} ${path.join(ROOT, 'tests/ui-fixtures/a.md')} ${path.join(ROOT, 'tests/ui-fixtures/sub/a.md')}`,
      cwd: os.tmpdir(),
      url: `http://127.0.0.1:${MULTI_PORT}/`,
      reuseExistingServer: !process.env.CI,
      timeout: 180000,
    },
  ],
});

module.exports.FOLDER_PORT = FOLDER_PORT;
module.exports.ONE_FILE_PORT = ONE_FILE_PORT;
module.exports.MULTI_PORT = MULTI_PORT;
