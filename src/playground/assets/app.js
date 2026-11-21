(() => {
  'use strict';

  const el = (id) => document.getElementById(id);
  const source = el('source');
  const view = el('view');
  const entry = el('entry');
  const inputs = el('inputs');
  const output = el('output');
  const run = el('run');
  const reset = el('reset');
  const copy = el('copy');
  const kernelViews = new Set(['rir', 'ir-eval', 'nvptx', 'vulkan']);
  const validViews = new Set(Array.from(view.options, (option) => option.value));
  const drafts = new Map();
  let lessons = [];
  let lesson = null;
  let request = null;
  let copyTimer = null;

  function status(text, state) {
    el('output-status').textContent = text;
    el('output-status').dataset.state = state;
    output.setAttribute('aria-busy', String(state === 'busy'));
  }

  function clearCopy() {
    window.clearTimeout(copyTimer);
    copy.textContent = 'Copy';
    copy.disabled = !output.textContent;
  }

  function clearRequest() {
    if (request) request.abort();
    request = null;
  }

  function displaySource() {
    const text = source.value;
    const highlight = el('highlight');
    const lineCount = text.split('\n').length;
    el('numbers').textContent = Array.from({ length: lineCount }, (_, i) => i + 1).join('\n');
    // Colour is a reading aid. The Tokens view invokes Rill's actual lexer.
    if (text.length > 32768) {
      highlight.textContent = text + '\n';
    } else {
      const fragment = document.createDocumentFragment();
      const tokens = /(\/\/[^\n]*|"[^"\\]*(?:\\.[^"\\]*)*"|\b(?:fn|let|mut|read|inout|if|else|while|match|type|export|import|extern|then|in|do)\b|\b(?:i32|bool|unit|bytes|str|u8x16)\b|\b(?:true|false|[0-9]+)\b)/g;
      let end = 0;
      for (const token of text.matchAll(tokens)) {
        fragment.append(text.slice(end, token.index));
        const part = document.createElement('span');
        const word = token[0];
        if (word.startsWith('//')) part.className = 'syntax-comment';
        else if (/^(?:"|[0-9]|true$|false$)/.test(word)) part.className = 'syntax-literal';
        else if (/^(?:i32|bool|unit|bytes|str|u8x16)$/.test(word)) part.className = 'syntax-type';
        else part.className = 'syntax-keyword';
        part.textContent = word;
        fragment.append(part);
        end = token.index + word.length;
      }
      fragment.append(text.slice(end) + '\n');
      highlight.replaceChildren(fragment);
    }
    syncScroll();
    cursor();
  }

  function syncScroll() {
    el('highlight').scrollTop = source.scrollTop;
    el('highlight').scrollLeft = source.scrollLeft;
    el('numbers').scrollTop = source.scrollTop;
  }

  function cursor() {
    const pos = source.selectionDirection === 'backward' ? source.selectionStart : source.selectionEnd;
    const lines = source.value.slice(0, pos).split('\n');
    el('cursor').textContent = 'Ln ' + lines.length + ', Col ' + (Array.from(lines[lines.length - 1]).length + 1);
  }

  function defaults(item) {
    return {
      source: item.source,
      view: validViews.has(item.view) ? item.view : 'run',
      entry: item.entry ?? 'main',
      inputs: (item.inputs ?? []).join(', '),
    };
  }

  function draftKey(id) { return 'rill.playground.draft.v1.' + id; }

  function loadDraft(item) {
    const original = defaults(item);
    let saved = drafts.get(item.id);
    if (!saved) {
      try { saved = JSON.parse(localStorage.getItem(draftKey(item.id))); }
      catch (_) { /* This browser may disable local storage. */ }
    }
    if (!saved || typeof saved !== 'object') return original;
    for (const key of ['source', 'entry', 'inputs']) {
      if (typeof saved[key] === 'string') original[key] = saved[key];
    }
    if (validViews.has(saved.view)) original.view = saved.view;
    return original;
  }

  function saveDraft() {
    if (!lesson) return;
    const original = defaults(lesson);
    const current = { source: source.value, view: view.value, entry: entry.value, inputs: inputs.value };
    const changes = Object.fromEntries(Object.entries(current).filter(([key, value]) => value !== original[key]));
    const changed = Object.keys(changes).length > 0;
    reset.disabled = !changed;
    if (changed) drafts.set(lesson.id, changes);
    else drafts.delete(lesson.id);
    try {
      if (changed) localStorage.setItem(draftKey(lesson.id), JSON.stringify(changes));
      else localStorage.removeItem(draftKey(lesson.id));
    } catch (_) { /* In-memory drafts still survive lesson navigation. */ }
    el('source-status').textContent = source.value === original.source ? 'Lesson source' : 'Edited source';
  }

  function edited() {
    clearRequest();
    saveDraft();
    status('Changes pending', 'stale');
  }

  function updateControls() {
    const kernel = kernelViews.has(view.value);
    el('view-help').textContent = view.selectedOptions[0].dataset.help;
    el('kernel-controls').hidden = !kernel;
    el('inputs-control').hidden = view.value !== 'ir-eval';
    el('run-label').textContent = view.value === 'run' || view.value === 'ir-eval' ? 'Run' : 'Inspect';
  }

  function lessonHref(item) {
    const url = new URL(location.href);
    url.searchParams.set('lesson', item.id);
    return url.pathname + url.search;
  }

  function showList() {
    const search = el('filter').value.trim().toLowerCase();
    const groups = new Map();
    for (const item of lessons) {
      if (!(item.title + ' ' + item.topic).toLowerCase().includes(search)) continue;
      if (!groups.has(item.topic)) groups.set(item.topic, []);
      groups.get(item.topic).push(item);
    }
    const fragment = document.createDocumentFragment();
    for (const [name, items] of groups) {
      const group = document.createElement('div');
      group.className = 'lesson-group';
      const heading = document.createElement('h3');
      heading.textContent = name;
      const list = document.createElement('ul');
      for (const item of items) {
        const li = document.createElement('li');
        const anchor = document.createElement('a');
        anchor.textContent = item.title;
        anchor.href = lessonHref(item);
        anchor.dataset.lesson = item.id;
        if (item.id === lesson?.id) anchor.setAttribute('aria-current', 'page');
        li.append(anchor);
        list.append(li);
      }
      group.append(heading, list);
      fragment.append(group);
    }
    el('lesson-list').replaceChildren(fragment);
    el('filter-empty').hidden = groups.size !== 0;
  }

  function pager(link, item, direction) {
    link.hidden = !item;
    if (!item) return;
    link.dataset.lesson = item.id;
    link.href = lessonHref(item);
    link.textContent = direction === 'next' ? item.title + ' →' : '← ' + item.title;
    link.setAttribute('aria-label', (direction === 'next' ? 'Next lesson, ' : 'Previous lesson, ') + item.title);
  }

  function setMenu(open) {
    el('library').classList.toggle('open', open);
    el('toggle-lessons').setAttribute('aria-expanded', String(open));
  }

  function selectLesson(item, push = false) {
    clearRequest();
    lesson = item;
    const current = loadDraft(item);
    source.value = current.source;
    view.value = current.view;
    entry.value = current.entry;
    inputs.value = current.inputs;
    document.title = item.title + ' · Rill explorer';
    el('lesson-title').textContent = item.title;
    el('lesson-context').textContent = item.topic;
    el('description').textContent = item.description;
    el('source-file').textContent = item.file;
    el('exercise').textContent = item.exercise || '';
    el('exercise-panel').hidden = !item.exercise;
    el('retry-lessons').hidden = true;
    const index = lessons.indexOf(item);
    pager(el('previous'), lessons[index - 1], 'prev');
    pager(el('next'), lessons[index + 1], 'next');
    el('lesson-pager').hidden = lessons.length < 2;
    source.disabled = view.disabled = run.disabled = false;
    source.scrollTop = source.scrollLeft = 0;
    source.setSelectionRange(0, 0);
    displaySource();
    saveDraft();
    updateControls();
    // Keep the selected link focused for keyboard navigation.
    for (const link of el('lesson-list').querySelectorAll('a')) {
      if (link.dataset.lesson === item.id) link.setAttribute('aria-current', 'page');
      else link.removeAttribute('aria-current');
    }
    setMenu(false);
    if (push) history.pushState(null, '', lessonHref(item));
    inspect();
  }

  function readInputs() {
    if (!inputs.value.trim()) return [];
    const values = inputs.value.trim().split(/[\s,]+/);
    return values.map((value) => {
      const n = Number(value);
      if (!/^[+-]?\d+$/.test(value) || !Number.isInteger(n) || n < -2147483648 || n > 2147483647) {
        throw new Error('Enter signed i32 values separated by commas or spaces.');
      }
      return n;
    });
  }

  async function inspect() {
    if (!lesson) return;
    clearRequest();
    const controller = new AbortController();
    request = controller;
    output.textContent = '';
    output.removeAttribute('data-error');
    clearCopy();
    status('Working…', 'busy');
    try {
      const body = { source: source.value, view: view.value };
      if (kernelViews.has(view.value)) {
        if (!entry.value.trim()) throw new Error('Enter a kernel entry function.');
        body.entry = entry.value.trim();
      }
      if (view.value === 'ir-eval') body.inputs = readInputs();
      const response = await fetch('/api/inspect', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        signal: controller.signal,
      });
      const data = await response.json();
      if (request !== controller) return;
      if (!response.ok || data.ok !== true) throw new Error(data.diagnostics || 'The compiler request failed.');
      if (typeof data.output !== 'string') throw new Error('The compiler returned an unreadable response.');
      output.textContent = data.output;
      status('Completed', 'done');
    } catch (error) {
      if (request !== controller) return;
      output.dataset.error = 'true';
      output.textContent = error instanceof Error ? error.message : String(error);
      status('Diagnostic', 'error');
    } finally {
      if (request === controller) {
        request = null;
        clearCopy();
        output.scrollTop = output.scrollLeft = 0;
      }
    }
  }

  function openLocation() {
    const id = new URL(location.href).searchParams.get('lesson');
    const item = lessons.find((candidate) => candidate.id === id) || lessons[0];
    if (item) selectLesson(item);
  }

  async function loadLessons() {
    el('retry-lessons').hidden = true;
    try {
      const response = await fetch('/api/lessons');
      if (!response.ok) throw new Error('Failed to load the lesson library.');
      const data = await response.json();
      if (!Array.isArray(data.lessons) || !data.lessons.length) throw new Error('The lesson library is empty.');
      lessons = data.lessons;
      el('lesson-count').textContent = String(lessons.length);
      showList();
      openLocation();
    } catch (error) {
      el('description').textContent = String(error.message || error);
      el('retry-lessons').hidden = false;
      status('Lesson library unavailable', 'error');
    }
  }

  document.addEventListener('click', (event) => {
    const link = event.target.closest('a[data-lesson]');
    if (!link || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey || event.button !== 0) return;
    const item = lessons.find((candidate) => candidate.id === link.dataset.lesson);
    if (!item) return;
    event.preventDefault();
    if (item !== lesson) selectLesson(item, true);
    else setMenu(false);
    if (getComputedStyle(el('toggle-lessons')).display !== 'none') {
      el('lesson-title').focus({ preventScroll: true });
      el('lesson-title').scrollIntoView({ block: 'nearest' });
    }
  });
  document.addEventListener('keydown', (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
      event.preventDefault();
      inspect();
    }
    if (event.key === 'Escape' && el('library').classList.contains('open')) {
      setMenu(false);
      el('toggle-lessons').focus();
    }
  });
  window.addEventListener('popstate', openLocation);
  el('filter').addEventListener('input', showList);
  el('toggle-lessons').addEventListener('click', () => setMenu(!el('library').classList.contains('open')));
  el('retry-lessons').addEventListener('click', loadLessons);
  source.addEventListener('input', () => { displaySource(); edited(); });
  source.addEventListener('scroll', syncScroll);
  for (const event of ['click', 'select', 'keyup']) source.addEventListener(event, cursor);
  for (const field of [entry, inputs]) field.addEventListener('input', edited);
  view.addEventListener('change', () => { updateControls(); saveDraft(); inspect(); });
  run.addEventListener('click', inspect);
  reset.addEventListener('click', () => {
    if (source.value !== lesson.source && !window.confirm('Restore this lesson’s original source and settings?')) return;
    drafts.delete(lesson.id);
    try { localStorage.removeItem(draftKey(lesson.id)); } catch (_) { /* storage may be disabled */ }
    selectLesson(lesson);
  });
  copy.addEventListener('click', async () => {
    const text = output.textContent;
    try {
      await navigator.clipboard.writeText(text);
      if (text !== output.textContent) return;
      copy.textContent = 'Copied';
    } catch (_) {
      if (text !== output.textContent) return;
      // Retain a keyboard-copy fallback when the browser denies clipboard access.
      const range = document.createRange();
      range.selectNodeContents(output);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      output.focus();
      copy.textContent = 'Selected';
    }
    window.clearTimeout(copyTimer);
    copyTimer = window.setTimeout(clearCopy, 1500);
  });
  if (/Mac|iPhone|iPad/.test(navigator.platform)) el('run-shortcut').textContent = '⌘ ↵';
  loadLessons();
})();
