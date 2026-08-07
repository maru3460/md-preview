// UI スモークテストの設定。
//
// 実物のページを `examples/serve.rs`（handle_request を HTTP で公開する開発用サーバ）
// 越しに開いて叩く。ブラウザは WebKit を使う——本番の実行環境が WKWebView なので、
// キーイベントの扱いや CSS Custom Highlight API の有無を一番忠実に再現できる。
//
// フォルダモードと単一ファイルモードで挙動が違うショートカット（⌘P など）があるので、
// サーバは 2 つ立てる。
const path = require('path');
const os = require('os');
const { defineConfig, devices } = require('@playwright/test');

const FOLDER_PORT = 7878;
const SINGLE_PORT = 7879;

const ROOT = __dirname;
// 単一ファイルモードは「cwd の外にあるファイル」を開いたときのモードなので、
// cwd を一時ディレクトリにしてサーバを起動する（cwd 配下のファイルを渡すと、
// 製品の仕様どおりフォルダモードで開いてしまう）。cargo は --manifest-path で
// どこからでも動かせる。
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
      command: `cargo run -q --example serve -- tests/ui-fixtures ${FOLDER_PORT}`,
      url: `http://127.0.0.1:${FOLDER_PORT}/`,
      reuseExistingServer: !process.env.CI,
      timeout: 180000,
    },
    {
      command: `${CARGO.join(' ')} -- ${path.join(ROOT, 'tests/ui-fixtures/a.md')} ${SINGLE_PORT}`,
      cwd: os.tmpdir(),
      url: `http://127.0.0.1:${SINGLE_PORT}/`,
      reuseExistingServer: !process.env.CI,
      timeout: 180000,
    },
  ],
});

module.exports.FOLDER_PORT = FOLDER_PORT;
module.exports.SINGLE_PORT = SINGLE_PORT;
