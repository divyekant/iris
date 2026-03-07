<script lang="ts">
  import './app.css';
  import Router from 'svelte-spa-router';
  import { wrap } from 'svelte-spa-router/wrap';
  import AppShell from './components/AppShell.svelte';
  import Inbox from './pages/Inbox.svelte';
  import ThreadView from './pages/ThreadView.svelte';
  import AccountSetup from './pages/AccountSetup.svelte';
  import Settings from './pages/Settings.svelte';
  import Search from './pages/Search.svelte';
  import FolderView from './pages/FolderView.svelte';
  import { api } from './lib/api';

  // Sync theme from server config on mount
  $effect(() => {
    api.config.get().then(config => {
      if (config?.theme) {
        localStorage.setItem('iris-theme', config.theme);
        // Apply if different from what main.ts set
        if (config.theme === 'light') {
          document.documentElement.setAttribute('data-brand', 'light');
        } else if (config.theme === 'dark') {
          document.documentElement.removeAttribute('data-brand');
        } else if (config.theme === 'system') {
          const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
          if (!prefersDark) document.documentElement.setAttribute('data-brand', 'light');
          else document.documentElement.removeAttribute('data-brand');
        }
      }
    }).catch(() => {});
  });

  const routes = {
    '/': Inbox,
    '/search': Search,
    '/thread/:id': ThreadView,
    '/setup': AccountSetup,
    '/setup/*': AccountSetup,
    '/settings': Settings,
    // Cast needed: svelte-spa-router v4 types expect class-based ComponentType, but Svelte 5 uses function components
    '/sent': wrap({ component: FolderView as any, props: { folder: 'Sent', title: 'Sent' } }),
    '/drafts': wrap({ component: FolderView as any, props: { folder: 'Drafts', title: 'Drafts' } }),
    '/starred': wrap({ component: FolderView as any, props: { folder: 'Starred', title: 'Starred' } }),
    '/archive': wrap({ component: FolderView as any, props: { folder: 'Archive', title: 'Archive' } }),
    '/trash': wrap({ component: FolderView as any, props: { folder: 'Trash', title: 'Trash' } }),
  };
</script>

<AppShell>
  <Router {routes} />
</AppShell>
