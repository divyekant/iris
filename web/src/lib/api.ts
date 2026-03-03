const BASE = '';

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...options,
  });
  if (!res.ok) throw new Error(`API error: ${res.status}`);
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
    list: (params?: { account_id?: string; folder?: string; limit?: number; offset?: number }) => {
      const query = new URLSearchParams();
      if (params?.account_id) query.set('account_id', params.account_id);
      if (params?.folder) query.set('folder', params.folder);
      if (params?.limit) query.set('limit', String(params.limit));
      if (params?.offset) query.set('offset', String(params.offset));
      return request<{ messages: any[]; unread_count: number; total: number }>(`/api/messages?${query}`);
    },
  },
  config: {
    get: () => request<{ theme: string }>('/api/config'),
    setTheme: (theme: string) => request<void>('/api/config/theme', { method: 'PUT', body: JSON.stringify({ theme }) }),
  },
  auth: {
    startOAuth: (provider: string) => request<{ url: string }>(`/api/auth/oauth/${provider}`),
  },
};
