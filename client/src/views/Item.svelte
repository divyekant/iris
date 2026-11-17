<!--
  One item. The words of the mail that made it come first, so that a person
  seldom needs the mail itself. Each source opens its mail in one step.
-->
<script lang="ts">
  import type { Item } from "../api";
  import { watch } from "../clock";
  import { due, field, money, party, RECORD_FIELDS, when } from "../format";
  import { href } from "../route";
  import Act from "./Act.svelte";
  import { canStart } from "../start";
  import { quiet } from "../act";
  import Code from "./Code.svelte";

  let { item: given, now: start = new Date() }: { item: Item; now?: Date } = $props();
  // The item after a change of the person (Done, Later, Decline).
  // svelte-ignore state_referenced_locally
  let item = $state(given);
  // svelte-ignore state_referenced_locally
  let now = $state(start);
  $effect(() => (item.form === "signal" && item.expires ? watch(item.expires, (d) => (now = d)) : undefined));

  const FORMS: Record<string, string> = {
    commitment: "Commitment",
    conversation: "Conversation",
    record: "Record",
    feed: "To read",
    signal: "Signal",
  };

  let quoted = $derived(item.sources.find((s) => s.role === "origin" && s.quote));
  let rows: [string, string][] = $derived.by(() => {
    const r: [string, string][] = [];
    switch (item.form) {
      case "commitment": {
        const d = due(item, now);
        r.push(["Who acts", party(item.owner)], ["Who waits", party(item.counterparty)]);
        if (d) r.push(["Due", `${when(item.due?.date ?? item.due?.at)}, ${d.text}${item.due?.strict ? ". The date is firm." : ""}`]);
        if (item.amount) r.push(["Amount", money(item.amount)]);
        r.push(["Kind", item.kind ?? ""]);
        if (item.status === "deferred" && item.deferred_until) r.push(["Back on", when(item.deferred_until)]);
        break;
      }
      case "conversation":
        r.push(["With", (item.participants ?? []).filter((p) => p.kind !== "self").map(party).join(", ")]);
        r.push(["Messages", `${item.messages ?? 0}, of which ${item.new ?? 0} new`]);
        r.push(["Next step", item.your_turn ? "Your reply" : "Theirs"]);
        if (item.latest) r.push(["Latest", `${party(item.latest.from)}, ${when(item.latest.at)}: ${item.latest.snippet}`]);
        if (item.summary) r.push(["Summary", item.summary]);
        break;
      case "record":
        for (const key of RECORD_FIELDS[item.type ?? "other"] ?? []) {
          const value = field(item.fields?.[key]);
          if (value) r.push([key.replaceAll("_", " "), value]);
        }
        r.push(["From", party(item.issuer)]);
        break;
      case "feed":
        r.push(["From", party(item.publisher)], ["Sent", when(item.published)]);
        if (item.summary) r.push(["Summary", item.summary]);
        break;
      case "signal":
        r.push(["From", party(item.issuer)], ["Kind", item.kind ?? ""], ["Ends", when(item.expires)]);
        break;
    }
    r.push(["State", item.status]);
    return r;
  });
  // "Handle this" (R4-10): the start of a job, on its own page.
  let startable = $derived(canStart(item));
  // "Reply" (R3-10): the write view of the thread. A gone conversation has no mail to answer.
  let thread = $derived(item.form === "conversation" && item.status !== "gone" ? item.thread : undefined);
  let made = $derived(
    item.derived.by === "rule" ? `a fixed rule (${item.derived.rule})` : item.derived.by === "model" ? `the model ${item.derived.model}` : "you",
  );
</script>

