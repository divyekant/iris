import { getSessionToken } from './api';

type WsEventHandler = (event: WsEvent) => void;

interface WsEvent {
  type: 'NewEmail' | 'SyncStatus' | 'SyncComplete';
  data: any;
}

class WebSocketClient {
  private ws: WebSocket | null = null;
  private handlers: Map<string, Set<WsEventHandler>> = new Map();
  private reconnectTimer: number | null = null;

  private buildUrl(): string {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const token = getSessionToken();
    const base = `${protocol}//${window.location.host}/ws`;
    return token ? `${base}?token=${encodeURIComponent(token)}` : base;
  }

  connect() {
    if (this.ws?.readyState === WebSocket.OPEN) return;
    this.ws = new WebSocket(this.buildUrl());

    this.ws.onopen = () => {
      console.log('[WS] Connected');
      if (this.reconnectTimer) {
        clearTimeout(this.reconnectTimer);
        this.reconnectTimer = null;
      }
    };

    this.ws.onmessage = (event) => {
      try {
        const parsed: WsEvent = JSON.parse(event.data);
        this.handlers.get(parsed.type)?.forEach((h) => h(parsed));
        this.handlers.get('*')?.forEach((h) => h(parsed));
      } catch (e) {
        console.warn('[WS] Parse error:', event.data);
      }
    };

    this.ws.onclose = () => {
      console.log('[WS] Disconnected, reconnecting in 3s...');
      this.reconnectTimer = window.setTimeout(() => this.connect(), 3000);
    };

    this.ws.onerror = () => { this.ws?.close(); };
  }

  on(type: string, handler: WsEventHandler) {
    if (!this.handlers.has(type)) this.handlers.set(type, new Set());
    this.handlers.get(type)!.add(handler);
    return () => this.handlers.get(type)?.delete(handler);
  }

  disconnect() {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.ws?.close();
  }
}

export const wsClient = new WebSocketClient();
