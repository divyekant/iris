// Desktop notification utilities for Iris email client.
// Wraps the browser Notification API with permission management and localStorage toggle.

let permissionGranted = false;

export async function requestNotificationPermission(): Promise<boolean> {
  if (!('Notification' in window)) return false;
  if (Notification.permission === 'granted') {
    permissionGranted = true;
    return true;
  }
  if (Notification.permission === 'denied') return false;
  const result = await Notification.requestPermission();
  permissionGranted = result === 'granted';
  return permissionGranted;
}

export function isEnabled(): boolean {
  return localStorage.getItem('iris-notifications') !== 'false' && permissionGranted;
}

export function setEnabled(enabled: boolean) {
  localStorage.setItem('iris-notifications', String(enabled));
}

export function getPermissionState(): string {
  if (!('Notification' in window)) return 'unsupported';
  return Notification.permission; // 'granted' | 'denied' | 'default'
}

export function showNotification(title: string, options?: NotificationOptions) {
  if (!isEnabled()) return;
  try {
    const n = new Notification(title, {
      icon: '/favicon.ico',
      badge: '/favicon.ico',
      ...options,
    });
    n.onclick = () => {
      window.focus();
      n.close();
    };
    // Auto-dismiss after 5s
    setTimeout(() => n.close(), 5000);
  } catch { /* ignore — may fail in insecure contexts */ }
}

export function init() {
  if (localStorage.getItem('iris-notifications') !== 'false') {
    requestNotificationPermission();
  }
}
