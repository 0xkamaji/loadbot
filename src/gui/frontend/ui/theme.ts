import type { CSSProperties } from 'react';
import manifest from '../../loadbot-gui-assets/spec/asset-manifest.json';

// Import the supplied masters only. Vite emits their original bytes; no artwork copies
// or screenshot backgrounds. Slice geometry and content padding come from the manifest.
const images = import.meta.glob<string>([
  '../../loadbot-gui-assets/assets/ui/{buttons,frames,inputs,menus,icons,icons-inverse,checkboxes}/*.png',
  '../../loadbot-gui-assets/assets/terminal/panel.png',
], { eager: true, query: '?url', import: 'default' });

export function asset(id: string): string {
  const entry = manifest.assets.find((item) => item.id === id);
  const url = entry && images[`../../loadbot-gui-assets/${entry.file}`];
  if (!url) throw new Error(`Missing approved asset: ${id}`);
  return url;
}

const tokens: Record<string, string | number> = {
  '--lb-text': manifest.theme.text,
  '--lb-selected-text': manifest.theme.text_selected,
  '--lb-disabled-text': manifest.theme.text_disabled,
  '--lb-surface': manifest.theme.surface,
  '--lb-terminal-surface': '#ecd4af',
  '--lb-terminal-text': manifest.terminal_text,
  '--lb-terminal-muted': '#5f5141',
  '--lb-terminal-success': '#285c32',
  '--lb-terminal-error': '#7a3027',
  '--lb-terminal-accent': '#74521d',
  '--lb-background': '#0e0e0c',
  '--lb-font-size': `${manifest.font.body_px}px`,
  '--lb-title-size': `${manifest.font.title_px}px`,
  '--lb-line-height': manifest.font.line_height,
};
for (const item of manifest.assets) {
  if (!images[`../../loadbot-gui-assets/${item.file}`]) continue;
  const name = `--${item.id.replaceAll('.', '-')}`;
  tokens[name] = `url("${asset(item.id)}")`;
  if ('nine_slice' in item && item.nine_slice) {
    tokens[`${name}-slice`] = item.nine_slice.join(' ');
    tokens[`${name}-border`] = item.nine_slice.map((n) => `${n}px`).join(' ');
    if (item.content_padding) {
      tokens[`${name}-padding`] = item.content_padding.map((n, i) => `${Math.max(0, n - item.nine_slice![i])}px`).join(' ');
    }
  }
}
export const themeStyle = tokens as CSSProperties;
