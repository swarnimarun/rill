'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const textmate = require('vscode-textmate');
const oniguruma = require('vscode-oniguruma');

async function loadGrammar() {
  const binary = fs.readFileSync(require.resolve('vscode-oniguruma/release/onig.wasm'));
  await oniguruma.loadWASM(binary.buffer.slice(binary.byteOffset, binary.byteOffset + binary.byteLength));
  const registry = new textmate.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: patterns => new oniguruma.OnigScanner(patterns),
      createOnigString: value => new oniguruma.OnigString(value),
    }),
    async loadGrammar(scopeName) {
      assert.equal(scopeName, 'source.rill');
      return JSON.parse(fs.readFileSync(path.join(__dirname, '../syntaxes/rill.tmLanguage.json'), 'utf8'));
    },
  });
  return registry.loadGrammar('source.rill');
}

function assertScope(source, tokens, word, scope) {
  const start = source.indexOf(word);
  assert.ok(start >= 0);
  for (let index = start; index < start + word.length; index += 1) {
    assert.ok(tokens.some(token => token.startIndex <= index && token.endIndex > index && token.scopes.includes(scope)), `${source}: ${word} lacks ${scope}`);
  }
}

test('real TextMate engine highlights current Rill syntax and negation', async () => {
  const grammar = await loadGrammar();
  for (const [source, expected] of [
    ['fn main() { not !false }', { fn: 'storage.type.function.rill', main: 'entity.name.function.rill', not: 'keyword.operator.rill', '!': 'keyword.operator.rill', false: 'constant.language.boolean.rill' }],
    ['fn partial', { fn: 'storage.type.function.rill', partial: 'entity.name.function.rill' }],
    ['fn answer() -> Option<i32> { None }', { answer: 'entity.name.function.rill', '->': 'keyword.operator.rill', Option: 'entity.name.type.rill', i32: 'storage.type.rill' }],
    ['@include("SDL3/SDL.h");', { '@include': 'storage.type.annotation.rill', '"SDL3/SDL.h"': 'string.quoted.double.rill' }],
    ['type Point = { x: i32; y: i32 }', { type: 'storage.type.rill', Point: 'entity.name.type.rill', i32: 'storage.type.rill' }],
    ['if !sdl.SDL_Init(u32(0)) { -3 } else { 0 }', { if: 'keyword.control.rill', '!': 'keyword.operator.rill', SDL_Init: 'entity.name.function.rill', u32: 'storage.type.rill', else: 'keyword.control.rill' }],
    ['// not fn fake() c"string"', { '// not fn fake() c"string"': 'comment.line.double-slash.rill' }],
  ]) {
    const result = grammar.tokenizeLine(source, textmate.INITIAL);
    for (const [word, scope] of Object.entries(expected)) {
      assertScope(source, result.tokens, word, scope);
    }
  }
});

test('lambdas capture function bindings, modes and parameters', async () => {
  const grammar = await loadGrammar();
  const source = 'let transform = fn(read input: Pair<Option<i32>, bool>, inout output, extra) { extra };';
  const { tokens } = grammar.tokenizeLine(source, textmate.INITIAL);
  for (const [word, scope] of [
    ['let', 'keyword.other.rill'], ['transform', 'entity.name.function.rill'],
    ['fn', 'storage.type.function.rill'], ['read', 'keyword.other.rill'],
    ['input', 'variable.parameter.rill'], ['output', 'variable.parameter.rill'],
    ['extra', 'variable.parameter.rill'], ['Option', 'entity.name.type.rill'],
    ['i32', 'storage.type.rill'], ['bool', 'storage.type.rill'], ['inout', 'keyword.other.rill'],
  ]) {
    assertScope(source, tokens, word, scope);
  }
  const body = source.lastIndexOf('extra');
  const bodyToken = tokens.find(token => token.startIndex <= body && token.endIndex > body);
  assert.ok(bodyToken.scopes.includes('variable.other.rill'));
  assert.ok(!bodyToken.scopes.includes('variable.parameter.rill'));
});

test('multiline lambda parameter scopes end before the body', async () => {
  const grammar = await loadGrammar();
  let state = textmate.INITIAL;
  for (const [source, word, scope] of [
    ['fn(', 'fn', 'storage.type.function.rill'],
    ['  read value: Option<Option<i32>>,', 'value', 'variable.parameter.rill'],
    ['  other', 'other', 'variable.parameter.rill'],
    [') { value }', 'value', 'variable.other.rill'],
  ]) {
    const result = grammar.tokenizeLine(source, state);
    state = result.ruleStack;
    assertScope(source, result.tokens, word, scope);
  }
});

test('function application and composition use function and whole operator scopes', async () => {
  const grammar = await loadGrammar();
  for (const [source, functions] of [
    ['transform input', ['transform']],
    ['transform(input)(2)', ['transform']],
    ['(callback)(1)', ['callback']],
    ['value |> increment >> double', ['increment', 'double']],
    ['increment << double <| value', ['increment', 'double']],
  ]) {
    const { tokens } = grammar.tokenizeLine(source, textmate.INITIAL);
    for (const name of functions) assertScope(source, tokens, name, 'entity.name.function.rill');
    for (const operator of ['|>', '<|', '>>', '<<']) {
      if (!source.includes(operator)) continue;
      assert.ok(tokens.some(token => source.slice(token.startIndex, token.endIndex) === operator && token.scopes.includes('keyword.operator.rill')), `${source}: ${operator} must be one operator token`);
    }
  }
});

test('C strings keep keywords inside strings and reject NUL escapes lexically', async () => {
  const grammar = await loadGrammar();
  const source = 'c"not // fn \\n \\0"';
  const { tokens } = grammar.tokenizeLine(source, textmate.INITIAL);
  for (const token of tokens) {
    assert.ok(token.scopes.includes('string.quoted.double.c.rill'));
    assert.ok(!token.scopes.includes('keyword.control.rill'));
    assert.ok(!token.scopes.includes('comment.line.double-slash.rill'));
  }
  assert.ok(tokens.some(token => source.slice(token.startIndex, token.endIndex) === '\\n' && token.scopes.includes('constant.character.escape.rill')));
  assert.ok(tokens.some(token => source.slice(token.startIndex, token.endIndex) === '\\0' && token.scopes.includes('invalid.illegal.escape.rill')));
});
