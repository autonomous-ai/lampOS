import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import MarkdownIt from 'markdown-it';
import GithubSlugger from 'github-slugger';

const markdown = new MarkdownIt({ html: true });

// Read only tags emitted as HTML by the Markdown parser, never code examples.
function htmlAnchors(html) {
  const anchors = [];
  const uncommented = html.replace(/<!--[\s\S]*?-->/g, '');
  for (const tag of uncommented.matchAll(/<[a-z][a-z\d-]*(?:"[^"]*"|'[^']*'|[^'">])*>/gi)) {
    const attributes = tag[0].replace(/^<[a-z][a-z\d-]*/i, '').slice(0, -1);
    for (const attribute of attributes.matchAll(/([^\s=/>]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g)) {
      const name = attribute[1].toLowerCase();
      if (attribute[2] === undefined && attribute[3] === undefined && attribute[4] === undefined) continue;
      if (name === 'id' || (name === 'name' && /^<a(?:\s|>)/i.test(tag[0]))) {
        anchors.push(markdown.utils.unescapeAll(attribute[2] ?? attribute[3] ?? attribute[4]));
      }
    }
  }
  return anchors;
}

function headingText(tokens) {
  return tokens.map(token => {
    if (token.type === 'text' || token.type === 'code_inline') return token.content;
    if (token.type === 'image') return headingText(token.children ?? []);
    if (token.type === 'softbreak' || token.type === 'hardbreak') return '\n';
    return '';
  }).join('');
}

export function parseDocument(source) {
  const tokens = markdown.parse(source, {});
  const slugger = new GithubSlugger();
  const anchors = new Set();
  const links = [];
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    if (token.type === 'heading_open') {
      anchors.add(slugger.slug(headingText(tokens[i + 1].children ?? [])));
    }
    if (token.type === 'html_block') {
      for (const anchor of htmlAnchors(token.content)) anchors.add(anchor);
    }
    const visit = children => {
      for (const child of children) {
        const url = child.type === 'link_open' ? child.attrGet('href')
          : child.type === 'image' ? child.attrGet('src') : null;
        if (url !== null) links.push({ url, line: (token.map?.[0] ?? 0) + 1 });
        if (child.type === 'html_inline') {
          for (const anchor of htmlAnchors(child.content)) anchors.add(anchor);
        }
        if (child.children) visit(child.children);
      }
    };
    visit(token.children ?? []);
  }
  return { anchors, links };
}

export function checkDocs(root) {
  root = fs.realpathSync(root);
  const docs = path.join(root, 'docs');
  const files = fs.readdirSync(docs, { withFileTypes: true })
    .filter(entry => entry.isFile() && entry.name.endsWith('.md'))
    .map(entry => path.join(docs, entry.name)).sort();
  if (files.length === 0) throw new Error(`no top-level Markdown docs in ${docs}`);
  const parsed = new Map();
  const read = file => {
    if (!parsed.has(file)) parsed.set(file, parseDocument(fs.readFileSync(file, 'utf8')));
    return parsed.get(file);
  };
  const errors = [];
  let checked = 0;
  for (const file of files) {
    for (const { url, line } of read(file).links) {
      // HTTP, mail, data and other URI schemes are deliberately offline.
      if (/^[a-z][a-z\d+.-]*:/i.test(url) || url.startsWith('//')) continue;
      checked++;
      const report = reason => errors.push(`${path.relative(root, file)}:${line}: ${url}: ${reason}`);
      const hash = url.indexOf('#');
      const beforeHash = hash < 0 ? url : url.slice(0, hash);
      let relative, fragment;
      try {
        relative = decodeURIComponent(beforeHash.split('?')[0]);
        fragment = hash < 0 ? '' : decodeURIComponent(url.slice(hash + 1));
      } catch {
        report('invalid percent encoding');
        continue;
      }
      const target = relative === '' ? file : relative.startsWith('/')
        ? path.resolve(root, `.${relative}`) : path.resolve(path.dirname(file), relative);
      if (!fs.existsSync(target)) {
        report('target does not exist');
        continue;
      }
      // A local check must not silently succeed by finding a sibling checkout.
      const canonical = fs.realpathSync(target);
      if (canonical !== root && !canonical.startsWith(`${root}${path.sep}`)) {
        report('target is outside the lampOS source tree; use an explicit URL');
        continue;
      }
      if (fragment && /\.md$/i.test(target) && !read(target).anchors.has(fragment)) {
        report('heading or HTML anchor does not exist');
      }
    }
  }
  return { files: files.length, checked, errors };
}

function main() {
  if (process.argv.length !== 3) {
    console.error('usage: node check.mjs <lampOS source root>');
    return 2;
  }
  try {
    const result = checkDocs(process.argv[2]);
    for (const error of result.errors) console.error(error);
    console.log(`${result.files} docs, ${result.checked} local links, ${result.errors.length} broken`);
    return result.errors.length ? 1 : 0;
  } catch (error) {
    console.error(`doc links: ${error.message}`);
    return 2;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}
