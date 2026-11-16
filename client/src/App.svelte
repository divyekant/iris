<script lang="ts">
  import { access } from "./access.svelte.ts";
  import { followSignIn, get, loadBrief, own, type Changeset, type HandledPage, type Item as ItemData, type Message as MessageData, type Page, type Policy, type Status } from "./api";
  import { home } from "./format";
  import { NAV, parse, SECTIONS, type Route } from "./route";
  import Setup from "./views/Setup.svelte";
  import Brief from "./views/Brief.svelte";
  import List from "./views/List.svelte";
  import Records from "./views/Records.svelte";
  import Item from "./views/Item.svelte";
  import Mail from "./views/Mail.svelte";
  import Measures from "./views/Measures.svelte";
  import Rules from "./views/Rules.svelte";
  import Review from "./views/Review.svelte";
  import Handled from "./views/Handled.svelte";
  import PolicyNotice from "./views/PolicyNotice.svelte";
  import SignInNotice from "./views/SignInNotice.svelte";

  let hash = $state(typeof location === "undefined" ? "" : location.hash);
  let route: Route = $derived(parse(hash));
  let active = $derived(route.view === "home" || route.view === "section" || route.view === "bundle" || route.view === "handled" ? "#/brief" : `#/${hash.replace(/^#\/?/, "").split("/")[0]}`);

  $effect(() => {
    const update = () => {
      // A sign-in address opened in this tab: the access state shows a failure.
      if (/^#code=/.test(location.hash)) return void followSignIn().then(() => (hash = location.hash));
      hash = location.hash;
      scrollTo(0, 0);
    };
    addEventListener("hashchange", update);
    return () => removeEventListener("hashchange", update);
  });

  // Read the status one time at start, so a view opened from a link also shows an account that needs sign-in.
  // svelte-ignore state_referenced_locally
  if (typeof location !== "undefined" && !["home", "setup", "brief"].includes(route.view)) get("status").catch(() => {});

  const TITLES = { commitment: "Commitments", conversation: "Conversations", feed: "Feed", signal: "Signals" };
  const back = () => (history.length > 1 ? history.back() : (location.hash = "#/brief"));
</script>

<header>
  <a class="name" href="#/brief">
    <svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true">
      <circle cx="12" cy="12" r="9" fill="none" stroke="var(--color-primary)" stroke-width="2" />
      <circle cx="12" cy="12" r="3.5" fill="var(--color-primary)" />
    </svg>
    Iris
  </a>
  <nav aria-label="Views">
    {#each NAV as [link, label] (link)}
      <a href={link} aria-current={active === link ? "page" : undefined}>{label}</a>
    {/each}
  </nav>
</header>

{#snippet failed(error: unknown)}
  {#if own(error)}<p role="alert">{own(error)}</p>{/if}
{/snippet}

<main>
  {#if access.problem}<p class="problem" role="alert">{access.problem}</p>{/if}
  <SignInNotice />
  {#if access.none && !["home", "setup", "brief"].includes(route.view)}
    <p class="problem">Iris has no account yet. <a href="#/setup">Set up Iris</a> to copy your mail.</p>
  {/if}
  {#if route.view === "item" || route.view === "mail" || route.view === "section" || route.view === "bundle" || route.view === "handled"}
    <button class="back" type="button" onclick={back}>Back</button>
  {/if}
  {#key hash}
    {#if route.view === "home" || route.view === "setup" || route.view === "brief"}
      {#await get<Status>("status") then status}
        {#if route.view === "setup" || home(status, route.view === "brief" ? "brief" : "home") === "setup"}
          <Setup {status} />
        {:else}
          <PolicyNotice />
          {#await loadBrief() then brief}<Brief {brief} {status} />{:catch error}{@render failed(error)}{/await}
        {/if}
      {:catch error}{@render failed(error)}
      {/await}
    {:else if route.view === "section"}
      {@const key = route.key}
      {#await get<Page>(`brief/${key}?limit=50`) then page}
        <List title={SECTIONS[key]} {page} more={`brief/${key}?limit=50`} cursor="after" expiring={key === "signals"} />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "bundle"}
      {@const rule = route.rule}
      {#await Promise.all([get<Page>(`brief/bundle-${rule}?limit=50`), get<Policy>("policy").catch(() => null)]) then [page, policy]}
        <List title={policy?.rules.find((r) => r.id === rule)?.name ?? `Rule ${rule}`} {page} more={`brief/bundle-${rule}?limit=50`} cursor="after" />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "handled"}
      {#await Promise.all([get<HandledPage>("handled?limit=50"), get<Page & { count: number }>("brief/hidden?limit=50")]) then [changes, hidden]}
        <Handled {changes} {hidden} />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "list"}
      {@const form = route.form}
      {#await get<Page>(`items?form=${form}&limit=50`) then page}<List title={TITLES[form]} {page} more={`items?form=${form}&limit=50`} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "records"}
      {@const type = route.type}
      {#await get<Record<string, number>>("records") then counts}<Records {counts} {type} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "item"}
      {#await get<ItemData>(`items/${encodeURIComponent(route.id)}`) then item}<Item {item} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "mail"}
      {#await get<MessageData>(`messages/${encodeURIComponent(route.id)}`) then message}<Mail {message} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "measures"}
      <Measures />
    {:else if route.view === "review"}
      {#await Promise.all([get<{ changesets: Changeset[] }>("changesets"), get<Status>("status")]) then [open, status]}
        <Review changesets={open.changesets} mode={status.mode === "observe" ? "observe" : "apply"} />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "rules"}
      {#await get<Policy>("policy") then policy}<Rules {policy} />{:catch error}{@render failed(error)}{/await}
    {/if}
  {/key}
</main>

<footer class="site">
  <a href="#/rules">Rules</a>
  <a href="#/measures">Measures</a>
  <span>Iris runs on your machine.</span>
</footer>

<style>
  :global(body) {
    margin: 0;
    background: var(--color-background);
    color: var(--color-text);
    font-family: var(--font-family);
    font-size: var(--font-size-0);
    line-height: 1.5;
    -webkit-font-smoothing: antialiased;
  }
  :global(a) {
    color: var(--color-text);
    text-underline-offset: 3px;
  }
  :global(:focus-visible) {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  :global(code) {
    font-family: var(--font-family-mono);
    font-size: var(--font-size--1);
    background: var(--color-background-surface);
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
  }
  .problem {
    margin: 0 0 var(--space-8);
    border-left: 4px solid var(--color-warning);
    padding-left: var(--space-3);
  }
  :global([role="alert"]) {
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
  }
  header {
    position: sticky;
    top: 0;
    z-index: 2;
    display: flex;
    align-items: center;
    gap: var(--space-8);
    padding: var(--space-3) var(--space-6);
    border-bottom: 1px solid var(--color-border-subtle);
    background: var(--color-background);
  }
  .name {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--font-size-1);
    font-weight: 600;
    letter-spacing: -0.01em;
    text-decoration: none;
  }
  nav {
    display: flex;
    gap: var(--space-6);
    overflow-x: auto;
    scrollbar-width: none;
  }
  nav a {
    flex: none;
    color: var(--color-text-muted);
    text-decoration: none;
    padding: var(--space-2) 0;
    border-bottom: 2px solid transparent;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }
  nav a:hover {
    color: var(--color-text);
  }
  nav a[aria-current="page"] {
    color: var(--color-text);
    border-bottom-color: var(--color-primary);
  }
  main {
    max-width: 720px;
    margin: 0 auto;
    padding: var(--space-6) var(--space-6) var(--space-16);
  }
  .back {
    font: inherit;
    font-size: var(--font-size--1);
    color: var(--color-text-muted);
    background: none;
    border: 0;
    padding: var(--space-1) 0;
    cursor: pointer;
  }
  .back::before {
    content: "‹ ";
  }
  .back:hover {
    color: var(--color-text);
  }
  .site {
    max-width: 720px;
    margin: 0 auto;
    padding: var(--space-6);
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-6);
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  .site a {
    color: var(--color-text-muted);
  }
  @media (max-width: 600px) {
    header {
      gap: var(--space-4);
      padding: var(--space-3) var(--space-4);
    }
    main {
      padding: var(--space-4) var(--space-4) var(--space-12);
    }
    nav {
      gap: var(--space-4);
      /* Fade the right edge, so that the row shows that it scrolls. */
      mask-image: linear-gradient(to right, black 85%, transparent);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    nav a {
      transition: none;
    }
  }
</style>
