import './app.css';
import App from './App.svelte';
import { mount } from 'svelte';

// Apply theme before mount to prevent flash
const isDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
document.documentElement.classList.toggle('dark', isDark);

const app = mount(App, { target: document.getElementById('app')! });
export default app;
