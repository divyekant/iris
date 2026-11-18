<script lang="ts">
  import { access } from "./access.svelte.ts";
  import { followSignIn, get, loadBrief, loadOutbox, own, type Changeset, type GatePage, type HandledPage, type LedgerPage, type JobDetail, type JobPage, type MatterList, type MatterView, type Item as ItemData, type Message as MessageData, type Page, type Policy, type ReplyStart, type Status } from "./api";
  import { home } from "./format";
  import { pageSize } from "./jobs";
  import type { Measures as MeasuresData } from "./measures";
  import { NAV, parse, SECTIONS, type Route } from "./route";
  import Setup from "./views/Setup.svelte";
  import Brief from "./views/Brief.svelte";
  import List from "./views/List.svelte";
  import Records from "./views/Records.svelte";
  import Item from "./views/Item.svelte";
  import StartPage from "./views/StartPage.svelte";
  import Mail from "./views/Mail.svelte";
  import Measures from "./views/Measures.svelte";
  import Ask from "./views/Ask.svelte";
  import Rules from "./views/Rules.svelte";
  import Review from "./views/Review.svelte";
  import Handled from "./views/Handled.svelte";
  import Ledger from "./views/Ledger.svelte";
  import Outbox from "./views/Outbox.svelte";
  import Write from "./views/Write.svelte";
  import Gate from "./views/Gate.svelte";
  import Jobs from "./views/Jobs.svelte";
  import Job from "./views/Job.svelte";
  import Matters from "./views/Matters.svelte";
  import Matter from "./views/Matter.svelte";
  import PolicyNotice from "./views/PolicyNotice.svelte";
  import SignInNotice from "./views/SignInNotice.svelte";
  import Notices from "./views/Notices.svelte";
  import Stop, { refresh as refreshStop } from "./views/Stop.svelte";
  import Accounts from "./views/Accounts.svelte";

  let hash = $state(typeof location === "undefined" ? "" : location.hash);
  let route: Route = $derived(parse(hash));
  let active = $derived(route.view === "home" || route.view === "section" || route.view === "bundle" || route.view === "handled" || route.view === "gate" ? "#/brief" : `#/${hash.replace(/^#\/?/, "").split("/")[0]}`);

  $effect(() => {
    const update = () => {
      // A sign-in address opened in this tab: the access state shows a failure.
      if (/^#code=/.test(location.hash)) return void followSignIn().then(() => (hash = location.hash));
      hash = location.hash;
      refreshStop();
      if (moreOpen) document.getElementById("more")?.hidePopover();
      scrollTo(0, 0);
    };
    addEventListener("hashchange", update);
    // A stop from the terminal shows when the tab comes back.
    addEventListener("focus", refreshStop);
    refreshStop();
    return () => {
      removeEventListener("hashchange", update);
      removeEventListener("focus", refreshStop);
    };
  });

  // Read the status one time at start, so a view opened from a link also shows an account that needs sign-in.
  // svelte-ignore state_referenced_locally
  if (typeof location !== "undefined" && !["home", "setup", "brief"].includes(route.view)) get("status").catch(() => {});

  // Line icons of the phone tabs, drawn on a 24px grid.
  const ICONS: Record<string, string> = {
    "#/brief": "M5 6h14M5 12h14M5 18h9",
    "#/matters": "M3.5 7.5a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z",
    "#/ask": "M4.5 6.5a2 2 0 0 1 2-2h11a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H10l-4 3.5v-3.5h0.5a2 2 0 0 1-2-2z",
    "#/review": "M4 12.5l4.5 4.5L20 6.5",
    more: "M5.5 12h.01M12 12h.01M18.5 12h.01",
  };
  const JOB_STATES = ["needs_you", "working", "waiting", "done"] as const;
  const TITLES = { commitment: "Commitments", conversation: "Conversations", feed: "Feed", signal: "Signals" };
  // On a phone, the first views of NAV are tabs at the bottom. "More" opens the rest.
  const TABS = NAV.slice(0, 4);
  const MORE = NAV.slice(4);
  let moreOpen = $state(false);
  const back = () => (history.length > 1 ? history.back() : (location.hash = "#/brief"));
</script>

<!-- The Mac app drags its window by the empty part of this bar and of its
     view row (apps/mac). A link still opens its view. -->
<header data-tauri-drag-region>
  <a class="name touch" href="#/brief">
    <svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true">
      <circle cx="12" cy="12" r="9" fill="none" stroke="var(--color-primary)" stroke-width="2" />
      <circle cx="12" cy="12" r="3.5" fill="var(--color-primary)" />
    </svg>
    Iris
  </a>
  <nav aria-label="Views" data-tauri-drag-region>
    {#each NAV as [link, label] (link)}
      <a href={link} aria-current={active === link ? "page" : undefined}>{label}</a>
    {/each}
  </nav>
  <Stop place="button" />
</header>

<nav class="tabs" aria-label="Main views">
  {#each TABS as [link, label] (link)}
    <a href={link} aria-current={active === link ? "page" : undefined}>
      <svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true"><path d={ICONS[link]} /></svg>
      {label}
    </a>
  {/each}
  <button type="button" popovertarget="more" class:current={MORE.some(([link]) => link === active)}>
    <svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true"><path d={ICONS.more} /></svg>
    More
  </button>
</nav>
<div id="more" popover ontoggle={(e) => (moreOpen = (e as ToggleEvent).newState === "open")}>
  <ul>
    {#each MORE as [link, label] (link)}
      <!-- A view closes More, also the view that is open, where the hash does not change. -->
      <li><a href={link} aria-current={active === link ? "page" : undefined} onclick={() => document.getElementById("more")?.hidePopover()}>{label}</a></li>
    {/each}
  </ul>
</div>

{#snippet failed(error: unknown)}
  {#if own(error)}<p role="alert">{own(error)}</p>{/if}
{/snippet}

<main>
  <Stop place="band" />
  {#if access.problem}<p class="problem" role="alert">{access.problem}</p>{/if}
  <SignInNotice />
  <Notices />
  {#if access.none && !["home", "setup", "brief"].includes(route.view)}
    <p class="problem">Iris has no account yet. <a href="#/setup">Set up Iris</a> to copy your mail.</p>
  {/if}
  {#if route.view === "item" || route.view === "start" || route.view === "mail" || route.view === "section" || route.view === "bundle" || route.view === "handled" || route.view === "gate" || route.view === "job" || route.view === "matter" || (route.view === "write" && route.thread)}
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
        <List title={SECTIONS[key]} {page} more={`brief/${key}?limit=50`} cursor="after" expiring={key === "signals"} act />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "bundle"}
      {@const rule = route.rule}
      {#await Promise.all([get<Page>(`brief/bundle-${rule}?limit=50`), get<Policy>("policy").catch(() => null)]) then [page, policy]}
        <List title={policy?.rules.find((r) => r.id === rule)?.name ?? `Rule ${rule}`} {page} more={`brief/bundle-${rule}?limit=50`} cursor="after" act />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "handled"}
      {#await Promise.all([get<HandledPage>("handled?limit=50"), get<Page & { count: number }>("brief/hidden?limit=50")]) then [changes, hidden]}
        <Handled {changes} {hidden} />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "history"}
      {#await get<LedgerPage>("ledger?limit=50") then page}
        <Ledger {page} />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "outbox"}
      {#await loadOutbox() then first}
        <Outbox {first} />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "write"}
      {#if route.thread}
        {@const thread = route.thread}
        {#await get<ReplyStart>(`threads/${encodeURIComponent(thread)}/reply`) then reply}<Write start={{ thread, reply }} />{:catch error}{@render failed(error)}{/await}
      {:else}
        {#await get<Status>("status") then status}<Write start={{ accounts: status.accounts }} />{:catch error}{@render failed(error)}{/await}
      {/if}
    {:else if route.view === "gate"}
      {@const path = route.declined ? "gate/declined?limit=50" : "gate?limit=50"}
      {#await get<GatePage>(path) then page}
        <Gate {page} title={route.declined ? "Senders that you ignored" : "From senders you do not know"} declinedList={route.declined} more={path} />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "list"}
      {@const form = route.form}
      {#await get<Page>(`items?form=${form}&limit=50`) then page}<List title={TITLES[form]} {page} more={`items?form=${form}&limit=50`} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "records"}
      {@const type = route.type}
      {#await get<Record<string, number>>("records") then counts}<Records {counts} {type} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "item"}
      {#await get<ItemData>(`items/${encodeURIComponent(route.id)}`) then item}<Item {item} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "start"}
      {#await get<ItemData>(`items/${encodeURIComponent(route.id)}`) then item}<StartPage {item} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "mail"}
      {#await get<MessageData>(`messages/${encodeURIComponent(route.id)}`) then message}<Mail {message} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "ask"}
      <Ask />
    {:else if route.view === "measures"}
      {#await get<MeasuresData>("measures") then measures}<Measures {measures} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "review"}
      {#await Promise.all([get<{ changesets: Changeset[] }>("changesets"), get<Status>("status")]) then [open, status]}
        <Review changesets={open.changesets} mode={status.mode === "observe" ? "observe" : "apply"} />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "jobs"}
      {#await Promise.all(JOB_STATES.map((state) => get<JobPage>(`jobs?state=${state}&limit=${pageSize(state)}`))) then [needs_you, working, waiting, done]}
        <Jobs pages={{ needs_you, working, waiting, done }} />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "job"}
      {#await get<JobDetail>(`jobs/${encodeURIComponent(route.id)}`) then detail}
        <Job {detail} />
      {:catch error}{@render failed(error)}{/await}
    {:else if route.view === "matters"}
      {#await get<MatterList>("matters") then list}<Matters {list} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "matter"}
      {#await get<MatterView>(`matters/${encodeURIComponent(route.id)}`) then view}<Matter {view} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "accounts"}
      {#await get<Status>("status") then status}<Accounts {status} />{:catch error}{@render failed(error)}{/await}
    {:else if route.view === "rules"}
      {#await get<Policy>("policy") then policy}<Rules {policy} />{:catch error}{@render failed(error)}{/await}
    {/if}
  {/key}
</main>

<footer class="site">
  <a class="touch" href="#/rules">Rules</a>
  <a class="touch" href="#/measures">Measures</a>
  <a class="touch" href="#/accounts">Accounts</a>
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
    padding: max(var(--space-3), env(safe-area-inset-top)) max(var(--space-6), env(safe-area-inset-right)) var(--space-3) max(var(--space-6), env(safe-area-inset-left));
    border-bottom: 1px solid var(--color-border-subtle);
    background: var(--color-background);
  }
  /* The Mac app (apps/mac) sets data-shell="mac": the window buttons sit at
     the left of this bar, and the menu bar of the Mac names the app. */
  :global(:root[data-shell="mac"]) header {
    padding-left: calc(var(--space-16) + var(--space-8));
  }
  :global(:root[data-shell="mac"]) .name {
    display: none;
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
    padding: var(--space-6) max(var(--space-6), env(safe-area-inset-right)) var(--space-16) max(var(--space-6), env(safe-area-inset-left));
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
    padding: var(--space-6) max(var(--space-6), env(safe-area-inset-right)) max(var(--space-6), env(safe-area-inset-bottom)) max(var(--space-6), env(safe-area-inset-left));
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-6);
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  .site a {
    color: var(--color-text-muted);
  }
  /* A touch screen: each control is at least the size of a finger tip. */
  @media (max-width: 600px), (pointer: coarse) {
    main :global(:is(button, summary, select, input:not([type="checkbox"], [type="radio"]))) {
      min-width: var(--size-touch);
      min-height: var(--size-touch);
    }
    /* A link that stands alone, not in a sentence, has the class "touch". */
    :global(.touch) {
      display: inline-flex;
      align-items: center;
      min-width: var(--size-touch);
      min-height: var(--size-touch);
    }
    header nav a {
      display: flex;
      align-items: center;
      justify-content: center;
      min-width: var(--size-touch);
      min-height: var(--size-touch);
      padding: 0;
    }
  }
  /* The height of the phone tabs. An element that sticks to the bottom of the screen sits above it. */
  :global(:root) {
    --tabs-height: 0px;
  }
  .tabs,
  #more {
    display: none;
  }
  /* A phone: the tabs stay at the bottom, in reach of the thumb. */
  @media (max-width: 600px) {
    :global(:root) {
      --tabs-height: calc(var(--space-16) + env(safe-area-inset-bottom));
    }
    header {
      position: static;
      padding: max(var(--space-3), env(safe-area-inset-top)) max(var(--space-4), env(safe-area-inset-right)) var(--space-3) max(var(--space-4), env(safe-area-inset-left));
    }
    header nav {
      display: none;
    }
    main {
      padding: var(--space-4) max(var(--space-4), env(safe-area-inset-right)) var(--space-12) max(var(--space-4), env(safe-area-inset-left));
    }
    .site {
      padding: var(--space-6) max(var(--space-4), env(safe-area-inset-right)) calc(var(--space-8) + var(--tabs-height)) max(var(--space-4), env(safe-area-inset-left));
    }
    .tabs {
      position: fixed;
      inset: auto 0 0;
      z-index: 2;
      display: grid;
      grid-template-columns: repeat(5, 1fr);
      gap: 0;
      overflow: visible;
      mask-image: none;
      padding: 0 max(var(--space-2), env(safe-area-inset-right)) env(safe-area-inset-bottom) max(var(--space-2), env(safe-area-inset-left));
      /* A shadow line, not a border, so that the bar is exactly --tabs-height high. */
      box-shadow: 0 -1px 0 var(--color-border-subtle);
      background: var(--color-background-elevated);
    }
    .tabs a,
    .tabs button {
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      gap: var(--space-1);
      min-height: var(--space-16);
      padding: 0;
      border: 0;
      background: none;
      font: inherit;
      font-size: var(--font-size--2);
      font-weight: 500;
      color: var(--color-text-muted);
      cursor: pointer;
    }
    .tabs button svg {
      stroke-width: 3;
    }
    .tabs svg {
      fill: none;
      stroke: currentColor;
      stroke-width: 1.75;
      stroke-linecap: round;
      stroke-linejoin: round;
    }
    .tabs [aria-current="page"],
    .tabs .current {
      color: var(--color-primary);
    }
    #more {
      inset: auto max(var(--space-2), env(safe-area-inset-right)) calc(var(--tabs-height) + var(--space-2)) auto;
      margin: 0;
      min-width: calc(var(--space-32) + var(--space-16));
      /* A short screen: the list scrolls, so each view stays in reach. */
      max-height: calc(100dvh - var(--tabs-height) - var(--space-4) - max(var(--space-2), env(safe-area-inset-top)));
      overflow-y: auto;
      box-sizing: border-box;
      padding: var(--space-2);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-lg);
      background: var(--color-background-elevated);
      color: var(--color-text);
    }
    #more:popover-open {
      display: block;
    }
    #more ul {
      margin: 0;
      padding: 0;
      list-style: none;
    }
    #more a {
      display: flex;
      align-items: center;
      min-height: calc(var(--space-12) - var(--space-1));
      padding: 0 var(--space-3);
      border-radius: var(--radius-md);
      text-decoration: none;
    }
    #more a[aria-current="page"] {
      color: var(--color-primary);
      background: var(--color-background-surface);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    nav a {
      transition: none;
    }
  }
</style>
