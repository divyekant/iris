import { writable } from 'svelte/store';

export interface AuthState {
  bootstrapping: boolean;
  authenticated: boolean;
  requiresLogin: boolean;
  error: string | null;
}

export const authState = writable<AuthState>({
  bootstrapping: true,
  authenticated: false,
  requiresLogin: false,
  error: null,
});
