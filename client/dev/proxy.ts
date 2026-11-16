// The dev proxy from the Vite page (localhost:5173) to `iris serve`.
// Spec 2 section 8.6: the engine refuses a Host that is not its bound address,
// and an Origin of another site. So send the engine's Host, and change the
// Origin only when it is the dev page itself. Another site keeps its Origin.
import type { ProxyOptions } from "vite";

type Headers = Record<string, string | string[] | undefined>;
// The client has no Node types, so the event methods of the proxy are written here.
type Events = { on(event: "proxyReq", fn: (out: { setHeader(k: string, v: string): void }, req: { headers: Headers }) => void): void };

export function engineProxy(target: string): ProxyOptions {
  return {
    target,
    changeOrigin: true,
    configure(proxy) {
      (proxy as unknown as Events).on("proxyReq", (out, req) => {
        const { origin, host } = req.headers;
        if (origin && origin === `http://${host}`) out.setHeader("origin", target);
      });
    },
  };
}
