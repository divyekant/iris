<!--
  One matter (card R6-01; edits D19, additions D20, signals D21 of
  docs/specs/2026-11-24-matters-edit.md): the title, how many items it holds
  and when you kept it together, the items that Iris thinks belong here,
  then its commitments, records, conversations and signals, each on a line
  that opens the item. A proposal shows on its card, with each member and
  the two decisions. A declined matter says so above its items.

  A confirmed matter has Edit: the title becomes a field, each line gets
  Remove, and Save sends the changes in one request. Delete asks once, and
  the engine keeps no Undo for it.
-->
<script lang="ts">
  import { get, own, post, type Item, type MatterView } from "../api";
  import { capital, count } from "../format";
  import { href } from "../route";
  import Line from "./Line.svelte";
  import MatterCard from "./MatterCard.svelte";

  let { view: first, now = new Date() }: { view: MatterView; now?: Date } = $props();
  // svelte-ignore state_referenced_locally
  let view = $state(first);
  let error = $state("");
  // The engine dropped the matter after the page loaded it (a member changed).
  let gone = $state(false);
  let matter = $derived(view.matter);
  let commitments = $derived(view.items.commitment);
  let additions = $derived(view.additions ?? []);
  let confirmed = $derived(matter.status === "confirmed");

  // After a decision the page shows the answer of the engine. Without one (a 404 or a 409), it loads the matter again.
  async function decided(answer: MatterView | null) {
    if (answer) return void (view = answer);
    try {
      view = await get<MatterView>(`matters/${encodeURIComponent(matter.id)}`);
    } catch (e) {
      if ((e as { status?: number }).status === 404) gone = true;
      else error = own(e);
    }
  }
  let sections = $derived(
    [
      { key: "you_owe", title: "You owe", items: commitments.filter((i) => i.direction === "you_owe") },
      { key: "owed_to_you", title: "Owed to you", items: commitments.filter((i) => i.direction !== "you_owe") },
      { key: "records", title: "Records", items: view.items.record },
      { key: "conversations", title: "Conversations", items: view.items.conversation },
      { key: "signals", title: "Signals", items: view.items.signal ?? [] },
    ].filter((s) => s.items.length > 0),
  );
  let all = $derived(sections.flatMap((s) => s.items));
  let day = (value: string) => new Date(value).toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" });

  // Edit: the title and the members to remove, sent together on Save.
  let editing = $state(false);
  let title = $state("");
  let removed: string[] = $state([]);
  let kept = $derived(all.length - removed.length);
  let changed = $derived(title.trim() !== matter.title || removed.length > 0);
  let ready = $derived(title.trim().length > 0 && kept > 0 && changed);
  let busy = $state(false);
  let asking = $state(false);

  function start() {
    title = matter.title;
    removed = [];
    error = "";
    asking = false;
    editing = true;
  }

  function toggle(id: string) {
    removed = removed.includes(id) ? removed.filter((r) => r !== id) : [...removed, id];
  }

  /** One edit request; the answer is the matter after the change. A 404 or 409 loads it again. */
  async function send(body: { title?: string; add?: string[]; remove?: string[] }): Promise<boolean> {
    busy = true;
    error = "";
    try {
      view = await post<MatterView>(`matters/${encodeURIComponent(matter.id)}/edit`, body);
      return true;
    } catch (e) {
      const status = (e as { status?: number }).status ?? 0;
      error = status >= 400 && status < 500 ? `Iris did not save it: ${own(e)}` : "Iris does not know if it saved the change. Reload the page to see.";
      if (status === 404 || status === 409) await decided(null);
      return false;
    } finally {
      busy = false;
    }
  }

  async function save() {
    const t = title.trim();
    const body = { ...(t !== matter.title ? { title: t } : {}), ...(removed.length ? { remove: removed } : {}) };
    if (await send(body)) editing = false;
  }

  async function remove() {
    busy = true;
    error = "";
    try {
      await post(`matters/${encodeURIComponent(matter.id)}/delete`, {});
      location.hash = "#/matters";
    } catch (e) {
      const status = (e as { status?: number }).status ?? 0;
      error = status >= 400 && status < 500 ? `Iris did not delete it: ${own(e)}` : "Iris does not know if it deleted the matter. Reload the page to see.";
      busy = false;
    }
  }

  const add = (item: Item) => send({ add: [item.id] });
  const refuse = (item: Item) => send({ remove: [item.id] });
</script>

