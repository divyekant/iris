<!-- One item with its fields and its sources. Each source opens its mail in one step. -->
<script lang="ts">
  import type { Item } from "../api";
  import { field, money, party, RECORD_FIELDS, when } from "../format";
  import { href } from "../route";

  let { item, now = new Date() }: { item: Item; now?: Date } = $props();

  let rows: [string, string][] = $derived.by(() => {
    const r: [string, string][] = [["Status", item.status]];
    switch (item.form) {
      case "commitment":
        r.push(["Who must act", party(item.owner)], ["Who waits", party(item.counterparty)], ["Kind", item.kind ?? ""]);
        if (item.due) r.push(["Due", `${when(item.due.date ?? item.due.at)}${item.due.strict ? " (firm)" : ""}`]);
        if (item.amount) r.push(["Amount", money(item.amount)]);
        break;
      case "conversation":
        r.push(
          ["With", (item.participants ?? []).filter((p) => p.kind !== "self").map(party).join(", ")],
          ["Messages", `${item.messages ?? 0}, ${item.new ?? 0} new`],
          ["Your turn", item.your_turn ? "Yes" : "No"],
        );
        if (item.latest) r.push(["Latest", `${party(item.latest.from)}, ${when(item.latest.at)}: ${item.latest.snippet}`]);
        if (item.summary) r.push(["Summary", item.summary]);
        break;
      case "record":
        r.push(["Type", item.type ?? ""], ["Issuer", party(item.issuer)]);
        for (const key of RECORD_FIELDS[item.type ?? "other"] ?? []) {
          const value = field(item.fields?.[key]);
          if (value) r.push([key.replaceAll("_", " "), value]);
        }
        break;
      case "feed":
        r.push(["Publisher", party(item.publisher)], ["Published", when(item.published)]);
        if (item.summary) r.push(["Summary", item.summary]);
        break;
      case "signal":
        r.push(["Kind", item.kind ?? ""], ["Issuer", party(item.issuer)], ["Expires", when(item.expires)]);
        break;
    }
    return r;
  });

  // Spec 1 section 6.6: never show an expired code in large type.
  let code = $derived(
    item.form === "signal" && item.code && item.status === "active" && item.expires && new Date(item.expires) > now ? item.code : "",
  );
  let made = $derived(
    item.derived.by === "rule" ? `a rule (${item.derived.rule})` : item.derived.by === "model" ? `the model ${item.derived.model}` : "a person",
  );
</script>

<p class="form">{item.form}</p>
<h1>{item.title}</h1>
{#if code}<p class="code" aria-label="Code">{code}</p>{/if}

<dl>
  {#each rows as [label, value] (label)}
    <dt>{label}</dt>
    <dd>{value}</dd>
  {/each}
</dl>

<h2>Sources</h2>
<ul>
  {#each item.sources as source (source.message)}
    <li>
      <a href={href.mail(source.message)}>Open the mail</a>
      <span class="role">{source.role}</span>
      {#if source.quote}<blockquote>{source.quote}</blockquote>{/if}
    </li>
  {/each}
</ul>
<p class="note">
  Made by {made}{item.confidence != null ? `, confidence ${Math.round(item.confidence * 100)} %` : ""}. Updated {when(item.updated)}.
</p>

<style>
  .form {
    color: var(--color-text-muted);
    text-transform: capitalize;
    margin: 0;
  }
  h1 {
    font-size: var(--font-size-3);
    margin: 0 0 var(--space-4);
  }
  .code {
    font-family: var(--font-family-mono);
    font-size: var(--font-size-4);
    letter-spacing: 0.1em;
    margin: 0 0 var(--space-4);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-2) var(--space-6);
    margin: 0 0 var(--space-8);
  }
  dt {
    color: var(--color-text-muted);
    text-transform: capitalize;
  }
  dd {
    margin: 0;
  }
  h2 {
    font-size: var(--font-size-1);
  }
  ul {
    list-style: none;
    padding: 0;
  }
  li {
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .role {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    margin-left: var(--space-2);
  }
  blockquote {
    margin: var(--space-2) 0 0;
    padding-left: var(--space-3);
    border-left: 2px solid var(--color-accent-border);
    color: var(--color-text-muted);
  }
  .note {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
</style>
