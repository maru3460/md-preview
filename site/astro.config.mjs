import { defineConfig } from 'astro/config';

// プロジェクトページ（maru3460.github.io/md-preview/）なので base が要る。
// 忘れると CSS と画像だけが 404 になる。
export default defineConfig({
  site: 'https://maru3460.github.io',
  base: '/md-preview',
  build: { format: 'directory' },
});
