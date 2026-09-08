// テーマ一覧の本家。Themes のカード・Home のチップ・着せ替えスクリプトの
// 既知名チェックが全部ここを見る。色は src/assets/themes/*.css の実値
// （body の背景、.tree-item:hover の背景、--md-accent）から取っている。推測で書かないこと。
export const THEMES = [
  { name: 'default', kind: 'auto', bg: '#ffffff', alt: '#f6f8fa', accent: '#3b82f6', note: 'OS のダークモードに追従する既定のテーマ' },
  { name: 'minimal', kind: 'light', bg: '#ffffff', alt: '#f1f0ee', accent: '#2e7cd6', note: '装飾を削った素の白' },
  { name: 'editorial', kind: 'light', bg: '#fffefb', alt: '#efe9df', accent: '#8a1c1c', note: '読み物向け。赤の差し色' },
  { name: 'ink', kind: 'light', bg: '#ffffff', alt: '#ebebeb', accent: '#000000', note: '黒一色のコントラスト' },
  { name: 'paper', kind: 'light', bg: '#faf6ee', alt: '#ece4d5', accent: '#1f6f6b', note: '紙の色。本文はセリフ体' },
  { name: 'mono', kind: 'light', bg: '#f7f5ef', alt: '#e9e4d6', accent: '#9b2c2c', note: '全部等幅' },
  { name: 'solarized-light', kind: 'light', bg: '#fdf6e3', alt: '#eee8d5', accent: '#268bd2', note: '定番の Solarized' },
  { name: 'nord', kind: 'dark', bg: '#2e3440', alt: '#3b4252', accent: '#88c0d0', note: '寒色の低コントラスト' },
  { name: 'dracula', kind: 'dark', bg: '#282a36', alt: '#44475a', accent: '#bd93f9', note: '紫の差し色' },
  { name: 'gruvbox', kind: 'dark', bg: '#282828', alt: '#32302f', accent: '#fe8019', note: 'レトロな暖色' },
  { name: 'rose-pine', kind: 'dark', bg: '#191724', alt: '#26233a', accent: '#c4a7e7', note: '紫みのある暗色' },
  { name: 'terminal', kind: 'dark', bg: '#0b0f0b', alt: '#14241a', accent: '#6ee787', note: '緑の等幅。端末そのもの' },
  { name: 'blueprint', kind: 'dark', bg: '#0e2a4a', alt: '#123455', accent: '#6fb3e0', note: '製図の青' },
];

export const THEME_NAMES = THEMES.map((t) => t.name);
