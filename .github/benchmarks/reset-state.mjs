// Restore a fixture's .craft state from its seed before each measured run.
// Hyperfine runs --prepare through the platform shell (cmd.exe on Windows), so this stays shell-free.
import { cpSync, rmSync } from 'node:fs';

const [seed, target] = process.argv.slice(2);
if (!seed || !target) throw new Error('usage: reset-state.mjs SEED TARGET');
rmSync(target, { recursive: true, force: true });
cpSync(seed, target, { recursive: true });
