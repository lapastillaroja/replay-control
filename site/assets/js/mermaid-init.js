import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@11.12.0/dist/mermaid.esm.min.mjs';

(() => {
  const elements = Array.from(document.querySelectorAll('.mermaid'));
  if (!elements.length) {
    return;
  }

  elements.forEach((element) => {
    element.setAttribute('data-mermaid-src', element.textContent || '');
  });

  const resetElements = () => {
    elements.forEach((element) => {
      element.textContent = element.getAttribute('data-mermaid-src') || '';
      element.removeAttribute('data-processed');
    });
  };

  const getTheme = () => {
    const theme = document.documentElement.getAttribute('data-bs-theme');
    return theme === 'dark' ? 'dark' : 'default';
  };

  const init = (theme) => {
    mermaid.initialize({ theme, startOnLoad: false });
    mermaid.run({ nodes: elements });
  };

  init(getTheme());

  document.addEventListener('themeChanged', () => {
    resetElements();
    init(getTheme());
  });
})();
