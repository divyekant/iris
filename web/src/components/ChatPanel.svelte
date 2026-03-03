<script lang="ts">
  import { api } from '../lib/api';

  let { open, onclose }: { open: boolean; onclose: () => void } = $props();

  let sessionId = $state(crypto.randomUUID());
  let messages = $state<any[]>([]);
  let input = $state('');
  let loading = $state(false);
  let error = $state('');
  let messagesContainer: HTMLDivElement | undefined = $state();

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
      const res = await api.ai.chatConfirm({ session_id: sessionId, message_id: messageId });
      if (res.executed) {
        messages = [...messages, {
          role: 'assistant',
          content: `Done! Updated ${res.updated} email${res.updated === 1 ? '' : 's'}.`,
          id: crypto.randomUUID(),
        }];
        scrollToBottom();
      }
    } catch {
      error = 'Failed to execute action.';
    }
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
  <div class="w-80 border-l border-gray-200 dark:border-gray-700 flex flex-col bg-white dark:bg-gray-900 h-full">
    <!-- Header -->
    <div class="px-4 py-3 border-b border-gray-200 dark:border-gray-700 flex items-center justify-between">
      <h3 class="text-sm font-semibold">AI Chat</h3>
      <div class="flex items-center gap-1">
        <button
          class="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 text-xs"
          onclick={newSession}
          title="New conversation"
        >New</button>
        <button
          class="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
          onclick={onclose}
          title="Close chat"
        >&times;</button>
      </div>
    </div>

    <!-- Messages -->
    <div class="flex-1 overflow-y-auto p-3 space-y-3" bind:this={messagesContainer}>
      {#if messages.length === 0 && !loading}
        <div class="text-center py-8">
          <p class="text-sm text-gray-500 dark:text-gray-400 mb-4">Ask me anything about your inbox</p>
          <div class="space-y-2">
            {#each suggestions as { label, prompt }}
              <button
                class="w-full text-left px-3 py-2 text-xs rounded-lg border border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800 transition-colors text-gray-600 dark:text-gray-400"
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
          <div class="max-w-[85%] {msg.role === 'user'
            ? 'bg-blue-500 text-white rounded-2xl rounded-br-sm'
            : 'bg-gray-100 dark:bg-gray-800 text-gray-800 dark:text-gray-200 rounded-2xl rounded-bl-sm'} px-3 py-2 text-sm leading-relaxed">
            {msg.content}

            <!-- Citations -->
            {#if msg.citations?.length}
              <div class="mt-2 pt-2 border-t {msg.role === 'user' ? 'border-blue-400' : 'border-gray-200 dark:border-gray-700'}">
                <p class="text-[10px] font-medium opacity-70 mb-1">References:</p>
                {#each msg.citations as citation}
                  <p class="text-[11px] opacity-80 truncate">
                    {citation.from}: {citation.subject}
                  </p>
                {/each}
              </div>
            {/if}

            <!-- Action proposal -->
            {#if msg.proposed_action}
              <div class="mt-2 pt-2 border-t {msg.role === 'user' ? 'border-blue-400' : 'border-gray-200 dark:border-gray-700'}">
                <p class="text-xs font-medium mb-1">{msg.proposed_action.description}</p>
                <button
                  class="px-3 py-1 text-xs font-medium rounded-lg bg-blue-500 text-white hover:bg-blue-600 transition-colors"
                  onclick={() => confirmAction(msg.id)}
                >
                  Confirm
                </button>
              </div>
            {/if}
          </div>
        </div>
      {/each}

      {#if loading}
        <div class="flex justify-start">
          <div class="bg-gray-100 dark:bg-gray-800 rounded-2xl rounded-bl-sm px-3 py-2 text-sm text-gray-400">
            <span class="inline-flex gap-1">
              <span class="w-1.5 h-1.5 bg-gray-400 rounded-full animate-bounce" style="animation-delay: 0ms"></span>
              <span class="w-1.5 h-1.5 bg-gray-400 rounded-full animate-bounce" style="animation-delay: 150ms"></span>
              <span class="w-1.5 h-1.5 bg-gray-400 rounded-full animate-bounce" style="animation-delay: 300ms"></span>
            </span>
          </div>
        </div>
      {/if}

      {#if error}
        <div class="text-center">
          <p class="text-xs text-red-500">{error}</p>
        </div>
      {/if}
    </div>

    <!-- Input -->
    <div class="px-3 py-3 border-t border-gray-200 dark:border-gray-700">
      <div class="flex gap-2">
        <input
          type="text"
          bind:value={input}
          onkeydown={handleKeydown}
          placeholder="Ask about your inbox..."
          disabled={loading}
          class="flex-1 px-3 py-2 text-sm rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:opacity-50"
        />
        <button
          class="px-3 py-2 text-sm bg-blue-500 text-white rounded-lg hover:bg-blue-600 disabled:opacity-50 transition-colors"
          onclick={() => sendMessage()}
          disabled={loading || !input.trim()}
        >
          Send
        </button>
      </div>
    </div>
  </div>
{/if}
