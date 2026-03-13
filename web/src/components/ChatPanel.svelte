<script lang="ts">
  import { api } from '../lib/api';
  import ComposeCard from './chat/ComposeCard.svelte';

  let { open, onclose }: { open: boolean; onclose: () => void } = $props();

  let sessionId = $state(crypto.randomUUID());
  let messages = $state<any[]>([]);
  let input = $state('');
  let loading = $state(false);
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
    { label: 'Briefing', prompt: 'Give me a briefing of my important unread emails' },
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
      {#if messages.length === 0 && !loading}
        <div class="text-center py-8">
          <p class="text-sm mb-4" style="color: var(--iris-color-text-muted);">Ask me anything about your inbox</p>
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
      {/each}

      {#if loading}
        <div class="flex justify-start">
          <div class="rounded-2xl rounded-bl-sm px-3 py-2 text-sm" style="background: var(--iris-color-bg-surface); color: var(--iris-color-text-faint);">
            <span class="inline-flex gap-1">
              <span class="w-1.5 h-1.5 rounded-full animate-bounce" style="background: var(--iris-color-text-faint); animation-delay: 0ms"></span>
              <span class="w-1.5 h-1.5 rounded-full animate-bounce" style="background: var(--iris-color-text-faint); animation-delay: 150ms"></span>
              <span class="w-1.5 h-1.5 rounded-full animate-bounce" style="background: var(--iris-color-text-faint); animation-delay: 300ms"></span>
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

    <!-- Input -->
    <div class="px-3 py-3 border-t" style="border-color: var(--iris-color-border);">
      <div class="flex gap-2">
        <input
          type="text"
          bind:value={input}
          onkeydown={handleKeydown}
          placeholder="Ask about your inbox..."
          disabled={loading}
          class="flex-1 px-3 py-2 text-sm rounded-lg focus:outline-none disabled:opacity-50"
          style="background: var(--iris-color-bg-surface); border: 1px solid var(--iris-color-border); color: var(--iris-color-text);"
        />
        <button
          class="px-3 py-2 text-sm rounded-lg hover:opacity-90 disabled:opacity-50 transition-colors"
          style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
          onclick={() => sendMessage()}
          disabled={loading || !input.trim()}
        >
          Send
        </button>
      </div>
    </div>
  </div>
{/if}
