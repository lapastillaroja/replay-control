const pathInput = document.getElementById('path');
const current = document.getElementById('current');
const browser = document.getElementById('browser');
const browserPath = document.getElementById('browser-path');
const directories = document.getElementById('directories');
const parentButton = document.getElementById('parent');
const preview = document.getElementById('preview');
const message = document.getElementById('message');
const selectButton = document.getElementById('select');
const english = {
  locale: 'en',
  current: 'Current storage: {0}',
  none: 'No storage location selected yet.',
  unavailable: 'The configured storage is unavailable: {0}',
  found: 'Storage root found. Replay Control will read games from {0}/roms/.',
  invalid: 'This folder does not contain a readable roms/ directory.',
  browse_failed: 'Could not browse folders.',
  switching: 'Switching storage and checking the new library…',
  switch_timeout: 'The switch is taking longer than expected. Check replay-control.service on this computer, then refresh this page.',
  switch_failed: 'The storage switch did not finish.',
  could_not_switch: 'Could not switch storage.',
  select_directory: 'Select a directory.',
  already_selected: 'This folder is already selected.',
  invalid_path: 'The path contains an invalid character.',
  switch_busy: 'A storage switch is already in progress. Try again after it finishes.',
  validation_required: 'This folder must contain a readable roms/ directory.',
  switch_restored: 'The selected folder could not start Replay Control. The previous storage selection was restored.',
  switch_interrupted: 'The storage switch stopped before reporting a result. Restart Replay Control to restore the previous selection.',
  switch_start_failed: 'Could not start the storage switch.',
  package_only: 'Storage switching is available in packaged installs.'
};
let messages = english;
let selectedPath = '';
let parentPath = null;
let switching = false;
let browseGeneration = 0;
let firstRun = false;
const firstRunKey = 'replay-control-first-storage-selection';

function tr(key, ...values) {
  return values.reduce((value, argument, index) => value.replace(`{${index}}`, argument), messages[key]);
}

async function loadMessages() {
  try {
    const response = await fetch('/storage-setup/messages', { cache: 'no-store' });
    if (response.ok) messages = { ...english, ...await response.json() };
  } catch (_) {
    // English remains available when the service is restarting.
  }
  document.documentElement.lang = messages.locale;
  document.querySelectorAll('[data-i18n]').forEach(node => {
    if (messages[node.dataset.i18n]) node.textContent = messages[node.dataset.i18n];
  });
  if (messages.title) document.title = messages.title + ' · Replay Control';
}

function localizeError(raw) {
  const known = [
    'invalid', 'browse_failed', 'switch_failed', 'could_not_switch',
    'select_directory', 'already_selected', 'invalid_path', 'switch_busy',
    'validation_required', 'switch_restored', 'switch_interrupted',
    'switch_start_failed', 'package_only'
  ];
  const key = known.find(candidate => english[candidate] === raw);
  return key ? messages[key] : raw;
}

function firstRunPending() {
  try { return sessionStorage.getItem(firstRunKey) === '1'; } catch (_) { return false; }
}

function markFirstRunPending(pending) {
  try {
    if (pending) sessionStorage.setItem(firstRunKey, '1');
    else sessionStorage.removeItem(firstRunKey);
  } catch (_) { /* Selection still works without session storage. */ }
}

function say(text, error = false) {
  message.textContent = text;
  message.className = error ? 'error' : '';
}

function showPreview(info) {
  preview.replaceChildren();
  selectedPath = info.valid ? info.path : '';
  selectButton.disabled = !info.valid || switching;
  if (info.error) {
    say(localizeError(info.error), true);
    return;
  }
  const summary = document.createElement('p');
  summary.textContent = info.valid
    ? tr('found', info.path)
    : messages.invalid;
  preview.append(summary);
  const list = document.createElement('div');
  list.className = 'folder-list';
  for (const name of ['roms', 'bios', 'saves', 'captures', 'config', 'skins']) {
    const chip = document.createElement('span');
    chip.textContent = name + '/';
    chip.className = info.folders.includes(name) ? 'folder-present' : 'folder-missing';
    list.append(chip);
  }
  preview.append(list);
  say('');
}

