<script lang="ts">
  import { push, querystring } from 'svelte-spa-router';
  import { api } from '../lib/api';

  type Step = 'select' | 'oauth' | 'manual' | 'syncing' | 'success' | 'error';

  let step = $state<Step>('select');
  let selectedProvider = $state('');
  let errorMessage = $state('');

  // Manual IMAP/SMTP form fields
  let email = $state('');
  let imapHost = $state('');
  let imapPort = $state('993');
  let smtpHost = $state('');
  let smtpPort = $state('587');
  let username = $state('');
  let password = $state('');

  const providers = [
    { id: 'gmail', label: 'Gmail', icon: '\u{1F4E7}', oauth: true },
    { id: 'outlook', label: 'Outlook', icon: '\u{1F4E8}', oauth: true },
    { id: 'yahoo', label: 'Yahoo', icon: '\u{1F4EC}', oauth: false },
    { id: 'fastmail', label: 'Fastmail', icon: '\u{2709}\u{FE0F}', oauth: false },
    { id: 'imap', label: 'Other IMAP', icon: '\u{1F5A5}\u{FE0F}', oauth: false },
  ];

  const providerDefaults: Record<string, { imapHost: string; smtpHost: string }> = {
    yahoo: { imapHost: 'imap.mail.yahoo.com', smtpHost: 'smtp.mail.yahoo.com' },
    fastmail: { imapHost: 'imap.fastmail.com', smtpHost: 'smtp.fastmail.com' },
  };

  // Check for OAuth callback on mount
  $effect(() => {
    const qs = $querystring;
    if (qs) {
      const params = new URLSearchParams(qs);
      if (params.get('account_id')) {
        step = 'success';
      }
    }
  });

  function selectProvider(providerId: string) {
    selectedProvider = providerId;
    const provider = providers.find((p) => p.id === providerId);
    if (provider?.oauth) {
      step = 'oauth';
    } else {
      // Pre-fill defaults for known providers
      const defaults = providerDefaults[providerId];
      if (defaults) {
        imapHost = defaults.imapHost;
        smtpHost = defaults.smtpHost;
      } else {
        imapHost = '';
        smtpHost = '';
      }
      imapPort = '993';
      smtpPort = '587';
      email = '';
      username = '';
      password = '';
      step = 'manual';
    }
  }

  async function startOAuth() {
    try {
      step = 'syncing';
      const { url } = await api.auth.startOAuth(selectedProvider);
      window.location.href = url;
    } catch (err: any) {
      errorMessage = err.message || 'OAuth failed. Please try again.';
      step = 'error';
    }
  }

  async function submitManual() {
    try {
      step = 'syncing';
      await api.accounts.create({
        provider: selectedProvider,
        email,
        imap_host: imapHost,
        imap_port: Number(imapPort),
        smtp_host: smtpHost,
        smtp_port: Number(smtpPort),
        username: username || email,
        password,
      });
      step = 'success';
    } catch (err: any) {
      errorMessage = err.message || 'Failed to add account. Please check your settings.';
      step = 'error';
    }
  }

  function reset() {
    step = 'select';
    selectedProvider = '';
    errorMessage = '';
  }

  function providerLabel(): string {
    return providers.find((p) => p.id === selectedProvider)?.label ?? selectedProvider;
  }
</script>

