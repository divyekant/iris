<script lang="ts">
  import { api } from '../../lib/api';

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

  async function handleSend() {
    if (!to.trim()) {
      error = 'Please add at least one recipient.';
      return;
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
      error = e.message || 'Failed to send';
    } finally {
      sending = false;
    }
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
  class="fixed inset-0 z-50 flex items-end sm:items-center justify-center bg-black/30"
  role="dialog"
  aria-modal="true"
  onkeydown={handleKeydown}
>
  <div class="bg-white dark:bg-gray-900 w-full sm:max-w-2xl sm:rounded-xl shadow-2xl flex flex-col max-h-[90vh]">
    <!-- Header -->
    <div class="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-gray-700">
      <h3 class="font-semibold text-sm">
        {context.mode === 'new' ? 'New Message' : context.mode === 'reply' ? 'Reply' : context.mode === 'reply-all' ? 'Reply All' : 'Forward'}
      </h3>
      <button
        class="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
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
        <label class="text-xs text-gray-500 w-8" for="compose-to">To</label>
        <input
          id="compose-to"
          type="text"
          bind:value={to}
          oninput={scheduleAutoSave}
          class="flex-1 text-sm bg-transparent border-b border-gray-200 dark:border-gray-700 focus:border-blue-500 outline-none py-1"
          placeholder="recipient@example.com"
        />
        {#if !showCcBcc}
          <button
            class="text-xs text-blue-500 hover:text-blue-600"
            onclick={() => (showCcBcc = true)}
          >
            Cc/Bcc
          </button>
        {/if}
      </div>

      <!-- Cc -->
      {#if showCcBcc}
        <div class="flex items-center gap-2">
          <label class="text-xs text-gray-500 w-8" for="compose-cc">Cc</label>
          <input
            id="compose-cc"
            type="text"
            bind:value={cc}
            oninput={scheduleAutoSave}
            class="flex-1 text-sm bg-transparent border-b border-gray-200 dark:border-gray-700 focus:border-blue-500 outline-none py-1"
          />
        </div>

        <!-- Bcc -->
        <div class="flex items-center gap-2">
          <label class="text-xs text-gray-500 w-8" for="compose-bcc">Bcc</label>
          <input
            id="compose-bcc"
            type="text"
            bind:value={bcc}
            oninput={scheduleAutoSave}
            class="flex-1 text-sm bg-transparent border-b border-gray-200 dark:border-gray-700 focus:border-blue-500 outline-none py-1"
          />
        </div>
      {/if}

      <!-- Subject -->
      <div class="flex items-center gap-2">
        <label class="text-xs text-gray-500 w-8" for="compose-subject">Subj</label>
        <input
          id="compose-subject"
          type="text"
          bind:value={subject}
          oninput={scheduleAutoSave}
          class="flex-1 text-sm bg-transparent border-b border-gray-200 dark:border-gray-700 focus:border-blue-500 outline-none py-1"
          placeholder="Subject"
        />
      </div>

      <!-- Body -->
      <textarea
        bind:value={body}
        oninput={scheduleAutoSave}
        class="w-full min-h-[200px] text-sm bg-transparent outline-none resize-y mt-2 leading-relaxed"
        placeholder="Write your message..."
      ></textarea>
    </div>

    <!-- Footer -->
    <div class="px-4 py-3 border-t border-gray-200 dark:border-gray-700 flex items-center gap-2">
      {#if error}
        <p class="text-xs text-red-500 flex-1">{error}</p>
      {:else}
        <span class="flex-1"></span>
      {/if}
      <button
        class="px-3 py-1.5 text-sm text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
        onclick={handleSaveDraft}
        disabled={sending}
      >
        Save Draft
      </button>
      <!-- AI Assist dropdown -->
      <div class="relative">
        <button
          class="px-3 py-1.5 text-sm text-gray-500 hover:text-blue-500 transition-colors disabled:opacity-50"
          onclick={() => (showAiMenu = !showAiMenu)}
          disabled={aiAssisting || sending || !body.trim()}
          title="AI Assist"
        >
          {aiAssisting ? 'Thinking...' : 'AI'}
        </button>
        {#if showAiMenu}
          <div class="absolute bottom-full left-0 mb-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg shadow-lg py-1 min-w-[160px] z-10">
            {#each aiActions as { action, label }}
              <button
                class="w-full text-left px-3 py-1.5 text-sm hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-700 dark:text-gray-300"
                onclick={() => handleAiAssist(action)}
              >{label}</button>
            {/each}
          </div>
        {/if}
      </div>
      <button
        class="px-4 py-1.5 text-sm bg-blue-600 hover:bg-blue-700 text-white rounded-lg font-medium disabled:opacity-50 transition-colors"
        onclick={handleSend}
        disabled={sending}
      >
        {sending ? 'Sending...' : 'Send'}
      </button>
      <span class="text-xs text-gray-400" title="Cmd/Ctrl + Enter to send">
        {navigator.platform.includes('Mac') ? '\u2318' : 'Ctrl'}+&#x23CE;
      </span>
    </div>
  </div>
</div>
