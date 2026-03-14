// Pull-to-refresh for PWA standalone mode.
// Attaches to .content element, shows a spinner indicator, reloads on release.
(function () {
  var THRESHOLD = 80;
  var MAX_PULL = 120;
  var el, indicator, startY, pulling, dist;

  function init() {
    el = document.querySelector('.content');
    if (!el) return;

    indicator = document.createElement('div');
    indicator.className = 'ptr-indicator';
    indicator.innerHTML = '<div class="ptr-spinner"></div>';
    el.parentNode.insertBefore(indicator, el);

    el.addEventListener('touchstart', onStart, { passive: true });
    el.addEventListener('touchmove', onMove, { passive: false });
    el.addEventListener('touchend', onEnd, { passive: true });
  }

  function onStart(e) {
    if (el.scrollTop > 0) return;
    startY = e.touches[0].clientY;
    pulling = true;
    dist = 0;
    indicator.classList.remove('ptr-loading');
  }

  function onMove(e) {
    if (!pulling) return;
    var y = e.touches[0].clientY - startY;
    if (y < 0) { dist = 0; return; }
    // Only activate if content is scrolled to top
    if (el.scrollTop > 0) { pulling = false; dist = 0; update(); return; }
    e.preventDefault();
    dist = Math.min(y * 0.5, MAX_PULL);
    update();
  }

  function onEnd() {
    if (!pulling) return;
    pulling = false;
    if (dist >= THRESHOLD) {
      indicator.classList.add('ptr-loading');
      indicator.style.height = '48px';
      indicator.style.opacity = '1';
      location.reload();
    } else {
      update();
    }
    dist = 0;
  }

  function update() {
    var h = Math.max(0, dist);
    var pct = Math.min(dist / THRESHOLD, 1);
    indicator.style.height = h + 'px';
    indicator.style.opacity = pct;
    var spinner = indicator.firstChild;
    if (spinner) spinner.style.transform = 'rotate(' + (pct * 360) + 'deg)';
  }

  // Init after DOM ready or hydration.
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
