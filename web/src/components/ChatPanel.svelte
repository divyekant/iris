<script lang="ts">
  import { api } from '../lib/api';
  import ComposeCard from './chat/ComposeCard.svelte';

  let { open, onclose }: { open: boolean; onclose: () => void } = $props();

  let sessionId = $state(crypto.randomUUID());
  let messages = $state<any[]>([]);
  let input = $state('');
  let loading = $state(false);
  let briefingLoading = $state(false);
  let error = $state('');
  let messagesContainer: HTMLDivElement | undefined = $state();

  let chatWidth = $state(parseInt(localStorage.getItem('iris-chat-width') || '320'));
  let resizing = $state(false);

  function startResize(e: MouseEvent) {
    e.preventDefault();
    resizing = true;
    const startX = e.clientX;
    const startWidth = chatWidth;

    function onMouseMove(e: MouseEvent) {
      const delta = startX - e.clientX; // drag left = wider
      chatWidth = Math.min(600, Math.max(280, startWidth + delta));
    }

    function onMouseUp() {
      resizing = false;
      localStorage.setItem('iris-chat-width', String(chatWidth));
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    }

    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
  }

  const suggestions = [
    { label: 'Action items', prompt: 'What action items do I have from recent emails?' },
    { label: 'Unread summary', prompt: 'Summarize my unread emails from today' },
  ];

  function scrollToBottom() {
    if (messagesContainer) {
      setTimeout(() => {
        messagesContainer!.scrollTop = messagesContainer!.scrollHeight;
      }, 50);
    }
  }

  function reasonLabel(reason: string): string {
    switch (reason) {
      case 'urgent': return 'Urgent';
      case 'needs_reply': return 'Needs reply';
      case 'high_priority': return 'High priority';
      default: return reason;
    }
  }

  function reasonColor(reason: string): string {
    switch (reason) {
      case 'urgent': return 'var(--iris-color-error)';
      case 'needs_reply': return 'var(--iris-color-warning)';
      case 'high_priority': return 'var(--iris-color-warning)';
      default: return 'var(--iris-color-text-muted)';
    }
  }

  async function fetchBriefing() {
    if (briefingLoading || loading) return;
    error = '';
    briefingLoading = true;

    try {
      const res = await api.ai.briefing();
      // Add a briefing message to the chat as a special type
      messages = [...messages, {
        role: 'briefing',
        content: res.summary,
        stats: res.stats,
        highlights: res.highlights,
        id: crypto.randomUUID(),
      }];
      scrollToBottom();
    } catch (e: any) {
      if (e.message?.includes('503')) {
        error = 'Enable AI in Settings to use briefing.';
      } else {
        error = 'Failed to generate briefing. Try again.';
      }
    } finally {
      briefingLoading = false;
    }
  }

  async function sendMessage(text?: string) {
    const msg = text || input.trim();
    if (!msg || loading) return;
    input = '';
    error = '';

    // Add user message to UI immediately
    messages = [...messages, { role: 'user', content: msg, id: crypto.randomUUID() }];
    scrollToBottom();

    loading = true;
    try {
      const res = await api.ai.chat({ session_id: sessionId, message: msg });
      messages = [...messages, res.message];
      scrollToBottom();
    } catch (e: any) {
      if (e.message?.includes('503')) {
        error = 'Enable AI in Settings to use chat.';
      } else {
        error = 'Failed to get response. Try again.';
      }
    } finally {
      loading = false;
    }
  }

  async function confirmAction(messageId: string) {
    try {
      const res: any = await api.ai.chatConfirm({ session_id: sessionId, message_id: messageId });
      if (res.executed) {
        if (res.draft_id) {
          // Compose action confirmed — open compose modal with the draft
          messages = [...messages, {
            role: 'assistant',
            content: 'Draft saved! Opening compose window...',
            id: crypto.randomUUID(),
          }];
          scrollToBottom();
          // Dispatch event to open compose modal (handled by Inbox page)
          window.dispatchEvent(new CustomEvent('open-compose', {
            detail: { mode: 'new', accountId: null, draftId: res.draft_id }
          }));
        } else {
          messages = [...messages, {
            role: 'assistant',
            content: `Done! Updated ${res.updated} email${res.updated === 1 ? '' : 's'}.`,
            id: crypto.randomUUID(),
          }];
          scrollToBottom();
        }
      }
    } catch {
      error = 'Failed to execute action.';
    }
  }

  function handleComposeEdit(data: any) {
    // Dispatch open-compose event with pre-filled data from the AI draft
    window.dispatchEvent(new CustomEvent('open-compose', {
      detail: {
        mode: data.reply_to_message_id ? 'reply' : 'new',
        accountId: null,
        prefill: {
          to: data.to?.join(', ') || '',
          cc: data.cc?.join(', ') || '',
          subject: data.subject || '',
          body: data.body?.replace(/<[^>]*>/g, '') || '',
        }
      }
    }));
  }

  function handleComposeDiscard(messageId: string) {
    // Remove the compose proposal from the message
    messages = messages.map(m => {
      if (m.id === messageId && m.proposed_action?.action === 'compose_email') {
        return { ...m, proposed_action: null };
      }
      return m;
    });
  }

  function newSession() {
    sessionId = crypto.randomUUID();
    messages = [];
    error = '';
    input = '';
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  }
</script>

