// The phone view (card R6-05): at 360, 390 and 768 px on a touch screen, the
// brief, the review, an item and a mail show no horizontal scroll, and each
// control is at least 44 px in each direction. Runs the fixtures dev server in
// a real browser (Playwright).
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

const ROUTES = ["#/brief", "#/review", "#/item/c-311", "#/item/r-1201", "#/mail/m-55210"];
const WIDTHS = [360, 390, 768];
const TOUCH = 44;

// The controls in the page that are smaller than TOUCH, and the overflow to the right.
// A link inside a sentence is exempt (WCAG 2.5.8). A link with an absolute ::after
// covers its positioned parent, so that parent is its target.
function measure(touch) {
  const small = [];
  for (const e of document.querySelectorAll("a[href], button, input, select, textarea, summary")) {
    const style = getComputedStyle(e);
    if (style.visibility === "hidden" || style.display === "none") continue;
    let box = e.getBoundingClientRect();
    if (!box.width || !box.height) continue;
    const after = getComputedStyle(e, "::after");
    if (after.content !== "none" && after.position === "absolute" && e.offsetParent) box = e.offsetParent.getBoundingClientRect();
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
    const page = await browser.newPage({ viewport: { width, height: 800 }, isMobile: true, hasTouch: true });
    for (const route of ROUTES) {
      await page.goto(base + route);
      await page.waitForSelector("main h1, main h2", { timeout: 10000 }).catch(() => assert.fail(`${route} at ${width} px did not show`));
      const { overflow, small } = await page.evaluate(measure, TOUCH);
      assert.equal(overflow, 0, `${route} at ${width} px scrolls sideways`);
      assert.deepEqual(small, [], `${route} at ${width} px has small controls`);
    }
    await page.close();
  }
});

test("on a phone, More opens the other views and a tap goes to one", { skip: !playwright && "Playwright is not installed" }, async () => {
  const { base, browser } = await serve();
  const page = await browser.newPage({ viewport: { width: 360, height: 800 }, isMobile: true, hasTouch: true });
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
  assert.equal(await tabs.getByRole("button", { name: "More" }).evaluate((b) => b.classList.contains("current")), true);
});
