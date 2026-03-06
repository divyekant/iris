import './app.css';
import App from './App.svelte';
import { mount } from 'svelte';
import { initSession } from './lib/api';

// Apply saved theme before first render to prevent flash
const savedTheme = localStorage.getItem('iris-theme') || 'dark';
if (savedTheme === 'system') {
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
  if (!prefersDark) document.documentElement.setAttribute('data-brand', 'light');
} else if (savedTheme === 'light') {
  document.documentElement.setAttribute('data-brand', 'light');
}
// Dark is the default (:root), so no attribute needed

// Bootstrap session token, then mount the app
initSession()
  .then(() => {
    mount(App, { target: document.getElementById('app')! });
  })
  .catch((err) => {
    console.error('Session bootstrap failed:', err);
    // Mount anyway — health endpoint still works, user sees error on protected routes
    mount(App, { target: document.getElementById('app')! });
  });
