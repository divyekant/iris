<!--
  Ask (card R5-09, option B, the owner's pick): questions and answers stack
  as a conversation, with the question box at the bottom. Each fact of an
  answer has a numbered mark that opens its source mail, and each answer
  has one chip for each source. The answer is data: Svelte escapes it.
-->
<script lang="ts">
  import { ask, cost, parts, thread } from "../ask.svelte.ts";
  import type { AskSource } from "../api";
  import { href } from "../route";

  let end: HTMLElement | undefined = $state();

  const from = (s: AskSource) => s.message.from.name || s.message.from.address || "Unknown sender";
  const label = (s: AskSource) => `${from(s)}: ${s.message.subject || "No subject"}`;

  // ask() gives the question back to the box after an error.
  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const sent = thread.draft;
    thread.draft = "";
    if (!(await ask(sent)) && !thread.draft) thread.draft = sent;
  }

  // Show the newest question and its answer.
  $effect(() => {
    void thread.turns.length;
    void thread.busy;
    end?.scrollIntoView?.({ block: "end" });
  });
</script>

{#snippet mark()}
  <svg class="iris" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true">
    <circle cx="12" cy="12" r="9" fill="none" stroke="var(--color-primary)" stroke-width="2" />
    <circle cx="12" cy="12" r="3.5" fill="var(--color-primary)" />
  </svg>
{/snippet}

<section class="ask">
  <h1 class={thread.turns.length ? "hidden" : ""}>Ask your mail</h1>
  {#if !thread.turns.length}
    <p class="intro">Ask a question about your mail. Iris answers from your mail and shows the mail that each fact comes from.</p>
  {/if}

  <ol class="thread" aria-live="polite">
    {#each thread.turns as turn, i (i)}
      {@const older = i < thread.turns.length - 1}
      <li class:older>
        <p class="question">{turn.question}</p>
        <div class="reply" class:wait={!turn.answer && !turn.error}>
          {@render mark()}
          <div class="body">
            {#if turn.error}
              <p class="error" role="alert">{turn.error}</p>
            {:else if !turn.answer}
              <p class="muted">Iris reads your mail.</p>
            {:else if turn.answer.answer === null}
              <p class="muted">
                {turn.answer.read === 0 ? "No mail has the words of this question." : "Your mail does not answer this question."}
              </p>
            {:else}
              {@const sources = turn.answer.sources}
              <p class="text">
                {#each parts(turn.answer.answer, sources) as part, k (k)}
                  {#if "text" in part}{part.text}{:else}<a class="cite" href={href.mail(part.source.message.id)} aria-label={`Source ${part.source.n}: ${label(part.source)}`}>{part.source.n}</a>{/if}
                {/each}
              </p>
              <ul class="sources" aria-label="Sources">
                {#each sources as s (s.n)}
                  <li>
                    <a class="chip" href={href.mail(s.message.id)}><span class="n" aria-hidden="true">{s.n}</span><span class="label">{label(s)}</span></a>
                  </li>
                {/each}
              </ul>
            {/if}
            {#if turn.answer && turn.answer.read > 0}
              <p class="foot">
                Iris read {turn.answer.read} {turn.answer.read === 1 ? "mail" : "mails"}.{#if cost(turn.answer.cost_micro_usd)}
                  {" "}{cost(turn.answer.cost_micro_usd)}.{/if}
              </p>
            {/if}
          </div>
        </div>
      </li>
    {/each}
  </ol>
  <div class="end" bind:this={end}></div>

  <form onsubmit={submit}>
    <label class="hidden" for="ask-question">Your question</label>
    <div class="field">
      <input
        id="ask-question"
        bind:value={thread.draft}
        maxlength="500"
        autocomplete="off"
        placeholder={thread.turns.length ? "Ask another question" : "When does my flight leave?"}
      />
      <button class="go" type="submit" aria-label="Ask" disabled={thread.busy || !thread.draft.trim()}>
        <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">
          <path d="M4 10h11M11 5.5 15.5 10 11 14.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </button>
    </div>
  </form>
</section>

<style>
  .ask {
    display: flex;
    flex-direction: column;
    min-height: calc(100dvh - 160px);
  }
  .hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    letter-spacing: -0.02em;
    line-height: 1.2;
    margin: var(--space-6) 0 var(--space-3);
  }
  .intro {
    margin: 0;
    color: var(--color-text-muted);
  }
  .thread {
    flex: 1;
    list-style: none;
    margin: 0;
    padding: var(--space-4) 0 0;
    display: grid;
    gap: var(--space-8);
    align-content: start;
  }
  li {
    display: grid;
    gap: var(--space-6);
  }
  p {
    margin: 0;
  }
  .question {
    justify-self: end;
    max-width: 80%;
    background: var(--color-background-surface);
    border-radius: var(--radius-lg) var(--radius-lg) var(--radius-sm) var(--radius-lg);
    padding: var(--space-3) var(--space-4);
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .reply {
    display: flex;
    gap: var(--space-3);
  }
  .iris {
    flex: none;
  }
  .wait .iris {
    animation: breathe 1.6s ease-in-out infinite;
  }
  @keyframes breathe {
    50% {
      opacity: 0.4;
    }
  }
  .body {
    min-width: 0;
    flex: 1;
  }
  .text {
    line-height: 1.6;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .muted,
  .older .text,
  .older .question {
    color: var(--color-text-muted);
  }
  .error {
    margin: 0;
  }
  .cite {
    display: inline-grid;
    place-items: center;
    min-width: 20px;
    height: 20px;
    padding: 0 var(--space-1);
    margin-left: var(--space-1);
    border-radius: var(--radius-full);
    border: 1px solid var(--color-primary);
    color: var(--color-primary);
    font-size: var(--font-size--2);
    font-weight: 600;
    line-height: 1;
    text-decoration: none;
    vertical-align: 2px;
    transition: background var(--transition-fast), color var(--transition-fast);
  }
  .cite:hover {
    background: var(--color-primary);
    color: var(--color-on-primary);
  }
  .sources {
    list-style: none;
    margin: var(--space-3) 0 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .sources li {
    display: block;
    max-width: 100%;
  }
  .chip {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    max-width: 100%;
    padding: var(--space-1) var(--space-3) var(--space-1) var(--space-1);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    font-size: var(--font-size--1);
    text-decoration: none;
    transition: border-color var(--transition-fast);
  }
  .chip:hover {
    border-color: var(--color-primary);
  }
  .n {
    flex: none;
    display: grid;
    place-items: center;
    width: var(--space-6);
    height: var(--space-6);
    border-radius: var(--radius-full);
    border: 1px solid var(--color-primary);
    color: var(--color-primary);
    font-size: var(--font-size--2);
    font-weight: 600;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .foot {
    margin-top: var(--space-2);
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  /* The question box is sticky, so a scroll to the end keeps room for it. */
  .end {
    scroll-margin-bottom: var(--space-32);
  }
  form {
    position: sticky;
    bottom: 0;
    padding: var(--space-6) 0 var(--space-4);
    background: linear-gradient(to bottom, transparent, var(--color-background) 30%);
  }
  .field {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    padding: var(--space-2) var(--space-2) var(--space-2) var(--space-6);
    transition: border-color var(--transition-fast);
  }
  .field:focus-within {
    border-color: var(--color-primary);
  }
  input {
    flex: 1;
    min-width: 0;
    font: inherit;
    font-size: var(--font-size-1);
    color: var(--color-text);
    background: none;
    border: 0;
    outline: none;
    padding: var(--space-2) 0;
  }
  input::placeholder {
    color: var(--color-text-faint);
  }
  .go {
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: var(--radius-full);
    border: 0;
    background: var(--color-primary);
    color: var(--color-on-primary);
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }
  .go:disabled {
    opacity: 0.4;
    cursor: default;
  }
  @media (max-width: 600px) {
    input {
      font-size: var(--font-size-0);
    }
    .question {
      max-width: 90%;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .wait .iris {
      animation: none;
    }
  }
</style>
