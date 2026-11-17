<!--
  A matter that Iris proposes, on a raised card (card R6-01, option A of
  the owner): the title, the counts by form, each member on a line, then
  "Keep together" and "Not a matter". "Keep together" opens the confirm
  step: the person can change the title and clear members. Each decision
  holds for Undo first, because the engine cannot undo it. `all` shows each
  member (the matter page); the brief shows the first 5.
-->
<script lang="ts">
  import { post, type Item, type MatterView } from "../api";
  import { hold, HOLD } from "../act";
  import { matterCounts, other, party } from "../format";
  import { href } from "../route";

  let {
    view,
    ondecided,
    all = false,
    delay = HOLD,
  }: { view: MatterView; ondecided?: (answer: MatterView | null) => void; all?: boolean; delay?: number } = $props();
  let matter = $derived(view.matter);
  let members = $derived([...view.items.record, ...view.items.conversation, ...view.items.commitment]);
  const SHOWN = 5;

  type Choice = "confirm" | "decline";
  type Phase =
    | { at: "idle" }
    | { at: "edit" }
    | { at: "held" | "sending"; choice: Choice }
    | { at: "failed"; why: string; known: boolean };
  let phase = $state<Phase>({ at: "idle" });
  let pending: ReturnType<typeof hold> | undefined;
  let editing = $derived(phase.at === "edit");
  let shown = $derived(all || editing ? members : members.slice(0, SHOWN));

  // The confirm step: the title and the members that the person clears.
  let title = $state("");
  let cleared: string[] = $state([]);
  let keep = $derived(matter.items.filter((id) => !cleared.includes(id)));
  // The engine takes a title of 1 to 80 characters after trim (spec step 5).
  let ready = $derived(title.trim().length > 0 && keep.length > 0);

  function edit() {
    title = matter.title;
    cleared = [];
    phase = { at: "edit" };
  }

  function toggle(id: string) {
    cleared = cleared.includes(id) ? cleared.filter((c) => c !== id) : [...cleared, id];
  }

  function choose(choice: Choice) {
    phase = { at: "held", choice };
    // The decision takes the matter as the person left it at the click.
    const id = matter.id;
    const body = choice === "confirm" ? { title: title.trim(), items: keep } : {};
    pending = hold(() => send(id, choice, body), delay);
  }

  // A held decision is sent when the person leaves: the page closes, or the view of this card goes.
  $effect(() => {
    const leave = () => pending?.flush();
    window.addEventListener("pagehide", leave);
    return () => {
      window.removeEventListener("pagehide", leave);
      leave();
    };
  });

  function undo() {
    pending?.cancel();
    pending = undefined;
    phase = { at: "idle" };
  }

  async function send(id: string, choice: Choice, body: object) {
    pending = undefined;
    phase = { at: "sending", choice };
    try {
      // Not `ondecided?.(await post(...))`: with no `ondecided`, that skips the request.
      const answer = await post<MatterView>(`matters/${encodeURIComponent(id)}/${choice}`, body);
      ondecided?.(answer);
    } catch (error) {
      const status = (error as { status?: number }).status ?? 0;
      // A 4xx answer wrote nothing. With no answer, or a 5xx, Iris cannot say.
      phase = { at: "failed", why: (error as Error).message, known: status >= 400 && status < 500 };
      // A 404 or a 409: the proposal changed after the page loaded it. Load it again.
      if (status === 404 || status === 409) ondecided?.(null);
    }
  }

  const said = { confirm: "Kept together. It shows in Matters.", decline: "Not a matter. Iris does not propose it again." };

  function detail(item: Item): string {
    if (item.form === "commitment") return `${item.direction === "you_owe" ? "You owe" : "Owed by"} ${party(other(item))}`;
    return party(other(item));
  }
</script>

