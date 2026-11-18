// The phone view (card R6-05): at 360, 390 and 768 px on a touch screen, the
// brief, the review, an item and a mail show no horizontal scroll, and each
// control is at least 44 px in each direction. Runs the fixtures dev server in
// headless Chromium (playwright-core; the client setup installs the shell), with reduced
// motion, so that no animation changes a size.
import { test, after } from "node:test";
import assert from "node:assert/strict";
import { chromium } from "playwright-core";
import { createServer } from "vite";

// c-311: a commitment with a quote. v-88: a conversation with 3 source mails. r-1201: a record.
const ROUTES = ["#/brief", "#/review", "#/item/c-311", "#/item/v-88", "#/item/r-1201", "#/mail/m-55210", "#/write", "#/write/t-8841"];
const WIDTHS = [360, 390, 768];
const TOUCH = 44;

// The controls in the page that are smaller than TOUCH, and the overflow to the right.
// A control inside a sentence (its parent has text of its own) is exempt (WCAG 2.5.8). A link with an absolute ::after
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
    if (parent && [...parent.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.textContent.trim())) continue;
    if (box.height < touch || box.width < touch) small.push(`${e.tagName.toLowerCase()} "${(e.textContent || "").trim().slice(0, 30)}" ${Math.round(box.width)}x${Math.round(box.height)}`);
  }
  return { overflow: document.documentElement.scrollWidth - innerWidth, small };
}

// Ask one question on the Ask view, and wait for the answer and its sources.
async function ask(page, question) {
  const answers = await page.getByRole("list", { name: "Sources" }).count();
  await page.locator("main form input").fill(question);
  await page.locator("main form button[type=submit]").click();
  await page.getByRole("list", { name: "Sources" }).nth(answers).waitFor();
}

// The fixtures dev server and a browser, closed after the tests. Each one gets its
// close hook as soon as it opens, so a step that fails later leaves nothing open.
// The browser starts first: it is the step that fails on a machine without the
// Chromium system libraries.
async function serve(launch = () => chromium.launch()) {
  const browser = await launch().catch((e) => {
    throw new Error(`Chromium did not start. On Linux, install its system libraries one time: cd client && npx playwright-core install-deps chromium\n${e.message}`);
  });
  after(() => browser.close());
  const vite = await createServer({
    root: new URL("..", import.meta.url).pathname,
    mode: "fixtures",
    // Its own cache: the other test files start Vite at the same time, and a shared dependency cache reloads the page.
    cacheDir: "node_modules/.vite-phone",
    server: { port: 0, hmr: false },
    logLevel: "error",
  });
  after(() => vite.close());
  await vite.listen();
  return { base: vite.resolvedUrls.local[0], browser };
}

test("a browser that does not start names the fix and leaves no server open", async () => {
  const servers = () => process.getActiveResourcesInfo().filter((r) => r === "TCPServerWrap").length;
  const open = servers();
  await assert.rejects(serve(() => Promise.reject(new Error("libnss3.so: cannot open shared object file"))), /install-deps chromium/);
  assert.equal(servers(), open);
});

test("the brief, the review, an item and a mail fit a phone", async () => {
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
    // A job that waits, with its mail links.
    await page.goto(base + "#/job/j-12");
    await page.waitForSelector("main h1, main h2");
    assert.deepEqual((await page.evaluate(measure, TOUCH)).small, [], `a waiting job at ${width} px has small controls`);
    // An Ask answer, with its sources.
    await page.goto(base + "#/ask");
    await ask(page, "What do I owe Dana?");
    assert.deepEqual((await page.evaluate(measure, TOUCH)).small, [], `an Ask answer at ${width} px has small controls`);
    // The edit state of a proposed matter has a checkbox for each item.
    await page.goto(base + "#/brief");
    await page.getByRole("button", { name: "Keep together" }).click();
    await page.getByRole("checkbox").first().waitFor();
    assert.deepEqual((await page.evaluate(measure, TOUCH)).small, [], `the matter edit state at ${width} px has small controls`);
    await page.close();
  }
});

