import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { act, answer, answerSender, askAnswer, commentPost, replyDraft, decide, noticePost, undoEntry, outboxPost, policyPost, raw, startJob, stopPost, watchPost, withdraw } from "./dev/fixtures.ts";
import { engineProxy } from "./dev/proxy.ts";

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
          if (url === "/ask" && req.method === "POST") {
            let text = "";
            req.on("data", (part: Buffer) => (text += part));
            req.on("end", () => {
              res.setHeader("content-type", "application/json");
              let question: unknown;
              try {
                question = JSON.parse(text)?.question;
              } catch {
                question = undefined;
              }
              // As the engine: a body with no question text is a 400.
              if (typeof question !== "string" || !question.trim()) {
                res.statusCode = 400;
                res.end(JSON.stringify({ error: "A question has 1 to 500 characters." }));
                return;
              }
              // A pause, so the wait of the view shows.
              setTimeout(() => res.end(JSON.stringify(askAnswer(question))), 900);
            });
            return;
          }
          // A draft in your voice (R3-04 intent path), after a pause, as the engine.
          if (url === "/threads/t-8841/draft" && req.method === "POST") {
            res.setHeader("content-type", "application/json");
            setTimeout(() => res.end(JSON.stringify(replyDraft)), 900);
            return;
          }
          const undo = url.match(/^\/ledger\/([^/?]+)\/undo$/);
          if (undo && req.method === "POST") {
            const done = undoEntry(decodeURIComponent(undo[1]));
            res.statusCode = done ? 200 : 409;
            res.setHeader("content-type", "application/json");
            res.end(JSON.stringify(done ?? { error: "This change has no undo now." }));
            return;
          }
          const comment = url.match(/^\/changesets\/([^/?]+)\/comments$/);
          if (comment && req.method === "POST") {
            let text = "";
            req.on("data", (part: Buffer) => (text += part));
            req.on("end", () => {
              const [status, body] = commentPost(decodeURIComponent(comment[1]), JSON.parse(text || "{}"));
              res.statusCode = status;
              res.setHeader("content-type", "application/json");
              res.end(JSON.stringify(body));
            });
            return;
          }
          const decision = url.match(/^\/changesets\/([^/?]+)\/(decide|withdraw)$/);
          if (decision && req.method === "POST") {
            let text = "";
            req.on("data", (part: Buffer) => (text += part));
            req.on("end", () => {
              const id = decodeURIComponent(decision[1]);
              const done = decision[2] === "decide" ? decide(id, JSON.parse(text)) : withdraw(id);
              res.statusCode = done ? 200 : 404;
              res.setHeader("content-type", "application/json");
              res.end(JSON.stringify(done ?? { error: "Not found." }));
            });
            return;
          }
          const gate = url.match(/^\/senders\/([^/?]+)\/([a-z]+)$/);
          if (gate && req.method === "POST") {
            const done = answerSender(decodeURIComponent(gate[1]), gate[2]);
            res.statusCode = done ? 200 : 404;
            res.setHeader("content-type", "application/json");
            res.end(JSON.stringify(done ?? { error: "Not found." }));
            return;
          }
          const verb = url.match(/^\/items\/([^/?]+)\/(close|decline|defer)$/);
          if (verb && req.method === "POST") {
            let text = "";
            req.on("data", (part: Buffer) => (text += part));
            req.on("end", () => {
              const done = act(decodeURIComponent(verb[1]), verb[2], JSON.parse(text || "{}"));
              res.statusCode = done ? 200 : 404;
              res.setHeader("content-type", "application/json");
              res.end(JSON.stringify(done ?? { error: "Not found." }));
            });
            return;
          }
          const stop = req.method === "POST" ? stopPost(url) : undefined;
          if (stop) {
            res.statusCode = stop[0];
            res.setHeader("content-type", "application/json");
            res.end(JSON.stringify(stop[1]));
            return;
          }
          const cancel = req.method === "POST" ? outboxPost(url) : undefined;
          if (cancel) {
            res.statusCode = cancel[0];
            res.setHeader("content-type", "application/json");
            res.end(JSON.stringify(cancel[1]));
            return;
          }
          const notice = req.method === "POST" ? noticePost(url) : undefined;
          if (notice) {
            res.statusCode = notice[0];
            res.setHeader("content-type", "application/json");
            res.end(JSON.stringify(notice[1]));
            return;
          }
          if ((url === "/policy/grant" || url === "/policy/remove") && req.method === "POST") {
            let text = "";
            req.on("data", (part: Buffer) => (text += part));
            req.on("end", () => {
              const [code, out] = policyPost(url, JSON.parse(text || "{}"));
              res.statusCode = code;
              res.setHeader("content-type", "application/json");
              res.end(JSON.stringify(out));
            });
            return;
          }
          if (url === "/jobs" && req.method === "POST") {
            const [code, out] = startJob();
            res.statusCode = code;
            res.setHeader("content-type", "application/json");
            res.end(JSON.stringify(out));
            return;
          }
          if (url.startsWith("/watches") && req.method === "POST") {
            let text = "";
            req.on("data", (part: Buffer) => (text += part));
            req.on("end", () => {
              const [code, out] = watchPost(url, JSON.parse(text || "{}"));
              res.statusCode = code;
              res.setHeader("content-type", "application/json");
              res.end(JSON.stringify(out));
            });
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
  server: fixtures ? {} : { proxy: { "/api": engineProxy("http://127.0.0.1:4800") } },
  build: { outDir: "dist", emptyOutDir: true },
  };
});
