<script lang="ts">
  import { api } from '../../lib/api';
  import FormSelect from '../shared/FormSelect.svelte';
  import { Palette, Type, Check, Layers } from 'lucide-svelte';

  type Theme = 'light' | 'dark' | 'system';

  let currentTheme = $state<Theme>('system');
  let accentColor = $state('#d4af37');
  let customHex = $state('');
  let fontFamily = $state('Inter');
  let fontMono = $state('SF Mono');
  let loading = $state(true);
  let saving = $state(false);

  const themes: { value: Theme; label: string; desc: string }[] = [
    { value: 'light', label: 'Light', desc: 'Warm light background' },
    { value: 'dark', label: 'Dark', desc: 'Easy on the eyes' },
    { value: 'system', label: 'System', desc: 'Match your OS' },
  ];

  const accentPresets = [
    { color: '#d4af37', name: 'Gold' },
    { color: '#3B82F6', name: 'Blue' },
    { color: '#8B5CF6', name: 'Purple' },
    { color: '#10B981', name: 'Green' },
    { color: '#F43F5E', name: 'Rose' },
    { color: '#F97316', name: 'Orange' },
    { color: '#06B6D4', name: 'Cyan' },
    { color: '#EC4899', name: 'Pink' },
  ];

  // Google Fonts name → URL family parameter mapping
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

  const fontOptions = [
    { value: 'System Default', label: 'System Default' },
    { value: 'Inter', label: 'Inter' },
    { value: 'Roboto', label: 'Roboto' },
    { value: 'Open Sans', label: 'Open Sans' },
    { value: 'Lato', label: 'Lato' },
    { value: 'Source Sans 3', label: 'Source Sans 3' },
    { value: 'IBM Plex Sans', label: 'IBM Plex Sans' },
    { value: 'Noto Sans', label: 'Noto Sans' },
  ];

  const monoFontOptions = [
    { value: 'System Mono', label: 'System Mono' },
    { value: 'JetBrains Mono', label: 'JetBrains Mono' },
    { value: 'Fira Code', label: 'Fira Code' },
    { value: 'Source Code Pro', label: 'Source Code Pro' },
    { value: 'IBM Plex Mono', label: 'IBM Plex Mono' },
    { value: 'Ubuntu Mono', label: 'Ubuntu Mono' },
  ];

  // Preset theme combos
  interface ThemePreset {
    name: string;
    accent: string;
    font: string;
    mono: string;
    desc: string;
  }

  const presets: ThemePreset[] = [
    { name: 'Default', accent: '#d4af37', font: 'Inter', mono: 'JetBrains Mono', desc: 'Gold accent, clean type' },
    { name: 'Professional', accent: '#3B82F6', font: 'IBM Plex Sans', mono: 'IBM Plex Mono', desc: 'Blue, IBM typefaces' },
    { name: 'Minimal', accent: '#06B6D4', font: 'Inter', mono: 'Fira Code', desc: 'Cyan, ligature-ready' },
    { name: 'Warm', accent: '#F97316', font: 'Lato', mono: 'Source Code Pro', desc: 'Orange, friendly type' },
    { name: 'Elegant', accent: '#8B5CF6', font: 'Source Sans 3', mono: 'Source Code Pro', desc: 'Purple, Adobe family' },
    { name: 'Terminal', accent: '#10B981', font: 'JetBrains Mono', mono: 'JetBrains Mono', desc: 'Green, all monospace' },
  ];

  // Track loaded Google Fonts to avoid duplicate <link> tags
  const loadedFonts = new Set<string>();

  function loadGoogleFont(fontName: string) {
    const spec = GOOGLE_FONTS[fontName];
    if (!spec || loadedFonts.has(fontName)) return;

    const link = document.createElement('link');
    link.rel = 'stylesheet';
    link.href = `https://fonts.googleapis.com/css2?family=${spec}&display=swap`;
    document.head.appendChild(link);
    loadedFonts.add(fontName);

    // Persist which fonts are loaded so main.ts can reload them
    const stored = JSON.parse(localStorage.getItem('iris-loaded-fonts') || '[]') as string[];
    if (!stored.includes(fontName)) {
      stored.push(fontName);
      localStorage.setItem('iris-loaded-fonts', JSON.stringify(stored));
    }
  }

  function applyTheme(theme: string) {
    if (theme === 'system') {
      const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      if (!prefersDark) {
        document.documentElement.setAttribute('data-brand', 'light');
      } else {
        document.documentElement.removeAttribute('data-brand');
      }
    } else if (theme === 'light') {
      document.documentElement.setAttribute('data-brand', 'light');
    } else {
      document.documentElement.removeAttribute('data-brand');
    }
    localStorage.setItem('iris-theme', theme);
  }

  function applyAccentColor(color: string) {
    const root = document.documentElement;
    root.style.setProperty('--iris-color-primary', color);
    root.style.setProperty('--iris-color-unread', color);
    root.style.setProperty('--iris-color-border', `${color}26`);
    root.style.setProperty('--iris-color-accent-border', `${color}2E`);
    root.style.setProperty('--iris-color-focus-ring', `${color}66`);
    localStorage.setItem('iris-accent-color', color);
  }

  function resolveFontStack(font: string): string {
    if (font === 'System Default') return 'system-ui, -apple-system, sans-serif';
    return `'${font}', system-ui, sans-serif`;
  }

  function resolveMonoStack(font: string): string {
    if (font === 'System Mono') return "'SF Mono', 'Cascadia Mono', 'Menlo', 'Consolas', monospace";
    return `'${font}', monospace`;
  }

  function applyFontFamily(font: string) {
    loadGoogleFont(font);
    document.documentElement.style.setProperty('--iris-font-family', resolveFontStack(font));
    localStorage.setItem('iris-font-family', font);
  }

  function applyFontMono(font: string) {
    loadGoogleFont(font);
    document.documentElement.style.setProperty('--iris-font-mono', resolveMonoStack(font));
    localStorage.setItem('iris-font-mono', font);
  }

  async function setTheme(theme: Theme) {
    currentTheme = theme;
    applyTheme(theme);
    try {
      await api.config.setTheme(theme);
    } catch {
      // Applied locally already
    }
  }

  async function setAccentColor(color: string) {
    accentColor = color;
    customHex = '';
    applyAccentColor(color);
    saving = true;
    try {
      await api.config.setAppearance({ accent_color: color });
    } catch {
      // Applied locally already
    } finally {
      saving = false;
    }
  }

  function applyCustomHex() {
    const hex = customHex.trim();
    if (/^#[0-9a-fA-F]{6}$/.test(hex)) {
      setAccentColor(hex);
    }
  }

  async function persistFont(key: string, value: string) {
    try {
      await api.config.setAppearance({ [key]: value });
    } catch {
      // Applied locally already
    }
  }

  async function applyPreset(preset: ThemePreset) {
    // Apply all three at once
    accentColor = preset.accent;
    fontFamily = preset.font;
    fontMono = preset.mono;
    prevFont = preset.font;
    prevMono = preset.mono;

    applyAccentColor(preset.accent);
    applyFontFamily(preset.font);
    applyFontMono(preset.mono);

    try {
      await api.config.setAppearance({
        accent_color: preset.accent,
        font_family: preset.font,
        font_mono: preset.mono,
      });
    } catch {
      // Applied locally
    }
  }

  // Watch font changes from FormSelect binding
  let prevFont = $state('Inter');
  let prevMono = $state('System Mono');
  $effect(() => {
    if (fontFamily !== prevFont) {
      prevFont = fontFamily;
      applyFontFamily(fontFamily);
      persistFont('font_family', fontFamily);
    }
  });
  $effect(() => {
    if (fontMono !== prevMono) {
      prevMono = fontMono;
      applyFontMono(fontMono);
      persistFont('font_mono', fontMono);
    }
  });

  // Load on mount
  $effect(() => {
    async function loadAppearance() {
      try {
        const config = await api.config.get();
        currentTheme = (config.theme as Theme) || 'system';
        applyTheme(currentTheme);
      } catch {
        currentTheme = 'system';
      }

      try {
        const appearance = await api.config.getAppearance();
        accentColor = appearance.accent_color;
        fontFamily = appearance.font_family;
        fontMono = appearance.font_mono;
        prevFont = fontFamily;
        prevMono = fontMono;
        applyAccentColor(accentColor);
        applyFontFamily(fontFamily);
        applyFontMono(fontMono);
      } catch {
        // Use defaults — also load default fonts
        loadGoogleFont('Inter');
      }

      loading = false;
    }
    loadAppearance();
  });

  function isCurrentAccent(color: string): boolean {
    return accentColor.toLowerCase() === color.toLowerCase();
  }

  function isActivePreset(preset: ThemePreset): boolean {
    return accentColor.toLowerCase() === preset.accent.toLowerCase()
      && fontFamily === preset.font
      && fontMono === preset.mono;
  }
