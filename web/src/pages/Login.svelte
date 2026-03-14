<script lang="ts">
  import { api } from '../lib/api';
  import { authState } from '../lib/auth';

  let password = $state('');
  let submitting = $state(false);
  let error = $state('');

  async function submitLogin() {
    if (!password || submitting) return;

    submitting = true;
    error = '';

    try {
      const result = await api.auth.login(password);
      authState.set({
        bootstrapping: false,
        authenticated: result.authenticated,
        requiresLogin: result.requires_login,
        error: null,
      });
      password = '';
    } catch {
      error = 'Password check failed. Try again.';
    } finally {
      submitting = false;
    }
  }
</script>

<div class="min-h-screen flex items-center justify-center px-6" style="background: var(--iris-color-bg);">
  <div
    class="w-full max-w-md rounded-2xl p-8"
    style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border); box-shadow: 0 24px 80px rgba(0, 0, 0, 0.16);"
  >
    <div class="mb-6">
      <p class="text-xs uppercase tracking-[0.24em] mb-3" style="color: var(--iris-color-primary);">Protected Access</p>
      <h1 class="text-2xl font-semibold mb-2" style="color: var(--iris-color-text);">Unlock Iris</h1>
      <p class="text-sm leading-6" style="color: var(--iris-color-text-muted);">
        This server requires a password before the mailbox UI can load.
      </p>
    </div>

    <form class="space-y-4" onsubmit={(e: Event) => { e.preventDefault(); submitLogin(); }}>
      <div>
        <label for="iris-password" class="block text-sm font-medium mb-2" style="color: var(--iris-color-text);">
          Password
        </label>
        <input
          id="iris-password"
          type="password"
          bind:value={password}
          autocomplete="current-password"
          class="w-full rounded-xl px-4 py-3 outline-none"
          style="background: var(--iris-color-bg-surface); color: var(--iris-color-text); border: 1px solid var(--iris-color-border);"
          placeholder="Enter server password"
        />
      </div>

      {#if error}
        <div
          class="rounded-xl px-4 py-3 text-sm"
          style="background: color-mix(in srgb, var(--iris-color-error) 10%, transparent); color: var(--iris-color-error); border: 1px solid color-mix(in srgb, var(--iris-color-error) 24%, transparent);"
        >
          {error}
        </div>
      {/if}

      <button
        type="submit"
        class="w-full rounded-xl px-4 py-3 text-sm font-medium transition-opacity"
        style="background: var(--iris-color-primary); color: var(--iris-color-bg); opacity: {submitting ? '0.72' : '1'};"
        disabled={submitting || !password}
      >
        {submitting ? 'Checking...' : 'Sign In'}
      </button>
    </form>
  </div>
</div>
