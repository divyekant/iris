<script lang="ts">
  import { api } from '../../lib/api';
  import DlpWarning from './DlpWarning.svelte';

  interface DlpScanResult {
    findings: { type: string; match: string; location: string; line: number }[];
    risk_level: 'none' | 'low' | 'high';
    allow_send: boolean;
  }

  type ComposeMode = 'new' | 'reply' | 'reply-all' | 'forward';

  interface ComposeContext {
    mode: ComposeMode;
    accountId: string;
    /** Original message for reply/forward context */
    original?: {
      message_id?: string;
      from_address?: string;
      from_name?: string;
      to_addresses?: string;
      cc_addresses?: string;
      subject?: string;
      body_text?: string;
      date?: number;
      /** References chain for threading */
      references?: string;
    };
  }

  let {
    context,
    onclose,
    onsent,
  }: {
    context: ComposeContext;
    onclose: () => void;
    onsent?: () => void;
  } = $props();

  let to = $state('');
  let cc = $state('');
  let bcc = $state('');
  let subject = $state('');
  let body = $state('');
  let showCcBcc = $state(false);
  let sending = $state(false);
  let error = $state('');
  let draftId = $state<string | null>(null);
  let saveTimeout: ReturnType<typeof setTimeout> | null = null;

  // DLP state
  let dlpResult = $state<DlpScanResult | null>(null);
  let scanning = $state(false);
  let dlpOverrideToken = $state<string | null>(null);

  // AI assist state
  let aiAssisting = $state(false);
  let showAiMenu = $state(false);

  const aiActions = [
    { action: 'rewrite', label: 'Improve writing' },
    { action: 'formal', label: 'Make formal' },
    { action: 'casual', label: 'Make casual' },
    { action: 'shorter', label: 'Make shorter' },
    { action: 'longer', label: 'Expand' },
  ];

  async function handleAiAssist(action: string) {
    if (!body.trim()) return;
    showAiMenu = false;
    aiAssisting = true;
    error = '';
    try {
      const res = await api.ai.assist({ action, content: body });
      body = res.result;
    } catch {
      error = 'AI assist failed. Check AI settings.';
    } finally {
      aiAssisting = false;
    }
  }

  // Pre-populate fields based on mode
  function initFields() {
    const orig = context.original;
    if (!orig) return;

    switch (context.mode) {
      case 'reply':
        to = orig.from_address || '';
        subject = orig.subject?.startsWith('Re:') ? orig.subject : `Re: ${orig.subject || ''}`;
        body = quoteOriginal(orig);
        break;
      case 'reply-all': {
        to = orig.from_address || '';
        // Add original To/Cc minus self
        const origTo = parseAddresses(orig.to_addresses);
        const origCc = parseAddresses(orig.cc_addresses);
        const allCc = [...origTo, ...origCc].filter(
          (addr) => addr.toLowerCase() !== to.toLowerCase()
        );
        if (allCc.length > 0) {
          cc = allCc.join(', ');
          showCcBcc = true;
        }
        subject = orig.subject?.startsWith('Re:') ? orig.subject : `Re: ${orig.subject || ''}`;
        body = quoteOriginal(orig);
        break;
      }
      case 'forward':
        to = '';
        subject = orig.subject?.startsWith('Fwd:') ? orig.subject : `Fwd: ${orig.subject || ''}`;
        body = forwardOriginal(orig);
        break;
    }
  }

  function parseAddresses(json: string | null | undefined): string[] {
    if (!json) return [];
    try {
      const parsed = JSON.parse(json);
      return Array.isArray(parsed) ? parsed : [json];
    } catch {
      return json.split(',').map((s) => s.trim()).filter(Boolean);
    }
  }

  function quoteOriginal(orig: NonNullable<ComposeContext['original']>): string {
    const date = orig.date ? new Date(orig.date * 1000).toLocaleString() : '';
    const from = orig.from_name ? `${orig.from_name} <${orig.from_address}>` : orig.from_address || '';
    const quoted = (orig.body_text || '')
      .split('\n')
      .map((line) => `> ${line}`)
      .join('\n');
    return `\n\nOn ${date}, ${from} wrote:\n${quoted}`;
  }

  function forwardOriginal(orig: NonNullable<ComposeContext['original']>): string {
    const date = orig.date ? new Date(orig.date * 1000).toLocaleString() : '';
    return `\n\n---------- Forwarded message ----------\nFrom: ${orig.from_name || ''} <${orig.from_address || ''}>\nDate: ${date}\nSubject: ${orig.subject || ''}\n\n${orig.body_text || ''}`;
  }

  function splitAddresses(input: string): string[] {
    return input
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);
  }

  async function handleSend(overrideToken?: string) {
    if (!to.trim()) {
      error = 'Please add at least one recipient.';
      return;
    }

    // Run DLP scan before sending (skip if we have an override token)
    if (!overrideToken && !dlpOverrideToken) {
      scanning = true;
      error = '';
      try {
        const scanResult = await api.dlp.scan({
          subject,
          body,
          to: splitAddresses(to),
        });
        if (scanResult.findings.length > 0) {
          dlpResult = scanResult;
          scanning = false;
          return; // Show DLP warning dialog
        }
      } catch {
        // If scan fails, allow send to proceed (fail-open)
      } finally {
        scanning = false;
      }
    }

    sending = true;
    error = '';
    try {
      const req: any = {
        account_id: context.accountId,
        to: splitAddresses(to),
        cc: cc ? splitAddresses(cc) : [],
        bcc: bcc ? splitAddresses(bcc) : [],
        subject,
        body_text: body,
      };
      // Add threading headers for replies
      if (context.mode === 'reply' || context.mode === 'reply-all') {
        if (context.original?.message_id) {
          req.in_reply_to = context.original.message_id;
          req.references = context.original.references
            ? `${context.original.references} ${context.original.message_id}`
            : context.original.message_id;
        }
      }
      await api.send(req);
      // Clean up draft if we had one
      if (draftId) {
        await api.drafts.delete(draftId);
      }
      onsent?.();
      onclose();
    } catch (e: any) {
      const msg = e.message || 'Failed to send';
      error = msg.includes('502') ? 'Send failed — check account connection in Settings.' : msg;
    } finally {
      sending = false;
    }
  }

  function handleDlpCancel() {
    dlpResult = null;
    dlpOverrideToken = null;
  }

  function handleDlpOverride(token: string) {
    dlpResult = null;
    dlpOverrideToken = token;
    handleSend(token);
  }

  async function handleSaveDraft() {
    try {
      const res = await api.drafts.save({
        account_id: context.accountId,
        draft_id: draftId,
        to: to ? splitAddresses(to) : undefined,
        cc: cc ? splitAddresses(cc) : undefined,
        bcc: bcc ? splitAddresses(bcc) : undefined,
        subject: subject || undefined,
        body_text: body,
      });
      draftId = res.draft_id;
    } catch {
      // Silent fail for auto-save
    }
  }

  function scheduleAutoSave() {
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(handleSaveDraft, 3000);
  }

  function handleKeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      handleSend();
    }
    if (e.key === 'Escape') {
      onclose();
    }
  }

  // Initialize fields on mount
  $effect(() => {
    initFields();
    return () => {
      if (saveTimeout) clearTimeout(saveTimeout);
    };
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_interactive_supports_focus -->
<div
  class="fixed inset-0 z-50 flex items-end sm:items-center justify-center"
  style="background: var(--iris-color-overlay);"
  role="dialog"
  aria-modal="true"
  onkeydown={handleKeydown}
>
  <div class="w-full sm:max-w-2xl sm:rounded-xl shadow-2xl flex flex-col max-h-[90vh]" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);">
    <!-- Header -->
    <div class="flex items-center justify-between px-4 py-3 border-b" style="border-color: var(--iris-color-border);">
      <h3 class="font-semibold text-sm" style="color: var(--iris-color-text);">
        {context.mode === 'new' ? 'New Message' : context.mode === 'reply' ? 'Reply' : context.mode === 'reply-all' ? 'Reply All' : 'Forward'}
      </h3>
      <button
        class="p-1"
        style="color: var(--iris-color-text-faint);"
        onclick={onclose}
        title="Close"
      >
        &times;
      </button>
    </div>

    <!-- Form -->
    <div class="flex-1 overflow-y-auto px-4 py-3 space-y-2">
      <!-- To -->
      <div class="flex items-center gap-2">
        <label class="text-xs w-8" style="color: var(--iris-color-text-faint);" for="compose-to">To</label>
        <input
          id="compose-to"
          type="text"
          bind:value={to}
          oninput={scheduleAutoSave}
          class="flex-1 text-sm bg-transparent border-b outline-none py-1"
          style="color: var(--iris-color-text); border-color: var(--iris-color-border);"
          placeholder="recipient@example.com"
        />
        {#if !showCcBcc}
          <button
            class="text-xs"
            style="color: var(--iris-color-primary);"
            onclick={() => (showCcBcc = true)}
          >
            Cc/Bcc
          </button>
        {/if}
      </div>

      <!-- Cc -->
      {#if showCcBcc}
        <div class="flex items-center gap-2">
          <label class="text-xs w-8" style="color: var(--iris-color-text-faint);" for="compose-cc">Cc</label>
          <input
            id="compose-cc"
            type="text"
            bind:value={cc}
            oninput={scheduleAutoSave}
            class="flex-1 text-sm bg-transparent border-b outline-none py-1"
            style="color: var(--iris-color-text); border-color: var(--iris-color-border);"
          />
        </div>

        <!-- Bcc -->
        <div class="flex items-center gap-2">
          <label class="text-xs w-8" style="color: var(--iris-color-text-faint);" for="compose-bcc">Bcc</label>
          <input
            id="compose-bcc"
            type="text"
            bind:value={bcc}
            oninput={scheduleAutoSave}
            class="flex-1 text-sm bg-transparent border-b outline-none py-1"
            style="color: var(--iris-color-text); border-color: var(--iris-color-border);"
          />
        </div>
      {/if}

      <!-- Subject -->
      <div class="flex items-center gap-2">
        <label class="text-xs w-8" style="color: var(--iris-color-text-faint);" for="compose-subject">Subj</label>
        <input
          id="compose-subject"
          type="text"
          bind:value={subject}
          oninput={scheduleAutoSave}
          class="flex-1 text-sm bg-transparent border-b outline-none py-1"
          style="color: var(--iris-color-text); border-color: var(--iris-color-border);"
          placeholder="Subject"
        />
      </div>

      <!-- Body -->
      <textarea
        bind:value={body}
        oninput={scheduleAutoSave}
        class="w-full min-h-[200px] text-sm bg-transparent outline-none resize-y mt-2 leading-relaxed"
        style="color: var(--iris-color-text);"
        placeholder="Write your message..."
      ></textarea>
    </div>

    <!-- Footer -->
    <div class="px-4 py-3 border-t flex items-center gap-2" style="border-color: var(--iris-color-border);">
      {#if error}
        <p class="text-xs flex-1" style="color: var(--iris-color-error);">{error}</p>
      {:else}
        <span class="flex-1"></span>
      {/if}
      <button
        class="px-3 py-1.5 text-sm compose-secondary-btn"
        style="color: var(--iris-color-text-muted);"
        onclick={handleSaveDraft}
        disabled={sending}
      >
        Save Draft
      </button>
      <!-- AI Assist dropdown -->
      <div class="relative">
        <button
          class="px-3 py-1.5 text-sm transition-colors disabled:opacity-50 compose-secondary-btn"
          style="color: var(--iris-color-text-muted);"
          onclick={() => (showAiMenu = !showAiMenu)}
          disabled={aiAssisting || sending || !body.trim()}
          title="AI Assist"
        >
          {aiAssisting ? 'Thinking...' : 'AI'}
        </button>
        {#if showAiMenu}
          <div class="absolute bottom-full left-0 mb-1 rounded-lg shadow-lg py-1 min-w-[160px] z-10" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);">
            {#each aiActions as { action, label }}
              <button
                class="w-full text-left px-3 py-1.5 text-sm compose-dropdown-item"
                style="color: var(--iris-color-text-muted);"
                onclick={() => handleAiAssist(action)}
              >{label}</button>
            {/each}
          </div>
        {/if}
      </div>
      <button
        class="px-4 py-1.5 text-sm rounded-lg font-medium disabled:opacity-50 transition-colors compose-send-btn"
        style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
        onclick={() => handleSend()}
        disabled={sending || scanning}
      >
        {scanning ? 'Scanning...' : sending ? 'Sending...' : 'Send'}
      </button>
      <span class="text-xs" style="color: var(--iris-color-text-faint);" title="Cmd/Ctrl + Enter to send">
        {navigator.platform.includes('Mac') ? '\u2318' : 'Ctrl'}+&#x23CE;
      </span>
    </div>
  </div>
</div>

{#if dlpResult}
  <DlpWarning
    result={dlpResult}
    oncancel={handleDlpCancel}
    onoverride={handleDlpOverride}
  />
{/if}

<style>
  .compose-dropdown-item:hover {
    background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent);
  }

  /* Send button */
  .compose-send-btn {
    transition: all 120ms ease;
  }
  .compose-send-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }
  .compose-send-btn:active:not(:disabled) {
    transform: scale(0.98);
  }

  /* Secondary buttons (Save Draft, AI) */
  .compose-secondary-btn {
    transition: all 120ms ease;
  }
  .compose-secondary-btn:hover:not(:disabled) {
    background: var(--iris-color-bg-surface);
    color: var(--iris-color-text);
  }
</style>
