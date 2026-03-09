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
  // Signature state
  type SignatureItem = { id: string; account_id: string; name: string; body_text: string; body_html: string; is_default: boolean; created_at: number };
  let signatures = $state<SignatureItem[]>([]);
  let selectedSignatureId = $state<string | null>(null);
  let showSignatureMenu = $state(false);

  let aiAssisting = $state(false);
  let showAiMenu = $state(false);

  // Undo send state
  let undoSendId = $state<string | null>(null);
  let undoCountdown = $state(0);
  let undoTimer: ReturnType<typeof setInterval> | null = null;
  let undoCancelling = $state(false);

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

  const SIGNATURE_SEPARATOR = '\n\n--\n';

  function getSignatureBlock(sig: SignatureItem | null | undefined): string {
    if (!sig || !sig.body_text.trim()) return '';
    return SIGNATURE_SEPARATOR + sig.body_text;
  }

  function stripSignature(text: string): string {
    const sepIdx = text.lastIndexOf(SIGNATURE_SEPARATOR);
    if (sepIdx === -1) return text;
    return text.substring(0, sepIdx);
  }

  function switchSignature(sigId: string | null) {
    const cleaned = stripSignature(body);
    selectedSignatureId = sigId;
    if (sigId) {
      const sig = signatures.find(s => s.id === sigId);
      body = cleaned + getSignatureBlock(sig);
    } else {
      body = cleaned;
    }
    showSignatureMenu = false;
  }

  async function loadSignatures() {
    try {
      signatures = await api.signatures.list(context.accountId);
      const defaultSig = signatures.find(s => s.is_default);
      if (defaultSig) {
        selectedSignatureId = defaultSig.id;
        body = body + getSignatureBlock(defaultSig);
      }
    } catch {
      // Signatures not available — continue without
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

  function clearUndoTimer() {
    if (undoTimer) {
      clearInterval(undoTimer);
      undoTimer = null;
    }
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
      const result = await api.send(req);

      // Clean up draft if we had one
      if (draftId) {
        await api.drafts.delete(draftId).catch(() => {});
      }

      if (result.can_undo) {
        // Start undo countdown
        const now = Math.floor(Date.now() / 1000);
        undoCountdown = Math.max(1, result.send_at - now);
        undoSendId = result.id;

        undoTimer = setInterval(() => {
          undoCountdown--;
          if (undoCountdown <= 0) {
            clearUndoTimer();
            undoSendId = null;
            onsent?.();
            onclose();
          }
        }, 1000);
      } else {
        onsent?.();
        onclose();
      }
    } catch (e: any) {
      const msg = e.message || 'Failed to send';
      error = msg.includes('502') ? 'Send failed -- check account connection in Settings.' : msg;
    } finally {
      sending = false;
    }
  }

  async function handleUndoSend() {
    if (!undoSendId || undoCancelling) return;
    undoCancelling = true;
    try {
      const result = await api.cancelSend(undoSendId);
      clearUndoTimer();
      if (result.cancelled) {
        // Successfully cancelled — keep compose modal open with same content
        undoSendId = null;
        undoCountdown = 0;
        error = '';
        // The compose modal stays open with the same content
      } else {
        // Already sent
        undoSendId = null;
        onsent?.();
        onclose();
      }
    } catch {
      // Cancel failed, message may already be sent
      clearUndoTimer();
      undoSendId = null;
      onsent?.();
      onclose();
    } finally {
      undoCancelling = false;
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
      if (undoSendId) {
        // Don't close during undo countdown, dismiss toast instead
        clearUndoTimer();
        undoSendId = null;
        onsent?.();
        onclose();
      } else {
        onclose();
      }
    }
  }

  // Initialize fields on mount
  $effect(() => {
    initFields();
    loadSignatures();
    return () => {
      if (saveTimeout) clearTimeout(saveTimeout);
      clearUndoTimer();
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
          disabled={!!undoSendId}
        />
        {#if !showCcBcc}
          <button
            class="text-xs"
            style="color: var(--iris-color-primary);"
            onclick={() => (showCcBcc = true)}
            disabled={!!undoSendId}
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
            disabled={!!undoSendId}
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
            disabled={!!undoSendId}
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
          disabled={!!undoSendId}
        />
      </div>

      <!-- Body -->
      <textarea
        bind:value={body}
        oninput={scheduleAutoSave}
        class="w-full min-h-[200px] text-sm bg-transparent outline-none resize-y mt-2 leading-relaxed"
        style="color: var(--iris-color-text);"
        placeholder="Write your message..."
        disabled={!!undoSendId}
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
        disabled={sending || !!undoSendId}
      >
        Save Draft
      </button>
      <!-- Signature dropdown -->
      {#if signatures.length > 0}
        <div class="relative">
          <button
            class="px-3 py-1.5 text-sm transition-colors compose-secondary-btn"
            style="color: var(--iris-color-text-muted);"
            onclick={() => (showSignatureMenu = !showSignatureMenu)}
            disabled={sending}
            title="Signature"
          >
            Sig{selectedSignatureId ? ': ' + (signatures.find(s => s.id === selectedSignatureId)?.name || '') : ''}
          </button>
          {#if showSignatureMenu}
            <div class="absolute bottom-full left-0 mb-1 rounded-lg shadow-lg py-1 min-w-[160px] z-10" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);">
              <button
                class="w-full text-left px-3 py-1.5 text-sm compose-dropdown-item"
                style="color: {selectedSignatureId === null ? 'var(--iris-color-primary)' : 'var(--iris-color-text-muted)'};"
                onclick={() => switchSignature(null)}
              >None</button>
              {#each signatures as sig}
                <button
                  class="w-full text-left px-3 py-1.5 text-sm compose-dropdown-item flex items-center gap-2"
                  style="color: {selectedSignatureId === sig.id ? 'var(--iris-color-primary)' : 'var(--iris-color-text-muted)'};"
                  onclick={() => switchSignature(sig.id)}
                >
                  {sig.name}
                  {#if sig.is_default}
                    <span class="px-1.5 py-0.5 text-[10px] rounded-full" style="background: color-mix(in srgb, var(--iris-color-primary) 20%, transparent); color: var(--iris-color-primary);">default</span>
                  {/if}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
      <!-- AI Assist dropdown -->
      <div class="relative">
        <button
          class="px-3 py-1.5 text-sm transition-colors disabled:opacity-50 compose-secondary-btn"
          style="color: var(--iris-color-text-muted);"
          onclick={() => (showAiMenu = !showAiMenu)}
          disabled={aiAssisting || sending || !body.trim() || !!undoSendId}
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
        onclick={handleSend}
        disabled={sending || !!undoSendId}
      >
        {sending ? 'Sending...' : 'Send'}
      </button>
      <span class="text-xs" style="color: var(--iris-color-text-faint);" title="Cmd/Ctrl + Enter to send">
        {navigator.platform.includes('Mac') ? '\u2318' : 'Ctrl'}+&#x23CE;
      </span>
    </div>
  </div>

  <!-- Undo Send Toast -->
  {#if undoSendId}
    <div class="fixed bottom-6 left-1/2 -translate-x-1/2 z-[60] undo-toast" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);">
      <div class="flex items-center gap-3 px-4 py-3 rounded-xl shadow-2xl">
        <span class="text-sm" style="color: var(--iris-color-text);">
          Message sent.
        </span>
        <button
          class="px-3 py-1 text-sm font-medium rounded-lg transition-colors undo-btn"
          style="color: var(--iris-color-primary); background: color-mix(in srgb, var(--iris-color-primary) 12%, transparent);"
          onclick={handleUndoSend}
          disabled={undoCancelling}
        >
          {undoCancelling ? 'Cancelling...' : `Undo (${undoCountdown}s)`}
        </button>
      </div>
    </div>
  {/if}
</div>

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

  /* Undo toast */
  .undo-toast {
    border-radius: 12px;
    animation: slide-up 200ms ease;
  }

  @keyframes slide-up {
    from {
      opacity: 0;
      transform: translate(-50%, 16px);
    }
    to {
      opacity: 1;
      transform: translate(-50%, 0);
    }
  }

  .undo-btn {
    transition: all 120ms ease;
  }
  .undo-btn:hover:not(:disabled) {
    filter: brightness(1.15);
    background: color-mix(in srgb, var(--iris-color-primary) 20%, transparent);
  }
</style>
