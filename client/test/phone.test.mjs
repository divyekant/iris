// The phone view (card R6-05): at 360, 390 and 768 px on a touch screen, the
// brief, the review, an item and a mail show no horizontal scroll, and each
// control is at least 44 px in each direction. Runs the fixtures dev server in
// a real browser (Playwright), with reduced motion, so that no animation changes a size.
import { test, after } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { createServer } from "vite";

let playwright = null;
try {
  playwright = createRequire(import.meta.url)("playwright");
} catch {
  // shortcut: no Playwright on this machine (it is not a dependency), so the test skips. Add it to package.json when CI runs the client in a browser.
}

// c-311: a commitment with a quote. v-88: a conversation with 3 source mails. r-1201: a record.
const ROUTES = ["#/brief", "#/review", "#/item/c-311", "#/item/v-88", "#/item/r-1201", "#/mail/m-55210"];
const WIDTHS = [360, 390, 768];
const TOUCH = 44;

// The controls in the page that are smaller than TOUCH, and the overflow to the right.
// A link inside a sentence is exempt (WCAG 2.5.8). A link with an absolute ::after
// covers its positioned parent, so that parent is its target. A checkbox or a radio
// in a label has the label as its target.
function measure(touch) {
  const small = [];
  for (const e of document.querySelectorAll("a[href], button, input, select, textarea, summary")) {
    const style = getComputedStyle(e);
    if (style.visibility === "hidden" || style.display === "none") continue;
    let box = e.getBoundingClientRect();
    if (!box.width || !box.height) continue;
    const after = getComputedStyle(e, "::after");
    if (e.tagName === "A" && after.content !== "none" && after.position === "absolute" && e.offsetParent) box = e.offsetParent.getBoundingClientRect();
    if (e.type === "checkbox" || e.type === "radio") {
      const label = e.closest("label");
      if (label) box = label.getBoundingClientRect();
    }
    const parent = e.parentElement;
    if (style.display === "inline" && parent && parent.textContent.trim().length > e.textContent.trim().length + 2) continue;
    if (box.height < touch || box.width < touch) small.push(`${e.tagName.toLowerCase()} "${(e.textContent || "").trim().slice(0, 30)}" ${Math.round(box.width)}x${Math.round(box.height)}`);
  }
  return { overflow: document.documentElement.scrollWidth - innerWidth, small };
}

// The fixtures dev server and a browser, closed after the tests.
async function serve() {
  const vite = await createServer({
    root: new URL("..", import.meta.url).pathname,
    mode: "fixtures",
    // Its own cache: the other test files start Vite at the same time, and a shared dependency cache reloads the page.
    cacheDir: "node_modules/.vite-phone",
    server: { port: 0, hmr: false },
    logLevel: "error",
  });
  await vite.listen();
  const browser = await playwright.chromium.launch();
  after(async () => {
    await browser.close();
    await vite.close();
  });
  return { base: vite.resolvedUrls.local[0], browser };
}

test("the brief, the review, an item and a mail fit a phone", { skip: !playwright && "Playwright is not installed" }, async () => {
  const { base, browser } = await serve();
  for (const width of WIDTHS) {
    const page = await browser.newPage({ viewport: { width, height: 800 }, isMobile: true, hasTouch: true, reducedMotion: "reduce" });
    for (const route of ROUTES) {
      await page.goto(base + route);
      await page.waitForSelector("main h1, main h2", { timeout: 10000 }).catch(() => assert.fail(`${route} at ${width} px did not show`));
      const { overflow, small } = await page.evaluate(measure, TOUCH);
      assert.equal(overflow, 0, `${route} at ${width} px scrolls sideways`);
      assert.deepEqual(small, [], `${route} at ${width} px has small controls`);
    }
    // The edit state of a proposed matter has a checkbox for each item.
    await page.goto(base + "#/brief");
    await page.getByRole("button", { name: "Keep together" }).click();
    await page.getByRole("checkbox").first().waitFor();
    assert.deepEqual((await page.evaluate(measure, TOUCH)).small, [], `the matter edit state at ${width} px has small controls`);
    await page.close();
  }
});

