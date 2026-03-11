<script lang="ts">
  import { api } from '../lib/api';
  import { wsClient } from '../lib/ws';
  import { push } from 'svelte-spa-router';
  import { Star, Archive, MailOpen, Trash2, Sparkles, ArrowLeft, Send } from 'lucide-svelte';
  import MessageCard from '../components/thread/MessageCard.svelte';
  import NotesPanel from '../components/thread/NotesPanel.svelte';

  let { params }: { params: { id: string } } = $props();

  let thread = $state<any>(null);
  let loading = $state(true);
  let error = $state('');

  // Inline reply state
  let replyMode = $state<'reply' | 'reply-all' | 'forward' | null>(null);
  let replyTo = $state('');
  let replyCc = $state('');
  let replySubject = $state('');
  let replyBody = $state('');
  let sending = $state(false);
  let sendError = $state('');

  // AI summary state
  let aiSummary = $state<string | null>(null);
  let summaryLoading = $state(false);
  let summaryOpen = $state(false);
  let summaryError = $state('');

  async function loadThread() {
    loading = true;
    error = '';
    try {
      thread = await api.threads.get(params.id);
      for (const msg of thread.messages) {
        if (!msg.is_read) {
          await api.messages.markRead(msg.id);
          msg.is_read = true;
        }
      }
    } catch (e: any) {
      error = e.message || 'Failed to load thread';
    } finally {
      loading = false;
    }
  }

  function lastMessage() {
    return thread?.messages?.[thread.messages.length - 1];
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

  function startReply(mode: 'reply' | 'reply-all' | 'forward') {
    const msg = lastMessage();
    if (!msg) return;
    replyMode = mode;
    sendError = '';
    replyBody = '';

    switch (mode) {
      case 'reply':
        replyTo = msg.from_address || '';
        replyCc = '';
        replySubject = thread.subject?.startsWith('Re:') ? thread.subject : `Re: ${thread.subject || ''}`;
        break;
      case 'reply-all': {
        replyTo = msg.from_address || '';
        const origTo = parseAddresses(msg.to_addresses);
        const origCc = parseAddresses(msg.cc_addresses);
        const allCc = [...origTo, ...origCc].filter(
          (addr) => addr.toLowerCase() !== replyTo.toLowerCase()
        );
        replyCc = allCc.join(', ');
        replySubject = thread.subject?.startsWith('Re:') ? thread.subject : `Re: ${thread.subject || ''}`;
        break;
      }
      case 'forward':
        replyTo = '';
        replyCc = '';
        replySubject = thread.subject?.startsWith('Fwd:') ? thread.subject : `Fwd: ${thread.subject || ''}`;
        const date = msg.date ? new Date(msg.date * 1000).toLocaleString() : '';
        replyBody = `\n\n---------- Forwarded message ----------\nFrom: ${msg.from_name || ''} <${msg.from_address || ''}>\nDate: ${date}\nSubject: ${thread.subject || ''}\n\n${msg.body_text || ''}`;
        break;
    }

    // Focus the textarea after render
    setTimeout(() => {
      const el = document.getElementById('inline-reply-body');
      if (el) el.focus();
    }, 50);
  }

  function cancelReply() {
    replyMode = null;
    replyTo = '';
    replyCc = '';
    replySubject = '';
    replyBody = '';
    sendError = '';
  }

  async function handleSend() {
    if (!replyTo.trim() && replyMode !== 'forward') {
      sendError = 'Please add a recipient.';
      return;
    }
    if (replyMode === 'forward' && !replyTo.trim()) {
      sendError = 'Please add a recipient to forward to.';
      return;
    }
    const msg = lastMessage();
    if (!msg) return;

    sending = true;
    sendError = '';
    try {
      const req: any = {
        account_id: msg.account_id,
        to: replyTo.split(',').map((s: string) => s.trim()).filter(Boolean),
        cc: replyCc ? replyCc.split(',').map((s: string) => s.trim()).filter(Boolean) : [],
        bcc: [],
        subject: replySubject,
        body_text: replyBody,
      };
      if (replyMode === 'reply' || replyMode === 'reply-all') {
        if (msg.message_id || msg.id) {
          req.in_reply_to = msg.message_id || msg.id;
          req.references = msg.message_id || msg.id;
        }
      }
      await api.send(req);
      cancelReply();
      await loadThread();
    } catch (e: any) {
      const m = e.message || 'Failed to send';
      sendError = m.includes('502') ? 'Send failed — check account connection in Settings.' : m;
    } finally {
      sending = false;
    }
  }

  function handleReplyKeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      handleSend();
    }
    if (e.key === 'Escape') {
      cancelReply();
    }
  }

  async function handleThreadAction(action: string) {
    if (!thread) return;
    const ids = thread.messages.map((m: any) => m.id);
    try {
      await api.messages.batch(ids, action);
      if (action === 'archive' || action === 'delete') {
        push('/');
      } else {
        await loadThread();
      }
    } catch (e: any) {
      error = e.message || 'Action failed';
    }
  }

  async function toggleSummary() {
    if (summaryOpen) {
      summaryOpen = false;
      return;
    }
    summaryOpen = true;
    if (aiSummary) return;
    summaryLoading = true;
    summaryError = '';
    try {
      const res = await api.threads.summarize(params.id);
      aiSummary = res.summary;
    } catch (e: any) {
      if (e.message?.includes('503')) {
        summaryError = 'Enable AI in Settings to use this feature.';
      } else {
        summaryError = 'Failed to generate summary.';
      }
    } finally {
      summaryLoading = false;
    }
  }

  $effect(() => {
    if (params.id) {
      aiSummary = null;
      summaryOpen = false;
      summaryError = '';
      cancelReply();
      loadThread();
    }
    const off = wsClient.on('NewEmail', () => {
      if (thread) loadThread();
    });
    return () => off();
  });
