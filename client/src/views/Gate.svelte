<!--
  The stranger gate (spec 1 section 6.7): the senders that you do not know, and
  what they ask of you. Accept a sender, and their mail joins the brief. Ignore
  a sender, and their mail stays out of the brief. A rule in the policy file can
  answer first. Each answer has an Undo.
-->
<script lang="ts">
  import { untrack } from "svelte";
  import { answerSender, get, own, type GateAnswer, type GatePage, type GateSender } from "../api";
  import { initials } from "../format";
  import { href } from "../route";

  let {
    page,
    declined = 0,
    declinedList = false,
    title = "",
    more = "",
    onanswer = () => {},
  }: {
    page: GatePage;
    /** The count of ignored senders, for the link under the gate. */
    declined?: number;
    /** The list of the senders that you ignored: "Ask again" in place of "Ignore". */
    declinedList?: boolean;
    /** The heading of a full page. The brief gives none. */
    title?: string;
    /** The API path of the list, for the next pages. */
    more?: string;
    onanswer?: () => void;
  } = $props();

  type Verb = "accept" | "decline" | "reopen";
  // The state of a sender after an answer, from the row that the engine gives back.
  // `other`: the sender is not a stranger now (for example, hostile), so no Undo.
  type State = "waiting" | "declined" | "accepted" | "other";
  const stateOf = (r: GateAnswer): State =>
    r.declined ? "declined" : r.kind === "person" && r.decided_by === "person" ? "accepted" : r.kind === "stranger" ? "waiting" : "other";
  // The state of each row before an answer: the list that the row is on.
  // svelte-ignore state_referenced_locally
  const base: State = declinedList ? "declined" : "waiting";
  // The state of each sender that got an answer in this view. A row whose state is not
  // the base state shows the answer with an Undo, also when the brief loads again and
  // the sender is not in the page any more.
  let done = $state<Record<string, State>>({});
  const answered = (s: GateSender) => !!done[s.id] && done[s.id] !== base;
  // The senders whose answer waits for the engine. A sender takes one answer at a time.
  let busy = $state<Record<string, boolean>>({});
  let problem = $state<Record<string, string>>({});
  // svelte-ignore state_referenced_locally
  let rows = $state<GateSender[]>(page.senders);
  // svelte-ignore state_referenced_locally
  let next = $state(page.next);
  let loading = $state(false);

  // A new page from the brief: keep the order of the rows on the screen, take the new
  // data of each sender, and add the new senders at the end.
  $effect(() => {
    const fresh = new Map(page.senders.map((s) => [s.id, s]));
    untrack(() => {
      // A row stays while it has something to tell: an answer, or the refusal of an answer.
      const kept = rows.filter((s) => fresh.has(s.id) || answered(s) || problem[s.id]).map((s) => fresh.get(s.id) ?? s);
      rows = [...kept, ...page.senders.filter((s) => !kept.some((k) => k.id === s.id))];
    });
  });

  const nameOf = (s: GateSender) => s.name ?? s.address;
  const items = (n: number) => `${n} ${n === 1 ? "item" : "items"}`;
  // The answers that take a row back to the base state. Reopen ends an accept or a decline.
  // On the ignored list, a decline follows, because the engine declines only a stranger.
  const undo = (now: State): Verb[] => (base === "waiting" ? ["reopen"] : now === "accepted" ? ["reopen", "decline"] : ["decline"]);

  async function answer(sender: GateSender, ...verbs: Verb[]) {
    if (busy[sender.id]) return;
    busy = { ...busy, [sender.id]: true };
    problem = { ...problem, [sender.id]: "" };
    try {
      for (const verb of verbs) {
        const row = await answerSender(sender.id, verb);
        done = { ...done, [sender.id]: stateOf(row) };
      }
    } catch (e) {
      problem = { ...problem, [sender.id]: own(e) || "Iris could not save the answer. Try again." };
    } finally {
      const { [sender.id]: _, ...rest } = busy;
      busy = rest;
      onanswer();
    }
  }

  async function load() {
    if (!next || !more) return;
    loading = true;
    try {
      const p = await get<GatePage>(`${more}&after=${encodeURIComponent(next)}`);
      rows = [...rows, ...p.senders.filter((s) => !rows.some((r) => r.id === s.id))];
      next = p.next;
    } catch (e) {
      problem = { ...problem, more: own(e) || "Iris could not load more senders." };
    } finally {
      loading = false;
    }
  }
</script>

