// Load the client source through Vite (Svelte and TypeScript) and render a
// component on the server. The tests check the HTML.
import { after } from "node:test";
import { createServer } from "vite";

const vite = await createServer({
  root: new URL("..", import.meta.url).pathname,
  server: { middlewareMode: true, hmr: false, ws: false },
  appType: "custom",
  logLevel: "error",
});
after(() => vite.close());

export const load = (path) => vite.ssrLoadModule(path);

export async function render(path, props = {}) {
  // Load svelte/server through Vite too, so that the component and the renderer share one Svelte.
  const [{ default: component }, { render: svelteRender }] = await Promise.all([load(path), load("svelte/server")]);
  return svelteRender(component, { props }).body;
}
