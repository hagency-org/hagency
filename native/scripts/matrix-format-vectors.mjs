import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { matrixMarkdownContent } from '../../lib/matrix-markdown.js';
const require = createRequire(import.meta.url);
const versions = { 'markdown-it': '15.0.1', 'sanitize-html': '2.17.7', 'linkify-it': '6.1.0' };
const lockSource = readFileSync(new URL('../../package-lock.json', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const lock = JSON.parse(lockSource);
for (const [name, version] of Object.entries(versions)) {
  if (require(`${name}/package.json`).version !== version || lock.packages[`node_modules/${name}`].version !== version) {
    throw new Error(`Review changed Markdown dependency: ${name}`);
  }
}
const source = readFileSync(new URL('../../lib/matrix-markdown.js', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const vectors = [];
const add = (name, input) => vectors.push({ name, input, expected: matrixMarkdownContent(input) });
const bodies = {
  'empty': '',
  'invalid definition keeps paragraph continuation': '[bad]: javascript:x\nordinary',
  'invalid definition before valid repeated identifier': '[x][id]\n\n[id]: javascript:x\n\n[id]: https://example.org',

  'IDN normalization retains raw punycode policy': '[x](https://ＥＸＡＭＰＬＥ.com) https://xn--fsqu00a.xn--fiqs8s/',
  'FTP punctuation and unknown protocol': 'www.example.org/x http:example.org ftp://x.test/a。next',
  'entity in automatic URL': 'https://x.test?a=1&amp;b=2',

  'unsafe image reference': '![alt][bad]\n\n[bad]: javascript:alert(1)',
  'unsafe image inline': '![**alt**](javascript:alert(1))',
  'data image hyperlink': '[pic](data:image/png;base64,AAAA)',
  'empty title': '[t](https://example.org "")',
  'IPv6 and port': '[v](https://[::1]:8443/path) [p](https://example.org:443/path)',
  'scheme case': '[a](HTTPS://Example.ORG/path) HTTPS://EXAMPLE.ORG/',
  'dangerous relative lookalikes': '[a](java&#10;script:bad) [b](/\\evil) [c](https://example.org/a%22onload%3Devil)',

  'emphasis source map regression': '> *a*\n> [',
  'percent display': '<https://example.org/a%20b> https://example.org/%E4%BD%A0%E5%A5%BD',
  'Unicode punctuation boundary': 'https://example.org/a。next https://example.org/b，later',
  'nested tables and quote': '> | a | b |\n> |---|---|\n> | c | d |',
  'empty code': '```\n```',
  'unusual list starts': '0. zero\n1. one',
  'empty link and quote title': '[empty]() [title](https://example.org "a &quot;b&quot;")',

  'Unicode plaintext': '你好 Edison 🙂 café',
  'paragraphs and breaks': 'first\nsecond\n\nthird  \nfourth\\\nfifth',
  'headings and emphasis': '# Title\n## 二\n### Three\n#### Four\n##### Five\n###### Six\n**bold** *em* ~~strike~~ __bold__ _em_',
  'list and quote': '> quote\n>\n> - first\n> - second\n\n3. three\n4. four\n\n---',
  'nested list': '- first\n  - inner\n\n    next paragraph\n- second',
  'inline and fenced code': '`<tag> & "x"`\n\n```js extra\nconst x = "<script>";\n```\n\n    indented & <code>',
  'tables': '| left | right | center |\n|:--|--:|:--:|\n| **bold** | `x` | 中 |',
  'raw HTML': '<div>**bold**</div>\n<script>alert(1)</script>\n<!-- comment -->\n<svg onload="bad">text</svg>',
  'HTML entities': 'A &quot; &amp; &#x27; &#60; &#x3e; &#0; &copy; &notit; &unknown; \' " > <',
  'safe explicit links': '[a](https://example.org/a?x=1&y=2 "title & more") [b](http://example.org) [c](mailto:a@example.org) [m](matrix:u/alice:example.org)',
  'relative and unknown links': '[a](/foo) [r](//evil.test) [b](foo:bar) [c](#anchor) [d](../path) [e](?q=1)',
  'unsafe schemes': '[a](javascript:alert(1)) [b](JaVaScRiPt:bad) [c](vbscript:bad) [d](file:///tmp/private) [e](data:text/html,bad)',
  'obfuscated links': '[a](jav&#x61;script:bad) [b](java&#x09;script:bad) [c](\\\\evil.test) [d](https://good.test/\"onclick=\"bad)',
  'generated images': 'before ![secret **alt**](https://example.org/a.png "title") after\n\n![only](data:image/png;base64,AAAA)',
  'automatic links': 'https://example.org/a?q=1&b=2 and http://example.org. email@example.org ftp://example.org/x',
  'no fuzzy domains': 'www.example.org example.org test@example.org',
  'links in protected content': '`https://code.test` [https://label.test](https://target.test) ![https://alt.test](https://image.test)\n\n```\nhttps://fence.test\n```',
  'automatic punctuation': '(https://example.org/a_(b)). https://example.org/foo, done.',
  'IDN and Unicode paths': '[中文](https://例子.中国/你好) https://例子.中国/你好。',
  'autolinks': '<https://example.org/a?x=1&y=2> <person@example.org>',
  'references': '[first][id] [id][]\n\n[id]: https://example.org "ref"',
  'raw HTML block Markdown': '<section>\n**still bold**\n</section>',
  'null and CRLF': 'a\u0000b\r\nc\rd',
  'linkifier lookahead regression': 'Hello world abcde[https://example.com/x](y)\n\u2028b[tp:///',
};
for (const [name, body] of Object.entries(bodies)) add(name, { msgtype: 'm.text', body });
add('notice thread', { msgtype: 'm.notice', body: '**notice**', 'm.relates_to': { rel_type: 'm.thread', event_id: '$root', 'm.in_reply_to': { event_id: '$reply' }, is_falling_back: false }, 'm.mentions': { user_ids: ['@edison:example.org'] } });
add('direct null root', { msgtype: 'm.text', body: '**direct**', 'm.relates_to': null });
add('replacement', { msgtype: 'm.text', body: '* replacement', 'm.relates_to': { rel_type: 'm.replace', event_id: '$prior' }, 'm.new_content': { msgtype: 'm.text', body: '**new**' } });
add('existing formatted HTML', { msgtype: 'm.text', body: '**plain**', format: 'org.matrix.custom.html', formatted_body: '<span data-existing="yes">already formatted</span>', 'm.relates_to': { rel_type: 'm.thread', event_id: '$root' } });
add('supplied HTML is not a sanitizer input', { msgtype: 'm.text', body: 'plain', formatted_body: '<script>caller trusted this</script>' });
for (const formatted_body of ['', null, false, 0, true, [], {}]) add(`formatted truthiness ${JSON.stringify(formatted_body)}`, { msgtype: 'm.text', body: '**body**', formatted_body });
for (const msgtype of ['m.image', 'm.file', 'm.audio', 'm.emote', null]) add(`passthrough ${msgtype}`, { msgtype, body: '**body**', url: 'mxc://example.org/media' });
add('nonstring body', { msgtype: 'm.text', body: 42 });
add('nested edit', { msgtype: 'm.text', body: 'first', 'm.new_content': { msgtype: 'm.notice', body: '*second*', 'm.new_content': { msgtype: 'm.text', body: '`third`' } } });
const output = JSON.stringify({ sourceSha256: createHash('sha256').update(source).digest('hex'), oracleLockSha256: createHash('sha256').update(lockSource).digest('hex'), versions, vectors }, null, 2) + '\n';
const file = new URL('../fixtures/matrix-format.json', import.meta.url);
if (process.argv.includes('--check')) {
  if (readFileSync(file, 'utf8').replaceAll('\r\n', '\n') !== output) throw new Error('Matrix formatting vectors differ from JavaScript');
} else writeFileSync(file, output);
console.log(JSON.stringify({ vectors: vectors.length }));
