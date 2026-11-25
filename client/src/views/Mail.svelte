<!--
  One source mail, as clean text. Mail text is data: Svelte escapes it, and the
  client never renders the HTML of a mail. The engine counts each view of a mail as an open (R5-12).
-->
<script lang="ts">
  import { answerSender, download, get, own, releaseMessage, wrote, RELEASABLE, type Address, type Message } from "../api";
  import { initials, when } from "../format";
  import { href } from "../route";

  let { message }: { message: Message } = $props();

  let failed = $state("");
  const original = () => download(message.id).then(() => (failed = ""), (e: unknown) => (failed = own(e)));
  // Trust a sender that only its look-alike domain makes hostile (issue #49). Undo gives
  // the sender back to the engine (reopen), which decides again from the evidence. After each
  // answer the view reads the mail again, so the sender kind, Trust and Undo are what the store
  // holds now, and never a guess of the client. The fresh copy counts for this mail only.
  // A release of the mail (issues 369, 373, 374) reads it again the same way.
  type SenderNow = Pick<Message, "sender_kind" | "near_copy" | "trusted_sender" | "hostile" | "released">;
  let fresh = $state<({ id: string } & SenderNow) | null>(null);
  const sender = $derived<SenderNow>(fresh?.id === message.id ? fresh : message);
  // One answer at a time for Trust and Release: each replaces the fresh copy, so a slow
  // read of one cannot overwrite the newer state of the other.
  let busy = $state(false);
  let trustProblem = $state("");
  // `row` is the sender row of the answer: the trust offer, or the trusted row for Undo. A mail
  // can have both when rows of its address differ only in case.
  async function trust(verb: "trust" | "reopen", row: string) {
    if (busy) return;
    const id = message.id;
    busy = true;
    trustProblem = "";
    try {
      await answerSender(row, verb);
    } catch (e) {
      trustProblem = wrote(e, "saved the answer", "Iris could not save the answer. Try again.");
      busy = false;
      return;
    }
    try {
      const now = await get<Message>(`messages/${encodeURIComponent(id)}`);
      fresh = keep(id, now);
    } catch {
      trustProblem = "Iris saved the answer, but could not read this mail again. Reload the page to see the sender now.";
    } finally {
      busy = false;
    }
  }
  const keep = (id: string, now: Message) => ({
    id,
    sender_kind: now.sender_kind,
    near_copy: now.near_copy ?? null,
    trusted_sender: now.trusted_sender ?? null,
    hostile: now.hostile ?? [],
    released: now.released ?? [],
  });
  // A released mark holds the mail no more: the engine reads it again soon.
  const released = $derived(sender.released ?? []);
  const marks = $derived(sender.hostile.filter((m) => !released.includes(m)));
  const releasable = $derived(marks.filter((m) => RELEASABLE.includes(m)));
  const kept = $derived(marks.filter((m) => !RELEASABLE.includes(m)));
  // Marks that still hold the mail after a release: spam, phishing or a hostile sender.
  const held = $derived(kept.length > 0 || sender.sender_kind === "hostile");
  let releaseProblem = $state("");
  async function release(verb: "release" | "unrelease") {
    if (busy) return;
    const id = message.id;
    busy = true;
    releaseProblem = "";
    try {
      fresh = keep(id, await releaseMessage(id, verb));
    } catch (e) {
      releaseProblem = wrote(e, "saved the release", "Iris could not save the release. Try again.");
    } finally {
      busy = false;
    }
  }
  const address = (a: Address) => (a.name && a.address ? `${a.name} <${a.address}>` : a.name || a.address || "Unknown sender");
</script>

