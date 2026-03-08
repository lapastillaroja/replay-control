// Minimal service worker for PWA installability.
// No offline caching — just registers so browsers treat the app as installable.

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (event) => event.waitUntil(self.clients.claim()));
