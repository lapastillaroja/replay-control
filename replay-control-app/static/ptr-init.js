// Pull-to-refresh bootstrap — iOS standalone mode only.
// Detects iOS + standalone PWA and lazy-loads PullToRefresh.js.
(function () {
  // Detect iOS (iPhone, iPad, iPod — including iPadOS 13+ reporting as Mac)
  var ua = navigator.userAgent || '';
  var isIOS = /ipad|iphone|ipod/i.test(ua) ||
    (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);

  if (!isIOS) return;

  // Detect standalone (installed PWA) mode
  var isStandalone = window.navigator.standalone ||
    window.matchMedia('(display-mode: standalone)').matches;

  if (!isStandalone) return;

  // Add class for CSS targeting (overscroll-behavior, PTR indicator theme)
  document.documentElement.classList.add('is-ios-standalone');

  // Lazy-load PullToRefresh.js
  var script = document.createElement('script');
  script.src = '/static/pulltorefresh.min.js';
  script.onload = function () {
    // Read safe-area-inset-top for Dynamic Island offset.
    // The .app element has padding-top: env(safe-area-inset-top).
    var appEl = document.querySelector('.app');
    var safeTop = appEl ? parseInt(getComputedStyle(appEl).paddingTop) || 0 : 0;

    PullToRefresh.init({
      mainElement: 'body',
      distIgnore: safeTop,
      distThreshold: 60,
      distMax: 80 + safeTop,
      distReload: 70 + safeTop,
      instructionsPullToRefresh: ' ',
      instructionsReleaseToRefresh: ' ',
      instructionsRefreshing: ' ',
      iconArrow: '&#8675;',
      iconRefreshing: '&#8635;',
      onRefresh: function () {
        window.location.reload();
      }
    });
  };
  document.head.appendChild(script);
})();
