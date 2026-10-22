# Receipt fixture

Node 22+, ESM, no dependencies. Reuse `src/normalize-key.mjs` for event identity;
do not add another normalizer or change the helper's contract. Preserve unrelated behavior.
Local source edits and checks are authorized; commits, remote actions, new dependencies
and delegation are outside the fixture task. Internal evidence stays outside source files.
Run `node --test --test-name-pattern=retry acceptance.test.mjs` for retry behavior and
`node --test --test-name-pattern=invalid acceptance.test.mjs` for invalid input behavior.
`node --check src/receipt.mjs` checks syntax only.
