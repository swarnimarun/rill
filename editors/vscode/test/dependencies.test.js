'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const { Minimatch, minimatch } = require('minimatch');

test('patched minimatch retains the language client constructor API', () => {
  const files = new Minimatch('**/*.rill', { noext: true });
  assert.equal(files.match('examples/sdl3-direct/main.rill'), true);
  assert.equal(files.match('examples/sdl3-direct/shim.c'), false);
  const insensitive = new Minimatch('**/*.RILL', { nocase: true, dot: true });
  assert.equal(insensitive.match('.local/main.rill'), true);
});

test('patched minimatch retains the VSIX packager named function API', () => {
  assert.equal(minimatch('.local/main.rill', '**/*.rill', { dot: true }), true);
  assert.equal(minimatch('node_modules/client.js', 'test/**', { dot: true }), false);
});