test("on a phone, More opens the other views and a tap goes to one", async () => {
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
  // A tap on the view that is open closes More too.
  await tabs.getByRole("button", { name: "More" }).tap();
  await more.getByRole("link", { name: "Jobs" }).tap();
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

test("the page keeps out of the notch and the home indicator", async () => {
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

// Each element that sticks to the bottom of the screen (position sticky or fixed) is above the tabs.
function covered() {
  const tabs = document.querySelector(".tabs");
  if (!tabs || getComputedStyle(tabs).display === "none") return [];
  const bar = tabs.getBoundingClientRect();
  const out = [];
  for (const e of document.querySelectorAll("body *")) {
    if (e.closest(".tabs, #more")) continue;
    const style = getComputedStyle(e);
    if (style.position !== "sticky" && style.position !== "fixed") continue;
    const box = e.getBoundingClientRect();
    if (box.height && box.bottom > bar.top + 0.5 && box.top < bar.bottom) out.push(`${e.tagName.toLowerCase()}.${e.className.split(" ")[0]} ends at ${Math.round(box.bottom)}, tabs start at ${Math.round(bar.top)}`);
  }
  return out;
}

test("the tabs cover nothing that sticks to the bottom of the screen", async () => {
  const { base, browser } = await serve();
  const page = await browser.newPage({ viewport: { width: 360, height: 640 }, isMobile: true, hasTouch: true, reducedMotion: "reduce" });
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Emulation.setSafeAreaInsetsOverride", { insets: { top: 0, right: 0, bottom: 34, left: 0 } });
  for (const route of ROUTES) {
    await page.goto(base + route);
    await page.waitForSelector("main h1, main h2");
    assert.deepEqual(await page.evaluate(covered), [], route);
  }
  // A long Ask conversation: the question box sticks to the bottom while the page scrolls.
  await page.goto(base + "#/ask");
  await ask(page, "What do I owe Dana?");
  await ask(page, "And what does Dana owe me?");
  await page.evaluate(() => scrollTo(0, 200));
  assert.equal(await page.locator("main form").evaluate((f) => getComputedStyle(f).position), "sticky");
  assert.deepEqual(await page.evaluate(covered), [], "a long Ask conversation");
  await page.locator("main form input").tap();
  await page.locator("main form input").fill("One more question");
  await page.locator("main form button[type=submit]").tap();
});

// A laptop with a touch screen and a mouse (issue #528): the mouse is the primary
// pointer, so `(pointer: coarse)` and `(hover: none)` do not match, but
// `(any-pointer: coarse)` does. Each control of the brief is a touch target, and the
// quiet verbs that show on hover show at once.
const HYBRID = "--blink-settings=primaryPointerType=4,availablePointerTypes=6,primaryHoverType=2,availableHoverTypes=3";

test("on a touch laptop with a mouse, the brief shows its quiet verbs and each control is a touch target", async () => {
  const { base, browser } = await serve(() => chromium.launch({ args: [HYBRID] }));
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, reducedMotion: "reduce" });
  assert.deepEqual(await page.evaluate(() => [matchMedia("(pointer: coarse)").matches, matchMedia("(any-pointer: coarse)").matches]), [false, true], "the browser is a hybrid device");
  await page.goto(base + "#/brief");
  await page.waitForSelector("main h1, main h2");
  await page.mouse.move(0, 0);
  const hidden = await page.evaluate(() => [...document.querySelectorAll(".away, .stop")].filter((e) => getComputedStyle(e).opacity !== "1").map((e) => e.textContent.trim()));
  assert.ok((await page.locator(".away").count()) > 0, "the brief has a quiet verb");
  assert.deepEqual(hidden, [], "each quiet verb shows with no hover");
  assert.deepEqual((await page.evaluate(measure, TOUCH)).small, [], "the brief has small controls");
  await page.close();
});