<article>
  <p class="form">{FORMS[item.form]}{item.form === "record" && item.type ? `: ${item.type}` : ""}</p>
  <h1>{item.title}</h1>

  {#snippet handle()}
    {#if startable}<a class="handle" href={href.start(item.id)}>Handle this</a>{/if}
  {/snippet}
  {#snippet reply()}
    {#if thread}<a class="reply" href={href.reply(thread)}>Reply</a>{/if}
  {/snippet}

  <!-- Reply sits outside Act, so it stays in each phase of a verb: the hold, the answer and a failure. -->
  <div class="verbs">
    {@render reply()}
    <Act {item} onchange={(next) => (item = next)} more={handle} />
  </div>
  <!-- Where Act shows no row with Mute, Skip or Dismiss, Handle this stands alone. A commitment keeps Handle this in its own row. -->
  {#if item.form !== "commitment" && !quiet(item) && startable}<div class="alone">{@render handle()}</div>{/if}

  {#if item.form === "signal" && item.code}
    <div class="code"><Code {item} {now} /></div>
  {/if}

  {#if quoted}
    <figure>
      <blockquote>{quoted.quote}</blockquote>
      <figcaption>
        From the mail that made this item.
        <a href={href.mail(quoted.message)}>Open the mail</a>
      </figcaption>
    </figure>
  {/if}

  <dl>
    {#each rows as [label, value] (label)}
      <dt>{label}</dt>
      <dd>{value}</dd>
    {/each}
  </dl>

  {#if item.sources.length > 1 || !quoted}
  <h2>Mail</h2>
  <ul class="sources">
    {#each item.sources as source, i (source.message)}
      <li>
        <a class="touch" href={href.mail(source.message)}>{source.role === "origin" ? "The mail that made this item" : source.role === "update" ? `Update ${i}` : `Evidence ${i}`}</a>
        {#if source.quote && source !== quoted}<span>{source.quote}</span>{/if}
      </li>
    {/each}
  </ul>
  {/if}

  <p class="made">
    Made by {made}{item.confidence != null ? `, ${Math.round(item.confidence * 100)} % sure` : ""}. Last change {when(item.updated)}.
  </p>
</article>

<style>
  article {
    max-width: 680px;
  }
  .form {
    color: var(--color-text-muted);
    margin: var(--space-4) 0 var(--space-1);
    text-transform: capitalize;
  }
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    letter-spacing: -0.01em;
    line-height: 1.25;
    margin: 0 0 var(--space-6);
  }
  .handle {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--size-touch);
    padding: 0 var(--space-4);
    margin-left: auto;
    border: 1px solid var(--color-primary);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--color-primary);
    font: inherit;
    font-weight: 500;
    text-decoration: none;
    transition: background var(--transition-fast);
  }
  .handle:hover {
    background: var(--color-background-elevated);
  }
  .alone {
    display: flex;
    margin: 0 0 var(--space-8);
  }
  .verbs {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: 0 var(--space-2);
  }
  /* The row or the note of Act takes the rest of the line. */
  .verbs > :global(div) {
    flex: 1;
  }
  .reply {
    margin-bottom: var(--space-8);
    display: inline-flex;
    align-items: center;
    min-height: var(--size-touch);
    padding: 0 var(--space-6);
    border-radius: var(--radius-full);
    background: var(--color-primary);
    color: var(--color-on-primary);
    font-weight: 500;
    text-decoration: none;
    transition: background var(--transition-fast);
  }
  .alone .handle {
    margin-left: 0;
  }
  .code {
    margin-bottom: var(--space-6);
  }
  figure {
    margin: 0 0 var(--space-8);
    padding: var(--space-4) var(--space-6);
    border-left: 2px solid var(--color-primary);
    background: var(--color-background-elevated);
    border-radius: 0 var(--radius-md) var(--radius-md) 0;
  }
  blockquote {
    margin: 0 0 var(--space-3);
    font-size: var(--font-size-1);
    line-height: 1.45;
  }
  figcaption {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  figcaption a {
    margin-left: var(--space-2);
  }
  dl {
    display: grid;
    grid-template-columns: minmax(96px, max-content) 1fr;
    gap: var(--space-3) var(--space-6);
    margin: 0 0 var(--space-8);
  }
  dt {
    color: var(--color-text-muted);
  }
  dt::first-letter {
    text-transform: uppercase;
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  h2 {
    font-size: var(--font-size-0);
    font-weight: 600;
    margin: 0 0 var(--space-2);
  }
  .sources {
    list-style: none;
    padding: 0;
    margin: 0 0 var(--space-8);
  }
  .sources li {
    display: grid;
    gap: var(--space-1);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .sources span {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .made {
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
</style>
