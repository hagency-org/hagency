import { execFileSync } from 'node:child_process';
import { checkSpecBindings } from '../../scripts/check-spec-bindings.js';
// Inventory every compiled selector, including the explicit browser lane.
// Listing is not execution; browser evidence requires its separate enabled job.
const raw = execFileSync('cargo', ['test', '--workspace', '--all-targets', '--all-features', '--locked', '--', '--list'], {
  encoding: 'utf8', timeout: 600_000, maxBuffer: 32 * 1024 * 1024,
});
const inventory = raw.split('\n').filter(line => line.endsWith(': test')).map(line => ({name: line.slice(0, -6)}));
if (!inventory.length) throw new Error('Cargo returned no registered tests');
const result = checkSpecBindings(inventory, { runtime: 'rust' });
console.log(JSON.stringify(result, null, 2));
if (!result.count || result.missing.length) process.exitCode = 1;
