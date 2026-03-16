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
  script.src = '/pulltorefresh.min.js';
  script.onload = function () {
    PullToRefresh.init({
      mainElement: 'body',
      distReload: 70,
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
