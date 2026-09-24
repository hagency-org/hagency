import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { normalizeProjectAgentDefinition, projectAgentRuntimeName, publicResourceId } from '../../lib/project-agent-definition.js';

const context = { fleetId: `hf_${'a'.repeat(32)}`, targetProjectId: 'project_test', requestId: 'request_1' };
const resourceId = publicResourceId({ id: 'preset_fixture' });
const definitions = [
  ...['edison', '小白', '中文验证-0909', 'Édison', 'E\u0301dison', 'A', 'é-1', 'Ångström', '你好123',
    'a_b-c', 'a'.repeat(64), '𐐀'.repeat(32), 'Å'.repeat(33), 'İbrahim', 'Σ', 'a\u0301',
    '', '1abc', '_abc', '-abc', ' agent', 'agent ', '../edison', 'a/b', 'a.b', 'a\n',
    'a'.repeat(65), '𐐀'.repeat(33), '\u0301a', 'agent👋', 'a b', 'a\u0000', 'ａｇｅｎｔ', 'Ⅵ',
  ].map(name => ({ name, resourceId })),
  { name: '小白', resourceId: 'preset_fixture' },
  { name: 'edison', resourceId, yolo: true },
  { name: 42, resourceId },
  null,
];
const vectors = definitions.map(definition => {
  try {
    const normalized = normalizeProjectAgentDefinition(definition);
    return { definition, normalized, runtimeName: projectAgentRuntimeName({ ...context, agentDefinition: normalized }) };
  } catch { return { definition, rejected: true }; }
});
const output = `${JSON.stringify({ context, preset: 'preset_fixture', resourceId, vectors }, null, 2)}\n`;
const path = fileURLToPath(new URL('../fixtures/project-identities.json', import.meta.url));
if (process.argv.includes('--check')) {
  if (readFileSync(path, 'utf8') !== output) throw new Error('Project identity golden vectors differ from JavaScript');
} else writeFileSync(path, output);
