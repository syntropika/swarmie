import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../', import.meta.url));
async function files(folder) {
  const entries = await readdir(folder, { withFileTypes: true });
  return (await Promise.all(entries.map((entry) => entry.isDirectory() ? files(path.join(folder, entry.name)) : [path.join(folder, entry.name)]))).flat();
}
const contentRoot = path.join(root, 'content/docs');
const pages = (await files(contentRoot)).filter((file) => file.endsWith('.mdx'));
const slug = (file) => '/docs/' + path.relative(contentRoot, file).replaceAll(path.sep, '/').replace(/(?:\/)?index\.mdx$/, '').replace(/\.mdx$/, '');
const routes = new Set(pages.map((file) => slug(file).replace(/\/$/, '')));
const publicFiles = await files(path.join(root, 'public'));
const downloads = new Set(publicFiles.map((file) => '/' + path.relative(path.join(root, 'public'), file).replaceAll(path.sep, '/')));
const failures = [];
for (const file of pages) {
  const source = await readFile(file, 'utf8');
  if (!/^title:\s*[^\n]+$/m.test(source.match(/^---\n([\s\S]*?)\n---/)?.[1] ?? '')) failures.push(`${file}: missing frontmatter`);
  for (const match of source.matchAll(/\]\((\/[^)#?]+)(?:[?#][^)]*)?\)/g)) {
    const link = match[1].replace(/\/$/, '');
    if (!routes.has(link) && !downloads.has(link)) failures.push(`${file}: broken link ${match[1]}`);
  }
  if (/\/(?:home|Users)\/[\w.-]+\/|\bdocs\/(?:client-runtime|cloud)\b|\b(?:AGENTS|PRODUCT|DESIGN)\.md\b|\bFIXTURE_[A-Z_]*RECEIPT\b/i.test(source)) failures.push(`${file}: private operational or build context`);
  if (/[áéíóúñ¿¡]/i.test(source)) failures.push(`${file}: unexpected non-English prose`);
}
for (const file of publicFiles.filter((file) => file.endsWith('.html'))) {
  const source = await readFile(file, 'utf8');
  const publicPath = '/' + path.relative(path.join(root, 'public'), file).replaceAll(path.sep, '/');
  const anchors = new Set([...source.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]));
  for (const [, link] of source.matchAll(/\b(?:src|href)="([^"]+)"/g)) {
    if (/^(?:[a-z][\w+.-]*:|\/\/)/i.test(link)) continue;
    const target = new URL(link, `https://docs.example.com${publicPath}`);
    if (target.pathname === publicPath && target.hash) {
      if (!anchors.has(decodeURIComponent(target.hash.slice(1)))) failures.push(`${file}: missing anchor ${link}`);
    } else if (target.pathname !== '/' && !routes.has(target.pathname.replace(/\/$/, '')) && !downloads.has(target.pathname)) {
      failures.push(`${file}: broken asset or page link ${link}`);
    }
  }
}
const guide = JSON.parse(await readFile(path.join(root, 'lib/extension-guide.json'), 'utf8'));
const ids = new Set(guide.slots.map((slot) => slot.id));
const schema = JSON.parse(await readFile(path.join(root, 'public/contracts/protocol.json'), 'utf8'));
const supported = new Set(schema.$defs.UiSlot.enum);
for (const id of ids) if (!supported.has(id)) failures.push(`Slot is absent from the public contract: ${id}`);
for (const id of supported) if (!ids.has(id)) failures.push(`Public slot is undocumented: ${id}`);
const captured = new Set(guide.captures.flatMap((capture) => capture.slots.map((slot) => slot.slot)));
for (const id of ids) if (!captured.has(id)) failures.push(`No visual capture for ${id}`);
for (const id of captured) if (!ids.has(id)) failures.push(`Unknown captured slot ${id}`);
for (const capture of guide.captures) if (!downloads.has(`/extensions/${capture.name}.png`)) failures.push(`Missing capture ${capture.name}`);
if (failures.length) throw new Error(failures.join('\n'));
console.log(`${pages.length} public pages, ${ids.size} slots, ${guide.captures.length} captures, and internal links checked.`);
