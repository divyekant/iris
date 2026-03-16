import { fade as svelteFade, slide as svelteSlide, scale as svelteScale } from 'svelte/transition';

// Duration constants (match tokens.css values)
const FAST = 120;
const NORMAL = 200;

// Named transitions for consistent use across all components
export function irisFade(node: HTMLElement, params: { duration?: number; delay?: number } = {}) {
  return svelteFade(node, { duration: params.duration ?? FAST, delay: params.delay ?? 0 });
}

export function irisSlide(node: HTMLElement, params: { duration?: number; delay?: number; axis?: 'x' | 'y' } = {}) {
  return svelteSlide(node, { duration: params.duration ?? NORMAL, delay: params.delay ?? 0, axis: params.axis ?? 'y' });
}

export function irisScale(node: HTMLElement, params: { duration?: number; delay?: number; start?: number } = {}) {
  return svelteScale(node, { duration: params.duration ?? 150, delay: params.delay ?? 0, start: params.start ?? 0.95 });
}

// Collapse: height auto-animate for list item removal
export function irisCollapse(node: HTMLElement, params: { duration?: number; delay?: number } = {}) {
  const height = node.offsetHeight;
  const paddingTop = parseFloat(getComputedStyle(node).paddingTop);
  const paddingBottom = parseFloat(getComputedStyle(node).paddingBottom);
  const marginTop = parseFloat(getComputedStyle(node).marginTop);
  const marginBottom = parseFloat(getComputedStyle(node).marginBottom);

  return {
    duration: params.duration ?? NORMAL,
    delay: params.delay ?? 0,
    css: (t: number) => `
      height: ${t * height}px;
      padding-top: ${t * paddingTop}px;
      padding-bottom: ${t * paddingBottom}px;
      margin-top: ${t * marginTop}px;
      margin-bottom: ${t * marginBottom}px;
      opacity: ${t};
      overflow: hidden;
    `
  };
}

// Staggered fade for lists of action buttons
export function staggeredFade(node: HTMLElement, params: { index?: number; stagger?: number } = {}) {
  const index = params.index ?? 0;
  const stagger = params.stagger ?? 30;
  return svelteFade(node, { duration: FAST, delay: index * stagger });
}
