#!/usr/bin/env node
// Exercise the shipped search UI logic without a browser, network or DOM dependency.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../src/docgen/assets/site.js'), 'utf8');

class Element {
  constructor(tag = 'div') {
    this.tag = tag;
    this.children = [];
    this.listeners = new Map();
    this.value = '';
    this.textContent = '';
    this.hidden = false;
  }
  set innerHTML(_) { throw new Error('Search must use inert text, never innerHTML'); }
  addEventListener(name, handler) { this.listeners.set(name, handler); }
  append(...nodes) {
    for (const node of nodes) {
      this.children.push(...(node.tag === '#fragment' ? node.children : [node]));
    }
  }
  replaceChildren() { this.children = []; }
  closest() { return ['input', 'textarea', 'select'].includes(this.tag) ? this : null; }
  focus() { this.focused = true; }
  fire(name, event = {}) { this.listeners.get(name)?.(event); }
}

function reader(entries, href = 'https://docs.example/reference/search.html', blockedHistory = false) {
  const ids = Object.fromEntries([
    ['docs-query', new Element('input')],
    ['search-results', new Element('ol')],
    ['search-status', new Element('p')],
    ['search-more', new Element('button')],
  ]);
  const form = new Element('form');
  const document = new Element('document');
  document.body = { dataset: { root: '' } };
  document.querySelector = (selector) => selector === '.search-form' ? form : null;
  document.getElementById = (id) => ids[id];
  document.createElement = (tag) => new Element(tag);
  document.createDocumentFragment = () => new Element('#fragment');
  const window = new Element('window');
  let address = new URL(href);
  Object.defineProperty(window, 'location', { get: () => address });
  window.history = { replaceState(_, __, next) {
    if (blockedHistory) throw new Error('file history is restricted');
    address = new URL(next);
  } };
  window.setTimeout = (handler) => { handler(); return 1; };
  window.clearTimeout = () => {};
  window.RILL_DOCS_INDEX = entries;
  vm.runInNewContext(source, { window, document, Element, URL, URLSearchParams, console });
  return { window, document, form, ids, setAddress: (value) => { address = new URL(value); } };
}

const entry = (title, text, url = 'api/math.html#fn-double') => ({
  title, text, url, page: 'Module math', section: 'API',
});
const fixture = [
  entry('Overview', 'An integer helper can double a value.'),
  entry('fn double', 'Return twice the supplied integer, with checked arithmetic.'),
  entry('fn other', 'A different operation.'),
  entry('<script>inert</script>', 'Literal markup stays inert.'),
  entry('Café', 'Unicode documentation is searchable.'),
];
const view = reader(fixture, 'https://docs.example/reference/search.html?q=DOUBLE%20integer');
assert.equal(view.ids['search-results'].children.length, 2);
assert.equal(view.ids['search-results'].children[0].children[0].textContent, 'fn double');
assert.equal(view.ids['search-results'].children[0].children[0].href, 'api/math.html#fn-double');
assert.match(view.ids['search-status'].textContent, /2 matching sections/);
view.ids['docs-query'].value = 'no-such-symbol';
view.ids['docs-query'].fire('input');
assert.equal(view.ids['search-results'].children.length, 0);
assert.match(view.ids['search-status'].textContent, /No matching/);
assert.equal(view.window.location.searchParams.get('q'), 'no-such-symbol');
view.ids['docs-query'].value = 'inert';
view.ids['docs-query'].fire('input');
assert.equal(view.ids['search-results'].children[0].children[0].textContent, '<script>inert</script>');
view.setAddress('https://docs.example/reference/search.html?q=CAF%C3%89');
view.window.fire('popstate');
assert.equal(view.ids['search-results'].children[0].children[0].textContent, 'Café');
view.ids['docs-query'].value = '';
view.ids['docs-query'].fire('input');
assert.equal(view.ids['search-results'].children.length, 0);
assert.equal(view.window.location.searchParams.has('q'), false);
let prevented = false;
view.document.fire('keydown', { key: '/', target: new Element(), preventDefault() { prevented = true; } });
assert.equal(prevented, true);
assert.equal(view.ids['docs-query'].focused, true);
prevented = false;
view.document.fire('keydown', { key: '/', target: view.ids['docs-query'], preventDefault() { prevented = true; } });
assert.equal(prevented, false);

const many = reader(Array.from({ length: 83 }, (_, i) => entry(`helper ${i}`, 'common query')));
many.ids['docs-query'].value = 'common query';
many.ids['docs-query'].fire('input');
assert.equal(many.ids['search-results'].children.length, 40);
assert.equal(many.ids['search-more'].hidden, false);
many.ids['search-more'].fire('click');
assert.equal(many.ids['search-results'].children.length, 80);
many.ids['search-more'].fire('click');
assert.equal(many.ids['search-results'].children.length, 83);
assert.equal(many.ids['search-more'].hidden, true);

const local = reader(fixture, 'file:///docs/search.html?q=double', true);
local.ids['docs-query'].value = 'integer double';
assert.doesNotThrow(() => local.ids['docs-query'].fire('input'));
assert.equal(local.ids['search-results'].children.length, 2);
prevented = false;
local.form.fire('submit', { preventDefault() { prevented = true; } });
assert.equal(prevented, false, 'file documents retain native GET navigation');
prevented = false;
view.form.fire('submit', { preventDefault() { prevented = true; } });
assert.equal(prevented, true, 'hosted search updates without navigation');
console.log('Docs search checks passed: matching, ranking, snippets, safe text, URL, paging, Unicode, keyboard and local-file fallback.');
