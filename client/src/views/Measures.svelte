<!--
  The measures page (card R5-12, design A, the owner's pick). One sentence, the main
  measure with a bar for each of the last 14 days, then one line for each other measure.
  The engine counts each measure on this machine. Nothing leaves it.
-->
<script lang="ts">
  import { average, bars, lead, rows, shortDay, type Measures } from "../measures";

  let { measures, now = new Date() }: { measures: Measures; now?: Date } = $props();

  let shown = $derived(bars(measures, now));
  let max = $derived(Math.max(1, ...shown.map((b) => b.opens ?? 0)));
  let opening = $derived(lead(measures, now));
  let mean = $derived(average(measures, now));
  let label = $derived(
    `Source mail opened each day: ${shown.map((b) => `${shortDay(b.day)}, ${b.opens === null ? "no use" : b.opens}`).join("; ")}.`,
  );
  const TONE = { met: "Meets the target", missed: "Misses the target", flat: "No fixed target", none: "No data yet" };
</script>

<h1>Measures</h1>
<p class="sub">Iris counts these on your machine. Nothing leaves it.</p>

<p class="lead">
  You opened <em>{opening.opens.toLocaleString("en-US")} source {opening.opens === 1 ? "mail" : "mails"}</em> today.
  {opening.text}
</p>

<section class="hero" aria-labelledby="opens">
  <div class="top">
    <h2 id="opens">Source mail opened</h2>
    <span class="big">{opening.opens.toLocaleString("en-US")}</span>
  </div>
  <div class="bars" role="img" aria-label={label}>
    {#each shown as bar, i (bar.day)}
      {#if bar.opens === null}
        <span class="off"></span>
      {:else}
        <span class:today={i === shown.length - 1} style:height={`${Math.max(6, (bar.opens / max) * 100)}%`}></span>
      {/if}
    {/each}
  </div>
  <div class="axis" aria-hidden="true"><span>{shortDay(shown[0].day)}</span><span>Today</span></div>
  <p>
    {#if mean.days === 0}No use in the last 14 days.{:else}{mean.perDay} a day on the {mean.days === 1 ? "day" : `${mean.days} days`} that you used Iris.{/if}
    Lower is better: the items tell you enough, so you seldom need the mail.
  </p>
</section>

<ul class="rows">
  {#each rows(measures, now) as row (row.name)}
    <li>
      <span class="dot {row.tone}" role="img" aria-label={TONE[row.tone]}></span>
      <span class="name">{row.name}</span>
      <span class="value" class:none={row.tone === "none"}>{row.value}</span>
      <span class="detail">{row.detail}</span>
    </li>
  {/each}
</ul>

<style>
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    margin: var(--space-4) 0 var(--space-1);
  }
  .sub {
    margin: 0 0 var(--space-8);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .lead {
    font-size: var(--font-size-2);
    font-weight: 500;
    letter-spacing: -0.01em;
    line-height: 1.3;
    margin: 0 0 var(--space-8);
    max-width: 30ch;
    text-wrap: balance;
  }
  .lead em {
    font-style: normal;
    color: var(--color-primary);
  }
  .hero {
    padding: var(--space-6) 0;
    border-top: 1px solid var(--color-border);
    border-bottom: 1px solid var(--color-border);
  }
  .top {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--space-4);
  }
  h2 {
    font-size: var(--font-size-0);
    font-weight: 600;
    margin: 0;
  }
  .big {
    font-size: var(--font-size-4);
    font-weight: 500;
    letter-spacing: -0.03em;
    line-height: 1;
    font-variant-numeric: tabular-nums;
  }
  .bars {
    display: grid;
    grid-template-columns: repeat(14, 1fr);
    gap: var(--space-1);
    align-items: end;
    height: 72px;
    margin: var(--space-6) 0 var(--space-2);
  }
  .bars span {
    display: block;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    background: var(--color-border);
    transition: height var(--transition-normal);
  }
  .bars span.today {
    background: var(--color-primary);
  }
  /* A day with no use: a dotted base line, no bar. */
  .bars span.off {
    height: 0;
    border-radius: 0;
    background: none;
    border-bottom: 2px dotted var(--color-border);
  }
  .axis {
    display: flex;
    justify-content: space-between;
    font-size: var(--font-size--1);
    color: var(--color-text-faint);
  }
  .hero p {
    margin: var(--space-3) 0 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .rows li {
    display: grid;
    grid-template-columns: var(--space-3) 1fr auto;
    gap: 0 var(--space-3);
    padding: var(--space-4) 0;
    border-bottom: 1px solid var(--color-border-subtle);
    align-items: baseline;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: var(--radius-full);
    align-self: center;
    box-sizing: border-box;
  }
  .dot.met {
    background: var(--color-success);
  }
  .dot.missed {
    background: var(--color-warning);
  }
  .dot.flat {
    background: var(--color-text-faint);
  }
  .dot.none {
    border: 1px solid var(--color-text-faint);
  }
  .name {
    font-weight: 500;
    min-width: 0;
  }
  .value {
    font-weight: 500;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .value.none {
    color: var(--color-text-faint);
    font-weight: 400;
  }
  .detail {
    grid-column: 2 / 4;
    font-size: var(--font-size--1);
    color: var(--color-text-muted);
  }
  @media (max-width: 600px) {
    .lead {
      font-size: var(--font-size-1);
    }
    .big {
      font-size: var(--font-size-3);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .bars span {
      transition: none;
    }
  }
</style>
