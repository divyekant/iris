<!--
  The brief (spec 1 section 8): the state of the mail, not a message list.
  It opens with what needs you, in words. Each section shows at most 7 lines
  and links its full list. Read only in release 1.
-->
<script lang="ts">
  import { get, loadBrief, newest, own, post, readStatus, type Brief, type Item, type Job, type Status } from "../api";
  import { poll as every, watch } from "../clock";
  import NoModel from "./NoModel.svelte";
  import ModelWait from "./ModelWait.svelte";
  import { NOTHING, across, copying, needsModel, dueSoon, greeting, nextDay, party, partyKey, signals, summary, unfinished, when } from "../format";
  import Code from "./Code.svelte";
  import Bundle from "./Bundle.svelte";
  import Line from "./Line.svelte";
  import Gate from "./Gate.svelte";
  import Arrived from "./Arrived.svelte";
  import Watches from "./Watches.svelte";
  import MatterCard from "./MatterCard.svelte";
  import Question from "./Question.svelte";
  import { loadIdle, loadQuestions, type Idle } from "../jobs";
  import { loadGrowing, loadProposed, type Proposed } from "../matters";
  import type { Matter } from "../api";
  import { arrivedLine, loadWatches, type Watch } from "../watches";
  import { flushHolds, HOLD } from "../act";
  import { href } from "../route";

  let {
    brief: firstBrief,
    status: firstStatus = null,
    watches: firstWatches,
    proposed: firstProposed,
    questions: firstQuestions,
    idle: firstIdle,
    now: start = new Date(),
    poll = 5000,
    hold = HOLD,
  }: { brief: Brief; status?: Status | null; watches?: Watch[]; proposed?: Proposed | null; questions?: Job[]; idle?: Idle[]; now?: Date; poll?: number; hold?: number } = $props();

  // The API puts a conversation with a stranger only under Strangers, with exact counts (dk, 2026-11-16).
  // svelte-ignore state_referenced_locally
  let brief = $state(firstBrief);
  // svelte-ignore state_referenced_locally
  let status = $state(firstStatus);
  // svelte-ignore state_referenced_locally
  let watches = $state(firstWatches ?? []);
  // svelte-ignore state_referenced_locally
  let now = $state(start);
  let arrived = $derived(watches.filter((w) => w.state === "arrived"));
  let waiting = $derived(watches.filter((w) => w.state === "waits"));
  // A watched mail that came leads the brief. With nothing else, it is the whole sentence.
  let opening = $derived.by(() => {
    const rest = summary(brief);
    if (!arrived.length) return rest;
    return rest === NOTHING ? arrivedLine(arrived) : `${arrivedLine(arrived)} ${rest}`;
  });
  // Each watch load and each change takes a ticket. A load applies only while its ticket is the newest,
  // so an older answer never undoes a newer change. A failed load keeps the last list.
  const ticket = newest();
  async function reloadWatches() {
    const fresh = ticket();
    try {
      const w = await loadWatches();
      if (fresh()) watches = w;
    } catch {
      // The next poll tries again.
    }
  }
  const change = (next: (w: Watch[]) => Watch[]) => (ticket(), (watches = next(watches)));
  const removed = (id: string) => change((w) => w.filter((x) => x.id !== id));
  const ready = (s: Status | null) => copying(s).length === 0;

  // Two loads of the brief can answer out of order (a poll, an answer at the gate):
  // each takes a ticket, and a brief shows only while its ticket is the newest.
  const briefTicket = newest();
  // Polls, the read at the end of a model wait and a Try now answer can arrive out of order: only
  // the newest status shows (api.ts), and an older answer gives null.
  const show = (s: Status | null) => s && (status = s);

  // Load the brief again while it is open: every 5 seconds while the first sync
  // runs (spec 1 section 4.2), then every minute for new mail.
  let syncing = $derived(!ready(status));
  $effect(() => {
    if (!poll) return;
    return every(
      () => {
        const fresh = briefTicket();
        return Promise.all([loadBrief(), readStatus(), fresh] as const);
      },
      ([b, s, fresh]) => (fresh() && (brief = b), show(s)),
      syncing ? poll : poll * 12,
    );
  });
  // A quick Done holds for Undo. If the person leaves the brief or the page, the click stands: send it now.
  $effect(() => {
    window.addEventListener("pagehide", flushHolds);
    return () => {
      window.removeEventListener("pagehide", flushHolds);
      flushHolds();
    };
  });
  // The proposal of a matter loads apart from the brief too, with its own ticket. A failed load keeps the last card.
  // svelte-ignore state_referenced_locally
  let proposed = $state(firstProposed ?? null);
  const proposedTicket = newest();
  // The confirmed matters with additions (D20): one line each, that opens the matter.
  let growing = $state<Matter[]>([]);
  // True while the card holds a decision for Undo or for its answer: a poll keeps the card, so the hold keeps its time (issue 345).
  // The newest read waits in `later` and shows when the hold ends.
  let matterHeld = false;
  let later: { proposal: typeof proposed } | undefined;
  function holdMatter(on: boolean) {
    matterHeld = on;
    if (!on && later) [proposed, later] = [later.proposal, undefined];
  }
  async function reloadProposed() {
    const fresh = proposedTicket();
    try {
      const [p, g] = await Promise.all([loadProposed(), loadGrowing()]);
      if (!fresh()) return;
      growing = g;
      if (matterHeld) later = { proposal: p };
      else [proposed, later] = [p, undefined];
    } catch {
      // The next poll tries again.
    }
  }
  // The questions of the agents (R5-05) load apart from the brief too, with their own ticket. A failed load keeps the last list.
  // svelte-ignore state_referenced_locally
  let questions = $state(firstQuestions ?? []);
  const questionsTicket = newest();
  async function reloadQuestions() {
    const fresh = questionsTicket();
    try {
      const q = await loadQuestions();
      if (fresh()) questions = q;
    } catch {
      // The next poll tries again.
    }
  }
  // svelte-ignore state_referenced_locally
  if (firstQuestions === undefined) reloadQuestions();
  // After an answer: one quiet line names the job, and the next question (or none) shows at once.
  let sent = $state<Job | null>(null);
  // The unsent answer of each question, by job and question time: a poll that changes the card keeps it.
  const questionKey = (q: Job) => `${q.id} ${q.updated}`;
  let drafts = $state<Record<string, string>>({});
  // The question that the person picked with "Next question", if it is still open.
  let picked = $state("");
  // Else a question with an unsent answer stays on the card while it is open, so a newer question never takes the field away.
  let shown = $derived(questions.find((q) => questionKey(q) === picked) ?? questions.find((q) => drafts[questionKey(q)]?.trim()) ?? questions[0]);
  // Spec 2 section 7.3: the line of the changes that Iris could not confirm stays until you dismiss it.
  // The dismissal names the entries that the line showed, so a later failure shows a new line.
  let dismissing = $state(false);
  let dismissProblem = $state("");
  async function dismissUnconfirmed() {
    const entries = brief.unconfirmed?.entries ?? [];
    dismissing = true;
    dismissProblem = "";
    try {
      await post("ledger/unconfirmed/dismiss", { entries });
      brief = { ...brief, unconfirmed: { messages: 0, entries: [] } };
    } catch (e) {
      dismissProblem = own(e);
    } finally {
      dismissing = false;
    }
  }
  function nextQuestion() {
    const at = shown ? questions.indexOf(shown) : -1;
    picked = questionKey(questions[(at + 1) % questions.length]);
  }
  const answeredJob = (job: Job) => {
    sent = job;
    delete drafts[questionKey(job)];
    questions = questions.filter((q) => q.id !== job.id);
    reloadQuestions();
  };
  // The working jobs that no agent works on (issue 791): the person must start an agent, or the job waits for nothing.
  // They load apart from the brief, with their own ticket. A failed load keeps the last list.
  // svelte-ignore state_referenced_locally
  let idle = $state<Idle[]>(firstIdle ?? []);
  const idleTicket = newest();
  async function reloadIdle() {
    const fresh = idleTicket();
    try {
      const list = await loadIdle();
      if (fresh()) idle = list;
    } catch {
      // The next poll tries again.
    }
  }
  // svelte-ignore state_referenced_locally
  if (firstIdle === undefined) reloadIdle();
  // The watches load apart from the brief, so a slow or failed watch load never holds the brief.
  // svelte-ignore state_referenced_locally
  if (firstWatches === undefined) reloadWatches();
  // svelte-ignore state_referenced_locally
  if (firstProposed === undefined) reloadProposed();
  $effect(() => {
    if (!poll) return;
    const timer = setInterval(() => (reloadWatches(), reloadProposed(), reloadQuestions(), reloadIdle()), poll * 12);
    return () => clearInterval(timer);
  });

  const SHOWN = 7;
  // After an answer at the gate, the items of the sender move: load the brief again.
  // The gate stays after the last answer, so that its Undo stays too.
  let answered = $state(false);
  const reload = () => {
    answered = true;
    const fresh = briefTicket();
    loadBrief().then((b) => void (fresh() && (brief = b)), () => {});
  };

  // After a quick Done, load the brief again, so the item and the counts change. The ticket keeps a poll
  // that started before the close from putting the item back. A failed load waits for the next poll.
  const done = () => {
    const fresh = briefTicket();
    loadBrief().then((b) => void (fresh() && (brief = b)), () => {});
  };

  let signal = $derived(signals(brief.signals, now));
  // The code cards with an open Dismiss (held, sending or failed) stay after their expiry, so the Undo or the result does not go.
  let kept: Item[] = $state([]);
  const keep = (item: Item, on: boolean) => {
    const has = kept.some((k) => k.id === item.id);
    if (on && !has) kept = [...kept, item];
    else if (!on && has) kept = kept.filter((k) => k.id !== item.id);
  };
  let codes = $derived([...signal.codes, ...kept.filter((k) => !signal.codes.some((c) => c.id === k.id))]);
  // A line with a held quick verb, or a result to read, stays in its section across a refresh, so its Undo
  // and its result do not go (issue 316). It shows in that section only, and the section stays while it shows.
  let held: { key: string; item: Item }[] = $state([]);
  const holdIn = (key: string) => (item: Item, on: boolean) => {
    held = held.filter((h) => h.item.id !== item.id);
    if (on) held = [...held, { key, item }];
  };
  const holds = (key: string) => held.some((h) => h.key === key);
  function lines(key: string, items: Item[]): Item[] {
    const mine = held.filter((h) => h.key === key).map((h) => h.item);
    const top = items.filter((i) => !held.some((h) => h.item.id === i.id && h.key !== key)).slice(0, SHOWN);
    const missing = mine.filter((m) => !top.some((t) => t.id === m.id));
    // At most SHOWN lines (docs/DESIGN.md): a held line takes the place of the last line that holds nothing.
    for (let i = top.length - 1; i >= 0 && top.length + missing.length > SHOWN; i--) {
      if (!mine.some((m) => m.id === top[i].id)) top.splice(i, 1);
    }
    return [...top, ...missing];
  }
  // A signal leaves at its expiry. The date and the due words change at midnight.
  // Keep `now` fresh until the first of the two.
  let until = $derived([signal.next, nextDay(now)].filter((t): t is string => !!t).sort((a, b) => Date.parse(a) - Date.parse(b))[0]);
  $effect(() => watch(until, (d) => (now = d)));
  let proposal = $derived(brief.proposal && brief.proposal.count > 0 ? brief.proposal : null);
  // What you owe that is due in 2 days or less, or late, leads the brief (R3-08). It shows only there, so You owe counts the rest.
  // shortcut: the API gives at most 200 items of You owe, so a due item past them stays in You owe; page the API if one person owes more.
  let soon = $derived(dueSoon(brief.you_owe.items, now));
  let parts = $derived([
    { key: "you_owe", title: "You owe", count: brief.you_owe.count - soon.length, items: brief.you_owe.items.filter((i) => !soon.includes(i)), duplicates: brief.you_owe.duplicates, total: brief.you_owe.count },
    { key: "owed_to_you", title: "Owed to you", ...brief.owed_to_you },
    { key: "conversations", title: "Conversations", ...brief.conversations },
    { key: "strangers", title: "From senders you do not know", ...brief.strangers },
    {
      key: "signals",
      title: "Signals",
      ...brief.signals,
      // The count of the API, less the signals that expired since. The live codes show at the top, so only the rows leave them out.
      count: signal.count,
      items: signal.rows,
    },
  ]);

  // The feed shows by publisher. Two senders with the same name stay apart.
  let publishers = $derived.by(() => {
    const groups = new Map<string, { name: string; items: Item[] }>();
    for (const item of brief.feed.items) {
      const key = partyKey(item.publisher);
      const group = groups.get(key) ?? { name: party(item.publisher), items: [] };
      group.items.push(item);
      groups.set(key, group);
    }
    return [...groups].sort((a, b) => b[1].items.length - a[1].items.length);
  });
  // Counts for each sender are exact only when the API gave every feed item.
  let feedComplete = $derived(brief.feed.items.length >= brief.feed.count);
  let bundles = $derived(brief.bundles ?? []);
  let hidden = $derived(brief.hidden ?? 0);
  // An API from before R2-09 gives only the hidden count.
  let handled = $derived(brief.handled ?? { count: hidden, verbs: {} });
  // The messages of the actions that a rule approved. An approved action can still wait to apply, so the brief says decided, not changed.
  let decided = $derived(Object.values(handled.verbs).reduce((a, b) => a + b, 0));
  let records = $derived(Object.entries(brief.records).filter(([, n]) => n > 0));
  let progress = $derived(status && unfinished(status).length ? status.messages : null);
  let stopped = $derived(copying(status).length === 0);
  let day = $derived(now.toLocaleDateString("en-US", { weekday: "long", month: "long", day: "numeric" }));
