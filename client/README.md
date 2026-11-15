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

1. Install the packages: `pnpm install --frozen-lockfile`.
2. Start the dev server: `pnpm dev`.
3. Before a commit, run `npm run lint` (type check) and `npm test` (a test build in `.test-build` and the tests).

## Design tokens

`src/tokens.css` holds the design tokens from `.kalos.yaml`: the dark and light
palettes, the type scale (1.25 from 16px), the 4px spacing, the radii and the
transitions. The dark palette is the default. The light palette applies when
the system asks for light.

If you change a color in `.kalos.yaml`, change the same value in
`src/tokens.css`. `test/tokens.test.mjs` fails if the two files do not agree,
or if a text color has a contrast below 4.5 (WCAG AA) on a background color.
