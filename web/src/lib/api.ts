const BASE = '';

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...options,
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
    batch: (ids: string[], action: string) =>
      request<{ updated: number }>('/api/messages/batch', {
        method: 'PATCH',
        body: JSON.stringify({ ids, action }),
      }),
  },
  threads: {
    get: (id: string) => request<any>(`/api/threads/${id}`),
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
  }) => request<{ message_id: string }>('/api/send', { method: 'POST', body: JSON.stringify(data) }),
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
  },
  search: (params: { q: string; has_attachment?: boolean; after?: number; before?: number; account_id?: string; limit?: number; offset?: number }) => {
    const query = new URLSearchParams();
    query.set('q', params.q);
    if (params.has_attachment) query.set('has_attachment', 'true');
    if (params.after) query.set('after', String(params.after));
    if (params.before) query.set('before', String(params.before));
    if (params.account_id) query.set('account_id', params.account_id);
    if (params.limit) query.set('limit', String(params.limit));
    if (params.offset) query.set('offset', String(params.offset));
    return request<{ results: any[]; total: number; query: string }>(`/api/search?${query}`);
  },
  auth: {
    startOAuth: (provider: string) => request<{ url: string }>(`/api/auth/oauth/${provider}`),
  },
};