{#snippet asks(sender: GateSender)}
  <ul class="asks">
    {#each sender.items as item (item.id)}<li><a href={href.item(item.id)}>{item.title}</a></li>{/each}
    {#if sender.count > sender.items.length}<li class="more">and {items(sender.count - sender.items.length)} more</li>{/if}
  </ul>
{/snippet}

{#snippet buttons(sender: GateSender)}
  <div class="answers">
    {#if declinedList}
      <button class="quiet" disabled={busy[sender.id]} onclick={() => answer(sender, "reopen")} aria-label={`Ask again about ${nameOf(sender)}`}>Ask again</button>
    {:else}
      <button class="quiet" disabled={busy[sender.id]} onclick={() => answer(sender, "decline")} aria-label={`Ignore ${nameOf(sender)}`}>Ignore</button>
    {/if}
    <button class="accept" disabled={busy[sender.id]} onclick={() => answer(sender, "accept")} aria-label={`Accept ${nameOf(sender)}`}>Accept</button>
  </div>
{/snippet}

{#snippet result(sender: GateSender)}
  {@const now = done[sender.id]}
  <p class="result" role="status">
    {#if now === "accepted"}
      You know {nameOf(sender)} now.
    {:else if now === "declined"}
      You ignored {nameOf(sender)}.
    {:else if now === "waiting"}
      Iris asks you about {nameOf(sender)} again.
    {:else}
      {nameOf(sender)} is not a sender that you do not know now.
    {/if}
    {#if now !== "other"}<button class="undo" disabled={busy[sender.id]} onclick={() => answer(sender, ...undo(now))}>Undo</button>{/if}
  </p>
  {#if problem[sender.id]}<p class="problem">{problem[sender.id]}</p>{/if}
{/snippet}

{#if title}<h1>{title}</h1>{/if}
{#if !rows.length}
  <p class="empty">{declinedList ? "You ignored no sender." : "No sender that you do not know waits for an answer."}</p>
{:else}
  <ul class="senders">
    {#each rows as sender (sender.id)}
      <li class="sender">
        {#if answered(sender)}
          {@render result(sender)}
        {:else}
          <span class="mark" aria-hidden="true">{initials({ kind: "sender", sender: sender.id, name: sender.name ?? undefined, address: sender.address })}</span>
          <div class="body">
            <p class="who">{nameOf(sender)}{#if sender.name}<span class="address">{sender.address}</span>{/if}</p>
            {@render asks(sender)}
            {#if problem[sender.id]}<p class="problem">{problem[sender.id]}</p>{/if}
          </div>
          {@render buttons(sender)}
        {/if}
      </li>
    {/each}
  </ul>
{/if}

{#if next && more}
  <button class="quiet" disabled={loading} onclick={load}>Show more senders</button>
  {#if problem.more}<p class="problem">{problem.more}</p>{/if}
{/if}
{#if !declinedList}
  <p class="foot">
    {#if !more && page.count > page.senders.length}<a href="#/brief/gate">All {page.count}</a>{/if}
    {#if declined > 0}<a href="#/brief/declined">Ignored {declined}</a>{/if}
    <a href="#/rules">Let a rule answer</a>
  </p>
{/if}

<style>
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    letter-spacing: -0.01em;
    margin: var(--space-4) 0 var(--space-6);
  }
  .empty {
    color: var(--color-text-muted);
  }
  .senders,
  .asks {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .sender {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: var(--space-8);
    height: var(--space-8);
    border-radius: var(--radius-full);
    background: var(--color-background-surface);
    border: 1px dashed var(--color-border);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    font-weight: 600;
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .who {
    margin: 0;
    font-weight: 500;
  }
  .address {
    display: block;
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
    font-weight: 400;
    overflow-wrap: anywhere;
  }
  .asks {
    margin-top: var(--space-1);
    font-size: var(--font-size--1);
  }
  .asks a {
    color: var(--color-text-muted);
    text-decoration: none;
  }
  .asks a:hover {
    color: var(--color-text);
    text-decoration: underline;
  }
  .more {
    color: var(--color-text-faint);
  }
  .answers {
    flex: none;
    display: flex;
    gap: var(--space-2);
  }
  button {
    font: inherit;
    font-size: var(--font-size--1);
    border-radius: var(--radius-full);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
    transition:
      border-color var(--transition-fast),
      color var(--transition-fast);
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .accept {
    background: none;
    border: 1px solid var(--color-primary);
    color: var(--color-primary);
  }
  .accept:hover:not(:disabled) {
    background: var(--color-primary);
    color: var(--color-on-primary);
  }
  .quiet,
  .undo {
    background: none;
    border: 1px solid transparent;
    color: var(--color-text-muted);
  }
  .quiet:hover:not(:disabled),
  .undo:hover:not(:disabled) {
    color: var(--color-text);
    border-color: var(--color-border);
  }
  .result {
    flex: 1;
    margin: 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .problem {
    margin: var(--space-1) 0 0;
    font-size: var(--font-size--1);
    border-left: 2px solid var(--color-error);
    padding-left: var(--space-2);
  }
  .foot {
    display: flex;
    gap: var(--space-4);
    margin: var(--space-2) 0 0;
    font-size: var(--font-size--1);
  }
  .foot a {
    color: var(--color-text-muted);
  }
  .foot a:hover {
    color: var(--color-text);
  }
  @media (max-width: 600px) {
    .sender {
      flex-wrap: wrap;
    }
    .sender .answers {
      width: 100%;
      justify-content: flex-end;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    button {
      transition: none;
    }
  }
</style>
