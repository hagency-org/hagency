// Build-time source inspection only. Never import the application being audited.
import { createHash } from 'node:crypto';
import { parse } from 'espree';

export const digest = value => createHash('sha256').update(value).digest('hex');
const verbs = new Set(['get', 'post', 'put', 'patch', 'delete', 'head', 'options', 'all', 'use']);
const excludedRoots = /^(?:native|tests|docs|knowledge|mockup|projects|\.github)\//;
export function helperCandidate(path, source) {
  return !excludedRoots.test(path) && (
    /^(?:backend-v2|bridge-matrix|mcp-server|push-relay)\.js$/.test(path)
    || /^(?:remote\/)?bin\//.test(path)
    || /^(?:(?:remote\/)?scripts|services|src|install|deploy)\/.*\.(?:m?js|sh|plist)$/.test(path)
    || /^(?:remote\/)?[^/]+\.(?:sh|service|plist)$/.test(path)
    || source.startsWith('#!')
  );
}
export function javascriptCandidate(path, source) {
  if (/^mockup\/app\/api\/.*\/route\.js$/.test(path)) return true;
  return !excludedRoots.test(path) && (
    /^(?:backend-v2|bridge-matrix|mcp-server|push-relay)\.js$/.test(path)
    || /^(?:lib|remote|scripts|services|src)\/.*\.(?:m?js)$/.test(path)
    || (/^(?:remote\/)?bin\//.test(path) && /^#![^\n]*\bnode\b/.test(source))
  );
}
function walk(node, visit, parents = []) {
  if (!node || typeof node !== 'object' || !node.type) return;
  visit(node, parents);
  for (const [key, child] of Object.entries(node)) {
    if (['range', 'loc', 'tokens', 'comments'].includes(key)) continue;
    if (Array.isArray(child)) child.forEach(value => walk(value, visit, [...parents, node]));
    else walk(child, visit, [...parents, node]);
  }
}
function literal(node) {
  if (node?.type === 'Literal' && typeof node.value === 'string') return node.value;
  if (node?.type === 'TemplateLiteral' && node.expressions.length === 0) return node.quasis[0].value.cooked;
  return undefined;
}
function property(node) {
  return node?.type === 'MemberExpression'
    ? node.computed ? literal(node.property) : node.property.name : undefined;
}
function paths(node) {
  const value = literal(node);
  if (value !== undefined) return [value];
  if (node?.regex) return [{ regex: node.regex.pattern, flags: node.regex.flags }];
  if (node?.type === 'ArrayExpression') {
    const values = node.elements.map(paths);
    if (values.every(Boolean)) return values.flat();
  }
  return null;
}
function locate(path, source, node) {
  return { source: path, line: node.loc.start.line, column: node.loc.start.column + 1,
    end_line: node.loc.end.line, expression: source.slice(...node.range),
    sha256: digest(source.slice(...node.range)) };
}
function name(node) {
  if (node?.type === 'Identifier') return node.name;
  if (node?.type === 'MemberExpression') return `${name(node.object) ?? '<expression>'}.${property(node) ?? '<computed>'}`;
  if (node?.type === 'CallExpression') return `${name(node.callee) ?? '<call>'}(…)`;
  return '<inline handler>';
}
function hasIdentifier(node, id) {
  let found = false;
  walk(node, child => { if (child.type === 'Identifier' && child.name === id) found = true; });
  return found;
}
export function parseSources(sources) {
  return new Map([...sources].map(([path, source]) => {
    try {
      return [path, { source, ast: parse(source, { ecmaVersion: 'latest', sourceType: 'module', loc: true, range: true }) }];
    } catch (error) {
      throw new Error(`Inventory cannot parse ${path}: ${error.message}`);
    }
  }));
}
export function registrations(sources) {
  const parsed = parseSources(sources);
  const routes = [], tools = [], listeners = [], calls = [];
  for (const [path, { source, ast }] of parsed) {
    const factories = new Set(['express']);
    const receivers = new Set(['app', 'router', 'this.app', 'this.router']);
    walk(ast, node => {
      if (node.type === 'ImportDeclaration' && node.source.value === 'express') {
        for (const item of node.specifiers) {
          if (item.type === 'ImportDefaultSpecifier' || item.imported?.name === 'Router') factories.add(item.local.name);
        }
      }
      if (node.type !== 'VariableDeclarator' || node.id.type !== 'Identifier') return;
      if (receivers.has(name(node.init))) receivers.add(node.id.name);
      if (node.init?.type === 'CallExpression' && (factories.has(name(node.init.callee))
        || property(node.init.callee) === 'Router' && factories.has(name(node.init.callee.object)))) receivers.add(node.id.name);
    });
    walk(ast, (node, parents) => {
      if (node.type !== 'CallExpression') return;
      const member = node.callee;
      const method = property(member);
      const receiver = name(member.object);
      calls.push({ path, source, node, method });
      if (method === 'createServer' || name(member) === 'createServer') {
        listeners.push(locate(path, source, node.callee));
      }
      if (['tool', 'registerTool'].includes(method) && receiver === 'server') {
        const tool = literal(node.arguments[0]);
        if (!tool) throw new Error(`Unresolved MCP registration ${path}:${node.loc.start.line}`);
        tools.push({ ...locate(path, source, node.callee), name: tool });
        return;
      }
      let chainRoot = member.object;
      while (chainRoot?.type === 'CallExpression' && verbs.has(property(chainRoot.callee))) chainRoot = chainRoot.callee.object;
      const chain = chainRoot?.type === 'CallExpression' && property(chainRoot.callee) === 'route';
      const knownReceiver = receivers.has(receiver) || chain && receivers.has(name(chainRoot.callee.object));
      if (!knownReceiver) return;
      if (!method) throw new Error(`Unresolved HTTP method ${path}:${node.loc.start.line}`);
      if (!verbs.has(method) || method === 'route') return;
      let argument = chain ? chainRoot.arguments[0] : node.arguments[0];
      if (method === 'use' && (!argument || ['ArrowFunctionExpression', 'FunctionExpression', 'CallExpression'].includes(argument.type))) {
        argument = null;
      }
      let resolved = argument === null ? ['/'] : paths(argument);
      let parameter = null;
      if (!resolved && argument?.type === 'Identifier') {
        const fn = [...parents].reverse().find(p => ['FunctionDeclaration', 'FunctionExpression', 'ArrowFunctionExpression'].includes(p.type));
        const index = fn?.params.findIndex(p => p.type === 'AssignmentPattern' && p.left.name === argument.name);
        if (index >= 0 && fn.id?.name) {
          const defaults = paths(fn.params[index].right);
          if (defaults) parameter = { installer: fn.id.name, index, defaults };
        }
      }
      if (!resolved && !parameter) throw new Error(`Unresolved HTTP path ${path}:${node.loc.start.line}: ${source.slice(...argument.range)}`);
      if (resolved?.some(p => typeof p === 'string' && !p.startsWith('/'))) {
        // Express app.get(setting) is not a route. A handler would make it ambiguous.
        if (method === 'get' && node.arguments.length === 1) return;
        throw new Error(`Non-absolute HTTP registration ${path}:${node.loc.start.line}`);
      }
      routes.push({ ...locate(path, source, node.callee), method: method.toUpperCase(),
        kind: method === 'use' ? 'middleware' : 'route', paths: resolved,
        handlers: node.arguments.slice(chain || argument === null ? 0 : 1).map(name),
        ...(parameter ? { parameter } : {}) });
    });
  }
  for (const route of routes.filter(route => route.parameter)) {
    const { installer, index, defaults } = route.parameter;
    const sites = calls.filter(call => call.method === installer || call.node.callee.type === 'Identifier' && call.node.callee.name === installer);
    if (!sites.length) throw new Error(`Uncalled dynamic route installer ${route.source}:${route.line}`);
    route.installed_at = sites.map(({ path, source, node }) => {
      const resolved = node.arguments.length <= index ? defaults : paths(node.arguments[index]);
      if (!resolved) throw new Error(`Unresolved installer argument ${path}:${node.loc.start.line}`);
      return { ...locate(path, source, node), paths: resolved };
    });
    route.paths = [...new Map(route.installed_at.flatMap(site => site.paths).map(value => [JSON.stringify(value), value])).values()];
    route.default_paths = defaults;
    delete route.parameter;
  }
  return { routes, tools, listeners, parsed };
}

export function reviewedLinks(parsed, declarations) {
  return declarations.map(link => {
    const file = parsed.get(link.source);
    if (!file || !parsed.has(link.target)) throw new Error(`Missing reviewed dispatcher link ${link.source} -> ${link.target}`);
    const sites = [];
    walk(file.ast, (node, parents) => {
      if (node.type !== 'CallExpression' || file.source.slice(...node.callee.range) !== link.callee) return;
      const enclosing = [...parents].reverse().find(parent => parent.type === 'Property');
      sites.push({ ...locate(link.source, file.source, node.callee),
        ...(enclosing ? { context_property: enclosing.key.name ?? literal(enclosing.key) } : {}) });
    });
    if (!sites.length) throw new Error(`Stale reviewed dispatcher link ${link.source}: ${link.callee}`);
    return { ...link, sites, resolution: 'reviewed-module-link; source calls verified, runtime reachability unproven' };
  });
}

export function customBranches(path, parsed) {
  const { source, ast } = parsed.get(path) ?? {};
  if (!ast) throw new Error(`Custom dispatcher is missing: ${path}`);
  const bindings = new Map();
  walk(ast, node => {
    if (node.type === 'VariableDeclarator' && node.id.type === 'Identifier' && node.init) bindings.set(node.id.name, node.init);
  });
  const result = [];
  walk(ast, node => {
    if (node.type !== 'IfStatement') return;
    const related = new Set(), methodValues = new Set(), pathValues = [];
    walk(node.test, child => {
      if (child.type === 'Identifier') related.add(child.name);
      if (child.type === 'BinaryExpression' && ['===', '=='].includes(child.operator)) {
        if (child.left.name === 'method' && literal(child.right)) methodValues.add(literal(child.right));
        if (child.left.name === 'path' && literal(child.right)) pathValues.push(literal(child.right));
      }
      if (child.type === 'CallExpression' && property(child.callee) === 'startsWith' && child.callee.object.name === 'path') {
        const prefix = literal(child.arguments[0]);
        if (!prefix) throw new Error(`Unresolved dispatcher prefix ${path}:${child.loc.start.line}`);
        pathValues.push({ prefix });
      }
      if (child.type === 'CallExpression' && ['exec', 'test'].includes(property(child.callee)) && child.callee.object.regex
        && hasIdentifier(child.arguments[0], 'path')) {
        pathValues.push({ regex: child.callee.object.regex.pattern, flags: child.callee.object.regex.flags });
      }
    });
    const resolved = [];
    for (const id of related) {
      const value = bindings.get(id);
      if (value?.type === 'CallExpression' && property(value.callee) === 'exec' && value.callee.object.regex
        && hasIdentifier(value.arguments[0], 'path')) {
        pathValues.push({ regex: value.callee.object.regex.pattern, flags: value.callee.object.regex.flags });
        resolved.push(locate(path, source, value));
      }
    }
    if (!pathValues.length && !methodValues.size) return;
    if (!pathValues.length) throw new Error(`Unresolved dispatcher branch ${path}:${node.loc.start.line}`);
    result.push({ ...locate(path, source, node.test), methods: [...methodValues], paths: pathValues, bindings: resolved });
  });
  if (!result.length) throw new Error(`No classified custom branches in ${path}`);
  return result;
}

export function proxySurface(path, parsed) {
  const { source, ast } = parsed.get(path) ?? {};
  if (!ast) throw new Error(`Proxy source is missing: ${path}`);
  const patterns = [], methods = [];
  walk(ast, node => {
    if (node.type === 'ExportNamedDeclaration') {
      for (const item of node.declaration?.declarations ?? []) {
        if (['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'].includes(item.id.name)) {
          methods.push({ ...locate(path, source, item.id), method: item.id.name });
        }
      }
    }
    if (node.type !== 'VariableDeclarator' || !['READS', 'WRITES'].includes(node.id.name)) return;
    if (node.init?.type !== 'ArrayExpression') throw new Error(`Unresolved proxy allowlist ${path}:${node.loc.start.line}`);
    for (const item of node.init.elements) {
      const values = Object.fromEntries((item.properties ?? []).map(p => [p.key.name, p.value]));
      const method = node.id.name === 'READS' ? 'GET' : literal(values.method);
      const re = node.id.name === 'READS' ? item : values.re;
      if (!method || !re?.regex) throw new Error(`Unresolved proxy rule ${path}:${item.loc.start.line}`);
      patterns.push({ ...locate(path, source, item), method, pattern: re.regex });
    }
  });
  if (!methods.length || !patterns.length) throw new Error(`Proxy registration is incomplete: ${path}`);
  return { source: path, path: '/api/hagency/[...path]', methods, allowlist: patterns,
    meaning: 'Dynamic Next server proxy; patterns select backend paths, not extra backend registrations. Static export has no forwarding server.' };
}
