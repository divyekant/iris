// Mount a component in a DOM (happy-dom), so that its effects and its event
// handlers run. render.mjs renders on the server, where effects do not run.
import { after } from "node:test";
import { Window } from "happy-dom";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { createServer } from "vite";

const window = new Window({ url: "http://localhost/" });
for (const name of ["window", "document", "navigator", "Node", "Element", "HTMLElement", "Text", "Comment", "Event", "MouseEvent", "getComputedStyle", "requestAnimationFrame", "HTMLMediaElement"]) {
  Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: name === "window" ? window : window[name] });
}

// Compile each component for the client, and load the client build of Svelte.
const vite = await createServer({
  configFile: false,
  // Its own cache: the render.mjs server of another test file runs at the same time.
  cacheDir: "node_modules/.vite-mount",
  root: new URL("..", import.meta.url).pathname,
  plugins: [svelte({ dynamicCompileOptions: () => ({ generate: "client", hmr: false }) })],
  ssr: { noExternal: true, resolve: { conditions: ["browser", "development"] } },
  server: { middlewareMode: true, hmr: false, ws: false },
  appType: "custom",
  logLevel: "error",
});
after(async () => {
  await vite.close();
  await window.happyDOM.close();
});

export const load = (path) => vite.ssrLoadModule(path);

/** Mount the component at `path` with `props`. Give the root element and a function to remove it. */
export async function mount(path, props = {}) {
  const [{ default: component }, { mount: svelteMount, unmount, flushSync }] = await Promise.all([load(path), load("svelte")]);
  const target = document.createElement("div");
  document.body.append(target);
  const app = svelteMount(component, { target, props });
  flushSync();
  return { target, flushSync, destroy: () => (unmount(app), target.remove()) };
}

/** Wait until `check()` gives a true value, or fail after about 2 seconds. */
export async function until(check) {
  for (let i = 0; i < 200; i++) {
    const value = check();
    if (value) return value;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error("The condition did not come true.");
}
