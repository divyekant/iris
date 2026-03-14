import './app.css';
import App from './App.svelte';
import { mount } from 'svelte';
import { initSession } from './lib/api';

// Apply saved theme before first render to prevent flash
const savedTheme = localStorage.getItem('iris-theme') || 'light';
if (savedTheme === 'system') {
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
  if (!prefersDark) document.documentElement.setAttribute('data-brand', 'light');
  else document.documentElement.removeAttribute('data-brand');
} else if (savedTheme === 'light') {
  document.documentElement.setAttribute('data-brand', 'light');
}
// else dark: remove data-brand attribute (:root default)

// Apply saved appearance settings (accent color, fonts) before first render
const savedAccent = localStorage.getItem('iris-accent-color');
if (savedAccent) {
  const root = document.documentElement;
  root.style.setProperty('--iris-color-primary', savedAccent);
  root.style.setProperty('--iris-color-unread', savedAccent);
  root.style.setProperty('--iris-color-border', `${savedAccent}26`);
  root.style.setProperty('--iris-color-accent-border', `${savedAccent}2E`);
  root.style.setProperty('--iris-color-focus-ring', `${savedAccent}66`);
}

const savedFont = localStorage.getItem('iris-font-family');
if (savedFont) {
  const resolved = savedFont === 'System Default'
    ? 'system-ui, -apple-system, sans-serif'
    : `'${savedFont}', system-ui, sans-serif`;
  document.documentElement.style.setProperty('--iris-font-family', resolved);
}

const savedMono = localStorage.getItem('iris-font-mono');
if (savedMono) {
  const resolved = savedMono === 'System Mono'
    ? "'SF Mono', 'Cascadia Mono', 'Menlo', 'Consolas', monospace"
    : `'${savedMono}', monospace`;
  document.documentElement.style.setProperty('--iris-font-mono', resolved);
}

// Load previously-used Google Fonts so they render before Settings is opened
const loadedFonts = JSON.parse(localStorage.getItem('iris-loaded-fonts') || '[]') as string[];
const GOOGLE_FONTS: Record<string, string> = {
  'Inter': 'Inter:wght@400;500;600;700',
  'Roboto': 'Roboto:wght@400;500;700',
  'Open Sans': 'Open+Sans:wght@400;500;600;700',
  'Lato': 'Lato:wght@400;700',
  'Source Sans 3': 'Source+Sans+3:wght@400;500;600;700',
  'IBM Plex Sans': 'IBM+Plex+Sans:wght@400;500;600;700',
  'Noto Sans': 'Noto+Sans:wght@400;500;600;700',
  'JetBrains Mono': 'JetBrains+Mono:wght@400;500;700',
  'Fira Code': 'Fira+Code:wght@400;500;700',
  'Source Code Pro': 'Source+Code+Pro:wght@400;500;700',
  'IBM Plex Mono': 'IBM+Plex+Mono:wght@400;500;700',
  'Ubuntu Mono': 'Ubuntu+Mono:wght@400;700',
};

for (const fontName of loadedFonts) {
  const spec = GOOGLE_FONTS[fontName];
  if (spec) {
    const link = document.createElement('link');
    link.rel = 'stylesheet';
    link.href = `https://fonts.googleapis.com/css2?family=${spec}&display=swap`;
    document.head.appendChild(link);
  }
}

// Bootstrap the HttpOnly session cookie, then mount the app.
initSession()
  .then(() => {
    mount(App, { target: document.getElementById('app')! });
  })
  .catch((err) => {
    console.error('Session bootstrap failed:', err);
    // Mount anyway — health endpoint still works, user sees error on protected routes
    mount(App, { target: document.getElementById('app')! });
  });
