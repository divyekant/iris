<!--
  One matter (card R6-01): the title, how many items it holds and when you
  kept it together, then its commitments, records and conversations, each
  on a line that opens the item. A proposal shows on its card, with each
  member and the two decisions. A declined matter says so above its items.
-->
<script lang="ts">
  import { get, own, type MatterView } from "../api";
  import { capital, count } from "../format";
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
    ].filter((s) => s.items.length > 0),
  );
  let all = $derived(sections.flatMap((s) => s.items));
  let day = (value: string) => new Date(value).toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" });
</script>

<h1>{matter.title}</h1>
<p class="sub">
  {capital(count(all.length, "item", "items"))}.
  {#if matter.status === "confirmed" && matter.decided}Kept together {day(matter.decided)}.{/if}
</p>
{#if error}<p role="alert">{error}</p>{/if}
{#if gone}
  <p class="note">Iris dropped this proposal, because one of its items changed. <a href="#/matters">See the matters</a></p>
{:else if matter.status === "proposed"}
  <MatterCard {view} all ondecided={decided} />
{:else}
  {#if matter.status === "declined"}
    <p class="note">You said that this is not a matter. Iris does not propose it again.</p>
  {/if}
  {#if all.length === 0}
    <p class="empty">No items now.</p>
  {/if}
  {#each sections as part (part.key)}
    <section aria-labelledby={part.key}>
      <h2 id={part.key}><span>{part.title}</span> <span class="count">{part.items.length}</span></h2>
      <ul>{#each part.items as item (item.id)}<Line {item} {now} />{/each}</ul>
    </section>
  {/each}
{/if}

<style>
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    letter-spacing: -0.02em;
    line-height: 1.2;
    margin: var(--space-4) 0 var(--space-2);
    text-wrap: balance;
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
  .empty {
    color: var(--color-text-muted);
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
</style>
