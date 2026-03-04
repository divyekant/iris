import './app.css';
import App from './App.svelte';
import { mount } from 'svelte';
import { initSession } from './lib/api';

// Apply theme before mount to prevent flash
const isDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
document.documentElement.classList.toggle('dark', isDark);

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
