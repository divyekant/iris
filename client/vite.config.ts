import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { answer, raw } from "./dev/fixtures.ts";

// `pnpm dev` sends /api to an engine on 127.0.0.1:4800 (`iris serve`).
// `pnpm dev --mode fixtures` answers /api from the schema examples, with no engine.
export default defineConfig(({ mode }) => {
  const fixtures = mode === "fixtures";
  return {
  plugins: [
    svelte(),
    fixtures && {
      name: "iris-fixtures",
      configureServer(server) {
        server.middlewares.use("/api/", (req, res) => {
          const url = (req as { url?: string }).url ?? "";
          const original = url.match(/^\/messages\/([^/?]+)\/raw$/);
          const source = original && raw(decodeURIComponent(original[1]));
          if (source) {
            res.setHeader("content-type", "message/rfc822");
            res.setHeader("content-disposition", `attachment; filename="${original[1]}.eml"`);
            res.end(source);
            return;
          }
          const body = answer(url);
          res.statusCode = body === undefined ? 404 : 200;
          res.setHeader("content-type", "application/json");
          res.end(JSON.stringify(body ?? { error: "Not found." }));
        });
      },
    },
  ],
  server: fixtures ? {} : { proxy: { "/api": "http://127.0.0.1:4800" } },
  build: { outDir: "dist", emptyOutDir: true },
  };
});
