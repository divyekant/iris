// Checks the design tokens in src/tokens.css against .kalos.yaml, and checks
// WCAG AA contrast (4.5) for each text color on each background color.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../src/tokens.css", import.meta.url), "utf8");
const kalos = readFileSync(new URL("../../.kalos.yaml", import.meta.url), "utf8");

const MIN_CONTRAST = 4.5;
const TEXT = ["text", "text_muted", "text_faint"];
const BACKGROUNDS = ["background", "background_elevated", "background_surface"];

// Get the `--color-*` values that follow the `/* palette: <name> */` comment.
function cssPalette(name) {
  const block = css.split(`/* palette: ${name} */`)[1].split("/* palette:")[0];
  const colors = {};
  for (const [, key, value] of block.matchAll(/--color-([\w-]+):\s*([^;]+);/g)) {
    colors[key.replaceAll("-", "_")] = value.trim().toLowerCase();
  }
  return colors;
}

// shortcut: reads only `key: "value"` lines under brands.palettes.<name>.colors
// in .kalos.yaml; use a YAML parser if the file gets other layouts.
function kalosPalette(name) {
  const block = kalos.split(`\n    ${name}:\n      colors:\n`)[1];
  const colors = {};
  for (const line of block.split("\n")) {
    const m = line.match(/^ {8}(\w+): "([^"]+)"$/);
    if (!m) break;
    colors[m[1]] = m[2].toLowerCase();
  }
  return colors;
}

export function luminance(hex) {
  const [r, g, b] = [1, 3, 5]
    .map((i) => parseInt(hex.slice(i, i + 2), 16) / 255)
    .map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

test("contrast gives the WCAG values for black and white", () => {
  assert.equal(contrast("#000000", "#ffffff"), 21);
  assert.equal(contrast("#777777", "#777777"), 1);
  assert.ok(contrast("#767676", "#ffffff") >= 4.5);
  assert.ok(contrast("#777777", "#ffffff") < 4.5);
});

for (const name of ["dark", "light"]) {
  test(`${name} palette in tokens.css agrees with .kalos.yaml`, () => {
    const fromKalos = kalosPalette(name);
    delete fromKalos.neutral; // a palette name for Tailwind, not a color
    const fromCss = cssPalette(name);
    for (const [key, value] of Object.entries(fromKalos)) {
      assert.equal(fromCss[key], value, `--color-${key.replaceAll("_", "-")}`);
    }
  });

  test(`${name} palette text meets WCAG AA contrast`, () => {
    const colors = cssPalette(name);
    for (const text of TEXT) {
      for (const bg of BACKGROUNDS) {
        const ratio = contrast(colors[text], colors[bg]);
        assert.ok(
          ratio >= MIN_CONTRAST,
          `${text} ${colors[text]} on ${bg} ${colors[bg]}: ${ratio.toFixed(2)}`,
        );
      }
    }
  });
}

test("spacing tokens are the .kalos.yaml scale times the 4px base", () => {
  const scale = JSON.parse(kalos.match(/^ {4}scale: (\[.*\])$/m)[1]);
  for (const n of scale) {
    const m = css.match(new RegExp(`--space-${n}: (\\d+)(px)?;`));
    assert.ok(m, `--space-${n}`);
    assert.equal(Number(m[1]), n * 4, `--space-${n}`);
  }
});

test("type scale is 1.25 from a 16px base", () => {
  for (let step = -2; step <= 4; step++) {
    const m = css.match(new RegExp(`--font-size-${step}: ([\\d.]+)rem;`));
    assert.ok(m, `--font-size-${step}`);
    assert.ok(Math.abs(Number(m[1]) - 1.25 ** step) < 0.0001, `--font-size-${step}`);
  }
});

test("the build shows the Iris name", () => {
  const html = readFileSync(new URL("../dist/index.html", import.meta.url), "utf8");
  assert.match(html, /<title>Iris<\/title>/);
});
