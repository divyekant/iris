<!--
  The step that a new user misses (issue 740): with no model, only the fixed
  rules read the mail, and the brief stays empty. The setup page and the brief
  show this note while mail waits for a model.
-->
<div class="no-model" role="status">
  <p><strong>Iris needs a model to read the rest of your mail.</strong> Until you set one, the brief stays almost empty.</p>
  <p>At the terminal, run <code>iris model set local &lt;base-url&gt; &lt;model&gt;</code> for a model on this machine, or <code>iris model set anthropic &lt;model&gt; --price-in &lt;usd&gt; --price-out &lt;usd&gt;</code>. The README, "Understand the mail", gives each step.</p>
</div>

<style>
  .no-model {
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-4);
    margin: var(--space-6) 0;
  }
  .no-model p {
    margin: 0 0 var(--space-2);
  }
  .no-model p:last-child {
    margin: 0;
  }
  code {
    font-family: var(--font-family-mono);
    overflow-wrap: anywhere;
  }
</style>