{#if open}
  <div class="border-l flex flex-col h-full relative" style="width: {chatWidth}px; background: var(--iris-color-bg-elevated); border-color: var(--iris-color-border);">
    <!-- Resize handle -->
    <div
      class="absolute left-0 top-0 bottom-0 w-1 cursor-col-resize hover:bg-[var(--iris-color-primary)] transition-colors z-10"
      style="background: {resizing ? 'var(--iris-color-primary)' : 'transparent'};"
      onmousedown={startResize}
    ></div>

    <!-- Header -->
    <div class="px-4 py-3 border-b flex items-center justify-between" style="border-color: var(--iris-color-border);">
      <div class="flex items-center gap-1.5">
        <svg class="w-4 h-4" style="color: var(--iris-color-primary);" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" d="M9.813 15.904 9 18.75l-.813-2.846a4.5 4.5 0 0 0-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 0 0 3.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 0 0 3.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 0 0-3.09 3.09ZM18.259 8.715 18 9.75l-.259-1.035a3.375 3.375 0 0 0-2.455-2.456L14.25 6l1.036-.259a3.375 3.375 0 0 0 2.455-2.456L18 2.25l.259 1.035a3.375 3.375 0 0 0 2.455 2.456L21.75 6l-1.036.259a3.375 3.375 0 0 0-2.455 2.456ZM16.894 20.567 16.5 21.75l-.394-1.183a2.25 2.25 0 0 0-1.423-1.423L13.5 18.75l1.183-.394a2.25 2.25 0 0 0 1.423-1.423l.394-1.183.394 1.183a2.25 2.25 0 0 0 1.423 1.423l1.183.394-1.183.394a2.25 2.25 0 0 0-1.423 1.423Z" />
        </svg>
        <h3 class="text-sm font-semibold" style="color: var(--iris-color-text);">AI Chat</h3>
      </div>
      <div class="flex items-center gap-1">
        <button
          class="p-1 text-xs hover:opacity-80 transition-opacity"
          style="color: var(--iris-color-text-faint);"
          onclick={newSession}
          title="New conversation"
        >New</button>
        <button
          class="p-1 hover:opacity-80 transition-opacity"
          style="color: var(--iris-color-text-faint);"
          onclick={onclose}
          title="Close chat"
        >&times;</button>
      </div>
    </div>

    <!-- Messages -->
    <div class="flex-1 overflow-y-auto p-3 space-y-3" bind:this={messagesContainer}>
      {#if messages.length === 0 && !loading && !briefingLoading}
        <div class="text-center py-6">
          <!-- Brief me button -->
          <button
            class="w-full flex items-center justify-center gap-2 px-4 py-3 text-sm font-medium transition-all hover:opacity-90 mb-4"
            style="background: color-mix(in srgb, var(--iris-color-primary) 12%, transparent); color: var(--iris-color-primary); border: 1px solid color-mix(in srgb, var(--iris-color-primary) 25%, transparent); border-radius: 12px;"
            onclick={fetchBriefing}
            disabled={briefingLoading}
          >
            <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 7.5h1.5m-1.5 3h1.5m-7.5 3h7.5m-7.5 3h7.5m3-9h3.375c.621 0 1.125.504 1.125 1.125V18a2.25 2.25 0 0 1-2.25 2.25M16.5 7.5V18a2.25 2.25 0 0 0 2.25 2.25M16.5 7.5V4.875c0-.621-.504-1.125-1.125-1.125H4.125C3.504 3.75 3 4.254 3 4.875V18a2.25 2.25 0 0 0 2.25 2.25h13.5M6 7.5h3v3H6V7.5Z" />
            </svg>
            Brief me on today
          </button>

          <p class="text-sm mb-3" style="color: var(--iris-color-text-muted);">Or ask anything about your inbox</p>
          <div class="space-y-2">
            {#each suggestions as { label, prompt }}
              <button
                class="w-full text-left px-3 py-2 text-xs transition-colors hover:opacity-80"
                style="border: 1px solid var(--iris-color-border); color: var(--iris-color-text-muted); border-radius: 9999px;"
                onclick={() => sendMessage(prompt)}
              >
                {label}
              </button>
            {/each}
          </div>
        </div>
      {/if}

      {#each messages as msg (msg.id)}
        {#if msg.role === 'briefing'}
          <!-- Briefing card -->
          <div class="rounded-xl overflow-hidden" style="border: 1px solid var(--iris-color-border); background: var(--iris-color-bg-surface);">
            <!-- Briefing header -->
            <div class="px-3 py-2 flex items-center gap-2" style="background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent); border-bottom: 1px solid var(--iris-color-border);">
              <svg class="w-4 h-4" style="color: var(--iris-color-primary);" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" d="M12 7.5h1.5m-1.5 3h1.5m-7.5 3h7.5m-7.5 3h7.5m3-9h3.375c.621 0 1.125.504 1.125 1.125V18a2.25 2.25 0 0 1-2.25 2.25M16.5 7.5V18a2.25 2.25 0 0 0 2.25 2.25M16.5 7.5V4.875c0-.621-.504-1.125-1.125-1.125H4.125C3.504 3.75 3 4.254 3 4.875V18a2.25 2.25 0 0 0 2.25 2.25h13.5M6 7.5h3v3H6V7.5Z" />
              </svg>
              <span class="text-xs font-semibold" style="color: var(--iris-color-primary);">Daily Briefing</span>
            </div>

            <!-- Stats grid -->
            <div class="grid grid-cols-2 gap-px" style="background: var(--iris-color-border);">
              <div class="px-3 py-2" style="background: var(--iris-color-bg-surface);">
                <div class="text-lg font-bold" style="color: var(--iris-color-text);">{msg.stats.total_today}</div>
                <div class="text-[10px]" style="color: var(--iris-color-text-muted);">Total today</div>
              </div>
              <div class="px-3 py-2" style="background: var(--iris-color-bg-surface);">
                <div class="text-lg font-bold" style="color: var(--iris-color-primary);">{msg.stats.unread}</div>
                <div class="text-[10px]" style="color: var(--iris-color-text-muted);">Unread</div>
              </div>
              <div class="px-3 py-2" style="background: var(--iris-color-bg-surface);">
                <div class="text-lg font-bold" style="color: var(--iris-color-error);">{msg.stats.urgent}</div>
                <div class="text-[10px]" style="color: var(--iris-color-text-muted);">Urgent</div>
              </div>
              <div class="px-3 py-2" style="background: var(--iris-color-bg-surface);">
                <div class="text-lg font-bold" style="color: var(--iris-color-warning);">{msg.stats.needs_reply}</div>
                <div class="text-[10px]" style="color: var(--iris-color-text-muted);">Needs reply</div>
              </div>
            </div>

            <!-- AI Summary -->
            <div class="px-3 py-2.5 text-xs leading-relaxed" style="color: var(--iris-color-text); border-top: 1px solid var(--iris-color-border);">
              {msg.content}
            </div>

            <!-- Highlights -->
            {#if msg.highlights?.length}
              <div class="px-3 pb-2.5" style="border-top: 1px solid var(--iris-color-border);">
                <div class="text-[10px] font-medium pt-2 pb-1" style="color: var(--iris-color-text-muted);">Highlights</div>
                {#each msg.highlights as highlight}
                  <div class="flex items-start gap-2 py-1.5" style="border-top: 1px solid color-mix(in srgb, var(--iris-color-border) 50%, transparent);">
                    <span class="shrink-0 text-[9px] font-semibold uppercase px-1.5 py-0.5 rounded" style="color: {reasonColor(highlight.reason)}; background: color-mix(in srgb, {reasonColor(highlight.reason)} 12%, transparent);">
                      {reasonLabel(highlight.reason)}
                    </span>
                    <div class="min-w-0 flex-1">
                      <div class="text-[11px] font-medium truncate" style="color: var(--iris-color-text);">{highlight.subject}</div>
                      <div class="text-[10px] truncate" style="color: var(--iris-color-text-muted);">{highlight.from}</div>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          <div class="flex {msg.role === 'user' ? 'justify-end' : 'justify-start'}">
            {#if msg.role === 'user'}
              <div class="max-w-[85%] rounded-2xl rounded-br-sm px-3 py-2 text-sm leading-relaxed" style="background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent); color: var(--iris-color-text); align-self: end;">
                {msg.content}

                <!-- Citations -->
                {#if msg.citations?.length}
                  <div class="mt-2 pt-2 border-t" style="border-color: var(--iris-color-border);">
                    <p class="text-[10px] font-medium mb-1" style="color: var(--iris-color-text-muted);">
                      <svg class="w-3 h-3 inline-block mr-0.5" style="color: var(--iris-color-text-muted);" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M21.75 6.75v10.5a2.25 2.25 0 0 1-2.25 2.25h-15a2.25 2.25 0 0 1-2.25-2.25V6.75m19.5 0A2.25 2.25 0 0 0 19.5 4.5h-15a2.25 2.25 0 0 0-2.25 2.25m19.5 0v.243a2.25 2.25 0 0 1-1.07 1.916l-7.5 4.615a2.25 2.25 0 0 1-2.36 0L3.32 8.91a2.25 2.25 0 0 1-1.07-1.916V6.75" /></svg>
                      References:
                    </p>
                    {#each msg.citations as citation}
                      <p class="text-[11px] truncate" style="color: var(--iris-color-text-muted);">
                        {citation.from}: {citation.subject}
                      </p>
                    {/each}
                  </div>
                {/if}

                <!-- Action proposal -->
                {#if msg.proposed_action}
                  <div class="mt-2 pt-2 border-t" style="border-color: var(--iris-color-border);">
                    <p class="text-xs font-medium mb-1" style="color: var(--iris-color-text);">{msg.proposed_action.description}</p>
                    <button
                      class="px-3 py-1 text-xs font-medium rounded-lg hover:opacity-90 transition-colors"
                      style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
                      onclick={() => confirmAction(msg.id)}
                    >
                      Confirm
                    </button>
                  </div>
                {/if}
              </div>
            {:else}
              <div class="max-w-[85%] rounded-2xl rounded-bl-sm px-3 py-2 text-sm leading-relaxed" style="background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                <!-- Tool activity -->
                {#if msg.tool_calls_made?.length}
                  <div class="flex flex-wrap gap-1 mb-2">
                    {#each msg.tool_calls_made as tc}
                      <span class="inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] rounded-md" style="background: color-mix(in srgb, var(--iris-color-primary) 12%, transparent); color: var(--iris-color-primary);">
                        {#if tc.name === 'search_emails'}
                          <svg class="w-2.5 h-2.5" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="m21 21-5.197-5.197m0 0A7.5 7.5 0 1 0 5.196 5.196a7.5 7.5 0 0 0 10.607 10.607Z"/></svg>
                          Searched: {tc.arguments?.query || '\u2026'}
                        {:else if tc.name === 'read_email'}
                          <svg class="w-2.5 h-2.5" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M21.75 6.75v10.5a2.25 2.25 0 0 1-2.25 2.25h-15a2.25 2.25 0 0 1-2.25-2.25V6.75m19.5 0A2.25 2.25 0 0 0 19.5 4.5h-15a2.25 2.25 0 0 0-2.25 2.25m19.5 0v.243a2.25 2.25 0 0 1-1.07 1.916l-7.5 4.615a2.25 2.25 0 0 1-2.36 0L3.32 8.91a2.25 2.25 0 0 1-1.07-1.916V6.75"/></svg>
                          Read email
                        {:else if tc.name === 'list_emails'}
                          <svg class="w-2.5 h-2.5" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M8.25 6.75h12M8.25 12h12m-12 5.25h12M3.75 6.75h.007v.008H3.75V6.75Zm.375 0a.375.375 0 1 1-.75 0 .375.375 0 0 1 .75 0ZM3.75 12h.007v.008H3.75V12Zm.375 0a.375.375 0 1 1-.75 0 .375.375 0 0 1 .75 0Zm-.375 5.25h.007v.008H3.75v-.008Zm.375 0a.375.375 0 1 1-.75 0 .375.375 0 0 1 .75 0Z"/></svg>
                          Listed emails
                        {:else if tc.name === 'inbox_stats'}
                          <svg class="w-2.5 h-2.5" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 0 1 3 19.875v-6.75ZM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 0 1-1.125-1.125V8.625ZM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25C20.496 3 21 3.504 21 4.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 0 1-1.125-1.125V4.125Z"/></svg>
                          Inbox stats
                        {:else}
                          {tc.name}
                        {/if}
                      </span>
                    {/each}
                  </div>
                {/if}

                {msg.content}

                <!-- Citations -->
                {#if msg.citations?.length}
                  <div class="mt-2 pt-2 border-t" style="border-color: var(--iris-color-border-subtle);">
                    <p class="text-[10px] font-medium mb-1" style="color: var(--iris-color-text-muted);">
                      <svg class="w-3 h-3 inline-block mr-0.5" style="color: var(--iris-color-text-muted);" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" d="M21.75 6.75v10.5a2.25 2.25 0 0 1-2.25 2.25h-15a2.25 2.25 0 0 1-2.25-2.25V6.75m19.5 0A2.25 2.25 0 0 0 19.5 4.5h-15a2.25 2.25 0 0 0-2.25 2.25m19.5 0v.243a2.25 2.25 0 0 1-1.07 1.916l-7.5 4.615a2.25 2.25 0 0 1-2.36 0L3.32 8.91a2.25 2.25 0 0 1-1.07-1.916V6.75" /></svg>
                      References:
                    </p>
                    {#each msg.citations as citation}
                      <p class="text-[11px] truncate" style="color: var(--iris-color-text-muted);">
                        {citation.from}: {citation.subject}
                      </p>
                    {/each}
                  </div>
                {/if}

                <!-- Compose email proposal (rendered as ComposeCard) -->
                {#if msg.proposed_action?.action === 'compose_email' && msg.proposed_action?.data}
                  <ComposeCard
                    data={msg.proposed_action.data}
                    onedit={(data) => handleComposeEdit(data)}
                    ondiscard={() => handleComposeDiscard(msg.id)}
                  />
                {:else if msg.proposed_action}
                  <!-- Generic action proposal -->
                  <div class="mt-2 pt-2 border-t" style="border-color: var(--iris-color-border-subtle);">
                    <p class="text-xs font-medium mb-1" style="color: var(--iris-color-text);">{msg.proposed_action.description}</p>
                    <button
                      class="px-3 py-1 text-xs font-medium rounded-lg hover:opacity-90 transition-colors"
                      style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
                      onclick={() => confirmAction(msg.id)}
                    >
                      Confirm
                    </button>
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      {/each}

      {#if loading || briefingLoading}
        <div class="flex justify-start">
          <div class="rounded-2xl rounded-bl-sm px-3 py-2 text-sm" style="background: var(--iris-color-bg-surface); color: var(--iris-color-text-faint);">
            <span class="inline-flex items-center gap-2">
              {#if briefingLoading}
                <span class="text-xs" style="color: var(--iris-color-text-muted);">Generating briefing</span>
              {:else}
                <span class="text-[11px]" style="color: var(--iris-color-text-muted);">Thinking…</span>
              {/if}
              <span class="inline-flex gap-1">
                <span class="w-1.5 h-1.5 rounded-full animate-bounce" style="background: var(--iris-color-primary); opacity: 0.6; animation-delay: 0ms"></span>
                <span class="w-1.5 h-1.5 rounded-full animate-bounce" style="background: var(--iris-color-primary); opacity: 0.6; animation-delay: 150ms"></span>
                <span class="w-1.5 h-1.5 rounded-full animate-bounce" style="background: var(--iris-color-primary); opacity: 0.6; animation-delay: 300ms"></span>
              </span>
            </span>
          </div>
        </div>
      {/if}

      {#if error}
        <div class="text-center">
          <p class="text-xs" style="color: var(--iris-color-error);">{error}</p>
        </div>
      {/if}
    </div>

    <!-- Brief me button (always visible when there are messages) -->
    {#if messages.length > 0}
      <div class="px-3 pt-2" style="border-top: 1px solid var(--iris-color-border);">
        <button
          class="w-full flex items-center justify-center gap-1.5 px-3 py-1.5 text-xs font-medium transition-all hover:opacity-90 disabled:opacity-50"
          style="color: var(--iris-color-primary); border: 1px solid color-mix(in srgb, var(--iris-color-primary) 25%, transparent); border-radius: 8px; background: color-mix(in srgb, var(--iris-color-primary) 6%, transparent);"
          onclick={fetchBriefing}
          disabled={briefingLoading || loading}
        >
          <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 7.5h1.5m-1.5 3h1.5m-7.5 3h7.5m-7.5 3h7.5m3-9h3.375c.621 0 1.125.504 1.125 1.125V18a2.25 2.25 0 0 1-2.25 2.25M16.5 7.5V18a2.25 2.25 0 0 0 2.25 2.25M16.5 7.5V4.875c0-.621-.504-1.125-1.125-1.125H4.125C3.504 3.75 3 4.254 3 4.875V18a2.25 2.25 0 0 0 2.25 2.25h13.5M6 7.5h3v3H6V7.5Z" />
          </svg>
          Brief me
        </button>
      </div>
    {/if}

    <!-- Input -->
    <div class="px-3 py-3 border-t" style="border-color: var(--iris-color-border);">
      <div class="flex gap-2">
        <input
          type="text"
          bind:value={input}
          onkeydown={handleKeydown}
          placeholder="Ask about your inbox..."
          disabled={loading || briefingLoading}
          class="flex-1 px-3 py-2 text-sm rounded-lg focus:outline-none disabled:opacity-50"
          style="background: var(--iris-color-bg-surface); border: 1px solid var(--iris-color-border); color: var(--iris-color-text);"
        />
        <button
          class="px-3 py-2 text-sm rounded-lg hover:opacity-90 disabled:opacity-50 transition-colors"
          style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
          onclick={() => sendMessage()}
          disabled={loading || briefingLoading || !input.trim()}
        >
          Send
        </button>
      </div>
    </div>
  </div>
{/if}
