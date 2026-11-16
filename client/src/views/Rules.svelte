<!--
  The policy view (R2-08, spec 2 section 6.3): the mode and the rules of the
  active policy, a change of policy.toml to confirm or discard, and the error
  of a file that is not valid. The person changes the mode or removes a rule
  here; the engine writes the file and keeps its comments.
-->
<script lang="ts">
  import { get, own, post, type Mode, type Policy, type Rule } from "../api";
  import { poll } from "../clock";
  import { conditions, example, GROUPS, keep, MODES, verbs } from "../rules";

  let { policy: initial, every = 30_000 }: { policy: Policy; every?: number } = $props();
  // svelte-ignore state_referenced_locally
  let policy = $state(initial);
  // svelte-ignore state_referenced_locally
  let choice: Mode = $state(initial.mode);
  let asking: number | null = $state(null);
  let busy = $state(false);
  let problem = $state("");
  // The name of the copy of a discarded file, after a discard.
  let saved: string | null = $state(null);

  // The person can edit the file while this screen is open: look again each `every` ms.
  $effect(() => {
    if (!every) return;
    return poll(
      () => get<Policy>("policy"),
      (next) => {
        if (busy) return;
        choice = keep(choice, policy.mode, next.mode);
        if (asking !== null && !next.rules.some((r) => r.id === asking)) asking = null;
        policy = next;
      },
      every,
    );
  });

  let file = $derived(policy.file);
  // A confirmed file that is gone. At the first start there is no file, and changes from here make it.
  let gone = $derived(file.state === "missing" && policy.version !== null);
  // Spec 2 section 6.3: a change from here waits until the file on disk is the active policy.
  let blocked = $derived(file.state === "changed" || file.state === "invalid" || gone || policy.error !== null);
  let empty = $derived(file.state === "changed" && file.mode === policy.mode && !file.added.length && !file.removed.length && !file.changed.length);

  async function send(path: string, body: unknown) {
    busy = true;
    problem = "";
    saved = null;
    try {
      const answer = await post<Policy & { saved?: string | null }>(`policy/${path}`, body);
      saved = answer.saved ?? null;
      policy = answer;
    } catch (e) {
      problem = own(e);
      // A 409 means that the policy or the file changed: show the new state.
      policy = await get<Policy>("policy").catch(() => policy);
    }
    choice = policy.mode;
    asking = null;
    busy = false;
  }

  const confirm = () => file.state === "changed" && send("confirm", { sha256: file.sha256 });
  const discard = () => send("discard", {});
  const setMode = () => send("mode", { version: policy.version, mode: choice });
  const remove = (id: number) => send("remove", { version: policy.version, id });
  const title = (r: Rule) => GROUPS.find((g) => g.effect === r.effect)?.title ?? r.effect;
  const author = (r: Rule) => (r.by === "proposal" ? "Iris proposed it" : "You wrote it");
  // Each field that can make a rule changed, and that the rule sentence does not show.
  const facts = (b: Rule, a: Rule, r: Rule) => [b.by !== a.by && author(r), b.added !== a.added && `Added ${r.added}`];
  // The old rule in one line: its name, group, author and date only where they changed, then its verbs and conditions.
  const was = (b: Rule, a: Rule) =>
    [b.name !== a.name && b.name, b.effect !== a.effect && title(b), ...facts(b, a, b), verbs(b), ...conditions(b)].filter(Boolean).join(", ");
  const meta = (r: Rule) => `Rule ${r.id} · line ${r.line}${r.by === "proposal" ? " · Iris proposed it" : ""}`;
</script>

