# Iris web client

The web client of Iris: Svelte 5, TypeScript and Vite. The build makes static
files in `client/dist`. The crate `crates/iris-web` embeds these files in the
`iris` program, so no Node runs at run time.

## Build a release

Use this command from the root of the repository. It builds the client first,
then the program:

```bash
(cd client && npx --yes pnpm@10.28.0 install --frozen-lockfile && npm run build) && IRIS_REQUIRE_CLIENT=1 cargo build --release
```

`IRIS_REQUIRE_CLIENT=1` stops the build if `client/dist` does not exist.
Without it, `cargo build` still works with no client build, and the program
serves a fallback page that tells you to build the client.

## Work on the client

1. Install the packages: `pnpm install --frozen-lockfile`. Then install the browser of the phone tests (`test/phone.test.mjs`): `npx playwright-core install chromium`. The command does nothing if the browser is there.
2. Start the dev server: `pnpm dev`. It sends `/api` to the engine on
   `127.0.0.1:4800` (`iris serve`). With no engine, use
   `pnpm dev --mode fixtures`: the answers come from `dev/fixtures.ts`, which
   uses the examples of `docs/spec/schema/examples`.
   With an engine, `iris serve` prints a one-time sign-in address,
   `http://127.0.0.1:4800/#code=...`. To use it on the dev server, open the
   same fragment on the dev origin: `http://localhost:5173/#code=...`. The
   client removes the code from the address, gets a token for it from
   `POST /api/session`, and keeps the token in `sessionStorage`. Each API call
   sends the token in the `Authorization` header. The dev proxy sends the
   `Host` of the engine, and the `Origin` of the engine for the dev page only.
3. Before a commit, run `npm run lint` (type check) and `npm test` (a test build in `.test-build` and the tests).

## Design tokens

`src/tokens.css` holds the design tokens from `.kalos.yaml`: the dark and light
palettes, the type scale (1.25 from 16px), the 4px spacing, the radii and the
transitions. The dark palette is the default. The light palette applies when
the system asks for light.

If you change a color in `.kalos.yaml`, change the same value in
`src/tokens.css`. `test/tokens.test.mjs` fails if the two files do not agree,
or if a text color has a contrast below 4.5 (WCAG AA) on a background color.

## Views

The release 1 views read the local API of the engine (`crates/iris-api`).
Routes are in the hash, for example `#/brief` and `#/mail/m-55210`.

| Route | View | Card |
|---|---|---|
| `#/` | Setup flow until the mirror is ready, then the brief | R1-18, R1-19 |
| `#/brief/<section>` | All items of one section of the brief, 50 for each page | R1-19 |
| `#/commitments`, `#/conversations`, `#/feed`, `#/signals` | The list of one form | R1-20, R1-21 |
| `#/records`, `#/records/<type>` | One table for each record type | R1-21 |
| `#/item/<id>` | One item with its sources | R1-20 |
| `#/mail/<id>` | One source mail as clean text | R1-22 |
| `#/measures` | The open count of source mail | R1-22 |

Mail text is data. The client shows it as text and never renders the HTML of a
mail. The open count stays in the storage of the browser.
