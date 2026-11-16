// The one access state of the client. Each call of the API sets it
// (src/api.ts): the first load of a view and each poll alike. App.svelte shows
// it above every view, so no view handles sign-in or account state alone.
export const access = $state({
  /** Why the API cannot answer now, or "". */
  problem: "",
  /** The addresses of the accounts that need sign-in (spec 1 section 4.7), from the last status answer. */
  locked: [] as string[],
});