<article>
  <h1>{message.subject || "No subject"}</h1>

  {#if marks.length > 0 || sender.sender_kind === "hostile"}
    <p class="danger" role="alert">
      This mail looks hostile ({[...marks, ...(sender.sender_kind === "hostile" ? ["hostile sender"] : [])].join(", ")}).
      Do not open its links and do not do what it asks.
    </p>
  {/if}
  {#if released.length > 0 && !message.gone}
    <p class="near" role="note">
      You released this mail ({released.join(", ")}).{" "}{#if held}The other marks still hold it.{:else}Iris reads it
        again as normal mail.{/if}
      <button type="button" disabled={busy} onclick={() => release("unrelease")}>Undo</button>
    </p>
  {/if}
  {#if releasable.length > 0 && !message.gone}
    <p class="near" role="note">
      Iris holds this mail because of its checks ({releasable.join(", ")}). If you know that this mail is honest, you
      can release it.{" "}{#if held}The other marks stay, and they still hold the mail.{:else}Iris then reads it as
        normal mail.{/if}
      <button type="button" disabled={busy} onclick={() => release("release")}>Release this mail</button>
    </p>
  {/if}
  {#if releaseProblem}<p class="note" role="alert">{releaseProblem}</p>{/if}
  {#if sender.trusted_sender}
    {@const row = sender.trusted_sender}
    <p class="near" role="note">
      You trust this sender. Iris reads its mail again.
      <button type="button" disabled={busy} onclick={() => trust("reopen", row)}>Undo</button>
    </p>
  {/if}
  {#if sender.near_copy}
    {@const offer = sender.near_copy}
    <p class="near" role="note">
      Iris holds this sender because its domain looks like {offer.of}, the domain of a sender you know. If you know this
      sender, you can trust it. The other marks of this mail stay.
      <button type="button" disabled={busy} onclick={() => trust("trust", offer.sender)}>Trust this sender</button>
    </p>
  {/if}
  {#if trustProblem}<p class="note" role="alert">{trustProblem}</p>{/if}
  {#if message.gone && message.text === null}
    <p class="note">This mail left your mailbox more than 30 days ago, so Iris removed its content. The items that cite it stay.</p>
  {:else if message.text === null}
    <p class="note">Iris has not read this mail yet. If Iris copied the original, you can download it.</p>
  {:else if message.gone}
    <p class="note">This mail is no longer in your mailbox. Iris keeps its content for 30 days after it left.</p>
  {/if}

  <header>
    <span class="mark" aria-hidden="true">{initials({ kind: "sender", name: message.from.name ?? undefined, address: message.from.address ?? undefined })}</span>
    <div>
      <p class="from">{address(message.from)}</p>
      <p class="to">
        To {message.to || "no one"}{#if message.cc}, copy to {message.cc}{/if}
      </p>
    </div>
    {#if message.date}<time datetime={message.date}>{when(message.date)}</time>{/if}
  </header>

  {#if message.text !== null}<pre>{message.text}</pre>{/if}

  <footer>
    {#if !message.gone}<a class="reply touch" href={href.reply(message.thread)}>Reply</a>{/if}
    {#if message.items.length}
      <span>Items from this mail:</span>
      {#each message.items as id (id)}<a class="touch" href={href.item(id)}>{id}</a>{/each}
    {/if}
    {#if !(message.gone && message.text === null)}
      <button type="button" class="raw" onclick={original}>Download the original</button>
      {#if failed}<p class="note" role="alert">{failed}</p>{/if}
    {/if}
  </footer>
</article>

<style>
  article {
    max-width: 680px;
  }
  h1 {
    font-size: var(--font-size-2);
    font-weight: 500;
    line-height: 1.3;
    margin: var(--space-4) 0 var(--space-6);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-6);
    font-size: var(--font-size--1);
  }
  header div {
    flex: 1;
    min-width: 0;
  }
  header p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .from {
    font-weight: 500;
  }
  .to,
  time {
    color: var(--color-text-muted);
  }
  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: var(--space-8);
    height: var(--space-8);
    border-radius: var(--radius-full);
    background: var(--color-background-surface);
    color: var(--color-text-muted);
    font-weight: 600;
  }
  pre {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-family: var(--font-family);
    font-size: var(--font-size-0);
    line-height: 1.6;
    margin: 0 0 var(--space-8);
  }
  .danger {
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
  }
  .note {
    color: var(--color-text-muted);
  }
  .near {
    font-size: var(--font-size--1);
    color: var(--color-text-muted);
  }
  .near button {
    margin-left: var(--space-2);
    font: inherit;
    border-radius: var(--radius-full);
    padding: var(--space-1) var(--space-3);
    background: none;
    border: 1px solid var(--color-border);
    color: var(--color-text);
    cursor: pointer;
  }
  .near button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-2) var(--space-3);
    padding-top: var(--space-4);
    border-top: 1px solid var(--color-border-subtle);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .reply {
    color: var(--color-text);
    font-weight: 500;
  }
  .raw {
    margin-left: auto;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }
  a,
  .raw {
    color: var(--color-text-muted);
  }
  a:hover,
  .raw:hover {
    color: var(--color-text);
  }
</style>