test("on a phone, More opens the other views and a tap goes to one", { skip: !playwright && "Playwright is not installed" }, async () => {
  const { base, browser } = await serve();
  const page = await browser.newPage({ viewport: { width: 360, height: 800 }, isMobile: true, hasTouch: true, reducedMotion: "reduce" });
  await page.goto(base + "#/brief");
  const tabs = page.getByRole("navigation", { name: "Main views" });
  assert.deepEqual(await tabs.getByRole("link").allInnerTexts(), ["Brief", "Matters", "Ask", "Review"]);
  // The top row of views is for a wide screen only.
  assert.equal(await page.getByRole("navigation", { name: "Views", exact: true }).isVisible(), false);
  await tabs.getByRole("button", { name: "More" }).tap();
  const more = page.locator("#more");
  await more.getByRole("link", { name: "Jobs" }).tap();
  await page.waitForURL(/#\/jobs$/);
  await more.waitFor({ state: "hidden", timeout: 2000 });

  // A short screen (a narrow window in landscape): the list fits above the tabs and scrolls.
  await page.setViewportSize({ width: 600, height: 390 });
  await tabs.getByRole("button", { name: "More" }).tap();
  const box = await more.boundingBox();
  assert.ok(box.y >= 0, `More starts at ${box.y} px`);
  const last = more.getByRole("link", { name: "Signals" });
  await last.scrollIntoViewIfNeeded();
  await last.tap();
  await page.waitForURL(/#\/signals$/);
  assert.equal(await tabs.getByRole("button", { name: "More" }).evaluate((b) => b.classList.contains("current")), true);
});

// Each control and line of text that is in the visible area keeps out of the safe-area
// insets (the notch, the rounded corners and the home indicator).
// At the end of the page (atEnd), nothing is under the tabs or the home indicator.
function unsafe([top, right, bottom, left, atEnd]) {
  const out = [];
  const tabs = document.querySelector(".tabs");
  const bar = tabs && getComputedStyle(tabs).display !== "none" ? tabs.getBoundingClientRect() : null;
  const name = (e) => `${e.tagName.toLowerCase()} "${(e.textContent || "").trim().slice(0, 20)}"`;
  for (const e of document.querySelectorAll("header *, main *, footer *, .tabs *")) {
    if (!e.matches("a, button, input, h1, h2, h3, p, li, svg")) continue;
    let box = e.getBoundingClientRect();
    if (!box.width || !box.height || box.bottom <= 0 || box.top >= innerHeight) continue;
    // An element in a box that scrolls (the top row of views, a long list) shows only inside that box.
    for (let a = e.parentElement; a && a !== document.body; a = a.parentElement) {
      if (getComputedStyle(a).overflow === "visible") continue;
      const clip = a.getBoundingClientRect();
      box = { left: Math.max(box.left, clip.left), right: Math.min(box.right, clip.right), top: Math.max(box.top, clip.top), bottom: Math.min(box.bottom, clip.bottom) };
    }
    if (box.right <= box.left || box.bottom <= box.top) continue;
    if (box.left < left - 0.5 || box.right > innerWidth - right + 0.5) out.push(`${name(e)} at the side`);
    if (!atEnd && box.top < top - 0.5) out.push(`${name(e)} at the top`);
    const limit = e.closest(".tabs") || !bar ? innerHeight - bottom : bar.top;
    if ((atEnd || e.closest(".tabs")) && box.bottom > limit + 0.5) out.push(`${name(e)} at the bottom`);
  }
  return out;
}

test("the page keeps out of the notch and the home indicator", { skip: !playwright && "Playwright is not installed" }, async () => {
  const { base, browser } = await serve();
  const cases = [
    { width: 390, height: 844, insets: { top: 47, right: 0, bottom: 34, left: 0 } },
    { width: 844, height: 390, insets: { top: 0, right: 47, bottom: 21, left: 47 } },
  ];
  for (const { width, height, insets } of cases) {
    const page = await browser.newPage({ viewport: { width, height }, isMobile: true, hasTouch: true, reducedMotion: "reduce" });
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Emulation.setSafeAreaInsetsOverride", { insets });
    const edges = [insets.top, insets.right, insets.bottom, insets.left, false];
    for (const route of ["#/brief", "#/review"]) {
      await page.goto(base + route);
      await page.waitForSelector("main h1, main h2");
      assert.deepEqual(await page.evaluate(unsafe, edges), [], `${route} at ${width} x ${height}, top of the page`);
      // At the end of the page, the footer is above the tabs or the home indicator.
      await page.evaluate(() => scrollTo(0, document.documentElement.scrollHeight));
      assert.deepEqual(await page.evaluate(unsafe, [0, insets.right, insets.bottom, insets.left, true]), [], `${route} at ${width} x ${height}, end of the page`);
    }
    await page.close();
  }
});