</script>

<div class="space-y-8">
  <!-- Presets -->
  <section>
    <div class="section-header">
      <div class="section-header-row">
        <Layers size={16} style="color: var(--iris-color-primary);" />
        <h3 class="section-title">Presets</h3>
      </div>
      <p class="section-desc">Quick-apply a curated accent + font combination</p>
    </div>
    <div class="preset-grid">
      {#each presets as preset}
        <button
          class="preset-card"
          class:preset-card--active={isActivePreset(preset)}
          onclick={() => applyPreset(preset)}
          disabled={loading}
        >
          <div class="preset-swatch" style="background: {preset.accent};"></div>
          <div class="preset-info">
            <span class="preset-name">{preset.name}</span>
            <span class="preset-desc">{preset.desc}</span>
          </div>
          {#if isActivePreset(preset)}
            <span class="preset-check"><Check size={14} /></span>
          {/if}
        </button>
      {/each}
    </div>
  </section>

  <!-- Theme Mode -->
  <section>
    <div class="section-header">
      <h3 class="section-title">Theme</h3>
      <p class="section-desc">Choose your preferred color scheme</p>
    </div>
    <div class="theme-cards">
      {#each themes as theme}
        <button
          class="theme-card"
          class:theme-card--active={currentTheme === theme.value}
          onclick={() => setTheme(theme.value)}
          disabled={loading}
        >
          <div class="theme-preview theme-preview--{theme.value}">
            <div class="preview-topbar"></div>
            <div class="preview-sidebar"></div>
            <div class="preview-content">
              <div class="preview-line"></div>
              <div class="preview-line short"></div>
            </div>
          </div>
          <div class="theme-card-info">
            <span class="theme-card-label">{theme.label}</span>
            <span class="theme-card-desc">{theme.desc}</span>
          </div>
          {#if currentTheme === theme.value}
            <div class="theme-check">
              <Check size={14} />
            </div>
          {/if}
        </button>
      {/each}
    </div>
  </section>

  <!-- Accent Color -->
  <section>
    <div class="section-header">
      <div class="section-header-row">
        <Palette size={16} style="color: var(--iris-color-primary);" />
        <h3 class="section-title">Accent Color</h3>
      </div>
      <p class="section-desc">Customize your primary brand color</p>
    </div>
    <div class="accent-grid">
      {#each accentPresets as preset}
        <button
          class="accent-swatch"
          class:accent-swatch--active={isCurrentAccent(preset.color)}
          style="--swatch-color: {preset.color};"
          onclick={() => setAccentColor(preset.color)}
          title={preset.name}
          disabled={loading}
        >
          <span class="swatch-fill">
            {#if isCurrentAccent(preset.color)}
              <span class="swatch-check">
                <Check size={12} strokeWidth={3} />
              </span>
            {/if}
          </span>
          <span class="swatch-label">{preset.name}</span>
        </button>
      {/each}
    </div>
    <!-- Custom hex input -->
    <div class="custom-hex-row">
      <div class="custom-hex-preview" style="background: {/^#[0-9a-fA-F]{6}$/.test(customHex) ? customHex : accentColor};"></div>
      <input
        class="custom-hex-input"
        type="text"
        placeholder="#custom hex"
        maxlength="7"
        bind:value={customHex}
        onkeydown={(e: KeyboardEvent) => { if (e.key === 'Enter') applyCustomHex(); }}
        disabled={loading}
      />
      <button
        class="custom-hex-apply"
        onclick={applyCustomHex}
        disabled={loading || !/^#[0-9a-fA-F]{6}$/.test(customHex)}
      >
        Apply
      </button>
    </div>
  </section>

  <!-- Interface Font -->
  <section>
    <div class="section-header">
      <div class="section-header-row">
        <Type size={16} style="color: var(--iris-color-text-muted);" />
        <h3 class="section-title">Interface Font</h3>
      </div>
      <p class="section-desc">Font used throughout the UI</p>
    </div>
    <div class="font-selector">
      <FormSelect
        bind:value={fontFamily}
        options={fontOptions}
        disabled={loading}
      />
      <p class="font-preview" style="font-family: {resolveFontStack(fontFamily)};">
        The quick brown fox jumps over the lazy dog — 0123456789
      </p>
    </div>
  </section>

  <!-- Code Font -->
  <section>
    <div class="section-header">
      <div class="section-header-row">
        <Type size={16} style="color: var(--iris-color-text-muted);" />
        <h3 class="section-title">Code Font</h3>
      </div>
      <p class="section-desc">Monospace font for code snippets and metadata</p>
    </div>
    <div class="font-selector">
      <FormSelect
        bind:value={fontMono}
        options={monoFontOptions}
        disabled={loading}
      />
      <p class="font-preview mono" style="font-family: {resolveMonoStack(fontMono)};">
        const iris = new EmailClient();
      </p>
    </div>
  </section>
</div>

<style>
  .section-header {
    margin-bottom: 12px;
  }

  .section-header-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .section-title {
    font-size: 13px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--iris-color-text-muted);
    margin: 0;
  }

  .section-desc {
    font-size: 12px;
    color: var(--iris-color-text-faint);
    margin: 4px 0 0 0;
  }

  /* Preset cards */
  .preset-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
  }

  .preset-card {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid var(--iris-color-border-subtle);
    border-radius: var(--iris-radius-md);
    background: transparent;
    cursor: pointer;
    transition: border-color var(--iris-transition-fast), background var(--iris-transition-fast);
    text-align: left;
    position: relative;
  }

  .preset-card:hover {
    border-color: var(--iris-color-text-faint);
    background: var(--iris-color-ghost-hover);
  }

  .preset-card--active {
    border-color: var(--iris-color-primary);
    background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent);
  }

  .preset-card--active:hover {
    border-color: var(--iris-color-primary);
  }

  .preset-swatch {
    width: 24px;
    height: 24px;
    border-radius: var(--iris-radius-full);
    flex-shrink: 0;
  }

  .preset-info {
    flex: 1;
    min-width: 0;
  }

  .preset-name {
    display: block;
    font-size: 13px;
    font-weight: 600;
    color: var(--iris-color-text);
  }

  .preset-desc {
    display: block;
    font-size: 11px;
    color: var(--iris-color-text-faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .preset-check {
    color: var(--iris-color-primary);
    flex-shrink: 0;
    display: flex;
  }

  /* Theme cards */
  .theme-cards {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
  }

  .theme-card {
    position: relative;
    display: flex;
    flex-direction: column;
    border: 2px solid var(--iris-color-border-subtle);
    border-radius: var(--iris-radius-lg);
    background: transparent;
    cursor: pointer;
    overflow: hidden;
    transition: border-color var(--iris-transition-fast), box-shadow var(--iris-transition-fast);
    padding: 0;
  }

  .theme-card:hover {
    border-color: var(--iris-color-text-faint);
  }

  .theme-card--active {
    border-color: var(--iris-color-primary);
    box-shadow: 0 0 0 1px var(--iris-color-primary);
  }

  .theme-card--active:hover {
    border-color: var(--iris-color-primary);
  }

  .theme-preview {
    height: 72px;
    position: relative;
    border-radius: var(--iris-radius-md) var(--iris-radius-md) 0 0;
    overflow: hidden;
  }

  .theme-preview--light {
    background: #FAF9F6;
  }

  .theme-preview--dark {
    background: #0a0a0a;
  }

  .theme-preview--system {
    background: linear-gradient(135deg, #FAF9F6 50%, #0a0a0a 50%);
  }

  .preview-topbar {
    height: 10px;
    background: rgba(128, 128, 128, 0.15);
  }

  .preview-sidebar {
    position: absolute;
    top: 10px;
    left: 0;
    width: 20%;
    height: 100%;
    background: rgba(128, 128, 128, 0.08);
  }

  .preview-content {
    position: absolute;
    top: 18px;
    left: 28%;
    right: 12%;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .preview-line {
    height: 4px;
    border-radius: 2px;
    background: rgba(128, 128, 128, 0.2);
  }

  .preview-line.short {
    width: 60%;
  }

  .theme-card-info {
    padding: 10px 12px;
    text-align: left;
  }

  .theme-card-label {
    display: block;
    font-size: 13px;
    font-weight: 600;
    color: var(--iris-color-text);
  }

  .theme-card-desc {
    display: block;
    font-size: 11px;
    color: var(--iris-color-text-faint);
    margin-top: 2px;
  }

  .theme-check {
    position: absolute;
    top: 8px;
    right: 8px;
    width: 20px;
    height: 20px;
    border-radius: var(--iris-radius-full);
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  /* Accent color swatches */
  .accent-grid {
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    gap: 8px;
    margin-bottom: 12px;
  }

  .accent-swatch {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    background: none;
    border: none;
    cursor: pointer;
    padding: 4px;
    border-radius: var(--iris-radius-md);
    transition: background var(--iris-transition-fast);
  }

  .accent-swatch:hover {
    background: var(--iris-color-ghost-hover);
  }

  .swatch-fill {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: var(--iris-radius-full);
    background: var(--swatch-color);
    border: 2px solid transparent;
    transition: border-color var(--iris-transition-fast), transform var(--iris-transition-fast);
  }

  .accent-swatch--active .swatch-fill {
    border-color: var(--iris-color-text);
    transform: scale(1.1);
  }

  .swatch-check {
    color: #fff;
    filter: drop-shadow(0 1px 2px rgba(0,0,0,0.3));
    display: flex;
    align-items: center;
    justify-content: center;
    line-height: 0;
  }

  .swatch-label {
    font-size: 10px;
    color: var(--iris-color-text-faint);
    white-space: nowrap;
  }

  /* Custom hex input */
  .custom-hex-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .custom-hex-preview {
    width: 28px;
    height: 28px;
    border-radius: var(--iris-radius-full);
    border: 2px solid var(--iris-color-border);
    flex-shrink: 0;
    transition: background var(--iris-transition-fast);
  }

  .custom-hex-input {
    flex: 1;
    max-width: 140px;
    font-family: var(--iris-font-mono);
    font-size: 13px;
    color: var(--iris-color-text);
    background: var(--iris-color-input-bg);
    border: 1px solid var(--iris-color-input-border);
    border-radius: var(--iris-radius-md);
    padding: 6px 10px;
    outline: none;
    transition: border-color var(--iris-transition-fast);
  }

  .custom-hex-input:focus {
    border-color: var(--iris-color-primary);
    box-shadow: 0 0 0 3px var(--iris-color-focus-ring);
  }

  .custom-hex-input::placeholder {
    color: var(--iris-color-text-faint);
  }

  .custom-hex-apply {
    font-size: 12px;
    font-weight: 500;
    padding: 6px 14px;
    border-radius: var(--iris-radius-md);
    border: 1px solid var(--iris-color-border);
    background: var(--iris-color-bg-elevated);
    color: var(--iris-color-text);
    cursor: pointer;
    transition: all var(--iris-transition-fast);
  }

  .custom-hex-apply:hover:not(:disabled) {
    border-color: var(--iris-color-primary);
    color: var(--iris-color-primary);
  }

  .custom-hex-apply:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  /* Font selector */
  .font-selector {
    max-width: 280px;
  }

  .font-preview {
    margin: 8px 0 0 0;
    padding: 10px 12px;
    border-radius: var(--iris-radius-md);
    background: var(--iris-color-bg-surface);
    border: 1px solid var(--iris-color-border-subtle);
    font-size: 14px;
    color: var(--iris-color-text);
    line-height: 1.5;
  }

  .font-preview.mono {
    font-size: 13px;
  }

  @media (max-width: 640px) {
    .accent-grid {
      grid-template-columns: repeat(4, 1fr);
    }

    .theme-cards {
      grid-template-columns: 1fr;
    }

    .preset-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
