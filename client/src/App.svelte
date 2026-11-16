<script lang="ts">
  import { get, type Brief as BriefData, type Item as ItemData, type Message as MessageData, type Page, type Status } from "./api";
  import { NAV, parse, type Route } from "./route";
  import Setup from "./views/Setup.svelte";
  import Brief from "./views/Brief.svelte";
  import List from "./views/List.svelte";
  import Records from "./views/Records.svelte";
  import Item from "./views/Item.svelte";
  import Mail from "./views/Mail.svelte";
  import Measures from "./views/Measures.svelte";

  let hash = $state(typeof location === "undefined" ? "" : location.hash);
  let route: Route = $derived(parse(hash));

  $effect(() => {
    const update = () => (hash = location.hash);
    addEventListener("hashchange", update);
    return () => removeEventListener("hashchange", update);
  });

  // The home view is the brief. Until the mirror is ready, it is the setup flow.
  function ready(status: Status): boolean {
    return status.accounts.some((a) => a.mirror_ready_at);
  }
</script>

<header>
  <a class="name" href="#/">Iris</a>
  <nav aria-label="Views">
    {#each NAV as [link, label] (link)}
      <a href={link} aria-current={hash === link ? "page" : undefined}>{label}</a>
    {/each}
  </nav>
</header>

<main>
  {#key hash}
    {#if route.view === "home" || route.view === "setup"}
      {#await get<Status>("status") then status}
        {#if route.view === "setup" || !ready(status)}
          <Setup {status} />
        {:else}
          {#await get<BriefData>("brief") then brief}<Brief {brief} />{:catch error}<p role="alert">{error.message}</p>{/await}
        {/if}
      {:catch error}
        <p role="alert">Iris cannot reach the engine: {error.message} Start it with <code>iris serve</code>.</p>
      {/await}
    {:else if route.view === "brief"}
      {#await get<BriefData>("brief") then brief}<Brief {brief} />{:catch error}<p role="alert">{error.message}</p>{/await}
    {:else if route.view === "list"}
      {@const form = route.form}
      {#await get<Page>(`items?form=${form}&limit=50`) then page}<List {form} {page} />{:catch error}<p role="alert">{error.message}</p>{/await}
    {:else if route.view === "records"}
      {#await get<BriefData>("brief") then brief}<Records counts={brief.records} />{:catch error}<p role="alert">{error.message}</p>{/await}
    {:else if route.view === "item"}
      {#await get<ItemData>(`items/${encodeURIComponent(route.id)}`) then item}<Item {item} />{:catch error}<p role="alert">{error.message}</p>{/await}
    {:else if route.view === "mail"}
      {#await get<MessageData>(`messages/${encodeURIComponent(route.id)}`) then message}<Mail {message} />{:catch error}<p role="alert">{error.message}</p>{/await}
    {:else if route.view === "measures"}
      <Measures />
    {/if}
  {/key}
</main>

<style>
  :global(body) {
    margin: 0;
    background: var(--color-background);
    color: var(--color-text);
    font-family: var(--font-family);
    font-size: var(--font-size-0);
    line-height: 1.5;
  }
  :global(a) {
    color: var(--color-text);
  }
  :global(code) {
    font-family: var(--font-family-mono);
    font-size: var(--font-size--1);
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-6);
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-background-elevated);
  }
  .name {
    font-size: var(--font-size-2);
    font-weight: 600;
    text-decoration: none;
  }
  nav {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-4);
  }
  nav a {
    color: var(--color-text-muted);
    text-decoration: none;
    padding: var(--space-1) 0;
    border-bottom: 2px solid transparent;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }
  nav a:hover,
  nav a[aria-current="page"] {
    color: var(--color-text);
    border-bottom-color: var(--color-primary);
  }
  main {
    max-width: 960px;
    margin: 0 auto;
    padding: var(--space-6) var(--space-4) var(--space-16);
  }
  @media (prefers-reduced-motion: reduce) {
    nav a {
      transition: none;
    }
  }
</style>
