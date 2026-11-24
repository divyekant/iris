<!--
  The matter of one item (D19 of docs/specs/2026-11-24-matters-edit.md):
  "Part of <matter>" when a confirmed matter keeps the item, else "Add to a
  matter". The picker lists the confirmed matters and "New matter", which
  asks for a title. A feed item is never a member, so it has no picker.
-->
<script lang="ts">
  import { get, own, post, type Item, type MatterList, type MatterView } from "../api";
  import { href } from "../route";

  let { item, onchange }: { item: Item; onchange: (matter: string) => void } = $props();

  // The status part of the live rule of `matter_live` (migration 46): only a
  // live item can join, and a signal is live only while it is active.
  const MEMBERS = ["commitment", "conversation", "record", "signal"];
  const ENDED = ["gone", "cancelled", "declined"];
  let can = $derived(
    MEMBERS.includes(item.form) && !ENDED.includes(item.status) && (item.form !== "signal" || item.status === "active"),
  );

  let name = $state("");
  $effect(() => {
    const id = item.matter;
    if (!id) return;
    get<MatterView>(`matters/${encodeURIComponent(id)}`).then(
      (v) => (name = v.matter.title),
      () => (name = "a matter"),
    );
  });

  let open = $state(false);
  let list = $state<MatterList | null>(null);
  let title = $state("");
  let busy = $state(false);
  let error = $state("");

  async function show() {
    open = true;
    error = "";
    try {
      list = await get<MatterList>("matters");
    } catch (e) {
      error = own(e);
    }
  }

  async function run(path: string, body: object) {
    busy = true;
    error = "";
    try {
      const v = await post<MatterView>(path, body);
      name = v.matter.title;
      open = false;
      onchange(v.matter.id);
    } catch (e) {
      // A lost answer or a retry (409, already a member) can hide a write that
      // committed. Read the item again before the page says what happened.
      const status = (e as { status?: number }).status;
      const fresh = await get<Item>(`items/${encodeURIComponent(item.id)}`).catch(() => null);
      if (fresh?.matter) {
        open = false;
        onchange(fresh.matter);
      } else if (status && status >= 400 && status < 500) {
        error = `Iris did not add it: ${own(e)}`;
      } else {
        error = "Iris does not know if it added the item. Reload the page to see.";
      }
    } finally {
      busy = false;
    }
  }

  const join = (id: string) => run(`matters/${encodeURIComponent(id)}/edit`, { add: [item.id] });
  const make = () => run("matters", { title: title.trim(), items: [item.id] });
</script>

{#if item.matter}
  <p class="part">Part of <a href={href.matter(item.matter)}>{name || "a matter"}</a></p>
{:else if can}
  {#if !open}
    <button class="quiet" type="button" onclick={show}>Add to a matter</button>
  {:else}
    <div class="pick">
      <p class="label">Add to a matter</p>
      {#if list}
        <ul>
          {#each list.confirmed as m (m.id)}
            <li><button type="button" class="matter" disabled={busy} onclick={() => join(m.id)}>{m.title}</button></li>
          {/each}
        </ul>
      {/if}
      <form class="new" onsubmit={(e) => (e.preventDefault(), make())}>
        <input type="text" bind:value={title} maxlength="80" placeholder="New matter" aria-label="Title of a new matter" />
        <button class="keep" type="submit" disabled={busy || !title.trim()}>Make</button>
      </form>
      <button class="quiet" type="button" onclick={() => (open = false)}>Cancel</button>
    </div>
  {/if}
{/if}
{#if error}<p class="why" role="alert">{error}</p>{/if}

<style>
  .part {
    color: var(--color-text-muted);
    margin: var(--space-4) 0 0;
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
  .pick {
    margin: var(--space-4) 0;
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
  }
  .label {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    margin: 0 0 var(--space-2);
  }
  ul {
    list-style: none;
    margin: 0 0 var(--space-3);
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .matter {
    background: var(--color-background-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    padding: var(--space-2) var(--space-4);
    color: var(--color-text);
  }
  .matter:hover {
    border-color: var(--color-primary);
  }
  .new {
    display: flex;
    gap: var(--space-2);
  }
  .new input {
    flex: 1;
    min-width: 0;
    font: inherit;
    color: var(--color-text);
    background: var(--color-background);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-3);
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
  }
  .why {
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
  }
</style>