</script>

<div class="opening">
  <p class="day">{greeting(now)} It is {day}.</p>
  <h1>{opening}</h1>
</div>

{#if needsModel(status)}<NoModel />{/if}
{#if status?.model_retry_at}<ModelWait at={status.model_retry_at} onstatus={show} />{/if}

{#if arrived.length}
  <div class="arrivals">
    {#each arrived as w (w.id)}<Arrived watch={w} ondone={removed} />{/each}
  </div>
{/if}


{#if progress}
  <div class="copying" role="status">
    <p>
      {#if stopped}
        Iris copied {progress.fetched.toLocaleString("en-US")} of {progress.total ? progress.total.toLocaleString("en-US") : "all"} messages{across(status)}, then the sign-in stopped.
      {:else}
        Iris still copies your mailbox: {progress.fetched.toLocaleString("en-US")} of {progress.total ? progress.total.toLocaleString("en-US") : "all"} messages{across(status)}.
      {/if}
      {brief.oldest_fetched ? `This brief covers the mail since ${when(brief.oldest_fetched)}.` : stopped ? "This brief covers the mail that Iris copied." : "The brief grows as the copy continues."}
    </p>
    <progress max={progress.total || 1} value={progress.fetched} aria-label="Messages copied"></progress>
  </div>
{/if}

{#if codes.length}
  <div class="codes">
    {#each codes as item (item.id)}<Code {item} {now} ondone={done} onkeep={keep} delay={hold} />{/each}
  </div>
{/if}

<!-- The question of an agent (R5-05): one card, the newest question, answered in place. -->
{#snippet sentLine(job: Job)}
  <p class="sent" role="status">
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path d="M3 8.5l3 3 7-7" fill="none" stroke="var(--color-primary)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /></svg>
    Sent. The job “{job.goal}” goes on with your answer.
  </p>
{/snippet}
{#if shown}
  <section aria-labelledby="questions">
    <h2 id="questions"><span>{questions.length === 1 ? "A question for you" : "Questions for you"}</span> <span class="count">{questions.length}</span></h2>
    {#if sent}{@render sentLine(sent)}{/if}
    <!-- A new question of the same job is a new card: the key has the time of the question. -->
    {#key questionKey(shown)}<Question job={shown} draft={drafts[questionKey(shown)] ?? ""} ondraft={(t) => (drafts[questionKey(shown)] = t)} onanswered={answeredJob} onchanged={reloadQuestions} />{/key}
    {#if questions.length > 1}
      <div class="more">
        <button type="button" class="all touch" onclick={nextQuestion}>Next question</button>
        <a class="all touch" href="#/jobs">All {questions.length} questions</a>
      </div>
    {/if}
  </section>
{:else if sent}
  <div class="alone">{@render sentLine(sent)}</div>
{/if}

<!-- A job with no agent (issue 791): its page tells how the last session ended and how to start one. -->
{#if idle.length}
  <section aria-labelledby="idle">
    <h2 id="idle"><span>No agent works on {idle.length === 1 ? "this job" : "these jobs"}</span> <span class="count">{idle.length}</span></h2>
    <ul class="idle">
      {#each idle.slice(0, SHOWN) as i (i.job.id)}
        <li><a class="touch" href={href.job(i.job.id)}>{i.job.goal}</a><span class="why">{i.ended ?? "No agent started on it yet."}</span></li>
      {/each}
    </ul>
    {#if idle.length > SHOWN}<a class="all touch" href="#/jobs">All {idle.length} jobs</a>{/if}
  </section>
{/if}

{#if soon.length || holds("soon")}
  <section class="soon" aria-labelledby="soon">
    <h2 id="soon"><span>Due soon</span> <span class="count">{soon.length}</span></h2>
    <ul>
      {#each lines("soon", soon) as item (item.id)}<Line {item} {now} twins={brief.you_owe.duplicates?.[item.id]} ondone={done} onheld={holdIn("soon")} delay={hold} />{/each}
    </ul>
    {#if soon.length > SHOWN}<a class="all touch" href="#/brief/you_owe">All {brief.you_owe.count} that you owe</a>{/if}
  </section>
{/if}

<!-- A matter that Iris proposes (R6-01, option A of the owner): one card, the newest proposal. -->
{#if proposed}
  <section aria-labelledby="matter">
    <h2 id="matter"><span>{proposed.count === 1 ? "A matter to confirm" : "Matters to confirm"}</span> <span class="count">{proposed.count}</span></h2>
    {#key proposed.view.matter.id}<MatterCard
        view={proposed.view}
        ondecided={() => (done(), reloadProposed())}
        onheld={holdMatter}
        delay={hold}
      />{/key}
    {#if proposed.count > 1}<a class="all touch" href="#/matters">All {proposed.count} that Iris proposes</a>{/if}
  </section>
{/if}

<!-- Items that Iris proposes to add to a confirmed matter (D20): one line for each matter. -->
{#if growing.length}
  <section aria-labelledby="growing">
    <h2 id="growing"><span>Your matters</span> <span class="count">{growing.length}</span></h2>
    <ul class="growing">
      {#each growing as m (m.id)}
        <li><a class="touch" href={href.matter(m.id)}>Add {m.additions?.length === 1 ? "1 item" : `${m.additions?.length} items`} to {m.title}?</a></li>
      {/each}
    </ul>
  </section>
{/if}

{#if brief.review?.actions}
  <section aria-labelledby="review">
    <h2 id="review"><span>Waits for your review</span> <span class="count">{brief.review.actions}</span></h2>
    {#if proposal}
      <p class="note">Triage proposes to {Object.entries(proposal.verbs).map(([verb, n]) => `${verb} ${n.toLocaleString("en-US")} ${n === 1 ? "message" : "messages"}`).join(", ")}.</p>
    {/if}
    <a class="all touch" href="#/review">Open the review</a>
  </section>
{/if}

{#each parts as part (part.key)}
  {#if part.key === "strangers" && brief.gate}
    {#if brief.gate.count > 0 || (brief.declined ?? 0) > 0 || answered}
      <section aria-labelledby="strangers">
        <h2 id="strangers"><span>{part.title}</span> <span class="count">{brief.gate.count}</span></h2>
        <Gate page={brief.gate} declined={brief.declined} onanswer={reload} />
      </section>
    {/if}
  {:else if part.count > 0 || holds(part.key)}
    <section aria-labelledby={part.key}>
      <h2 id={part.key}><span>{part.title}</span> <span class="count">{part.count}</span></h2>
      <!-- Live login codes show above the sections, so the Signals list can be empty. -->
      {#if part.items.length || holds(part.key)}
        <ul>
          {#each lines(part.key, part.items) as item (item.id)}<Line {item} {now} twins={part.duplicates?.[item.id]} ondone={done} onheld={holdIn(part.key)} delay={hold} />{/each}
        </ul>
      {/if}
      <a class="all touch" href={`#/brief/${part.key}`}>All {part.total ?? part.count}</a>
    </section>
  {/if}
{/each}

{#if bundles.length}
  <section aria-labelledby="bundles">
    <h2 id="bundles"><span>Bundled by your rules</span> <span class="count">{bundles.reduce((n, b) => n + b.count, 0)}</span></h2>
    <ul>
      {#each bundles as bundle (bundle.rule)}<Bundle {bundle} />{/each}
    </ul>
  </section>
{/if}

{#if handled.count > 0}
  <section aria-labelledby="handled">
    <h2 id="handled"><span>Handled without you</span> <span class="count">{handled.count}</span></h2>
    <p class="note">{[hidden ? `${hidden.toLocaleString("en-US")} hidden from this brief` : "", decided ? `${decided.toLocaleString("en-US")} ${decided === 1 ? "message" : "messages"} that your rules decided` : ""].filter(Boolean).join(", ")}.</p>
    <a class="all touch" href="#/handled">See what and why</a>
  </section>
{/if}

<!-- Watches (R5-07, option A of the owner): the watches that wait, and the quiet button that starts one. -->
<section class="watching" aria-labelledby={waiting.length ? "watching" : undefined} aria-label={waiting.length ? undefined : "Watches"}>
  {#if waiting.length}<h2 id="watching"><span>Iris watches for</span> <span class="count">{waiting.length}</span></h2>{/if}
  <Watches watches={waiting} onadded={(w) => (w ? change((all) => [w, ...all.filter((x) => x.id !== w.id)]) : reloadWatches())} onstopped={removed} />
</section>

{#if publishers.length || records.length}
  <section class="also" aria-labelledby="also">
    <h2 id="also">Also in your mail</h2>
    {#if publishers.length}
      <p class="label">To read when you choose</p>
      <ul class="chips">
        {#each publishers.slice(0, SHOWN) as [key, group] (key)}
          <li><a class="touch" href="#/brief/feed">{group.name}{#if feedComplete} <span>{group.items.length}</span>{/if}</a></li>
        {/each}
        {#if publishers.length > SHOWN || !feedComplete}<li><a class="touch" href="#/brief/feed">All {brief.feed.count}</a></li>{/if}
      </ul>
    {/if}
    {#if records.length}
      <p class="label">Records</p>
      <ul class="chips types">
        {#each records as [type, n] (type)}<li><a class="touch" href={`#/records/${type}`}>{type} <span>{n}</span></a></li>{/each}
      </ul>
    {/if}
  </section>
{/if}

<footer>
  {#if brief.hostile > 0}
    <p class="warning">Iris found {brief.hostile} hostile messages in the last 7 days. The brief does not show their subject or their sender.</p>
  {/if}
  {#if (brief.unconfirmed?.messages ?? 0) > 0}
    {@const n = brief.unconfirmed?.messages ?? 0}
    <p class="warning unconfirmed">
      Iris could not confirm a change to {n} {n === 1 ? "message" : "messages"}. Check {n === 1 ? "it" : "them"} in your mailbox.
      <span class="acts">
        <a class="touch" href="#/history">See what Iris did</a>
        <button type="button" class="all touch" onclick={dismissUnconfirmed} disabled={dismissing}>Dismiss</button>
      </span>
    </p>
    {#if dismissProblem}<p role="alert">{dismissProblem}</p>{/if}
  {/if}
  {#if brief.not_understood > 0}
    <p>Iris could not read {brief.not_understood} messages. They stay in your mailbox as they are.</p>
  {/if}
  {#if (status?.mode ?? "observe") === "observe" && decided === 0}<p>Iris only observes. It changed nothing in your mailbox.</p>{/if}
</footer>

<style>
  .opening {
    margin: var(--space-8) 0 var(--space-12);
    animation: arrive var(--transition-normal) both;
  }
  .day {
    color: var(--color-text-muted);
    margin: 0 0 var(--space-3);
  }
  h1 {
    font-size: var(--font-size-4);
    font-weight: 500;
    line-height: 1.2;
    letter-spacing: -0.02em;
    margin: 0;
    max-width: 28ch;
    text-wrap: balance;
  }
  @keyframes arrive {
    from {
      opacity: 0;
      transform: translateY(var(--space-1));
    }
  }
  .copying {
    margin-bottom: var(--space-8);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .copying p {
    margin: 0 0 var(--space-2);
  }
  progress {
    width: 100%;
    height: var(--space-1);
    accent-color: var(--color-primary);
  }
  .arrivals {
    display: grid;
    gap: var(--space-3);
    margin-bottom: var(--space-12);
    animation: arrive var(--transition-normal) both;
  }
  .codes {
    display: grid;
    gap: var(--space-3);
    margin-bottom: var(--space-12);
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
  .all {
    display: inline-block;
    margin-top: var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .all:hover {
    color: var(--color-text);
  }
  .more {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
  }
  button.all {
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    font-size: var(--font-size--1);
    cursor: pointer;
  }
  .alone {
    margin-bottom: var(--space-12);
  }
  ul.idle {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .idle li {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--color-border);
    overflow-wrap: anywhere;
  }
  .idle a {
    color: var(--color-text);
    font-weight: 500;
    text-decoration: none;
  }
  .idle a:hover,
  .idle a:focus-visible {
    text-decoration: underline;
  }
  .idle .why {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .sent {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0 0 var(--space-3);
    color: var(--color-text-muted);
    overflow-wrap: anywhere;
  }
  .note {
    margin: var(--space-2) 0 0;
    color: var(--color-text-muted);
  }
  .label {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    margin: var(--space-4) 0 var(--space-2);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .chips a {
    display: inline-flex;
    align-items: center;
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-full);
    background: var(--color-background-elevated);
    color: var(--color-text);
    text-decoration: none;
    transition: border-color var(--transition-fast);
  }
  .types a {
    text-transform: capitalize;
  }
  .chips a:hover {
    border-color: var(--color-primary);
  }
  .chips span {
    color: var(--color-text-muted);
    margin-left: var(--space-1);
  }
  footer {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    border-top: 1px solid var(--color-border-subtle);
    padding-top: var(--space-4);
  }
  footer p {
    margin: 0 0 var(--space-2);
  }
  .soon {
    border-left: 4px solid var(--color-warning);
    padding-left: var(--space-3);
  }
  .warning {
    color: var(--color-text);
    border-left: 4px solid var(--color-warning);
    padding-left: var(--space-3);
  }
  .unconfirmed .acts {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-4);
  }
  @media (max-width: 600px) {
    h1 {
      font-size: var(--font-size-2);
      line-height: 1.3;
    }
    .opening {
      margin: var(--space-4) 0 var(--space-8);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .arrivals,
    .opening {
      animation: none;
    }
    .chips a {
      transition: none;
    }
  }
</style>
