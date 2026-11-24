// The one access state of the client. Each call of the API sets it
// (src/api.ts): the first load of a view and each poll alike. App.svelte shows
// it above every view, so no view handles sign-in or account state alone.
export const access = $state({
  /** Why the API cannot answer now, or "". */
  problem: "",
  /** The accounts that need sign-in (spec 1 section 4.7), from the last status answer. */
  locked: [] as { address: string; provider: string }[],
  /** True when the last status answer had no account: the views then link to setup. */
  none: false,
  /** The short name of each account by its `a-` ID, from the last status answer. Empty with one account. */
  accounts: {} as Record<string, string>,
  /** The time zone of the owner (an IANA name) from the last status answer. Null: the engine has none, so the zone of the browser. Undefined: no status answer yet. */
  zone: undefined as string | null | undefined,
});
