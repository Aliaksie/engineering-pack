#!/usr/bin/env node
import fs from 'node:fs';
import { validateReview } from '../support/application/review.mjs';
try {
  if (process.argv.length !== 4) throw new Error('usage: check-review.mjs <snapshot.json> <review.json>');
  const input = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
  const review = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
  process.stdout.write(`${JSON.stringify(validateReview(input, review), null, 2)}\n`);
} catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
