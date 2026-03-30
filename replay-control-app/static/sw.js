// Service worker — app shell caching for offline support.
// Cache version is injected at compile time via main.rs (replaces __CACHE_VERSION__).
const CACHE_NAME = 'shell-__CACHE_VERSION__';

const SHELL_ASSETS = [
  '/static/style.css',
  '/static/pkg/replay_control_app.js',
  '/static/pkg/replay_control_app_bg.wasm',
  '/static/ptr-init.js',
  '/static/pulltorefresh.min.js',
  '/static/manifest.json',
  '/static/icons/icon-192.png',
  '/static/icons/icon-512.png',
];

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME)
      .then((cache) => cache.addAll(SHELL_ASSETS))
      .then(() => self.skipWaiting())
  );
});

self.addEventListener('activate', (event) => {
  // Delete old cache versions.
  event.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(
        keys
          .filter((key) => key.startsWith('shell-') && key !== CACHE_NAME)
          .map((key) => caches.delete(key))
      ))
      .then(() => self.clients.claim())
  );
});

self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url);

  // Static assets: cache-first.
  if (url.pathname.startsWith('/static/')) {
    event.respondWith(
      caches.match(event.request).then((cached) => cached || fetch(event.request))
    );
    return;
  }

  // Navigation requests: network-first, offline fallback.
  if (event.request.mode === 'navigate') {
    event.respondWith(
      fetch(event.request).catch(() =>
        new Response(
          '<!DOCTYPE html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Offline</title><style>body{font-family:system-ui;display:flex;justify-content:center;align-items:center;min-height:100vh;margin:0;background:#0f1115;color:#e5e7eb}div{text-align:center}h1{font-size:1.5rem;margin-bottom:.5rem}p{color:#9ca3af}</style></head><body><div><h1>You\'re offline</h1><p>Check your connection to the RePlayOS device.</p></div></body></html>',
          { status: 503, headers: { 'Content-Type': 'text/html' } }
        )
      )
    );
    return;
  }

  // Everything else (API calls, etc.): network only.
  event.respondWith(fetch(event.request));
});