{#snippet change(rule: Rule)}
  <b>{rule.name}</b> <span class="effect">{title(rule)}</span>
{/snippet}

{#snippet sentence(rule: Rule)}
  {#if rule.verbs.length}<span class="verbs">{verbs(rule)}</span>{/if}
  {#each conditions(rule) as c (c)}<span class="chip">{c}</span>{/each}
{/snippet}

<h1>Rules</h1>
<p class="lead">
  Your rules decide what Iris does by itself, and what the brief shows. They live in <code>policy.toml</code> in the data folder of Iris.
  Edit the file, or change them here.
</p>

{#if problem}<p role="alert">{problem}</p>{/if}
{#if saved}<p class="done" role="status">Discarded. Iris kept a copy of your file as <code>{saved}</code>.</p>{/if}

{#if policy.error}
  <section class="notice bad" aria-labelledby="active-error">
    <h2 id="active-error">Iris cannot use its rules</h2>
    <p>The active policy has an error on line {policy.error.line}: {policy.error.message}</p>
    <p class="muted">Until you correct the file and confirm it, Iris applies nothing.</p>
  </section>
{/if}

{#if file.state === "invalid"}
  <section class="notice bad" aria-labelledby="file-error">
    <h2 id="file-error">Your policy file has an error</h2>
    <p>{file.error.line ? `Line ${file.error.line}: ` : ""}{file.error.message}</p>
    <p class="muted">Iris uses the rules below until you correct the file.</p>
    <div class="actions">
      <button type="button" class="quiet" onclick={discard} disabled={busy}>Discard the file</button>
    </div>
  </section>
{:else if file.state === "changed"}
  <section class="notice change" aria-labelledby="file-change">
    <h2 id="file-change">Your policy file changed</h2>
    <p class="muted">Iris uses the rules below until you confirm the change.</p>
    <ul class="diff">
      {#if file.mode !== policy.mode}
        <li><span class="tag">Mode</span><span>{MODES[policy.mode].title} to {MODES[file.mode].title}</span></li>
      {/if}
      {#each file.added as rule (rule.id)}
        <li><span class="tag add">New</span><span>{@render change(rule)} {@render sentence(rule)}</span></li>
      {/each}
      {#each file.changed as { before, after } (after.id)}
        <li>
          <span class="tag edit">Changed</span>
          <span>
            {@render change(after)} {@render sentence(after)}
            {#each facts(before, after, after).filter(Boolean) as f (f)}<span class="chip">{f}</span>{/each}
            <span class="was">Before: {was(before, after)}</span>
          </span>
        </li>
      {/each}
      {#each file.removed as rule (rule.id)}
        <li><span class="tag del">Removed</span><span>{@render change(rule)}</span></li>
      {/each}
      {#if empty}<li class="muted">Only comments or spaces changed.</li>{/if}
    </ul>
    <details>
      <summary>Show the file</summary>
      <pre>{file.text}</pre>
    </details>
    <div class="actions">
      <button type="button" class="primary" onclick={confirm} disabled={busy}>Confirm the change</button>
      <button type="button" class="quiet" onclick={discard} disabled={busy}>Discard</button>
    </div>
    <p class="small">A discard keeps a copy of your file in the data folder.</p>
  </section>
{:else if gone}
  <section class="notice change" aria-labelledby="file-gone">
    <h2 id="file-gone">Your policy file is missing</h2>
    <p class="muted">Iris uses the rules below. Put the file back to change them here or in the file.</p>
    <div class="actions">
      <button type="button" class="primary" onclick={discard} disabled={busy}>Put the file back</button>
    </div>
  </section>
{/if}

<section aria-labelledby="mode">
  <h2 id="mode" class="label">Mode</h2>
  <div class="modes" role="radiogroup" aria-labelledby="mode">
    {#each Object.entries(MODES) as [key, m] (key)}
      <label class="mode" class:on={choice === key}>
        <input type="radio" name="mode" value={key} bind:group={choice} disabled={blocked || busy} />
        <span class="ring" aria-hidden="true"></span>
        <span><b>{m.title}</b><span class="muted">{m.note}</span></span>
      </label>
    {/each}
  </div>
  {#if choice !== policy.mode}
    <div class="actions">
      <button type="button" class="primary" onclick={setMode} disabled={busy}>Use {MODES[choice].title.toLowerCase()} mode</button>
      <button type="button" class="quiet" onclick={() => (choice = policy.mode)} disabled={busy}>Cancel</button>
    </div>
  {:else if blocked}
    <p class="small">
      {gone ? "Put your policy file back first" : "Confirm or discard the change of your file first"}. Then you can change the mode and the rules here.
    </p>
  {/if}
</section>

{#if policy.rules.length === 0}
  <section class="none" aria-labelledby="no-rules">
    <h2 id="no-rules">No rules yet</h2>
    {#if file.state === "missing"}
      <p class="muted">Make <code>policy.toml</code> in the data folder of Iris, then confirm it here. For example, this file shows the feed as one line in the brief:</p>
    {:else}
      <p class="muted">Add a rule to the end of <code>policy.toml</code>, then confirm the change here. For example, this rule shows the feed as one line in the brief:</p>
    {/if}
    <pre>{example(new Date().toISOString().slice(0, 10), file.state === "missing" ? policy.mode : null, policy.next_id)}</pre>
  </section>
{/if}

{#each GROUPS as group (group.effect)}
  {@const rules = policy.rules.filter((r) => r.effect === group.effect)}
  {#if rules.length}
    <section aria-labelledby={group.effect}>
      <h2 id={group.effect} class="label">{group.title}</h2>
      <p class="muted note">{group.note}</p>
      <ul class="rules">
        {#each rules as rule (rule.id)}
          <li>
            <div class="rule">
              <p class="name">{rule.name}</p>
              <p class="how">{@render sentence(rule)}</p>
              <p class="small">{meta(rule)}</p>
            </div>
            {#if asking === rule.id}
              <div class="ask" role="group" aria-label={`Remove ${rule.name}`}>
                <p>Remove this rule? Iris does not use the number {rule.id} again.</p>
                <div class="actions">
                  <button type="button" class="danger" onclick={() => remove(rule.id)} disabled={busy}>Remove</button>
                  <button type="button" class="quiet" onclick={() => (asking = null)} disabled={busy}>Keep</button>
                </div>
              </div>
            {:else}
              <button type="button" class="quiet remove" onclick={() => (asking = rule.id)} disabled={blocked || busy} aria-label={`Remove ${rule.name}`}>Remove</button>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}
{/each}

<style>
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    margin: var(--space-4) 0 var(--space-2);
  }
  .lead {
    color: var(--color-text-muted);
    max-width: 60ch;
    margin: 0 0 var(--space-8);
  }
  section {
    margin: 0 0 var(--space-8);
  }
  h2 {
    font-size: var(--font-size-1);
    font-weight: 600;
    margin: 0 0 var(--space-2);
  }
  .label {
    font-size: var(--font-size--1);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--color-text-muted);
    margin: 0;
  }
  p {
    margin: 0 0 var(--space-2);
  }
  .muted,
  .small {
    color: var(--color-text-muted);
  }
  .small {
    font-size: var(--font-size--1);
  }
  .note {
    font-size: var(--font-size--1);
    margin: var(--space-1) 0 0;
  }
  .modes + .small {
    margin-top: var(--space-3);
  }
  .done {
    border-left: 4px solid var(--color-success);
    padding-left: var(--space-3);
    margin-bottom: var(--space-8);
  }
  .notice {
    padding: var(--space-4) var(--space-6);
    border-radius: var(--radius-lg);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border-subtle);
    border-left: 4px solid var(--color-primary);
  }
  .notice.bad {
    border-left-color: var(--color-error);
  }
  .diff {
    list-style: none;
    padding: 0;
    margin: var(--space-4) 0;
    display: grid;
    gap: var(--space-3);
  }
  .diff li {
    display: grid;
    grid-template-columns: 80px 1fr;
    gap: var(--space-3);
    align-items: baseline;
  }
  .diff li.muted {
    display: block;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--font-size--1);
    color: var(--color-text-muted);
  }
  /* A dot gives the kind of change at a glance; the word says it too. */
  .tag::before {
    content: "";
    width: 8px;
    height: 8px;
    border-radius: var(--radius-full);
    background: var(--color-text-faint);
  }
  .tag.add::before {
    background: var(--color-success);
  }
  .tag.edit::before {
    background: var(--color-warning);
  }
  .tag.del::before {
    background: var(--color-error);
  }
  .effect {
    font-size: var(--font-size--1);
    color: var(--color-text-muted);
  }
  .was {
    display: block;
    font-size: var(--font-size--1);
    color: var(--color-text-muted);
    margin-top: var(--space-1);
  }
  details {
    margin: 0 0 var(--space-4);
  }
  summary {
    cursor: pointer;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  pre {
    font-family: var(--font-family-mono);
    font-size: var(--font-size--1);
    background: var(--color-background-surface);
    border-radius: var(--radius-md);
    padding: var(--space-3) var(--space-4);
    overflow-x: auto;
    margin: var(--space-3) 0 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    margin: var(--space-4) 0 var(--space-2);
  }
  button {
    font: inherit;
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }
  button:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .primary {
    font-weight: 600;
    color: var(--color-on-primary);
    background: var(--color-primary);
    border: 1px solid var(--color-primary);
  }
  .quiet,
  .danger {
    color: var(--color-text);
    background: none;
    border: 1px solid var(--color-border);
  }
  .quiet:hover:not(:disabled) {
    border-color: var(--color-text-faint);
  }
  .danger {
    border-color: var(--color-error);
  }
  .danger:hover:not(:disabled) {
    background: var(--color-background-surface);
  }
  .modes {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
    margin-top: var(--space-4);
  }
  .mode {
    position: relative;
    display: flex;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    cursor: pointer;
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }
  .mode b {
    display: block;
    margin-bottom: var(--space-1);
  }
  .mode.on {
    border-color: var(--color-primary);
    background: var(--color-background-elevated);
  }
  .mode:has(input:disabled) {
    cursor: default;
  }
  .mode:has(input:focus-visible) {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  .mode input {
    position: absolute;
    opacity: 0;
  }
  /* The mark of Iris: a ring that fills with a center when the mode is chosen. */
  .ring {
    flex: none;
    width: 16px;
    height: 16px;
    margin-top: var(--space-1);
    border: 2px solid var(--color-text-faint);
    border-radius: var(--radius-full);
    box-sizing: border-box;
    display: grid;
    place-items: center;
    transition: border-color var(--transition-fast);
  }
  .ring::after {
    content: "";
    width: 6px;
    height: 6px;
    border-radius: var(--radius-full);
    background: var(--color-primary);
    transform: scale(0);
    transition: transform var(--transition-normal);
  }
  .on .ring {
    border-color: var(--color-primary);
  }
  .on .ring::after {
    transform: scale(1);
  }
  .rules {
    list-style: none;
    padding: 0;
    margin: var(--space-2) 0 0;
  }
  .rules > li {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-4) 0;
  }
  .rules > li + li {
    border-top: 1px solid var(--color-border-subtle);
  }
  .rule {
    flex: 1;
    min-width: 0;
  }
  .name {
    font-weight: 600;
    margin: 0 0 var(--space-1);
  }
  .how {
    margin: 0 0 var(--space-1);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .verbs {
    margin-right: var(--space-1);
  }
  .chip {
    font-size: var(--font-size--1);
    padding: 0 var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    color: var(--color-text-muted);
    white-space: nowrap;
  }
  /* Quiet until the pointer or the focus is on the rule, so the list reads calm. */
  .diff .chip {
    margin-left: var(--space-1);
  }
  .remove {
    font-size: var(--font-size--1);
    padding: var(--space-1) var(--space-3);
    color: var(--color-text-muted);
    border-color: transparent;
  }
  .rules > li:hover .remove:not(:disabled),
  .remove:focus-visible {
    color: var(--color-text);
    border-color: var(--color-border);
  }
  .ask {
    flex-basis: 100%;
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-md);
    background: var(--color-background-surface);
  }
  .ask .actions {
    margin: var(--space-2) 0 0;
  }
  .none pre {
    max-width: 40ch;
  }
  @media (max-width: 600px) {
    .modes {
      grid-template-columns: 1fr;
    }
    .notice {
      padding: var(--space-4);
    }
    .diff li {
      grid-template-columns: 1fr;
      gap: var(--space-1);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .ring,
    .ring::after,
    .mode,
    button {
      transition: none;
    }
  }
</style>
