import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// バージョンの本家は Cargo.toml。サイトに書き写すと、リリースのたびに
// 「どこを直すんだっけ」が増えるので、ビルド時に読む。
// import.meta.url ではなく cwd を基準にするのは、バンドル後の import.meta.url が
// dist を指してしまい ../../ が外れるため（astro dev / build は site/ で走る）。
const toml = readFileSync(resolve(process.cwd(), '../Cargo.toml'), 'utf8');
const m = toml.match(/^version\s*=\s*"([^"]+)"/m);

export const VERSION = m ? m[1] : '0.0.0';
export const REPO = 'https://github.com/maru3460/md-preview';
