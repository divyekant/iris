<script lang="ts">
  import './app.css';
  import Router from 'svelte-spa-router';
  import AppShell from './components/AppShell.svelte';
  import Inbox from './pages/Inbox.svelte';
  import ThreadView from './pages/ThreadView.svelte';
  import AccountSetup from './pages/AccountSetup.svelte';
  import Settings from './pages/Settings.svelte';
  import Search from './pages/Search.svelte';

  const routes = {
    '/': Inbox,
    '/search': Search,
    '/thread/:id': ThreadView,
    '/setup': AccountSetup,
    '/setup/*': AccountSetup,
    '/settings': Settings,
  };
</script>

<AppShell>
  <Router {routes} />
</AppShell>
