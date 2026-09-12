// Deterministic M0 classification. Run --write only after reviewing source drift.
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { customBranches, digest, helperCandidate, javascriptCandidate, proxySurface, registrations, reviewedLinks } from './inventory-source.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const require = createRequire(import.meta.url);
const parity = 'parity-unverified';
export function repositorySources(directory = root) {
  const tracked = execFileSync('git', ['ls-files', '-z'], { cwd: directory, encoding: 'utf8' }).split('\0').filter(Boolean);
  return new Map(tracked.sort().map(file => [file, readFileSync(path.join(directory, file), 'utf8')]));
}
function migration(group) {
  for (const key of ['owner', 'phase', 'disposition', 'validation_gate']) {
    if (typeof group?.[key] !== 'string' || !group[key].trim()) throw new Error(`Missing migration ${key}`);
  }
  if (!/^M[0-9]$/.test(group.phase)) throw new Error('Invalid migration phase');
  return { owner: group.owner, phase: group.phase, disposition: group.disposition,
    validation_gate: group.validation_gate, native_status: parity };
}
function routeOwnership(route, groups) {
  const candidates = groups.flatMap(group => group.prefixes.map(prefix => ({ group, prefix })))
    .sort((a, b) => b.prefix.length - a.prefix.length);
  const owners = route.paths.map(value => {
    if (typeof value !== 'string') throw new Error(`Route regex requires explicit ownership: ${route.source}:${route.line}`);
    const found = candidates.find(({ group, prefix }) => value === prefix || group.id !== 'authority' && value.startsWith(`${prefix}/`));
    if (!found) throw new Error(`Unclassified route: ${route.method} ${value}`);
    return found.group;
  });
  if (new Set(owners.map(owner => owner.id)).size !== 1) throw new Error('A registration crosses migration owners');
  return migration(owners[0]);
}
export function buildInventory(all, policy) {
  if (policy.version !== 2 || !/^[a-f0-9]{40}$/.test(policy.legacy_baseline)) throw new Error('Invalid pinned inventory policy');
  if (!/^[a-f0-9]{40}$/.test(policy.source_baseline)) throw new Error('Missing pinned source baseline');
  if (!policy.remaining_gates?.length || policy.remaining_gates.some(gate => gate.status !== 'open' || !gate.gate)) {
    throw new Error('Source inventory cannot close release gates');
  }
  const parserVersion = require('espree/package.json').version;
  if (parserVersion !== policy.parser.version) throw new Error(`Parser changed: ${parserVersion}; review and pin its inventory output`);
  const helperPolicies = new Map();
  for (const group of policy.helper_groups) {
    migration(group);
    if (!['runtime', 'runtime-module', 'operator', 'install', 'build-or-test', 'operator-and-build'].includes(group.role)) throw new Error('Missing helper runtime role');
    for (const file of group.files) {
      if (helperPolicies.has(file)) throw new Error(`Duplicate helper classification: ${file}`);
      helperPolicies.set(file, group);
    }
  }
  const helperSources = [...all].filter(([file, source]) => helperCandidate(file, source));
  const entries = helperSources.map(([file, source]) => {
    const group = helperPolicies.get(file);
    if (!group) throw new Error(`Unclassified helper: ${file}`);
    helperPolicies.delete(file);
    const mentions = helperSources.flatMap(([from, content]) => from === file ? [] : content.split('\n').flatMap((line, index) => {
      const offset = line.indexOf(file);
      return offset >= 0 && !/[A-Za-z0-9._/-]/.test(line[offset + file.length] ?? '')
        ? [{ source: from, line: index + 1 }] : [];
    }));
    return { path: file, sha256: digest(source), role: group.role, migration: migration(group),
      entry: { line: 1, interpreter: source.startsWith('#!') ? source.split('\n')[0] : null },
      literal_path_mentions: mentions };
  });
  if (helperPolicies.size) throw new Error(`Stale helper classifications: ${[...helperPolicies.keys()].join(', ')}`);
  const sources = new Map([...all].filter(([file, source]) => javascriptCandidate(file, source)));
  const facts = registrations(sources);
  const listenerPolicies = new Map(policy.listeners.map(item => [`${item.source}:${item.expression}`, item]));
  const listeners = facts.listeners.map(listener => {
    const key = `${listener.source}:${listener.expression}`;
    const group = listenerPolicies.get(key);
    if (!group || !policy.dispatchers.includes(group.dispatcher)) throw new Error(`Unclassified HTTP listener: ${key}`);
    listenerPolicies.delete(key);
    return { ...listener, dispatcher: group.dispatcher,
      migration: migration({ ...group, disposition: 'replace-native' }) };
  });
  if (listenerPolicies.size) throw new Error(`Stale listener classifications: ${[...listenerPolicies.keys()]}`);
  const cliFile = 'scripts/cli-command-manifest.json';
  const cli = JSON.parse(all.get(cliFile) ?? '{}');
  const commands = Object.entries(cli.profiles ?? {}).flatMap(([profile, config]) => config.commands.map(command => {
    const target = path.posix.join(path.posix.dirname(config.cli), command.target);
    const entry = entries.find(entry => entry.path === target);
    if (!entry) throw new Error(`CLI target has no migration owner: ${target}`);
    return { profile, command: command.command, aliases: command.aliases ?? [], target,
      declaration: { source: cliFile, sha256: digest(all.get(cliFile)), json_pointer: `/profiles/${profile}/commands/${config.commands.indexOf(command)}` },
      migration: entry.migration };
  }));
  if (!commands.length) throw new Error('Missing CLI command declarations');
  const dispatcherMigration = { owner: 'native-matrix-transport', phase: 'M5', disposition: 'port-current-authority',
    validation_gate: 'Prove exact token/registration authority, bounded custody, retry and namespace semantics for this deployment profile.' };
  const mcpMigration = { owner: 'native-runner-tools', phase: 'M6', disposition: 'port-current-authority',
    validation_gate: 'Bind this tool to the current runtime capability; prove private scope, approvals and results through the actual runner adapter.' };
  return {
    version: 2, legacy_baseline: policy.legacy_baseline, source_baseline: policy.source_baseline,
    status: 'source-classified; native parity and M0 completion remain unproven',
    contract: policy.inventory_contract, requirement: 'REQ-RUST-MIGRATION-EXECUTION', parser: policy.parser,
    scope: 'Tracked legacy JavaScript runtime/build sources, helper entry candidates, CLI manifest and Next API route; no application modules execute.',
    limits: [
      'Registrations and static installer arguments do not establish runtime reachability or deployment enablement.',
      'Shell/service entries have explicit reviewed roles and source hashes; literal_path_mentions are navigation evidence, not a shell execution graph.',
      'Generated workspaces, external runtime internals, browser-only code and TypeScript router modules are not new HTTP servers in this corpus; their complete behavior traceability remains an M0 gate.',
    ],
    sources: [...sources].map(([file, source]) => ({ path: file, sha256: digest(source) })),
    entries, cli_commands: commands,
    http_registrations: facts.routes.map(route => ({ ...route, migration: routeOwnership(route, policy.route_groups) })),
    http_listeners: listeners,
    registration_links: reviewedLinks(facts.parsed, policy.registration_links),
    custom_dispatch: policy.dispatchers.map(file => ({ source: file, branches: customBranches(file, facts.parsed), migration: migration(dispatcherMigration) })),
    console_proxy: { ...proxySurface(policy.proxy, facts.parsed), migration: migration({ owner: 'native-console', phase: 'M7', disposition: 'replace-native', validation_gate: 'Replace Next server proxy with scoped native API; prove browser token isolation, allowlist and normalization without a Node deployment dependency.' }) },
    mcp_tools: facts.tools.map(tool => ({ ...tool, migration: migration(mcpMigration) })),
    remaining_gates: policy.remaining_gates,
  };
}
export function checkInventory(actual, expected) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error('Migration inventory drift: review changed sources/classifications, then run node native/scripts/inventory.mjs --write');
  }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const mode = process.argv[2] ?? '--check';
  if (!['--check', '--write'].includes(mode) || process.argv.length > 3) throw new Error('Usage: inventory.mjs [--check|--write]');
  const fixture = path.join(root, 'native/fixtures/legacy-inventory.json');
  const policy = JSON.parse(readFileSync(path.join(root, 'native/fixtures/inventory-policy.json'), 'utf8'));
  const inventory = buildInventory(repositorySources(), policy);
  if (mode === '--write') writeFileSync(fixture, `${JSON.stringify(inventory, null, 2)}\n`);
  else checkInventory(inventory, JSON.parse(readFileSync(fixture, 'utf8')));
  console.log(JSON.stringify({ mode, helpers: inventory.entries.length, registrations: inventory.http_registrations.length,
    custom_branches: inventory.custom_dispatch.reduce((n, item) => n + item.branches.length, 0),
    cli_commands: inventory.cli_commands.length, mcp_registrations: inventory.mcp_tools.length, native_status: parity }));
}
