<script lang="ts">
  import { api } from '../../lib/api';
  import TemplatePicker from './TemplatePicker.svelte';
  import SchedulePicker from './SchedulePicker.svelte';
  import RichTextEditor from './RichTextEditor.svelte';
  import { Clock } from 'lucide-svelte';

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

  interface AttachedFile {
    file: File;
    filename: string;
    content_type: string;
    size: number;
    data_base64: string;
  }

  const MAX_TOTAL_SIZE = 25 * 1024 * 1024; // 25MB

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
  let bodyHtml = $state('');
  let richEditor: RichTextEditor | undefined = $state();
  let showCcBcc = $state(false);
  let sending = $state(false);
  let error = $state('');
  let draftId = $state<string | null>(null);
  let saveTimeout: ReturnType<typeof setTimeout> | null = null;

  // Attachments state
  let attachedFiles = $state<AttachedFile[]>([]);
  let fileInputEl: HTMLInputElement | undefined = $state();
  let dragOver = $state(false);

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

  // Template picker state
  let showTemplatePicker = $state(false);
  let pendingTemplate = $state<{ subject: string; body_text: string } | null>(null);
  let showOverwriteConfirm = $state(false);

  // Schedule send state
  let showSchedulePicker = $state(false);
  let scheduleConfirmation = $state('');

  function handleTemplatePick(template: { subject: string; body_text: string }) {
    if ((subject.trim() || body.trim()) && (template.subject || template.body_text)) {
      pendingTemplate = template;
      showOverwriteConfirm = true;
    } else {
      applyTemplate(template);
    }
  }

  function applyTemplate(template: { subject: string; body_text: string }) {
    if (template.subject) subject = template.subject;
    if (template.body_text) {
      body = template.body_text;
      richEditor?.setContent(`<p>${template.body_text.replace(/\n\n/g, '</p><p>').replace(/\n/g, '<br>')}</p>`);
    }
    pendingTemplate = null;
    showOverwriteConfirm = false;
  }

  function cancelOverwrite() {
    pendingTemplate = null;
    showOverwriteConfirm = false;
  }

  const aiActions = [
    { action: 'rewrite', label: 'Improve writing' },
    { action: 'formal', label: 'Make formal' },
    { action: 'casual', label: 'Make casual' },
    { action: 'shorter', label: 'Make shorter' },
    { action: 'longer', label: 'Expand' },
  ];

  let totalAttachmentSize = $derived(
    attachedFiles.reduce((sum, f) => sum + f.size, 0)
  );

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  async function handleAiAssist(action: string) {
    if (!body.trim()) return;
    showAiMenu = false;
    aiAssisting = true;
    error = '';
    try {
      const res = await api.ai.assist({ action, content: body });
      body = res.result;
      // Update rich editor with the AI result as plain text
      richEditor?.setContent(`<p>${res.result.replace(/\n\n/g, '</p><p>').replace(/\n/g, '<br>')}</p>`);
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
    // Sync to rich editor
    richEditor?.setContent(`<p>${body.replace(/\n\n/g, '</p><p>').replace(/\n/g, '<br>')}</p>`);
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

  // File attachment helpers
  function fileToBase64(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => {
        const result = reader.result as string;
        const base64 = result.split(',')[1] || '';
        resolve(base64);
      };
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
  }

  async function addFiles(files: FileList | File[]) {
    error = '';
    for (const file of files) {
      if (totalAttachmentSize + file.size > MAX_TOTAL_SIZE) {
        error = `Total attachment size exceeds 25 MB limit.`;
        return;
      }
      if (attachedFiles.some((a) => a.filename === file.name && a.size === file.size)) {
        continue;
      }
      const data_base64 = await fileToBase64(file);
      attachedFiles = [...attachedFiles, {
        file,
        filename: file.name,
        content_type: file.type || 'application/octet-stream',
        size: file.size,
        data_base64,
      }];
    }
  }

  function removeAttachment(index: number) {
    attachedFiles = attachedFiles.filter((_, i) => i !== index);
  }

  function handleFileInput(e: Event) {
    const input = e.target as HTMLInputElement;
    if (input.files && input.files.length > 0) {
      addFiles(input.files);
      input.value = '';
    }
  }

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    dragOver = true;
  }

  function handleDragLeave() {
    dragOver = false;
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    dragOver = false;
    if (e.dataTransfer?.files && e.dataTransfer.files.length > 0) {
      addFiles(e.dataTransfer.files);
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
        body_html: bodyHtml || undefined,
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
      // Add attachments
      if (attachedFiles.length > 0) {
        req.attachments = attachedFiles.map((a) => ({
          filename: a.filename,
          content_type: a.content_type,
          data_base64: a.data_base64,
        }));
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

  async function handleScheduleSend(epochSeconds: number) {
    if (!to.trim()) {
      error = 'Please add at least one recipient.';
      return;
    }
    showSchedulePicker = false;
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
        body_html: bodyHtml || undefined,
        schedule_at: epochSeconds,
      };
      if (context.mode === 'reply' || context.mode === 'reply-all') {
        if (context.original?.message_id) {
          req.in_reply_to = context.original.message_id;
          req.references = context.original.references
            ? `${context.original.references} ${context.original.message_id}`
            : context.original.message_id;
        }
      }
      // Add attachments if present
      if (attachedFiles.length > 0) {
        req.attachments = attachedFiles.map((a) => ({
          filename: a.filename,
          content_type: a.content_type,
          data_base64: a.data_base64,
        }));
      }
      await api.send(req);
      if (draftId) {
        await api.drafts.delete(draftId);
      }
      const scheduledDate = new Date(epochSeconds * 1000);
      const formatted = scheduledDate.toLocaleDateString(undefined, {
        weekday: 'short', month: 'short', day: 'numeric',
      }) + ' at ' + scheduledDate.toLocaleTimeString(undefined, {
        hour: 'numeric', minute: '2-digit',
      });
      scheduleConfirmation = `Email scheduled for ${formatted}`;
      // Show confirmation briefly then close
      setTimeout(() => {
        onsent?.();
        onclose();
      }, 1500);
    } catch (e: any) {
      const msg = e.message || 'Failed to schedule';
      error = msg.includes('502') ? 'Schedule failed -- check account connection in Settings.' : msg;
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
        body_html: bodyHtml || undefined,
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
    // Sync initial body to rich editor after a tick (editor needs to mount first)
    if (body) {
      setTimeout(() => {
        richEditor?.setContent(`<p>${body.replace(/\n\n/g, '</p><p>').replace(/\n/g, '<br>')}</p>`);
      }, 50);
    }
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
    <div
      class="flex-1 overflow-y-auto px-4 py-3 space-y-2"
      class:drag-over={dragOver}
      ondragover={handleDragOver}
      ondragleave={handleDragLeave}
      ondrop={handleDrop}
    >
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

      <!-- Body (Rich Text Editor) -->
      <div class="mt-2 rounded-lg border overflow-hidden" style="border-color: var(--iris-color-border-subtle);">
        <RichTextEditor
          bind:this={richEditor}
          content={body ? `<p>${body.replace(/\n\n/g, '</p><p>').replace(/\n/g, '<br>')}</p>` : ''}
          disabled={!!undoSendId}
          onchange={(html, text) => { bodyHtml = html; body = text; scheduleAutoSave(); }}
        />
      </div>

      <!-- Attached files chips -->
      {#if attachedFiles.length > 0}
        <div class="flex flex-wrap gap-2 pt-2" style="border-top: 1px solid var(--iris-color-border-subtle);">
          {#each attachedFiles as att, i}
            <div class="flex items-center gap-1 px-2 py-1 rounded text-xs compose-attachment-chip">
              <span>{'\u{1F4CE}'}</span>
              <span class="truncate max-w-[160px]">{att.filename}</span>
              <span style="color: var(--iris-color-text-faint);">({formatSize(att.size)})</span>
              <button
                class="ml-1 hover:opacity-100 opacity-60"
                style="color: var(--iris-color-error);"
                onclick={() => removeAttachment(i)}
                title="Remove"
              >&times;</button>
            </div>
          {/each}
          <div class="text-xs self-center" style="color: var(--iris-color-text-faint);">
            {formatSize(totalAttachmentSize)} / 25 MB
          </div>
        </div>
      {/if}

      <!-- Hidden file input -->
      <input
        bind:this={fileInputEl}
        type="file"
        multiple
        class="hidden"
        onchange={handleFileInput}
      />
    </div>

    <!-- Footer -->
    <div class="px-4 py-3 border-t flex items-center gap-2" style="border-color: var(--iris-color-border);">
      {#if scheduleConfirmation}
        <p class="text-xs flex-1 font-medium" style="color: var(--iris-color-success);">{scheduleConfirmation}</p>
      {:else if error}
        <p class="text-xs flex-1" style="color: var(--iris-color-error);">{error}</p>
      {:else}
        <span class="flex-1"></span>
      {/if}
      <!-- Attach button -->
      <button
        class="px-3 py-1.5 text-sm compose-secondary-btn"
        style="color: var(--iris-color-text-muted);"
        onclick={() => fileInputEl?.click()}
        disabled={sending}
        title="Attach files"
      >
        {'\u{1F4CE}'} Attach
      </button>
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
      <!-- Template picker -->
      <div class="relative">
        <button
          class="px-3 py-1.5 text-sm transition-colors compose-secondary-btn flex items-center gap-1"
          style="color: var(--iris-color-text-muted);"
          onclick={() => (showTemplatePicker = !showTemplatePicker)}
          disabled={sending}
          title="Insert template"
        >
          <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"></path>
            <path d="M14 2v4a2 2 0 0 0 2 2h4"></path>
            <path d="M10 13H8"></path>
            <path d="M16 13h-2"></path>
            <path d="M10 17H8"></path>
            <path d="M16 17h-2"></path>
          </svg>
          Templates
        </button>
        {#if showTemplatePicker}
          <TemplatePicker
            onpick={handleTemplatePick}
            onclose={() => (showTemplatePicker = false)}
          />
        {/if}
      </div>
      <!-- Overwrite confirmation -->
      {#if showOverwriteConfirm && pendingTemplate}
        <div class="absolute bottom-full left-0 mb-2 p-3 rounded-lg shadow-lg z-20" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border); min-width: 240px;">
          <p class="text-xs mb-2" style="color: var(--iris-color-text);">Replace current subject and body with template?</p>
          <div class="flex gap-2">
            <button
              class="px-3 py-1 text-xs rounded-lg font-medium"
              style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
              onclick={() => pendingTemplate && applyTemplate(pendingTemplate)}
            >Replace</button>
            <button
              class="px-3 py-1 text-xs rounded-lg border"
              style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
              onclick={cancelOverwrite}
            >Cancel</button>
          </div>
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
      <!-- Schedule Send -->
      <div class="relative">
        <button
          class="px-3 py-1.5 text-sm transition-colors disabled:opacity-50 compose-secondary-btn flex items-center gap-1"
          style="color: var(--iris-color-text-muted);"
          onclick={() => { showSchedulePicker = !showSchedulePicker; showAiMenu = false; }}
          disabled={sending}
          title="Schedule Send"
        >
          <Clock size={14} />
          Schedule
        </button>
        {#if showSchedulePicker}
          <SchedulePicker
            onpick={handleScheduleSend}
            onclose={() => (showSchedulePicker = false)}
          />
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

  /* Secondary buttons (Save Draft, AI, Attach) */
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

  /* Drag-over state */
  .drag-over {
    outline: 2px dashed var(--iris-color-primary);
    outline-offset: -4px;
    background: color-mix(in srgb, var(--iris-color-primary) 5%, transparent);
  }

  /* Attachment chips in compose */
  .compose-attachment-chip {
    background: var(--iris-color-bg-surface);
    color: var(--iris-color-text-muted);
    border: 1px solid var(--iris-color-border-subtle);
  }
</style>
