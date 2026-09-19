import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';
import { modelTier, modelFamily, CAPABILITY_TIERS } from '../../lib/matrix-agent.js';

const policy = JSON.parse(readFileSync(new URL('../../lib/role-capacity.json', import.meta.url), 'utf8'));
const source = readFileSync(new URL('../../backend-v2.js', import.meta.url), 'utf8');
const names = ['runtimeProfileFromPreset', 'presetTier', 'modelsExcludedForRole', 'resourcesForRole'];
const functions = names.map(name => {
  const start = source.indexOf(`function ${name}(`);
  if (start < 0) throw new Error(`Missing source function ${name}`);
  const end = source.indexOf('\n}', start);
  if (end < 0) throw new Error(`Unterminated source function ${name}`);
  return source.slice(start, end + 2);
}).join('\n');
const resourcesForRole = runInNewContext(`${functions}\nresourcesForRole`, { modelTier, CAPABILITY_TIERS, roleCapacity: policy });
const profiles = [null, {}, { framework: 'codex', model: 'unrecognized' }];
for (const matches of Object.values(policy.tierAccepts)) for (const row of matches) {
  const { framework, model, provider, reasoning } = row;
  for (const p of [provider, undefined, '', 'contradictory']) {
    profiles.push({ framework, model, ...(p === undefined ? {} : { provider: p }), ...(reasoning === undefined ? {} : { reasoning }) });
  }
  for (const reason of [undefined, '', 'low', 'medium', 'high', 'unexpected']) {
    profiles.push({ framework, model, provider, ...(reason === undefined ? {} : { reasoning: reason }) });
  }
}
const unique = [...new Map(profiles.map(p => [JSON.stringify(p), p])).values()];
const cases = unique.map(profile => {
  const resource = { id: 'fixture', ...profile, ceiling: { tokens: 100 } };
  return { profile, tier: modelTier({ primary: profile }), family: modelFamily({ primary: profile }),
    roles: Object.fromEntries(Object.entries(policy.roles).map(([role, definition]) => [role, {
      default: resourcesForRole(role, definition.defaultTier, [resource]).length === 1,
      lightweight: resourcesForRole(role, 'lightweight', [resource]).length === 1,
    }])),
  };
});
const presets = [
  { id: 'z-strong', framework: 'claude', model: 'claude-opus-5', ceiling: { tokens: 100 } },
  { id: 'b-medium', framework: 'codex', model: 'gpt-5.6-sol', reasoning: 'medium', ceiling: { tokens: 100 } },
  { id: 'a-medium', framework: 'claude', model: 'claude-sonnet-5', ceiling: { tokens: 0 } },
  { id: 'c-light', framework: 'claude', model: 'claude-fable-5', ceiling: { tokens: 100 } },
  { id: 'no-ceiling', framework: 'claude', model: 'claude-opus-5' },
  { id: 'foreign', framework: 'codex', provider: 'wrong', model: 'gpt-5.6-sol', reasoning: 'high', ceiling: { tokens: 100 } },
];
const rankings = Object.keys(policy.roles).flatMap(role => CAPABILITY_TIERS.map(tier => ({ role, tier,
  ids: resourcesForRole(role, tier, presets).map(row => row.preset.id),
})));
const output = `${JSON.stringify({ cases, presets, rankings }, null, 2)}\n`;
const path = fileURLToPath(new URL('../fixtures/qualification.json', import.meta.url));
if (process.argv.includes('--check')) {
  if (readFileSync(path, 'utf8') !== output) throw new Error('Native qualification vectors differ from JavaScript');
} else writeFileSync(path, output);