async function openDirectory(path, showBrowser = true) {
  const generation = ++browseGeneration;
  selectedPath = '';
  selectButton.disabled = true;
  const response = await fetch('/storage-setup/list?path=' + encodeURIComponent(path));
  if (!response.ok) throw new Error(messages.browse_failed);
  const info = await response.json();
  if (generation !== browseGeneration) return;
  if (info.error) {
    showPreview(info);
    return;
  }
  pathInput.value = info.path;
  browser.hidden = !showBrowser;
  browserPath.textContent = info.path;
  parentPath = info.parent;
  parentButton.disabled = !parentPath;
  directories.replaceChildren();
  for (const name of info.directories) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'directory';
    button.textContent = '📁 ' + name;
    button.addEventListener('click', () => openDirectory(info.path.replace(/\/$/, '') + '/' + name));
    directories.append(button);
  }
  showPreview(info);
}

document.getElementById('browse').addEventListener('click', () => {
  if (messages.portable) {
    pickDirectory().catch(error => say(error.message, true));
    return;
  }
  openDirectory(pathInput.value || '/').catch(error => say(error.message, true));
});

async function pickDirectory() {
  const response = await fetch('/storage-setup/pick', {
    method: 'POST',
    headers: { 'x-replay-control-portal': 'pick-storage' }
  });
  if (response.status === 204) return;
  if (!response.ok) throw new Error(await response.text());
  const info = await response.json();
  pathInput.value = info.path;
  browser.hidden = true;
  showPreview(info);
}
parentButton.addEventListener('click', () => {
  if (parentPath) openDirectory(parentPath).catch(error => say(error.message, true));
});
pathInput.addEventListener('change', () => {
  openDirectory(pathInput.value, false).catch(error => say(error.message, true));
});
pathInput.addEventListener('input', () => {
  ++browseGeneration;
  selectedPath = '';
  selectButton.disabled = true;
});

async function pollSwitch() {
  const deadline = Date.now() + 300000;
  while (switching) {
    if (Date.now() > deadline) {
      switching = false;
      say(messages.switch_timeout, true);
      break;
    }
    await new Promise(resolve => setTimeout(resolve, 1200));
    try {
      const response = await fetch('/storage-setup/status', { cache: 'no-store' });
      const result = await response.json();
      if (result.status === 'error' || result.status === 'delayed') {
        switching = false;
        markFirstRunPending(false);
        selectButton.disabled = false;
        say(result.message ? localizeError(result.message) : messages.switch_failed, true);
      } else if (result.status === 'ok') {
        const health = await fetch('/api/core/status', { cache: 'no-store' });
        if (health.ok) {
          const destination = firstRun ? '/?setup' : '/';
          markFirstRunPending(false);
          window.location.assign(destination);
        }
      }
    } catch (_) {
      // The service is restarting; keep polling the same origin.
    }
  }
}

selectButton.addEventListener('click', async () => {
  if (!selectedPath || selectedPath !== pathInput.value || switching) return;
  switching = true;
  selectButton.disabled = true;
  say(messages.switching);
  if (firstRun) markFirstRunPending(true);
  try {
    const response = await fetch('/storage-setup/select', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ path: selectedPath })
    });
    if (response.status === 202) {
      pollSwitch();
      return;
    }
    if (response.ok) {
      switching = false;
      markFirstRunPending(false);
      selectButton.disabled = false;
      say(localizeError(await response.text()));
      return;
    }
    throw new Error(localizeError(await response.text()));
  } catch (error) {
    switching = false;
    markFirstRunPending(false);
    selectButton.disabled = false;
    say(error.message || messages.could_not_switch, true);
  }
});

loadMessages().then(() => {
  pathInput.readOnly = !!messages.portable;
  return fetch('/storage-setup/status', { cache: 'no-store' });
})
  .then(response => response.json())
  .then(result => {
    if (result.status === 'error' || result.status === 'delayed') markFirstRunPending(false);
    firstRun = !result.current || firstRunPending();
    document.getElementById('back').hidden = firstRun || !!result.storage_error;
    document.getElementById('setup-step').hidden = !firstRun;
    document.getElementById('setup-next').hidden = !firstRun;
    current.textContent = result.current
      ? tr('current', result.current)
      : messages.none;
    const problem = document.getElementById('storage-problem');
    problem.hidden = !result.storage_error;
    problem.textContent = result.storage_error
      ? tr('unavailable', localizeError(result.storage_error))
      : '';
    if (result.status === 'running' || (result.status === 'ok' && firstRunPending())) {
      switching = true;
      say(messages.switching);
      pollSwitch();
    }
    if (messages.portable) {
      pathInput.value = result.current || '';
      return;
    }
    return openDirectory(result.current || '/', false);
  })
  .catch(error => {
    if (messages.portable) say(error.message, true);
    else openDirectory('/', false).catch(browserError => say(browserError.message, true));
  });
