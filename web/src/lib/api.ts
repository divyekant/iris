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
  }) => request<{ id: string; send_at: number; can_undo: boolean }>('/api/send', { method: 'POST', body: JSON.stringify(data) }),
  cancelSend: (id: string) =>
    request<{ cancelled: boolean }>(`/api/send/cancel/${id}`, { method: 'POST' }),
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
    return request<{ results: any[]; total: number; query: string }>(`/api/search?${query}`);
  },
  ai: {
    getConfig: () => request<{ enabled: boolean; connected: boolean; providers: { name: string; model: string; healthy: boolean }[]; ollama_url: string; model: string; memories_url: string; memories_connected: boolean }>('/api/config/ai'),
    setConfig: (data: { ollama_url?: string; model?: string; enabled?: boolean; anthropic_api_key?: string; anthropic_model?: string; openai_api_key?: string; openai_model?: string; memories_url?: string; memories_api_key?: string }) =>
      request<{ enabled: boolean; connected: boolean; providers: { name: string; model: string; healthy: boolean }[]; ollama_url: string; model: string; memories_url: string; memories_connected: boolean }>('/api/config/ai', {
        method: 'PUT',
        body: JSON.stringify(data),
      }),
    testConnection: () => request<{ providers: { name: string; model: string; healthy: boolean }[] }>('/api/config/ai/test', { method: 'POST' }),
    assist: (data: { action: string; content: string }) =>
      request<{ result: string }>('/api/ai/assist', { method: 'POST', body: JSON.stringify(data) }),
    chat: (data: { session_id: string; message: string }) =>
      request<{ message: any }>('/api/ai/chat', { method: 'POST', body: JSON.stringify(data) }),
    chatHistory: (sessionId: string) => request<any[]>(`/api/ai/chat/${sessionId}`),
    chatConfirm: (data: { session_id: string; message_id: string }) =>
      request<{ executed: boolean; updated: number }>('/api/ai/chat/confirm', { method: 'POST', body: JSON.stringify(data) }),
    reprocess: () => request<{ enqueued: number }>('/api/ai/reprocess', { method: 'POST' }),
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
};