{#snippet icon(form: string)}
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    {#if form === "record"}
      <rect x="3" y="2" width="10" height="12" rx="2" />
      <path d="M5.5 6h5M5.5 9h5" />
    {:else if form === "conversation"}
      <path d="M3 4h10v6H7l-3 2.5V10H3z" stroke-linejoin="round" />
    {:else}
      <circle cx="8" cy="8" r="5.5" />
      <path d="M5.8 8.2l1.6 1.6 3-3.2" />
    {/if}
  </svg>
{/snippet}

<div class="card">
  <p class="eyebrow"><span class="mark" aria-hidden="true"></span>Iris sees a matter</p>
  {#if editing}
    <label class="field">
      <span>Title</span>
      <input type="text" bind:value={title} maxlength="80" />
    </label>
  {:else}
    <p class="title">{matter.title}</p>
  {/if}
  <p class="counts">{editing ? "Clear the items that do not belong." : matterCounts(matter.counts)}</p>
  <ul>
    {#each shown as item (item.id)}
      <li class:cleared={editing && cleared.includes(item.id)}>
        {#if editing}
          <input class="tick" type="checkbox" checked={!cleared.includes(item.id)} onchange={() => toggle(item.id)} aria-label={`Keep ${item.title}`} />
        {:else}
          <span class="form">{@render icon(item.form)}</span>
        {/if}
        <span class="what">
          <a href={href.item(item.id)}>{item.title}</a>
          <span class="detail">{detail(item)}</span>
        </span>
      </li>
    {/each}
  </ul>
  {#if shown.length < members.length}<a class="more touch" href={href.matter(matter.id)}>And {members.length - shown.length} more</a>{/if}
  <div class="acts" role="status">
    {#if phase.at === "idle"}
      <button class="keep" type="button" onclick={edit}>Keep together</button>
      <button class="quiet" type="button" onclick={() => choose("decline")}>Not a matter</button>
    {:else if phase.at === "edit"}
      <button class="keep" type="button" onclick={() => choose("confirm")} disabled={!ready}>Keep {keep.length === members.length ? "together" : `${keep.length} together`}</button>
      <button class="quiet" type="button" onclick={() => (phase = { at: "idle" })}>Cancel</button>
    {:else if phase.at === "failed"}
      <span class="why">{phase.known ? `Iris did not record it: ${phase.why}` : "Iris does not know if it recorded the decision. Reload the page to see."}</span>
      {#if phase.known}<button class="quiet" type="button" onclick={() => (phase = { at: "idle" })}>Close</button>{/if}
    {:else}
      <span class="said">{said[phase.choice]}</span>
      {#if phase.at === "held"}
        <button class="quiet" type="button" onclick={undo}>Undo</button>
        <span class="bar" style:animation-duration={`${delay}ms`} aria-hidden="true"></span>
      {/if}
    {/if}
  </div>
</div>

<style>
  .card {
    position: relative;
    overflow: hidden;
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-lg);
    padding: var(--space-4) var(--space-4) var(--space-3);
  }
  .eyebrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    margin: 0 0 var(--space-1);
  }
  /* The Iris mark, small: a ring with a dot. */
  .mark {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    box-sizing: border-box;
    border: 2px solid var(--color-primary);
    border-radius: var(--radius-full);
  }
  .mark::after {
    content: "";
    width: 4px;
    height: 4px;
    border-radius: var(--radius-full);
    background: var(--color-primary);
  }
  .title {
    font-size: var(--font-size-1);
    font-weight: 600;
    margin: 0;
  }
  .counts {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    margin: 0 0 var(--space-2);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-1) 0;
  }
  .form {
    flex: none;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-md);
    background: var(--color-background-surface);
    color: var(--color-text-muted);
  }
  .form svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
  }
  .what {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .what a {
    text-decoration: none;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  @media (max-width: 600px), (pointer: coarse) {
    .what a {
      display: block;
      line-height: var(--size-touch);
    }
  }
  .what a:hover {
    text-decoration: underline;
  }
  .detail,
  .more {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .more {
    display: inline-block;
    margin-top: var(--space-1);
  }
  .acts {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-4);
    margin-top: var(--space-2);
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border-subtle);
    min-height: 36px;
  }
  button {
    font: inherit;
    font-size: var(--font-size--1);
    cursor: pointer;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: var(--space-1) 0 var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .field input {
    font: inherit;
    font-size: var(--font-size-1);
    font-weight: 600;
    color: var(--color-text);
    background: var(--color-background);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-1) var(--space-2);
  }
  .field input:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 0;
  }
  .tick {
    flex: none;
    width: 20px;
    height: 20px;
    margin: 4px;
    accent-color: var(--color-primary);
    cursor: pointer;
  }
  li.cleared .what {
    opacity: 0.5;
  }
  li.cleared .what a {
    text-decoration: line-through;
  }
  .keep:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .keep {
    background: var(--color-primary);
    color: var(--color-on-primary);
    border: 0;
    border-radius: var(--radius-full);
    padding: var(--space-2) var(--space-4);
    font-weight: 600;
  }
  .quiet {
    background: none;
    border: 0;
    padding: var(--space-2) 0;
    color: var(--color-text-muted);
  }
  .quiet:hover {
    color: var(--color-text);
  }
  .said,
  .why {
    font-size: var(--font-size--1);
  }
  .why {
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
  }
  /* The Undo time, as a line that shrinks along the bottom of the card. */
  .bar {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 2px;
    width: 100%;
    background: var(--color-primary);
    transform-origin: left;
    animation: shrink linear forwards;
  }
  @keyframes shrink {
    to {
      transform: scaleX(0);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .bar {
      animation: none;
    }
  }
</style>
