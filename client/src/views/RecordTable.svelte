<!-- The records of one type as a table, with the fields of the type (spec 1 section 6.4). -->
<script lang="ts">
  import type { Item } from "../api";
  import { field, origin, RECORD_FIELDS } from "../format";
  import { href } from "../route";

  let { type, items }: { type: string; items: Item[] } = $props();
  let columns = $derived(RECORD_FIELDS[type] ?? RECORD_FIELDS.other);
</script>

<div class="scroll">
  <table>
    <caption>Records of the type {type}</caption>
    <thead>
      <tr>
        {#each columns as column (column)}<th scope="col">{column.replaceAll("_", " ")}</th>{/each}
        <th scope="col">Source</th>
      </tr>
    </thead>
    <tbody>
      {#each items as item (item.id)}
        <tr>
          {#each columns as column, i (column)}
            <td>
              {#if i === 0}<a class="touch" href={href.item(item.id)}>{field(item.fields?.[column]) || item.title}</a>{:else}{field(item.fields?.[column])}{/if}
            </td>
          {/each}
          <td><a class="touch" href={href.mail(origin(item))}>Mail</a></td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .scroll {
    overflow-x: auto;
  }
  table {
    border-collapse: collapse;
    width: 100%;
    font-size: var(--font-size--1);
  }
  caption {
    text-align: left;
    color: var(--color-text-muted);
    padding-bottom: var(--space-2);
  }
  th,
  td {
    text-align: left;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border-subtle);
    vertical-align: top;
  }
  th {
    color: var(--color-text-muted);
    font-weight: 500;
    text-transform: capitalize;
  }
</style>
