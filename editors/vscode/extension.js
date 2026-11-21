'use strict';

const vscode = require('vscode');
const { LanguageClient, TransportKind } = require('vscode-languageclient/node');

let client;

async function activate() {
  const executable = vscode.workspace.getConfiguration('rill').get('serverPath', 'rill');
  if (typeof executable !== 'string' || executable.trim().length === 0) {
    throw new Error('rill.serverPath must name a compiler executable');
  }
  client = new LanguageClient(
    'rill',
    'Rill Language Server',
    { command: executable, args: ['lsp'], transport: TransportKind.stdio },
    { documentSelector: [
      { scheme: 'file', language: 'rill' },
      { scheme: 'untitled', language: 'rill' },
    ] },
  );
  await client.start();
}

async function deactivate() {
  const active = client;
  client = undefined;
  if (active) {
    await active.stop();
  }
}

module.exports = { activate, deactivate };
