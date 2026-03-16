<script lang="ts">
  import { api } from '../lib/api';
  import { wsClient } from '../lib/ws';
  import { push } from 'svelte-spa-router';
  import { Star, Archive, MailOpen, Trash2, Sparkles, ArrowLeft, Send, Clock, ShieldAlert, VolumeX, Volume2, Forward, ListChecks, Ellipsis, PanelRight, StickyNote, Reply, ReplyAll, MessageSquare, Timer } from 'lucide-svelte';
  import { irisCollapse, irisSlide } from '$lib/transitions';
  import { currentThreadContext } from '$lib/threadContext';
  import { feedback } from '$lib/feedback';
  import SpamDialog from '../components/SpamDialog.svelte';
  import MessageCard from '../components/thread/MessageCard.svelte';
  import SnoozePicker from '../components/SnoozePicker.svelte';
  import ContactTopicsPanel from '../components/contacts/ContactTopicsPanel.svelte';
  import RedirectDialog from '../components/thread/RedirectDialog.svelte';
  import NotesPanel from '../components/thread/NotesPanel.svelte';
  import MultiReplyPicker from '../components/compose/MultiReplyPicker.svelte';
  import DropdownMenu from '../components/shared/DropdownMenu.svelte';

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

  // Mute state
  let isMuted = $state(false);
  let muteLoading = $state(false);

  // Snooze picker state
  let snoozePickerOpen = $state(false);

  // Spam dialog state
  let showSpamDialog = $state(false);

  // Contact topics state
  let topicsEmail = $state<string | null>(null);
  let topicsName = $state<string | null>(null);

  // AI summary state
  let aiSummary = $state<string | null>(null);
  let summaryLoading = $state(false);
  let summaryError = $state('');

  // Intelligence strip state
  let stripExpanded = $state(false);
  let stripTasksLoaded = $state(false);
  let stripActionCount = $state(0);
  let stripDeadline = $state<string | null>(null);

  // AI suggestion strip state
  let suggestionVisible = $state(false);
  let suggestionText = $state<string | null>(null);
  let suggestionLoading = $state(false);
  let suggestionDismissed = $state(false);

  // Session-level cache: threadId → suggestion text
  const suggestionCache = new Map<string, string>();

  async function loadSuggestion() {
    if (suggestionLoading || suggestionDismissed) return;
    const msg = lastMessage();
    if (!msg) return;
    const cached = suggestionCache.get(params.id);
    if (cached) {
      suggestionText = cached;
      return;
    }
    suggestionLoading = true;
    try {
      const body = msg.body_text || msg.snippet || '';
      const res = await api.ai.assist({ action: 'rewrite', content: body });
      // Trim to a 1-2 sentence preview
      const full = res.result || '';
      const sentences = full.match(/[^.!?]+[.!?]+/g) || [];
      const preview = sentences.slice(0, 2).join(' ').trim() || full.slice(0, 200);
      suggestionText = preview;
      suggestionCache.set(params.id, preview);
    } catch {
      // Non-critical — hide strip on error
      suggestionVisible = false;
    } finally {
      suggestionLoading = false;
    }
  }

  function dismissSuggestion() {
    suggestionDismissed = true;
    suggestionVisible = false;
  }

  function replyWithSuggestion() {
    if (!suggestionText) return;
    startReply('reply');
    // Pre-fill body after startReply has set up the reply state
    setTimeout(() => {
      replyBody = suggestionText!;
    }, 0);
    suggestionVisible = false;
  }

  // Redirect dialog state
  let redirectOpen = $state(false);
  let redirectMessageId = $state('');
  let redirectFromAddress = $state('');
  let redirectSubject = $state('');

  function openRedirect() {
    const msg = lastMessage();
    if (!msg) return;
    redirectMessageId = msg.id;
    redirectFromAddress = msg.from_address || '';
    redirectSubject = thread?.subject || '';
    redirectOpen = true;
  }

  function closeRedirect() {
    redirectOpen = false;
  }

  // Task extraction state
  type ExtractedTask = { task: string; priority: string; deadline: string | null; source_subject: string | null };
  let extractedTasks = $state<ExtractedTask[]>([]);
  let tasksLoading = $state(false);
  let tasksError = $state('');
  let checkedTasks = $state<Set<number>>(new Set());

  // Side panel state
  type SidePanelTab = 'summary' | 'notes' | 'tasks';
  let sidePanelOpen = $state(
    typeof localStorage !== 'undefined'
      ? localStorage.getItem('iris-thread-panel-open') !== 'false'
      : true
  );
  let activeTab = $state<SidePanelTab>('summary');

  function toggleSidePanel() {
    sidePanelOpen = !sidePanelOpen;
    localStorage.setItem('iris-thread-panel-open', String(sidePanelOpen));
  }

  // Multi-reply state
  let showMultiReply = $state(false);
  let multiReplyLoading = $state(false);
  let multiReplyOptions = $state<{ tone: string; subject: string; body: string }[]>([]);
  let multiReplyError = $state('');

  async function handleGenerateReplies() {
    showMultiReply = true;
    multiReplyLoading = true;
    multiReplyError = '';
    multiReplyOptions = [];
    try {
      const msg = lastMessage();
      const res = await api.ai.multiReply(params.id, msg?.message_id || msg?.id);
      multiReplyOptions = res.options;
    } catch (e: any) {
      if (e.message?.includes('503')) {
        multiReplyError = 'Enable AI in Settings to use this feature.';
      } else {
        multiReplyError = 'Failed to generate reply options.';
      }
    } finally {
      multiReplyLoading = false;
    }
  }

  function handlePickReply(option: { tone: string; subject: string; body: string }) {
    replySubject = option.subject;
    replyBody = option.body;
    showMultiReply = false;
  }

  async function loadThread() {
    loading = true;
    error = '';
    try {
      thread = await api.threads.get(params.id);
      currentThreadContext.set({ id: params.id, subject: thread.subject || '(no subject)' });
      const unreadIds = thread.messages.filter((m: any) => !m.is_read).map((m: any) => m.id);
      if (unreadIds.length > 0) {
        await api.messages.batch(unreadIds, 'mark_read');
        for (const msg of thread.messages) {
          if (!msg.is_read) msg.is_read = true;
        }
      }
      // Check mute status
      try {
        const muteRes = await api.mutedThreads.isMuted(params.id);
        isMuted = muteRes.muted;
      } catch {
        // Ignore — mute status is non-critical
      }
    } catch (e: any) {
      error = e.message || 'Failed to load thread';
    } finally {
      loading = false;
    }
  }

  async function toggleMute() {
    if (muteLoading) return;
    muteLoading = true;
    try {
      if (isMuted) {
        await api.mutedThreads.unmute(params.id);
        isMuted = false;
        feedback.success('Thread unmuted');
      } else {
        await api.mutedThreads.mute(params.id);
        isMuted = true;
        feedback.success('Thread muted');
      }
    } catch (e: any) {
      error = e.message || 'Failed to toggle mute';
    } finally {
      muteLoading = false;
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
    showMultiReply = false;
    multiReplyOptions = [];
    multiReplyError = '';
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
    const wasStarred = thread.is_starred;
    try {
      await api.messages.batch(ids, action);
      if (action === 'archive') {
        feedback.success('Thread archived');
        push('/');
      } else if (action === 'delete') {
        feedback.success('Thread deleted');
        push('/');
      } else {
        if (action === 'star') {
          feedback.success(wasStarred ? 'Unstarred' : 'Starred');
        }
        await loadThread();
      }
    } catch (e: any) {
      error = e.message || 'Action failed';
    }
  }

  async function handleSnooze(snoozeUntil: number) {
    if (!thread) return;
    const ids = thread.messages.map((m: any) => m.id);
    try {
      await api.messages.snooze(ids, snoozeUntil);
      push('/');
    } catch (e: any) {
      error = e.message || 'Snooze failed';
    }
  }

  function openSpamDialog() {
    showSpamDialog = true;
  }

  async function handleReportSpam(blockSender: boolean) {
    if (!thread) return;
    const ids = thread.messages.map((m: any) => m.id);
    try {
      await api.messages.reportSpam(ids, blockSender);
      showSpamDialog = false;
      push('/');
    } catch (e: any) {
      error = e.message || 'Failed to report spam';
      showSpamDialog = false;
    }
  }

  async function loadSummary() {
    if (aiSummary || summaryLoading) return;
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

  async function loadStripIntelligence() {
    if (stripTasksLoaded) return;
    stripTasksLoaded = true;
    try {
      const res = await api.ai.extractTasks(undefined, params.id);
      stripActionCount = res.tasks?.length ?? 0;
      const firstWithDeadline = res.tasks?.find((t: any) => t.deadline);
      stripDeadline = firstWithDeadline?.deadline ?? null;
      // Also populate the side-panel tasks so the Tasks tab works for free
      if (extractedTasks.length === 0) {
        extractedTasks = res.tasks ?? [];
        checkedTasks = new Set();
      }
    } catch {
      // Non-critical — strip still shows message count
    }
  }

  function toggleStrip() {
    stripExpanded = !stripExpanded;
    if (stripExpanded) {
      loadStripIntelligence();
      loadSummary();
    }
  }

  // Keyboard navigation state
  let showShortcutHelp = $state(false);
  let pendingGChord = $state(false);
  let gChordTimer: ReturnType<typeof setTimeout> | null = null;

  function isInputFocused(): boolean {
    const el = document.activeElement;
    if (!el) return false;
    const tag = el.tagName.toLowerCase();
    return tag === 'input' || tag === 'textarea' || tag === 'select' || (el as HTMLElement).isContentEditable;
  }

  function focusSearch() {
    const input = document.getElementById('topnav-search-input') as HTMLInputElement | null;
    if (input) {
      input.focus();
      input.select();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === '?' && !e.metaKey && !e.ctrlKey && !e.altKey) {
      if (isInputFocused()) return;
      e.preventDefault();
      showShortcutHelp = !showShortcutHelp;
      return;
    }

    if (e.key === 'Escape') {
      if (showShortcutHelp) {
        e.preventDefault();
        showShortcutHelp = false;
        return;
      }
      if (!isInputFocused() && !replyMode) {
        e.preventDefault();
        push('/');
        return;
      }
      return;
    }

    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      focusSearch();
      return;
    }

    if (isInputFocused()) return;

    if (pendingGChord) {
      pendingGChord = false;
      if (gChordTimer) { clearTimeout(gChordTimer); gChordTimer = null; }
      switch (e.key) {
        case 'i': e.preventDefault(); push('/'); return;
        case 's': e.preventDefault(); push('/sent'); return;
        case 'd': e.preventDefault(); push('/drafts'); return;
      }
      return;
    }

    if (e.key === 'g' && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
      e.preventDefault();
      pendingGChord = true;
      gChordTimer = setTimeout(() => { pendingGChord = false; }, 1000);
      return;
    }

    if (e.key === '/' && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      focusSearch();
      return;
    }

    if (!thread) return;

    if (e.key === 'u' && !e.metaKey && !e.ctrlKey && !e.shiftKey) {
      e.preventDefault();
      push('/');
      return;
    }

    if (e.key === 'r' && !e.metaKey && !e.ctrlKey && !e.shiftKey) {
      e.preventDefault();
      startReply('reply');
      return;
    }

    if (e.key === 'a' && !e.metaKey && !e.ctrlKey && !e.shiftKey) {
      e.preventDefault();
      startReply('reply-all');
      return;
    }

    if (e.key === 'f' && !e.metaKey && !e.ctrlKey && !e.shiftKey) {
      e.preventDefault();
      startReply('forward');
      return;
    }

    if (e.key === 'e' && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      handleThreadAction('archive');
      return;
    }

    if (e.key === '#') {
      e.preventDefault();
      handleThreadAction('delete');
      return;
    }

    if (e.key === 's' && !e.metaKey && !e.ctrlKey && !e.shiftKey) {
      e.preventDefault();
      handleThreadAction('star');
      return;
    }
  }

  async function loadTasks() {
    if (extractedTasks.length > 0 || tasksLoading) return;
    tasksLoading = true;
    tasksError = '';
    try {
      const res = await api.ai.extractTasks(undefined, params.id);
      extractedTasks = res.tasks;
      checkedTasks = new Set();
    } catch (e: any) {
      if (e.message?.includes('503')) {
        tasksError = 'Enable AI in Settings to use this feature.';
      } else {
        tasksError = 'Failed to extract tasks.';
      }
    } finally {
      tasksLoading = false;
    }
  }

  function toggleTaskCheck(index: number) {
    const next = new Set(checkedTasks);
    if (next.has(index)) {
      next.delete(index);
    } else {
      next.add(index);
    }
    checkedTasks = next;
  }

  // Auto-load strip intelligence for multi-message threads
  $effect(() => {
    if (thread && thread.message_count > 1 && !stripTasksLoaded) {
      loadStripIntelligence();
    }
  });

  // Show AI suggestion strip when latest message needs a reply
  $effect(() => {
    if (!thread || suggestionDismissed) return;
    const msg = lastMessage();
    const needsReply = msg?.ai_needs_reply === true || msg?.ai_needs_reply === 1;
    if (needsReply && !suggestionVisible) {
      suggestionVisible = true;
      loadSuggestion();
    }
  });

  // Auto-load content when side panel tab changes
  $effect(() => {
    if (!thread || !sidePanelOpen) return;
    if (activeTab === 'summary' && thread.message_count > 1) {
      loadSummary();
    } else if (activeTab === 'tasks') {
      loadTasks();
    }
  });

  $effect(() => {
    if (params.id) {
      aiSummary = null;
      summaryError = '';
      extractedTasks = [];
      tasksError = '';
      checkedTasks = new Set();
      stripExpanded = false;
      stripTasksLoaded = false;
      stripActionCount = 0;
      stripDeadline = null;
      suggestionVisible = false;
      suggestionText = null;
      suggestionLoading = false;
      suggestionDismissed = false;
      cancelReply();
      loadThread();
    }
    const off = wsClient.on('NewEmail', () => {
      if (thread) loadThread();
    });
    return () => {
      off();
      currentThreadContext.set(null);
    };
  });
</script>

<svelte:window onkeydown={handleKeydown} />

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
        <p class="text-xs truncate flex items-center gap-1.5" style="color: var(--iris-color-text-faint);">
          <span class="truncate">
            {thread.participants.map((p: any) => p.name || p.email).join(', ')}
            &middot; {thread.message_count} message{thread.message_count === 1 ? '' : 's'}
          </span>
          {#if thread.messages?.[0]?.from_address}
            <button
              class="flex-shrink-0 text-[10px] px-1.5 py-0.5 rounded transition-colors topics-link"
              onclick={(e) => { e.stopPropagation(); topicsEmail = thread.messages[0].from_address; topicsName = thread.messages[0].from_name || null; }}
            >Topics</button>
          {/if}
        </p>
      </div>
      <div class="flex items-center gap-1">
        <!-- Primary actions: Reply, Reply All, Forward -->
        <button
          class="action-primary-btn flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium transition-colors"
          onclick={() => startReply('reply')}
          title="Reply (r)"
        >
          <Reply size={14} />
          Reply
        </button>
        <button
          class="action-secondary-btn flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium transition-colors"
          onclick={() => startReply('reply-all')}
          title="Reply All (a)"
        >
          <ReplyAll size={14} />
          Reply All
        </button>
        <button
          class="action-secondary-btn flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium transition-colors"
          onclick={() => startReply('forward')}
          title="Forward (f)"
        >
          <Forward size={14} />
          Forward
        </button>

        <!-- Group divider -->
        <div class="flex-shrink-0" style="width: 1px; height: 20px; background: var(--iris-color-border); margin: 0 4px;"></div>

        <!-- Organize dropdown -->
        <div class="relative">
          <DropdownMenu
            triggerLabel="Organize"
            items={[
              {
                label: thread?.is_starred ? 'Unstar' : 'Star',
                shortcut: 's',
                onClick: () => handleThreadAction('star'),
                dividerAfter: false,
              },
              {
                label: 'Snooze',
                shortcut: 'b',
                onClick: () => { snoozePickerOpen = !snoozePickerOpen; },
                dividerAfter: false,
              },
              {
                label: 'Archive',
                shortcut: 'e',
                onClick: () => handleThreadAction('archive'),
                dividerAfter: false,
              },
              {
                label: 'Delete',
                shortcut: '#',
                onClick: () => handleThreadAction('delete'),
              },
            ]}
          />
          {#if snoozePickerOpen}
            <div class="absolute right-0 top-full mt-1 z-50">
              <SnoozePicker
                onpick={(epoch) => { snoozePickerOpen = false; handleSnooze(epoch); }}
                onclose={() => { snoozePickerOpen = false; }}
              />
            </div>
          {/if}
        </div>

        <!-- Group divider -->
        <div class="flex-shrink-0" style="width: 1px; height: 20px; background: var(--iris-color-border); margin: 0 4px;"></div>

        <!-- AI dropdown -->
        <DropdownMenu
          triggerLabel="AI"
          items={[
            {
              label: 'Summarize',
              onClick: () => { activeTab = 'summary'; if (!sidePanelOpen) toggleSidePanel(); loadSummary(); },
            },
            {
              label: 'Extract Tasks',
              onClick: () => { activeTab = 'tasks'; if (!sidePanelOpen) toggleSidePanel(); },
            },
            {
              label: 'Generate Replies',
              onClick: () => handleGenerateReplies(),
            },
          ]}
        />

        <!-- Group divider -->
        <div class="flex-shrink-0" style="width: 1px; height: 20px; background: var(--iris-color-border); margin: 0 4px;"></div>

        <!-- More dropdown -->
        <DropdownMenu
          triggerLabel="More"
          items={[
            {
              label: 'Report Spam',
              onClick: () => openSpamDialog(),
            },
            {
              label: isMuted ? 'Unmute Thread' : 'Mute Thread',
              onClick: () => toggleMute(),
              disabled: muteLoading,
            },
            {
              label: 'Redirect',
              onClick: () => openRedirect(),
            },
          ]}
        />

        <!-- Panel toggle -->
        <button
          class="p-1.5 transition-colors thread-action-btn ml-1"
          class:panel-active={sidePanelOpen}
          onclick={toggleSidePanel}
          title={sidePanelOpen ? 'Hide side panel' : 'Show side panel'}
        ><PanelRight size={15} /></button>
      </div>
    {/if}
  </div>

  <!-- Intelligence strip (multi-message threads only) -->
  {#if thread && thread.message_count > 1}
    <button
      class="intel-strip w-full flex items-center gap-4 px-4 py-2 text-left"
      onclick={toggleStrip}
      aria-expanded={stripExpanded}
    >
      <span class="flex items-center gap-1.5 text-xs" style="color: var(--iris-color-text-muted);">
        <MessageSquare size={12} />
        {thread.message_count} messages
      </span>
      {#if stripActionCount > 0}
        <span class="flex items-center gap-1.5 text-xs" style="color: var(--iris-color-warning);">
          <ListChecks size={12} />
          {stripActionCount} action item{stripActionCount === 1 ? '' : 's'}
        </span>
      {/if}
      {#if stripDeadline}
        <span class="flex items-center gap-1.5 text-xs" style="color: var(--iris-color-error);">
          <Timer size={12} />
          deadline {stripDeadline}
        </span>
      {/if}
      <span class="ml-auto text-xs" style="color: var(--iris-color-text-faint);">
        {stripExpanded ? 'Hide summary' : 'Click for full summary'}
      </span>
    </button>

    {#if stripExpanded}
      <div transition:irisCollapse class="intel-strip-body px-4 py-3" style="border-bottom: 1px solid var(--iris-color-border);">
        {#if summaryLoading}
          <div class="flex items-center gap-2" style="color: var(--iris-color-text-faint);">
            <div class="w-3 h-3 rounded-full animate-spin" style="border: 2px solid var(--iris-color-border); border-top-color: var(--iris-color-primary);"></div>
            <span class="text-xs">Summarizing thread...</span>
          </div>
        {:else if aiSummary}
          <p class="text-sm leading-relaxed" style="color: var(--iris-color-text);">{aiSummary}</p>
        {:else if summaryError}
          <p class="text-xs" style="color: var(--iris-color-text-faint);">{summaryError}</p>
        {:else}
          <p class="text-xs" style="color: var(--iris-color-text-faint);">Loading summary...</p>
        {/if}
      </div>
    {/if}
  {/if}

  <!-- AI suggestion strip (needs-reply threads) -->
  {#if suggestionVisible && !replyMode}
    <div
      transition:irisSlide
      class="ai-suggestion-strip flex items-start gap-3 px-4 py-3"
      style="background: var(--iris-color-bg-elevated); border-bottom: 1px solid var(--iris-color-border);"
    >
      <div class="flex items-center gap-1.5 flex-shrink-0 pt-0.5" style="color: var(--iris-color-primary);">
        <Sparkles size={14} />
        <span class="text-xs font-semibold whitespace-nowrap" style="color: var(--iris-color-primary);">AI suggests a reply</span>
      </div>

      <div class="flex-1 min-w-0">
        {#if suggestionLoading}
          <span class="text-xs italic" style="color: var(--iris-color-text-faint);">Generating suggestion...</span>
        {:else if suggestionText}
          <span class="text-xs leading-relaxed" style="color: var(--iris-color-text-muted);">{suggestionText}</span>
        {/if}
      </div>

      <div class="flex items-center gap-3 flex-shrink-0">
        {#if !suggestionLoading && suggestionText}
          <button
            class="suggestion-reply-btn px-3 py-1.5 text-xs rounded-lg font-medium transition-colors"
            onclick={replyWithSuggestion}
          >
            Reply with this
          </button>
        {/if}
        <button
          class="text-xs transition-colors"
          style="color: var(--iris-color-text-faint);"
          onclick={dismissSuggestion}
        >
          Dismiss
        </button>
      </div>
    </div>
  {/if}

  <!-- Main content: messages + side panel -->
  <div class="flex-1 flex min-h-0">
    <!-- Messages column -->
    <div class="flex-1 flex flex-col min-w-0">
      <!-- Messages scroll area -->
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

              <!-- Multi-reply picker (reply/reply-all only) -->
              {#if showMultiReply && (replyMode === 'reply' || replyMode === 'reply-all')}
                <MultiReplyPicker
                  options={multiReplyOptions}
                  loading={multiReplyLoading}
                  error={multiReplyError}
                  onpick={handlePickReply}
                  onclose={() => (showMultiReply = false)}
                />
              {/if}

              <!-- Send bar -->
              <div class="flex items-center gap-2">
                {#if sendError}
                  <p class="text-xs flex-1" style="color: var(--iris-color-error);">{sendError}</p>
                {:else}
                  <span class="flex-1"></span>
                {/if}
                {#if replyMode === 'reply' || replyMode === 'reply-all'}
                  <button
                    class="px-3 py-1.5 text-xs rounded-lg font-medium transition-colors disabled:opacity-50 reply-ai-btn"
                    onclick={handleGenerateReplies}
                    disabled={multiReplyLoading || sending}
                    title="Generate 3 AI reply options in different tones"
                  >
                    {multiReplyLoading ? 'Generating...' : 'AI Reply Options'}
                  </button>
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

    </div>

    <!-- Side panel divider + panel -->
    {#if sidePanelOpen && thread && !loading}
      <!-- Vertical divider -->
      <div class="flex-shrink-0" style="width: 1px; background: var(--iris-color-border);"></div>

      <!-- Side panel -->
      <div class="flex-shrink-0 flex flex-col" style="width: 300px; background: var(--iris-color-bg-surface);">
        <!-- Tab bar -->
        <div class="flex" style="border-bottom: 1px solid var(--iris-color-border);">
          <button
            class="flex-1 flex items-center justify-center gap-1.5 px-3 py-2.5 text-xs font-medium transition-colors side-panel-tab"
            class:active={activeTab === 'summary'}
            onclick={() => { activeTab = 'summary'; }}
          >
            <Sparkles size={13} />
            Summary
          </button>
          <button
            class="flex-1 flex items-center justify-center gap-1.5 px-3 py-2.5 text-xs font-medium transition-colors side-panel-tab"
            class:active={activeTab === 'notes'}
            onclick={() => { activeTab = 'notes'; }}
          >
            <StickyNote size={13} />
            Notes
          </button>
          <button
            class="flex-1 flex items-center justify-center gap-1.5 px-3 py-2.5 text-xs font-medium transition-colors side-panel-tab"
            class:active={activeTab === 'tasks'}
            onclick={() => { activeTab = 'tasks'; }}
          >
            <ListChecks size={13} />
            Tasks
          </button>
        </div>

        <!-- Tab content -->
        <div class="flex-1 overflow-y-auto">
          {#if activeTab === 'summary'}
            <!-- Summary tab content -->
            <div class="p-3">
              {#if thread.message_count <= 1}
                <p class="text-xs" style="color: var(--iris-color-text-faint);">No summary available for single-message threads.</p>
              {:else if summaryLoading}
                <div class="flex items-center gap-2 py-4" style="color: var(--iris-color-text-faint);">
                  <div class="w-3 h-3 rounded-full animate-spin" style="border: 2px solid var(--iris-color-border); border-top-color: var(--iris-color-primary);"></div>
                  <span class="text-xs">Summarizing thread...</span>
                </div>
              {:else if aiSummary}
                <div class="text-sm leading-relaxed" style="color: var(--iris-color-text);">
                  {aiSummary}
                </div>
              {:else if summaryError}
                <p class="text-xs" style="color: var(--iris-color-text-faint);">{summaryError}</p>
              {:else}
                <p class="text-xs" style="color: var(--iris-color-text-faint);">Loading summary...</p>
              {/if}
            </div>
          {:else if activeTab === 'notes'}
            <!-- Notes tab content -->
            <NotesPanel threadId={params.id} alwaysOpen={true} />
          {:else if activeTab === 'tasks'}
            <!-- Tasks tab content -->
            <div class="p-3">
              {#if tasksLoading}
                <div class="flex items-center gap-2 py-4" style="color: var(--iris-color-text-faint);">
                  <div class="w-3 h-3 rounded-full animate-spin" style="border: 2px solid var(--iris-color-border); border-top-color: var(--iris-color-primary);"></div>
                  <span class="text-xs">Extracting tasks...</span>
                </div>
              {:else if tasksError}
                <p class="text-xs py-1" style="color: var(--iris-color-text-faint);">{tasksError}</p>
              {:else if extractedTasks.length === 0}
                <p class="text-xs py-1" style="color: var(--iris-color-text-faint);">No action items found in this thread.</p>
              {:else}
                <div class="space-y-1.5">
                  {#each extractedTasks as task, i (i)}
                    <div class="flex items-start gap-2 rounded-lg px-2 py-1.5" style="background: var(--iris-color-bg-elevated);">
                      <button
                        class="mt-0.5 w-4 h-4 rounded flex-shrink-0 flex items-center justify-center task-checkbox"
                        class:checked={checkedTasks.has(i)}
                        onclick={() => toggleTaskCheck(i)}
                        aria-label="Toggle task"
                      >
                        {#if checkedTasks.has(i)}
                          <span class="text-[10px]">&#10003;</span>
                        {/if}
                      </button>
                      <div class="flex-1 min-w-0">
                        <p class="text-sm leading-snug" style="color: var(--iris-color-text);" class:line-through={checkedTasks.has(i)} class:opacity-50={checkedTasks.has(i)}>
                          {task.task}
                        </p>
                        <div class="flex items-center gap-2 mt-0.5 flex-wrap">
                          <span class="text-[10px] font-medium px-1.5 py-0.5 rounded-full task-priority-badge"
                            class:priority-high={task.priority === 'high'}
                            class:priority-medium={task.priority === 'medium'}
                            class:priority-low={task.priority === 'low'}
                          >
                            {task.priority}
                          </span>
                          {#if task.deadline}
                            <span class="text-[10px]" style="color: var(--iris-color-text-faint);">Due: {task.deadline}</span>
                          {/if}
                          {#if task.source_subject}
                            <span class="text-[10px] truncate" style="color: var(--iris-color-text-faint);">from: {task.source_subject}</span>
                          {/if}
                        </div>
                      </div>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>

{#if showSpamDialog && thread}
  <SpamDialog
    senderEmail={thread.messages[0]?.from_address || 'Unknown sender'}
    messageIds={thread.messages.map((m: any) => m.id)}
    onconfirm={handleReportSpam}
    onclose={() => { showSpamDialog = false; }}
  />
{/if}

{#if topicsEmail}
  <ContactTopicsPanel
    email={topicsEmail}
    name={topicsName}
    onclose={() => { topicsEmail = null; topicsName = null; }}
  />
{/if}

{#if redirectOpen}
  <RedirectDialog
    messageId={redirectMessageId}
    fromAddress={redirectFromAddress}
    subject={redirectSubject}
    onclose={closeRedirect}
  />
{/if}

<style>
  .action-primary-btn {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .action-primary-btn:hover {
    background: var(--iris-color-primary-hover);
  }
  .action-secondary-btn {
    background: transparent;
    color: var(--iris-color-text-muted);
    border: 1px solid var(--iris-color-border);
  }
  .action-secondary-btn:hover {
    background: var(--iris-color-bg-elevated);
    color: var(--iris-color-text);
  }
  .thread-action-btn {
    color: var(--iris-color-text-faint);
  }
  .thread-action-btn:hover {
    color: var(--iris-color-text-muted);
  }
  .retry-btn {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .retry-btn:hover {
    background: var(--iris-color-primary-hover);
  }
  .reply-send-btn {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .reply-send-btn:hover:not(:disabled) {
    background: var(--iris-color-primary-hover);
  }
  .topics-link {
    color: var(--iris-color-primary);
    background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent);
  }
  .topics-link:hover {
    background: color-mix(in srgb, var(--iris-color-primary) 15%, transparent);
  }
  .task-checkbox {
    border: 1.5px solid var(--iris-color-primary);
    background: transparent;
    color: var(--iris-color-primary);
    cursor: pointer;
  }
  .task-checkbox.checked {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .task-priority-badge {
    color: var(--iris-color-text-faint);
    background: var(--iris-color-bg-elevated);
  }
  .task-priority-badge.priority-high {
    color: var(--iris-color-error);
    background: color-mix(in srgb, var(--iris-color-error) 12%, transparent);
  }
  .task-priority-badge.priority-medium {
    color: var(--iris-color-warning);
    background: color-mix(in srgb, var(--iris-color-warning) 12%, transparent);
  }
  .task-priority-badge.priority-low {
    color: var(--iris-color-success);
    background: color-mix(in srgb, var(--iris-color-success) 12%, transparent);
  }
  .reply-ai-btn {
    color: var(--iris-color-primary);
    background: transparent;
    border: 1px solid var(--iris-color-primary);
  }
  .reply-ai-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent);
  }
  .thread-action-btn.panel-active {
    color: var(--iris-color-primary);
  }
  .side-panel-tab {
    color: var(--iris-color-text-faint);
    border-bottom: 2px solid transparent;
    background: transparent;
    cursor: pointer;
  }
  .side-panel-tab:hover {
    color: var(--iris-color-text-muted);
    background: var(--iris-color-bg-elevated);
  }
  .side-panel-tab.active {
    color: var(--iris-color-text);
    border-bottom-color: var(--iris-color-primary);
  }
  .suggestion-reply-btn {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .suggestion-reply-btn:hover {
    background: var(--iris-color-primary-hover);
  }
  .intel-strip {
    background: var(--iris-color-primary-dim);
    border-bottom: 1px solid var(--iris-color-border);
    cursor: pointer;
    transition: background var(--iris-transition-fast);
  }
  .intel-strip:hover {
    background: color-mix(in srgb, var(--iris-color-primary) 12%, transparent);
  }
  .intel-strip-body {
    background: var(--iris-color-primary-dim);
  }
</style>
