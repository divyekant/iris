export interface ThreadNote {
  id: string;
  thread_id: string;
  content: string;
  created_at: number;
  updated_at: number;
}

const BASE = '';

let sessionToken: string | null = null;

export async function initSession(): Promise<void> {
  const res = await fetch(`${BASE}/api/auth/bootstrap`, {
    headers: { 'Sec-Fetch-Site': 'same-origin' },
  });
  if (!res.ok) throw new Error(`Bootstrap failed: ${res.status}`);
  const data = await res.json();
  sessionToken = data.token;
}

export function getSessionToken(): string | null {
  return sessionToken;
}

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (sessionToken) {
    headers['x-session-token'] = sessionToken;
  }
  const res = await fetch(`${BASE}${path}`, {
    ...options,
    headers: { ...headers, ...(options?.headers as Record<string, string> || {}) },
  });
  if (!res.ok) throw new Error(`API error: ${res.status}`);
  if (res.status === 204 || res.headers.get('content-length') === '0') return undefined as T;
  return res.json();
}

export const api = {
  health: () => request<{ status: string; version: string }>('/api/health'),
  accounts: {
    list: () => request<any[]>('/api/accounts'),
    get: (id: string) => request<any>(`/api/accounts/${id}`),
    create: (data: any) => request<any>('/api/accounts', { method: 'POST', body: JSON.stringify(data) }),
  },
  messages: {
    list: (params?: { account_id?: string; folder?: string; category?: string; limit?: number; offset?: number }) => {
      const query = new URLSearchParams();
      if (params?.account_id) query.set('account_id', params.account_id);
      if (params?.folder) query.set('folder', params.folder);
      if (params?.category) query.set('category', params.category);
      if (params?.limit) query.set('limit', String(params.limit));
      if (params?.offset) query.set('offset', String(params.offset));
      return request<{ messages: any[]; unread_count: number; total: number }>(`/api/messages?${query}`);
    },
    get: (id: string) => request<any>(`/api/messages/${id}`),
    markRead: (id: string) => request<void>(`/api/messages/${id}/read`, { method: 'PUT' }),
    attachments: (messageId: string) =>
      request<{ id: string; message_id: string; filename: string | null; content_type: string; size: number; content_id: string | null }[]>(
        `/api/messages/${messageId}/attachments`
      ),
    batch: (ids: string[], action: string) =>
      request<{ updated: number }>('/api/messages/batch', {
        method: 'PATCH',
        body: JSON.stringify({ ids, action }),
      }),
    fixEncoding: () => request<{ fixed: number }>('/api/messages/fix-encoding', { method: 'POST' }),
    snooze: (ids: string[], snoozeUntil: number) =>
      request<{ updated: number }>('/api/messages/snooze', {
        method: 'POST',
        body: JSON.stringify({ ids, snooze_until: snoozeUntil }),
      }),
    unsnooze: (ids: string[]) =>
      request<{ updated: number }>('/api/messages/unsnooze', {
        method: 'POST',
        body: JSON.stringify({ ids }),
      }),
    listSnoozed: () =>
      request<{ messages: any[]; unread_count: number; total: number }>('/api/messages/snoozed'),
    reportSpam: (ids: string[], blockSender?: boolean) =>
      request<{ updated: number; blocked_sender?: string }>('/api/messages/report-spam', {
        method: 'POST',
        body: JSON.stringify({ ids, block_sender: blockSender }),
      }),
    unsubscribe: (id: string) =>
      request<{ success: boolean; method: string; url?: string }>(`/api/messages/${id}/unsubscribe`, { method: 'POST' }),
    needsReply: (params?: { account_id?: string; limit?: number; offset?: number }) => {
      const query = new URLSearchParams();
      if (params?.account_id) query.set('account_id', params.account_id);
      if (params?.limit) query.set('limit', String(params.limit));
      if (params?.offset) query.set('offset', String(params.offset));
      return request<{ messages: any[]; total: number }>(`/api/messages/needs-reply?${query}`);
    },
    redirect: (messageId: string, to: string) =>
      request<{ redirected: boolean; to: string }>(`/api/messages/${messageId}/redirect`, {
        method: 'POST',
        body: JSON.stringify({ to }),
      }),
  },
  threads: {
    get: (id: string) => request<any>(`/api/threads/${id}`),
    summarize: (id: string) =>
      request<{ summary: string; cached: boolean }>(`/api/threads/${id}/summarize`, { method: 'POST' }),
  },
  attachments: {
    downloadUrl: (attachmentId: string) => `/api/attachments/${attachmentId}/download`,
  },
  send: (data: {
    account_id: string;
    to: string[];
    cc?: string[];
    bcc?: string[];
    subject: string;
    body_text: string;
    body_html?: string;
    in_reply_to?: string;
    references?: string;
    attachments?: { filename: string; content_type: string; data_base64: string }[];
    schedule_at?: number;
  }) => request<{ id: string; send_at: number; can_undo: boolean; scheduled: boolean }>('/api/send', { method: 'POST', body: JSON.stringify(data) }),
  cancelSend: (id: string) =>
    request<{ cancelled: boolean }>(`/api/send/cancel/${id}`, { method: 'POST' }),
  scheduled: {
    list: () => request<{
      id: string;
      account_id: string;
      to_addresses: string;
      cc_addresses: string | null;
      bcc_addresses: string | null;
      subject: string;
      body_text: string;
      send_at: number;
      created_at: number;
      status: string;
    }[]>('/api/send/scheduled'),
    cancel: (id: string) => request<void>(`/api/send/scheduled/${id}`, { method: 'DELETE' }),
  },
  drafts: {
    list: (accountId: string) => request<any[]>(`/api/drafts?account_id=${accountId}`),
    save: (data: {
      account_id: string;
      draft_id?: string | null;
      to?: string[];
      cc?: string[];
      bcc?: string[];
      subject?: string;
      body_text: string;
      body_html?: string;
    }) => request<{ draft_id: string }>('/api/drafts', { method: 'POST', body: JSON.stringify(data) }),
    delete: (id: string) => request<void>(`/api/drafts/${id}`, { method: 'DELETE' }),
  },
  config: {
    get: () => request<{ theme: string; view_mode: string }>('/api/config'),
    setTheme: (theme: string) => request<void>('/api/config/theme', { method: 'PUT', body: JSON.stringify({ theme }) }),
    setViewMode: (view_mode: string) =>
      request<{ theme: string; view_mode: string }>('/api/config/view-mode', {
        method: 'PUT',
        body: JSON.stringify({ view_mode }),
      }),
    getUndoSendDelay: () => request<{ delay_seconds: number }>('/api/config/undo-send-delay'),
    setUndoSendDelay: (delay_seconds: number) =>
      request<{ delay_seconds: number }>('/api/config/undo-send-delay', {
        method: 'PUT',
        body: JSON.stringify({ delay_seconds }),
      }),
  },
  search: (params: { q: string; has_attachment?: boolean; after?: number; before?: number; account_id?: string; limit?: number; offset?: number; semantic?: boolean }) => {
    const query = new URLSearchParams();
    query.set('q', params.q);
    if (params.has_attachment) query.set('has_attachment', 'true');
    if (params.after) query.set('after', String(params.after));
    if (params.before) query.set('before', String(params.before));
    if (params.account_id) query.set('account_id', params.account_id);
    if (params.limit) query.set('limit', String(params.limit));
    if (params.offset) query.set('offset', String(params.offset));
    if (params.semantic) query.set('semantic', 'true');
    return request<{ results: any[]; total: number; query: string; parsed_operators: { key: string; value: string }[] }>(`/api/search?${query}`);
  },
  savedSearches: {
    list: () => request<{ id: string; name: string; query: string; account_id: string | null; created_at: number }[]>('/api/saved-searches'),
    create: (data: { name: string; query: string; account_id?: string }) =>
      request<{ id: string; name: string; query: string; account_id: string | null; created_at: number }>('/api/saved-searches', {
        method: 'POST',
        body: JSON.stringify(data),
      }),
    delete: (id: string) => request<void>(`/api/saved-searches/${id}`, { method: 'DELETE' }),
  },
  labels: {
    list: () => request<{ id: string; name: string; color: string; created_at: number; message_count: number }[]>('/api/labels'),
    create: (data: { name: string; color: string }) =>
      request<{ id: string; name: string; color: string; created_at: number }>('/api/labels', {
        method: 'POST',
        body: JSON.stringify(data),
      }),
    update: (id: string, data: { name: string; color: string }) =>
      request<{ id: string; name: string; color: string; created_at: number }>(`/api/labels/${id}`, {
        method: 'PUT',
        body: JSON.stringify(data),
      }),
    delete: (id: string) => request<void>(`/api/labels/${id}`, { method: 'DELETE' }),
  },
  filterRules: {
    list: () => request<{ id: string; name: string; conditions: { field: string; operator: string; value: string }[]; actions: { type: string; value?: string }[]; is_active: boolean; account_id: string | null; created_at: number }[]>('/api/filter-rules'),
    create: (data: { name: string; conditions: { field: string; operator: string; value: string }[]; actions: { type: string; value?: string }[]; account_id?: string }) =>
      request<any>('/api/filter-rules', { method: 'POST', body: JSON.stringify(data) }),
    update: (id: string, data: { name: string; conditions: { field: string; operator: string; value: string }[]; actions: { type: string; value?: string }[]; is_active: boolean }) =>
      request<any>(`/api/filter-rules/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
    delete: (id: string) => request<void>(`/api/filter-rules/${id}`, { method: 'DELETE' }),
  },
  aliases: {
    list: (accountId?: string) => {
      const query = accountId ? `?account_id=${accountId}` : '';
      return request<{ id: string; account_id: string; email: string; display_name: string; reply_to: string | null; is_default: boolean; created_at: number }[]>(`/api/aliases${query}`);
    },
    create: (data: { account_id: string; email: string; display_name: string; reply_to?: string; is_default: boolean }) =>
      request<{ id: string; account_id: string; email: string; display_name: string; reply_to: string | null; is_default: boolean; created_at: number }>('/api/aliases', {
        method: 'POST',
        body: JSON.stringify(data),
      }),
    update: (id: string, data: { email: string; display_name: string; reply_to?: string; is_default: boolean }) =>
      request<any>(`/api/aliases/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
    delete: (id: string) => request<void>(`/api/aliases/${id}`, { method: 'DELETE' }),
  },
  ai: {
    getConfig: () => request<{ enabled: boolean; connected: boolean; providers: { name: string; model: string; healthy: boolean }[]; ollama_url: string; model: string; memories_url: string; memories_connected: boolean; decay_enabled: boolean; decay_threshold_days: number; decay_factor: number }>('/api/config/ai'),
    setConfig: (data: { ollama_url?: string; model?: string; enabled?: boolean; anthropic_api_key?: string; anthropic_model?: string; openai_api_key?: string; openai_model?: string; memories_url?: string; memories_api_key?: string; decay_enabled?: boolean; decay_threshold_days?: number; decay_factor?: number }) =>
      request<{ enabled: boolean; connected: boolean; providers: { name: string; model: string; healthy: boolean }[]; ollama_url: string; model: string; memories_url: string; memories_connected: boolean; decay_enabled: boolean; decay_threshold_days: number; decay_factor: number }>('/api/config/ai', {
        method: 'PUT',
        body: JSON.stringify(data),
      }),
    testConnection: () => request<{ providers: { name: string; model: string; healthy: boolean }[] }>('/api/config/ai/test', { method: 'POST' }),
    assist: (data: { action: string; content: string }) =>
      request<{ result: string }>('/api/ai/assist', { method: 'POST', body: JSON.stringify(data) }),
    suggestSubject: (data: { body: string; current_subject?: string }) =>
      request<{ suggestions: string[] }>('/api/ai/suggest-subject', { method: 'POST', body: JSON.stringify(data) }),
    draftFromIntent: (data: { intent: string; context?: string }) =>
      request<{ subject: string; body: string; suggested_to: string[] }>('/api/ai/draft-from-intent', {
        method: 'POST', body: JSON.stringify(data),
      }),
    chat: (data: { session_id: string; message: string }) =>
      request<{ message: any }>('/api/ai/chat', { method: 'POST', body: JSON.stringify(data) }),
    chatHistory: (sessionId: string) => request<any[]>(`/api/ai/chat/${sessionId}`),
    chatConfirm: (data: { session_id: string; message_id: string }) =>
      request<{ executed: boolean; updated: number }>('/api/ai/chat/confirm', { method: 'POST', body: JSON.stringify(data) }),
    reprocess: () => request<{ enqueued: number }>('/api/ai/reprocess', { method: 'POST' }),
    briefing: () =>
      request<{
        summary: string;
        stats: { total_today: number; unread: number; needs_reply: number; urgent: number };
        highlights: { message_id: string; from: string; subject: string; reason: string }[];
      }>('/api/ai/briefing'),
    grammarCheck: (data: { content: string; subject?: string }) =>
      request<{ score: number; tone: string; issues: { kind: string; description: string; suggestion: string }[]; improved_content?: string }>('/api/ai/grammar-check', {
        method: 'POST', body: JSON.stringify(data),
      }),
    extractTasks: (messageId?: string, threadId?: string) =>
      request<{ tasks: { task: string; priority: string; deadline: string | null; source_subject: string | null }[] }>(
        '/api/ai/extract-tasks',
        { method: 'POST', body: JSON.stringify({ message_id: messageId, thread_id: threadId }) }
      ),
  },
  contacts: {
    topics: (email: string) =>
      request<{ email: string; topics: { topic: string; count: number }[]; total_emails: number; cached: boolean }>(
        `/api/contacts/${encodeURIComponent(email)}/topics`,
      ),
    top: () =>
      request<{ contacts: { email: string; name: string | null; email_count: number; last_contact: number | null }[] }>(
        '/api/contacts/top',
      ),
  },
  threadNotes: {
    list: (threadId: string) => request<{ notes: ThreadNote[] }>(`/api/threads/${threadId}/notes`),
    create: (threadId: string, content: string) => request<ThreadNote>(`/api/threads/${threadId}/notes`, { method: 'POST', body: JSON.stringify({ content }) }),
    update: (threadId: string, noteId: string, content: string) => request<ThreadNote>(`/api/threads/${threadId}/notes/${noteId}`, { method: 'PUT', body: JSON.stringify({ content }) }),
    delete: (threadId: string, noteId: string) => request<void>(`/api/threads/${threadId}/notes/${noteId}`, { method: 'DELETE' }),
  },
  auth: {
    startOAuth: (provider: string) => request<{ url: string }>(`/api/auth/oauth/${provider}`),
  },
  apiKeys: {
    list: () => request<any[]>('/api/api-keys'),
    create: (data: { name: string; permission: string; account_id?: string }) =>
      request<{ key: string; id: string; name: string; permission: string; key_prefix: string }>('/api/api-keys', {
        method: 'POST',
        body: JSON.stringify(data),
      }),
    revoke: (id: string) => request<void>(`/api/api-keys/${id}`, { method: 'DELETE' }),
  },
  blockedSenders: {
    list: () => request<{ id: string; email_address: string; reason?: string; created_at: number }[]>('/api/blocked-senders'),
    block: (data: { email_address: string; reason?: string }) =>
      request<{ id: string; email_address: string; reason?: string; created_at: number }>('/api/blocked-senders', {
        method: 'POST',
        body: JSON.stringify(data),
      }),
    unblock: (id: string) => request<void>(`/api/blocked-senders/${id}`, { method: 'DELETE' }),
  },
  subscriptions: {
    audit: () => request<{
      subscriptions: {
        sender: string;
        sender_name: string | null;
        total_count: number;
        read_count: number;
        read_rate: number;
        last_received: number;
        has_unsubscribe: boolean;
        category: string | null;
      }[];
    }>('/api/subscriptions/audit'),
  },
  auditLog: {
    list: (params?: { api_key_id?: string; limit?: number; offset?: number }) => {
      const query = new URLSearchParams();
      if (params?.api_key_id) query.set('api_key_id', params.api_key_id);
      if (params?.limit) query.set('limit', String(params.limit));
      if (params?.offset) query.set('offset', String(params.offset));
      return request<any[]>(`/api/audit-log?${query}`);
    },
  },
  signatures: {
    list: (accountId: string) =>
      request<{ id: string; account_id: string; name: string; body_text: string; body_html: string; is_default: boolean; created_at: number }[]>(
        `/api/signatures?account_id=${accountId}`
      ),
    create: (data: { account_id: string; name: string; body_text?: string; body_html?: string; is_default?: boolean }) =>
      request<{ id: string; account_id: string; name: string; body_text: string; body_html: string; is_default: boolean; created_at: number }>(
        '/api/signatures',
        { method: 'POST', body: JSON.stringify(data) }
      ),
    update: (id: string, data: { name?: string; body_text?: string; body_html?: string; is_default?: boolean }) =>
      request<{ id: string; account_id: string; name: string; body_text: string; body_html: string; is_default: boolean; created_at: number }>(
        `/api/signatures/${id}`,
        { method: 'PUT', body: JSON.stringify(data) }
      ),
    delete: (id: string) => request<void>(`/api/signatures/${id}`, { method: 'DELETE' }),
  },
  mutedThreads: {
    mute: (threadId: string) => request<{ muted: boolean }>(`/api/threads/${threadId}/mute`, { method: 'PUT' }),
    unmute: (threadId: string) => request<{ muted: boolean }>(`/api/threads/${threadId}/mute`, { method: 'DELETE' }),
    isMuted: (threadId: string) => request<{ muted: boolean }>(`/api/threads/${threadId}/mute`),
    list: () => request<string[]>('/api/muted-threads'),
  },
  notifications: {
    get: (accountId: string) => request<{ enabled: boolean }>(`/api/accounts/${accountId}/notifications`),
    set: (accountId: string, enabled: boolean) =>
      request<{ enabled: boolean }>(`/api/accounts/${accountId}/notifications`, {
        method: 'PUT',
        body: JSON.stringify({ enabled }),
      }),
  },
  templates: {
    list: () => request<any[]>('/api/templates'),
    create: (data: { name: string; subject?: string; body_text: string; body_html?: string }) =>
      request<any>('/api/templates', { method: 'POST', body: JSON.stringify(data) }),
    update: (id: string, data: { name: string; subject?: string; body_text: string; body_html?: string }) =>
      request<any>(`/api/templates/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
    delete: (id: string) => request<void>(`/api/templates/${id}`, { method: 'DELETE' }),
  },
};
