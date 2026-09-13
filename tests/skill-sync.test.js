import { afterEach, beforeEach, describe, expect, test } from 'vitest';
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, readlinkSync, realpathSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

let temp;
let checkout;
let testHome;
let fakeBin;
const source = path.resolve('bin/hagency-sync-skills');
const clients = ['.claude', '.codex'];
const skillAt = (client) => path.join(testHome, client, 'skills', 'hagency-inner-loop');

beforeEach(() => {
  temp = realpathSync(mkdtempSync(path.join(tmpdir(), 'hagency-skill-sync-')));
  checkout = path.join(temp, 'checkout with spaces');
  testHome = path.join(temp, 'test home');
  fakeBin = path.join(temp, 'fake-bin');
  for (const dir of ['bin', 'skills/hagency', 'skills/hagency-inner-loop/scripts', 'skills/hagency-inner-loop/native']) mkdirSync(path.join(checkout, dir), { recursive: true });
  mkdirSync(fakeBin);
  copyFileSync(source, path.join(checkout, 'bin/hagency-sync-skills'));
  writeFileSync(path.join(checkout, 'skills/hagency/SKILL.md'), 'legacy template');
  writeFileSync(path.join(checkout, 'skills/hagency-inner-loop/SKILL.md'), 'node scripts/monitor.mjs');
  writeFileSync(path.join(checkout, 'skills/hagency-inner-loop/scripts/monitor.mjs'), 'console.log("resource available")');
  for (const name of ['native-control-evidence.mjs', 'native-control.mjs', 'prepare-native-fault-attempt.mjs']) {
    copyFileSync(path.resolve('skills/hagency-inner-loop/scripts', name), path.join(checkout, 'skills/hagency-inner-loop/scripts', name));
  }
  copyFileSync(path.resolve('skills/hagency-inner-loop/scripts/run-stage-release.mjs'), path.join(checkout, 'skills/hagency-inner-loop/scripts/run-stage-release.mjs'));
  copyFileSync(path.resolve('skills/hagency-inner-loop/scripts/native-stage-evidence.mjs'), path.join(checkout, 'skills/hagency-inner-loop/scripts/native-stage-evidence.mjs'));
  copyFileSync(path.resolve('skills/hagency-inner-loop/scripts/native-handle-summary.mjs'), path.join(checkout, 'skills/hagency-inner-loop/scripts/native-handle-summary.mjs'));
  copyFileSync(path.resolve('skills/hagency-inner-loop/native/darwin-process-metadata.c'), path.join(checkout, 'skills/hagency-inner-loop/native/darwin-process-metadata.c'));
  writeFileSync(path.join(fakeBin, 'date'), '#!/usr/bin/env bash\necho fixed\n', { mode: 0o755 });
});
afterEach(() => rmSync(temp, { recursive: true, force: true }));

