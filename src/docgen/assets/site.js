(() => {
  'use strict';
  const form = document.querySelector('.search-form');
  const input = document.getElementById('docs-query');
  if (!form || !input) return;
  document.addEventListener('keydown', (event) => {
    const editing = event.target instanceof Element && event.target.closest('input, textarea, select, [contenteditable="true"]');
    if (event.key === '/' && !editing && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault();
      input.focus();
    }
  });
  const results = document.getElementById('search-results');
  const status = document.getElementById('search-status');
  const more = document.getElementById('search-more');
  if (!results || !status || !more) return;
  const root = document.body.dataset.root || '';
  const entries = (window.RILL_DOCS_INDEX || []).map((entry) => ({
    ...entry,
    haystack: `${entry.title} ${entry.page} ${entry.text}`.toLowerCase(),
    titleLower: entry.title.toLowerCase(),
    pageLower: entry.page.toLowerCase(),
  }));
  let matches = [];
  let shown = 0;
  let terms = [];
  let timer;

  const snippet = (text) => {
    const lower = text.toLowerCase();
    let position = terms.map((term) => lower.indexOf(term)).filter((value) => value >= 0).sort((a, b) => a - b)[0] || 0;
    position = Math.max(0, position - 70);
    return `${position ? '…' : ''}${text.slice(position, position + 240)}${text.length > position + 240 ? '…' : ''}`;
  };
  const append = () => {
    const next = Math.min(shown + 40, matches.length);
    const fragment = document.createDocumentFragment();
    for (const { entry } of matches.slice(shown, next)) {
      const item = document.createElement('li');
      const anchor = document.createElement('a');
      anchor.href = root + entry.url;
      anchor.textContent = entry.title;
      const meta = document.createElement('p');
      meta.className = 'result-meta';
      meta.textContent = `${entry.section} / ${entry.page}`;
      const text = document.createElement('p');
      text.className = 'result-snippet';
      text.textContent = snippet(entry.text);
      item.append(anchor, meta, text);
      fragment.append(item);
    }
    results.append(fragment);
    shown = next;
    more.hidden = shown >= matches.length;
    status.textContent = matches.length
      ? `${matches.length} matching ${matches.length === 1 ? 'section' : 'sections'}${shown < matches.length ? `; showing ${shown}` : ''}.`
      : 'No matching sections. Try another term or a shorter query.';
  };
  const search = (updateAddress) => {
    const query = input.value.trim().slice(0, 200);
    terms = [...new Set(query.toLowerCase().split(/\s+/).filter(Boolean))].slice(0, 12);
    results.replaceChildren();
    more.hidden = true;
    shown = 0;
    if (updateAddress) {
      const url = new URL(window.location.href);
      if (query) url.searchParams.set('q', query);
      else url.searchParams.delete('q');
      // Some browsers restrict history updates for file:// documents. Native
      // GET submission below still produces an addressable query in that case.
      try { window.history.replaceState(null, '', url); } catch (_) { /* keep live search */ }
    }
    if (!terms.length) {
      status.textContent = 'Enter a term in the search field.';
      matches = [];
      return;
    }
    matches = entries.filter((entry) => terms.every((term) => entry.haystack.includes(term)))
      .map((entry, index) => ({ entry, index, score: terms.reduce((score, term) => score
        + (entry.titleLower === term ? 80 : entry.titleLower.startsWith(term) ? 40 : entry.titleLower.includes(term) ? 20 : 0)
        + (entry.pageLower.includes(term) ? 10 : 0), 0) }))
      .sort((a, b) => b.score - a.score || a.index - b.index);
    append();
  };
  form.addEventListener('submit', (event) => {
    if (window.location.protocol === 'file:') return;
    event.preventDefault();
    window.clearTimeout(timer);
    search(true);
  });
  input.addEventListener('input', () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => search(true), 120);
  });
  more.addEventListener('click', append);
  window.addEventListener('popstate', () => {
    input.value = (new URLSearchParams(window.location.search).get('q') || '').slice(0, 200);
    search(false);
  });
  input.value = (new URLSearchParams(window.location.search).get('q') || '').slice(0, 200);
  search(false);
})();