<div class="max-w-2xl mx-auto py-12 px-6">
  <h2 class="text-2xl font-bold mb-8">Add Email Account</h2>

  {#if step === 'select'}
    <p class="mb-6" style="color: var(--iris-color-text-muted);">Choose your email provider to get started.</p>
    <div class="grid grid-cols-2 sm:grid-cols-3 gap-4">
      {#each providers as provider}
        <button
          class="provider-card flex flex-col items-center gap-2 p-6 rounded-xl border-2 transition-colors"
          onclick={() => selectProvider(provider.id)}
        >
          <span class="text-3xl">{provider.icon}</span>
          <span class="text-sm font-medium">{provider.label}</span>
        </button>
      {/each}
    </div>

  {:else if step === 'oauth'}
    <div class="text-center py-12">
      <p class="mb-6" style="color: var(--iris-color-text-muted);">
        Sign in with your {providerLabel()} account to securely connect your email.
      </p>
      <button
        class="px-6 py-3 rounded-lg font-medium transition-colors"
        style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
        onclick={startOAuth}
      >
        Sign in with {providerLabel()}
      </button>
      <div class="mt-4">
        <button
          class="back-btn text-sm transition-colors"
          onclick={reset}
        >
          Back
        </button>
      </div>
    </div>

  {:else if step === 'manual'}
    <form
      class="space-y-5"
      onsubmit={(e: Event) => { e.preventDefault(); submitManual(); }}
    >
      <p class="mb-2" style="color: var(--iris-color-text-muted);">
        Enter your {providerLabel()} email server settings.
      </p>

      <div>
        <label for="email" class="block text-sm font-medium mb-1">Email Address</label>
        <input
          id="email"
          type="email"
          bind:value={email}
          required
          placeholder="you@example.com"
          class="form-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
        />
      </div>

      <div class="grid grid-cols-2 gap-4">
        <div>
          <label for="imap-host" class="block text-sm font-medium mb-1">IMAP Host</label>
          <input
            id="imap-host"
            type="text"
            bind:value={imapHost}
            required
            placeholder="imap.example.com"
            class="form-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
          />
        </div>
        <div>
          <label for="imap-port" class="block text-sm font-medium mb-1">IMAP Port</label>
          <input
            id="imap-port"
            type="number"
            bind:value={imapPort}
            required
            class="form-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
          />
        </div>
      </div>

      <div class="grid grid-cols-2 gap-4">
        <div>
          <label for="smtp-host" class="block text-sm font-medium mb-1">SMTP Host</label>
          <input
            id="smtp-host"
            type="text"
            bind:value={smtpHost}
            required
            placeholder="smtp.example.com"
            class="form-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
          />
        </div>
        <div>
          <label for="smtp-port" class="block text-sm font-medium mb-1">SMTP Port</label>
          <input
            id="smtp-port"
            type="number"
            bind:value={smtpPort}
            required
            class="form-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
          />
        </div>
      </div>

      <div>
        <label for="username" class="block text-sm font-medium mb-1">Username <span class="font-normal" style="color: var(--iris-color-text-faint);">(defaults to email)</span></label>
        <input
          id="username"
          type="text"
          bind:value={username}
          placeholder={email || 'you@example.com'}
          class="form-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
        />
      </div>

      <div>
        <label for="password" class="block text-sm font-medium mb-1">Password</label>
        <input
          id="password"
          type="password"
          bind:value={password}
          required
          class="form-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
        />
      </div>

      <div class="flex items-center gap-4 pt-2">
        <button
          type="submit"
          class="px-6 py-2.5 rounded-lg font-medium transition-colors"
          style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
        >
          Connect Account
        </button>
        <button
          type="button"
          class="back-btn text-sm transition-colors"
          onclick={reset}
        >
          Back
        </button>
      </div>
    </form>

  {:else if step === 'syncing'}
    <div class="text-center py-16">
      <div class="spinner inline-block w-10 h-10 border-4 rounded-full animate-spin mb-4"></div>
      <p style="color: var(--iris-color-text-muted);">Connecting your account...</p>
    </div>

  {:else if step === 'success'}
    <div class="text-center py-16">
      <div class="success-icon w-16 h-16 mx-auto mb-4 rounded-full flex items-center justify-center">
        <svg class="w-8 h-8" style="color: var(--iris-color-success);" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
        </svg>
      </div>
      <h3 class="text-lg font-semibold mb-2">Account Connected</h3>
      <p class="mb-6" style="color: var(--iris-color-text-muted);">Your email account has been added successfully.</p>
      <button
        class="px-6 py-2.5 rounded-lg font-medium transition-colors"
        style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
        onclick={() => push('/')}
      >
        Go to Inbox
      </button>
    </div>

  {:else if step === 'error'}
    <div class="text-center py-16">
      <div class="error-icon w-16 h-16 mx-auto mb-4 rounded-full flex items-center justify-center">
        <svg class="w-8 h-8" style="color: var(--iris-color-error);" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
        </svg>
      </div>
      <h3 class="text-lg font-semibold mb-2">Connection Failed</h3>
      <p class="mb-6" style="color: var(--iris-color-text-muted);">{errorMessage}</p>
      <button
        class="px-6 py-2.5 rounded-lg font-medium transition-colors"
        style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
        onclick={reset}
      >
        Try Again
      </button>
    </div>
  {/if}
</div>

<style>
  .provider-card {
    background: var(--iris-color-bg-elevated);
    border-color: var(--iris-color-border);
    color: var(--iris-color-text);
  }
  .provider-card:hover {
    border-color: var(--iris-color-primary);
    background: var(--iris-color-bg-surface);
  }

  .form-input {
    background: var(--iris-color-bg-surface);
    border-color: var(--iris-color-border);
    color: var(--iris-color-text);
  }
  .form-input:focus {
    --tw-ring-color: var(--iris-color-primary);
    border-color: var(--iris-color-primary);
  }
  .form-input::placeholder {
    color: var(--iris-color-text-faint);
  }

  .back-btn {
    color: var(--iris-color-text-faint);
  }
  .back-btn:hover {
    color: var(--iris-color-text-muted);
  }

  .spinner {
    border-color: color-mix(in srgb, var(--iris-color-primary) 25%, transparent);
    border-top-color: var(--iris-color-primary);
  }

  .success-icon {
    background: color-mix(in srgb, var(--iris-color-success) 15%, transparent);
  }

  .error-icon {
    background: color-mix(in srgb, var(--iris-color-error) 15%, transparent);
  }
</style>