</script>

<div class="h-full flex flex-col">
  <!-- Thread header -->
  <div class="px-4 py-3 flex items-center gap-3" style="border-bottom: 1px solid var(--iris-color-border);">
    <button
      class="p-1 transition-colors"
      style="color: var(--iris-color-text-muted);"
      onclick={() => push('/')}
      title="Back to inbox"
    >
      <ArrowLeft size={16} />
    </button>
    {#if thread}
      <div class="flex-1 min-w-0">
        <h2 class="text-base font-semibold truncate" style="color: var(--iris-color-text);">{thread.subject || '(no subject)'}</h2>
        <p class="text-xs truncate" style="color: var(--iris-color-text-faint);">
          {thread.participants.map((p: any) => p.name || p.email).join(', ')}
          &middot; {thread.message_count} message{thread.message_count === 1 ? '' : 's'}
        </p>
      </div>
      <div class="flex items-center gap-0.5">
        <button class="p-1.5 transition-colors thread-action-btn star-btn" onclick={() => handleThreadAction('star')} title="Star"><Star size={15} /></button>
        <button class="p-1.5 transition-colors thread-action-btn" onclick={() => handleThreadAction('archive')} title="Archive"><Archive size={15} /></button>
        <button class="p-1.5 transition-colors thread-action-btn" onclick={() => handleThreadAction('mark_unread')} title="Mark unread"><MailOpen size={15} /></button>
        <button class="p-1.5 transition-colors thread-action-btn delete-btn" onclick={() => handleThreadAction('delete')} title="Delete"><Trash2 size={15} /></button>
      </div>
    {/if}
  </div>

  <!-- AI Summary panel -->
  {#if thread && thread.message_count > 1}
    <div class="px-4 py-2" style="border-bottom: 1px solid var(--iris-color-border);">
      <button
        class="text-xs flex items-center gap-1"
        style="color: var(--iris-color-primary);"
        onclick={toggleSummary}
      >
        <span class="text-[10px]">{summaryOpen ? '\u25BE' : '\u25B8'}</span> <Sparkles size={14} /> AI Summary
      </button>
      {#if summaryOpen}
        {#if summaryLoading}
          <div class="mt-2 text-xs flex items-center gap-2" style="color: var(--iris-color-text-faint);">
            <div class="w-3 h-3 rounded-full animate-spin" style="border: 2px solid var(--iris-color-border); border-top-color: var(--iris-color-primary);"></div>
            Summarizing thread...
          </div>
        {:else if aiSummary}
          <div class="mt-2 text-sm rounded-lg px-3 py-2 leading-relaxed" style="color: var(--iris-color-text); background: var(--iris-color-bg-surface);">
            {aiSummary}
          </div>
        {:else if summaryError}
          <div class="mt-2 text-xs" style="color: var(--iris-color-text-faint);">{summaryError}</div>
        {/if}
      {/if}
    </div>
  {/if}

  <!-- Notes panel -->
  {#if thread && !loading}
    <NotesPanel threadId={params.id} />
  {/if}

  <!-- Messages -->
  <div class="flex-1 overflow-y-auto p-4 space-y-3">
    {#if loading}
      <div class="flex items-center justify-center py-16">
        <div class="w-8 h-8 rounded-full animate-spin" style="border: 4px solid var(--iris-color-border); border-top-color: var(--iris-color-primary);"></div>
      </div>
    {:else if error}
      <div class="text-center py-16">
        <p class="mb-4" style="color: var(--iris-color-error);">{error}</p>
        <button class="px-4 py-2 rounded-lg text-sm font-medium transition-colors retry-btn" onclick={loadThread}>Retry</button>
      </div>
    {:else if thread}
      {#each thread.messages as message (message.id)}
        <MessageCard {message} />
      {/each}

      <!-- Inline reply area -->
      {#if replyMode}
        <div class="rounded-xl p-4 space-y-3" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);">
          <div class="flex items-center gap-2">
            <span class="text-xs font-medium" style="color: var(--iris-color-text-faint);">
              {replyMode === 'reply' ? 'Reply' : replyMode === 'reply-all' ? 'Reply All' : 'Forward'}
            </span>
            <span class="flex-1"></span>
            <button class="text-xs px-2 py-0.5 rounded" style="color: var(--iris-color-text-faint);" onclick={cancelReply}>&times; Cancel</button>
          </div>

          <!-- To field -->
          <div class="flex items-center gap-2">
            <label class="text-xs w-6" style="color: var(--iris-color-text-faint);" for="reply-to">To</label>
            <input
              id="reply-to"
              type="text"
              bind:value={replyTo}
              class="flex-1 text-sm bg-transparent border-b outline-none py-1"
              style="color: var(--iris-color-text); border-color: var(--iris-color-border);"
              placeholder="recipient@example.com"
            />
          </div>

          <!-- Cc field (reply-all or if user adds) -->
          {#if replyMode === 'reply-all' || replyCc}
            <div class="flex items-center gap-2">
              <label class="text-xs w-6" style="color: var(--iris-color-text-faint);" for="reply-cc">Cc</label>
              <input
                id="reply-cc"
                type="text"
                bind:value={replyCc}
                class="flex-1 text-sm bg-transparent border-b outline-none py-1"
                style="color: var(--iris-color-text); border-color: var(--iris-color-border);"
              />
            </div>
          {/if}

          <!-- Body -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <textarea
            id="inline-reply-body"
            bind:value={replyBody}
            onkeydown={handleReplyKeydown}
            class="w-full min-h-[120px] text-sm bg-transparent outline-none resize-y leading-relaxed rounded-lg p-2"
            style="color: var(--iris-color-text); border: 1px solid var(--iris-color-border);"
            placeholder="Write your reply..."
          ></textarea>

          <!-- Send bar -->
          <div class="flex items-center gap-2">
            {#if sendError}
              <p class="text-xs flex-1" style="color: var(--iris-color-error);">{sendError}</p>
            {:else}
              <span class="flex-1"></span>
            {/if}
            <span class="text-[10px]" style="color: var(--iris-color-text-faint);">
              {navigator.platform.includes('Mac') ? '\u2318' : 'Ctrl'}+Enter to send
            </span>
            <button
              class="px-4 py-1.5 text-sm rounded-lg font-medium disabled:opacity-50 transition-colors flex items-center gap-1.5 reply-send-btn"
              onclick={handleSend}
              disabled={sending}
            >
              <Send size={13} />
              {sending ? 'Sending...' : 'Send'}
            </button>
          </div>
        </div>
      {/if}
    {/if}
  </div>

  <!-- Reply buttons footer (only when not already replying) -->
  {#if thread && !loading && !replyMode}
    <div class="px-4 py-3 flex gap-2" style="border-top: 1px solid var(--iris-color-border); background: var(--iris-color-bg-elevated);">
      <button class="px-4 py-2 text-sm rounded-lg font-medium transition-colors reply-primary-btn" onclick={() => startReply('reply')}>Reply</button>
      <button class="px-4 py-2 text-sm rounded-lg font-medium transition-colors reply-secondary-btn" onclick={() => startReply('reply-all')}>Reply All</button>
      <button class="px-4 py-2 text-sm rounded-lg font-medium transition-colors reply-secondary-btn" onclick={() => startReply('forward')}>Forward</button>
    </div>
  {/if}
</div>

<style>
  .thread-action-btn {
    color: var(--iris-color-text-faint);
  }
  .thread-action-btn:hover {
    color: var(--iris-color-text-muted);
  }
  .thread-action-btn.star-btn:hover {
    color: var(--iris-color-primary);
  }
  .thread-action-btn.delete-btn:hover {
    color: var(--iris-color-error);
  }
  .retry-btn {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .retry-btn:hover {
    filter: brightness(1.1);
  }
  .reply-primary-btn {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .reply-primary-btn:hover {
    filter: brightness(1.1);
  }
  .reply-secondary-btn {
    background: var(--iris-color-bg-surface);
    color: var(--iris-color-text-muted);
    border: 1px solid var(--iris-color-border);
  }
  .reply-secondary-btn:hover {
    background: var(--iris-color-bg-elevated);
    color: var(--iris-color-text);
  }
  .reply-send-btn {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .reply-send-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }
</style>