function sync(...args) {
  return execFileSync('bash', [path.join(checkout, 'bin/hagency-sync-skills'), ...args], {
    encoding: 'utf8', env: { ...process.env, HOME: testHome, PATH: `${fakeBin}:${process.env.PATH}` },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
}

describe('skill directory synchronization', () => {
  test('skill sync refuses missing native handle summary before modifying client links', () => {
    sync();
    const snapshot = () => clients.map(client => ({ target: readlinkSync(skillAt(client)), inode: lstatSync(skillAt(client)).ino }));
    const before = snapshot();
    rmSync(path.join(checkout, 'skills/hagency-inner-loop/scripts/native-handle-summary.mjs'));
    let failure;
    try { sync(); } catch (error) { failure = error; }
    expect(failure?.status).toBe(1);
    expect(failure.stderr).toContain('scripts/native-handle-summary.mjs');
    expect(snapshot()).toEqual(before);
  });
  test('linked native control CLIs execute while library imports stay inert', () => {
    sync();
    for (const client of clients) {
      for (const name of ['native-control.mjs', 'prepare-native-fault-attempt.mjs', 'run-stage-release.mjs']) {
        const entry = path.join(skillAt(client), 'scripts', name);
        for (const flags of [[], ['--preserve-symlinks-main']]) {
          const run = spawnSync(process.execPath, [...flags, entry, '--invalid'], { encoding: 'utf8', timeout: 10000 });
          expect(run.status, `${client}/${name}: ${flags.join(' ')}`).toBe(1);
          expect(JSON.parse(run.stdout)).toMatchObject({ status: 'failed', reason: 'invalid_plan', full_acceptance: false });
          expect(run.stderr).toBe('');
        }
        const imported = spawnSync(process.execPath, ['--input-type=module', '-e', `await import(${JSON.stringify(pathToFileURL(entry).href)})`], { encoding: 'utf8', timeout: 10000 });
        expect(imported.status).toBe(0);
        expect(imported.stdout + imported.stderr).toBe('');
      }
    }
  });

  test('skill sync refuses missing native control resources before modifying client links', () => {
    sync();
    const snapshot = () => clients.map(client => ({ target: readlinkSync(skillAt(client)), inode: lstatSync(skillAt(client)).ino }));
    const before = snapshot();
    for (const name of ['native-control-evidence.mjs', 'native-control.mjs', 'prepare-native-fault-attempt.mjs']) {
      const target = path.join(checkout, 'skills/hagency-inner-loop/scripts', name);
      rmSync(target);
      for (const args of [['--check'], []]) {
        let failure;
        try { sync(...args); } catch (error) { failure = error; }
        expect(failure?.status).toBe(1);
        expect(failure.stderr).toContain(`scripts/${name}`);
        expect(snapshot()).toEqual(before);
      }
      copyFileSync(path.resolve('skills/hagency-inner-loop/scripts', name), target);
    }
  });

  test('skill sync refuses missing stage resources before modifying client links', () => {
    sync();
    const snapshot = () => clients.map(client => ({ target: readlinkSync(skillAt(client)), inode: lstatSync(skillAt(client)).ino }));
    const before = snapshot();
    for (const relative of ['scripts/run-stage-release.mjs', 'native/darwin-process-metadata.c', 'scripts/native-stage-evidence.mjs']) {
      const target = path.join(checkout, 'skills/hagency-inner-loop', relative);
      rmSync(target);
      for (const args of [['--check'], []]) {
        let failure;
        try { sync(...args); } catch (error) { failure = error; }
        expect(failure?.status).toBe(1);
        expect(failure.stderr).toContain(relative);
        expect(snapshot()).toEqual(before);
      }
      copyFileSync(path.resolve('skills/hagency-inner-loop', relative), target);
    }
  });

  test('skill sync links complete inner-loop resources for Claude and Codex', () => {
    sync();
    for (const client of clients) {
      const skill = skillAt(client);
      expect(existsSync(path.join(skill, 'scripts/monitor.mjs'))).toBe(true);
      for (const name of ['native-control-evidence.mjs', 'native-control.mjs', 'prepare-native-fault-attempt.mjs']) {
        expect(realpathSync(path.join(skill, 'scripts', name))).toBe(path.join(checkout, 'skills/hagency-inner-loop/scripts', name));
      }
      expect(readFileSync(path.join(skill, 'SKILL.md'), 'utf8')).toContain('scripts/monitor.mjs');
      expect(realpathSync(skill)).toBe(path.join(checkout, 'skills/hagency-inner-loop'));
      expect(realpathSync(path.join(testHome, client, 'skills/hagency/SKILL.md'))).toBe(path.join(checkout, 'skills/hagency/SKILL.md'));
      const output = execFileSync(process.execPath, [path.join(skill, 'scripts/monitor.mjs')], { encoding: 'utf8' });
      expect(output.trim()).toBe('resource available');
    }
    const before = clients.map((client) => lstatSync(skillAt(client)).mtimeMs);
    expect(sync('--check')).toContain('check passed');
    expect(clients.map((client) => lstatSync(skillAt(client)).mtimeMs)).toEqual(before);
  });

  test('skill sync preserves local content and existing backups', () => {
    for (const client of clients) {
      mkdirSync(skillAt(client), { recursive: true });
      writeFileSync(path.join(skillAt(client), 'local.txt'), 'first local skill');
      writeFileSync(`${skillAt(client)}.bak.fixed`, 'older backup');
    }
    sync();
    for (const client of clients) {
      expect(lstatSync(skillAt(client)).isSymbolicLink()).toBe(true);
      unlinkSync(skillAt(client));
      mkdirSync(skillAt(client));
      writeFileSync(path.join(skillAt(client), 'local.txt'), 'second local skill');
    }
    sync();
    for (const client of clients) {
      const parent = path.dirname(skillAt(client));
      const backups = readdirSync(parent).filter((name) => name.startsWith('hagency-inner-loop.bak.'));
      expect(backups).toHaveLength(3);
      const contents = backups.map((name) => {
        const file = path.join(parent, name);
        return readFileSync(lstatSync(file).isDirectory() ? path.join(file, 'local.txt') : file, 'utf8');
      });
      expect(contents.sort()).toEqual(['first local skill', 'older backup', 'second local skill']);
    }
  });

  test('skill check refuses missing resources and wrong links without mutation', () => {
    // Existing legacy links alone must not pass the expanded check.
    for (const client of clients) {
      const legacy = path.join(testHome, client, 'skills/hagency');
      mkdirSync(legacy, { recursive: true });
      symlinkSync(path.join(checkout, 'skills/hagency/SKILL.md'), path.join(legacy, 'SKILL.md'));
    }
    expect(() => sync('--check')).toThrow();
    for (const client of clients) expect(existsSync(skillAt(client))).toBe(false);
    sync();
    unlinkSync(skillAt('.claude'));
    symlinkSync(path.join(checkout, 'skills/hagency'), skillAt('.claude'));
    const wrong = readlinkSync(skillAt('.claude'));
    expect(() => sync('--check')).toThrow();
    expect(readlinkSync(skillAt('.claude'))).toBe(wrong);
    sync();
    rmSync(path.join(checkout, 'skills/hagency-inner-loop/scripts/monitor.mjs'));
    const before = clients.map((client) => lstatSync(skillAt(client)).mtimeMs);
    expect(() => sync('--check')).toThrow();
    expect(clients.map((client) => lstatSync(skillAt(client)).mtimeMs)).toEqual(before);
  });
});
