// Generated per-language sources for timing Project Intelligence extraction and lookup per
// language. Each file imports its predecessor, declares a type with one method and a chain of
// functions where each calls the one before; the shape is the same in every language, so a cell
// that moves points at that language's grammar or query, not at a different corpus.
import fs from 'node:fs';
import path from 'node:path';
import { git } from './fixture.mjs';

const chain = (n, k, call, first) => k === 0 ? first : call(`item${n}x${k - 1}`, k);

export const LANGUAGES = {
  typescript: { level: 'full', file: n => `src/item-${n}.ts`, source: (n, functions) => [
    n > 0 ? `import { item${n - 1}x0 } from './item-${n - 1}';` : '',
    `export class Box${n} {\n  run(): number {\n    return item${n}x0(1);\n  }\n}`,
    ...functions.map(k => `export function item${n}x${k}(value: number): number {\n  return ${chain(n, k, (f, k) => `${f}(value) + ${k}`, 'value')};\n}`)] },
  javascript: { level: 'full', file: n => `src/item-${n}.mjs`, source: (n, functions) => [
    n > 0 ? `import { item${n - 1}x0 } from './item-${n - 1}.mjs';` : '',
    `export class Box${n} {\n  run() {\n    return item${n}x0(1);\n  }\n}`,
    ...functions.map(k => `export function item${n}x${k}(value) {\n  return ${chain(n, k, (f, k) => `${f}(value) + ${k}`, 'value')};\n}`)] },
  python: { level: 'full', file: n => `bench/item_${n}.py`, source: (n, functions) => [
    n > 0 ? `from .item_${n - 1} import item${n - 1}x0\n` : '',
    `class Box${n}:\n    def run(self):\n        return item${n}x0(1)\n`,
    ...functions.map(k => `def item${n}x${k}(value):\n    return ${chain(n, k, (f, k) => `${f}(value) + ${k}`, 'value')}\n`)] },
  java: { level: 'full', file: n => `src/bench/Box${n}.java`, source: (n, functions) => [
    `package bench;\n\nimport java.util.List;\n`,
    `public class Box${n} {\n  int run() {\n    return item${n}x0(1);\n  }`,
    ...functions.map(k => `  static int item${n}x${k}(int value) {\n    return ${chain(n, k, (f, k) => `${f}(value) + ${k}`, 'value')};\n  }`), '}'] },
  go: { level: 'full', file: n => `bench/item_${n}.go`, source: (n, functions) => [
    `package bench\n\nimport "strings"\n`,
    `type Box${n} struct{}\n\nfunc (b Box${n}) Run() int {\n\treturn item${n}x0(1)\n}`,
    ...functions.map(k => `func item${n}x${k}(value int) int {\n\treturn ${chain(n, k, (f, k) => `${f}(value) + ${k}`, 'value')}\n}`)] },
  rust: { level: 'full', file: n => `src/item_${n}.rs`, source: (n, functions) => [
    n > 0 ? `use crate::item_${n - 1}::item${n - 1}x0;` : '',
    `pub struct Box${n};\n\nimpl Box${n} {\n    pub fn run(&self) -> i64 {\n        item${n}x0(1)\n    }\n}`,
    ...functions.map(k => `pub fn item${n}x${k}(value: i64) -> i64 {\n    ${chain(n, k, (f, k) => `${f}(value) + ${k}`, 'value')}\n}`)] },
  c: { level: 'basic', file: n => `src/item_${n}.c`, source: (n, functions) => [
    n > 0 ? `#include "item_${n - 1}.h"` : '',
    `struct box${n} {\n  int value;\n};`,
    ...functions.map(k => `int item${n}x${k}(int value) {\n  return ${chain(n, k, (f, k) => `${f}(value) + ${k}`, 'value')};\n}`)] },
  cpp: { level: 'basic', file: n => `src/item_${n}.cpp`, source: (n, functions) => [
    n > 0 ? `#include "item_${n - 1}.hpp"` : '',
    `class Box${n} {\n public:\n  int run() { return item${n}x0(1); }\n};`,
    ...functions.map(k => `int item${n}x${k}(int value) {\n  return ${chain(n, k, (f, k) => `${f}(value) + ${k}`, 'value')};\n}`)] },
  csharp: { level: 'basic', file: n => `src/Box${n}.cs`, source: (n, functions) => [
    `using System;\n\nnamespace Bench {\npublic class Box${n} {\n  public int Run() {\n    return item${n}x0(1);\n  }`,
    ...functions.map(k => `  static int item${n}x${k}(int value) {\n    return ${chain(n, k, (f, k) => `${f}(value) + ${k}`, 'value')};\n  }`), '}\n}'] },
  php: { level: 'basic', file: n => `src/item_${n}.php`, source: (n, functions) => [
    `<?php\nnamespace Bench;\n`,
    `class Box${n} {\n  public function run() {\n    return item${n}x0(1);\n  }\n}`,
    ...functions.map(k => `function item${n}x${k}($value) {\n  return ${chain(n, k, (f, k) => `${f}($value) + ${k}`, '$value')};\n}`)] },
  ruby: { level: 'basic', file: n => `lib/item_${n}.rb`, source: (n, functions) => [
    n > 0 ? `require_relative 'item_${n - 1}'\n` : '',
    `class Box${n}\n  def run\n    item${n}x0(1)\n  end\nend\n`,
    ...functions.map(k => `def item${n}x${k}(value)\n  ${chain(n, k, (f, k) => `${f}(value) + ${k}`, 'value')}\nend\n`)] }
};

// The declaration and its one caller every language fixture answers: `item1x3` is declared in
// file 1 and called only by `item1x4`.
export const PROBE = { file: 1, name: 'item1x3', caller: 'item1x4' };

// A committed Git repository of `files` sources in `language`, `functions` functions each.
export function languageFixture(root, language, { files = 100, functions = 20 } = {}) {
  const { file, source } = LANGUAGES[language];
  const indexes = Array.from({ length: functions }, (_, k) => k);
  for (let n = 0; n < files; n++) {
    const target = path.join(root, file(n));
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, source(n, indexes).filter(Boolean).join('\n\n') + '\n');
  }
  git(root, 'init', '-q'); git(root, 'add', '.'); git(root, 'commit', '-qm', 'fixture');
  return root;
}
