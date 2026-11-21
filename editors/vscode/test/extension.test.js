'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');

const root = path.join(__dirname, '..');

function extension(serverPath = 'rill') {
  const clients = [];
  class LanguageClient {
    constructor(...args) {
      this.args = args;
      this.starts = 0;
      this.stops = 0;
      clients.push(this);
    }
    async start() { this.starts += 1; }
    async stop() { this.stops += 1; }
  }
  const module = { exports: {} };
  vm.runInNewContext(fs.readFileSync(path.join(root, 'extension.js'), 'utf8'), {
    module,
    exports: module.exports,
    require(name) {
      if (name === 'vscode') {
        return { workspace: { getConfiguration: () => ({ get: () => serverPath }) } };
      }
      assert.equal(name, 'vscode-languageclient/node');
      return { LanguageClient, TransportKind: { stdio: 0 } };
    },
  });
  return { ...module.exports, clients };
}

test('launches the configured executable with a literal stdio argument array', async () => {
  const client = extension('/a path with spaces/rill;literal');
  await client.activate();
  assert.equal(client.clients.length, 1);
  const running = client.clients[0];
  assert.equal(running.starts, 1);
  assert.deepEqual(JSON.parse(JSON.stringify(running.args[2])), {
    command: '/a path with spaces/rill;literal',
    args: ['lsp'],
    transport: 0,
  });
  assert.deepEqual(JSON.parse(JSON.stringify(running.args[3].documentSelector)), [
    { scheme: 'file', language: 'rill' },
    { scheme: 'untitled', language: 'rill' },
  ]);
  await client.deactivate();
  await client.deactivate();
  assert.equal(running.stops, 1);
});

test('uses rill on PATH by default and rejects an empty executable', async () => {
  const defaults = extension();
  await defaults.activate();
  assert.equal(defaults.clients[0].args[2].command, 'rill');
  await defaults.deactivate();
  for (const value of ['', '   ', 4]) {
    const invalid = extension(value);
    await assert.rejects(invalid.activate(), /must name a compiler executable/);
    assert.equal(invalid.clients.length, 0);
  }
});

test('manifest includes working language, configuration and TextMate paths', () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  assert.equal(manifest.dependencies['vscode-languageclient'], '11.0.0');
  assert.equal(manifest.overrides.minimatch, '11.0.1');
  assert.equal(manifest.capabilities.untrustedWorkspaces.supported, false);
  const language = manifest.contributes.languages.find(item => item.id === 'rill');
  assert.deepEqual(language.extensions, ['.rill']);
  const grammar = manifest.contributes.grammars.find(item => item.language === 'rill');
  for (const relative of [manifest.main, language.configuration, grammar.path]) {
    assert.ok(fs.statSync(path.join(root, relative)).isFile());
  }
  const config = JSON.parse(fs.readFileSync(path.join(root, language.configuration), 'utf8'));
  assert.equal(config.comments.lineComment, '//');
  assert.ok(!config.brackets.some(pair => pair[0] === '<'));
});