{#if editing}
  <label class="name">
    <span>Title</span>
    <input type="text" bind:value={title} maxlength="80" />
  </label>
{:else}
  <div class="head">
    <h1>{matter.title}</h1>
    {#if confirmed && !gone}<button class="quiet edit" type="button" onclick={start}>Edit</button>{/if}
  </div>
{/if}
<p class="sub">
  {capital(count(all.length, "item", "items"))}.
  {#if confirmed && matter.decided}{matter.derived?.by === "person" ? "You made it" : "Kept together"} {day(matter.decided)}.{/if}
</p>
{#if error}<p class="why" role="alert">{error}</p>{/if}
{#if gone}
  <p class="note">Iris dropped this proposal, because one of its items changed. <a href="#/matters">See the matters</a></p>
{:else if matter.status === "proposed"}
  <MatterCard {view} all ondecided={decided} />
{:else}
  {#if matter.status === "declined"}
    <p class="note">You said that this is not a matter. Iris does not propose it again.</p>
  {/if}
  {#if additions.length && !editing}
    <section class="additions" aria-labelledby="additions">
      <h2 id="additions"><span class="mark" aria-hidden="true"></span><span>Iris thinks {additions.length === 1 ? "this belongs" : "these belong"} here</span></h2>
      <ul>
        {#each additions as item (item.id)}
          <li class="addition">
            <a class="what touch" href={href.item(item.id)}>{item.title}</a>
            <span class="acts">
              <button class="keep" type="button" disabled={busy} onclick={() => add(item)}>Add</button>
              <button class="quiet" type="button" disabled={busy} onclick={() => refuse(item)}>Not this</button>
            </span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
  {#if all.length === 0}
    <p class="empty">No items now.</p>
  {/if}
  {#each sections as part (part.key)}
    <section aria-labelledby={part.key}>
      <h2 id={part.key}><span>{part.title}</span> <span class="count">{part.items.length}</span></h2>
      <ul>
        {#each part.items as item (item.id)}
          {#if editing}
            <li class="row" class:out={removed.includes(item.id)}>
              <span class="what">{item.title}</span>
              <button class="quiet" type="button" onclick={() => toggle(item.id)} aria-label={`${removed.includes(item.id) ? "Keep" : "Remove"} ${item.title}`}>
                {removed.includes(item.id) ? "Keep" : "Remove"}
              </button>
            </li>
          {:else}
            <Line {item} {now} />
          {/if}
        {/each}
      </ul>
    </section>
  {/each}
  {#if editing}
    <div class="bar">
      <button class="keep" type="button" disabled={!ready || busy} onclick={save}>Save</button>
      <button class="quiet" type="button" disabled={busy} onclick={() => (editing = false)}>Cancel</button>
      {#if kept === 0}<span class="hint">A matter keeps one item or more. Delete it instead.</span>{/if}
    </div>
    <div class="danger">
      {#if asking}
        <p>Delete this matter? Its items stay in Iris, and Iris does not propose this group again.</p>
        <button class="delete" type="button" disabled={busy} onclick={remove}>Delete matter</button>
        <button class="quiet" type="button" disabled={busy} onclick={() => (asking = false)}>Keep it</button>
      {:else}
        <button class="quiet" type="button" onclick={() => (asking = true)}>Delete matter</button>
      {/if}
    </div>
  {/if}
{/if}

<style>
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-4);
  }
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    letter-spacing: -0.02em;
    line-height: 1.2;
    margin: var(--space-4) 0 var(--space-2);
    text-wrap: balance;
  }
  .name {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: var(--space-4) 0 var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .name input {
    font: inherit;
    font-size: var(--font-size-2);
    color: var(--color-text);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-3);
  }
  .sub {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    margin: 0 0 var(--space-8);
  }
  .note {
    border-left: 4px solid var(--color-primary);
    padding-left: var(--space-3);
    margin: calc(-1 * var(--space-4)) 0 var(--space-8);
  }
  .why {
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
    margin: 0 0 var(--space-6);
  }
  .empty,
  .hint {
    color: var(--color-text-muted);
  }
  .hint {
    font-size: var(--font-size--1);
  }
  section {
    margin-bottom: var(--space-12);
  }
  h2 {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    font-size: var(--font-size-0);
    font-weight: 600;
    margin: 0 0 var(--space-2);
    padding-bottom: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }
  .count {
    color: var(--color-text-muted);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .additions {
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
  }
  .additions h2 {
    justify-content: flex-start;
    gap: var(--space-2);
    border-bottom: 0;
    color: var(--color-text-muted);
    font-weight: 500;
    font-size: var(--font-size--1);
  }
  /* The Iris mark, small: a ring with a dot (as on the proposal card). */
  .mark {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    box-sizing: border-box;
    border: 2px solid var(--color-primary);
    border-radius: var(--radius-full);
    align-self: center;
  }
  .mark::after {
    content: "";
    width: 4px;
    height: 4px;
    border-radius: var(--radius-full);
    background: var(--color-primary);
  }
  .addition,
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    min-height: var(--size-touch);
    padding: var(--space-1) 0;
  }
  .row {
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .row.out .what {
    color: var(--color-text-faint);
    text-decoration: line-through;
  }
  .what {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  a.what {
    text-decoration: none;
  }
  a.what:hover {
    text-decoration: underline;
  }
  .acts,
  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex: none;
  }
  .bar {
    margin: calc(-1 * var(--space-6)) 0 var(--space-12);
  }
  .keep {
    background: var(--color-primary);
    color: var(--color-on-primary);
    border: 0;
    border-radius: var(--radius-full);
    padding: var(--space-2) var(--space-4);
    font-weight: 600;
  }
  .keep:disabled {
    opacity: 0.5;
    cursor: default;
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
  .edit {
    flex: none;
  }
  .danger {
    border-top: 1px solid var(--color-border);
    padding-top: var(--space-4);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-4);
  }
  .danger p {
    flex-basis: 100%;
    margin: 0;
  }
  .delete {
    background: none;
    color: var(--color-error);
    border: 1px solid var(--color-error);
    border-radius: var(--radius-full);
    padding: var(--space-2) var(--space-4);
    font-weight: 600;
  }
</style>
