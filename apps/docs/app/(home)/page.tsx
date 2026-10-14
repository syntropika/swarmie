import Link from 'next/link';
import { ArrowRight } from 'lucide-react';

const paths = [
  { title: 'Run your first conversation', description: 'Build the runtime, check an installed harness, and send a prompt.', href: '/docs/getting-started/first-session' },
  { title: 'Use the desktop workspace', description: 'Work with projects, conversations, execution nodes, and dictation.', href: '/docs/desktop' },
  { title: 'Build a client or extension', description: 'Use typed clients, JSON-RPC, commands, and named UI contribution slots.', href: '/docs/extensions' },
];
export default function HomePage() {
  return <main className="public-docs-home">
    <h1>Swarmie documentation</h1>
    <p className="intro">One workspace for coding agents. An independently installable runtime for execution. Start with a conversation, then connect the tools your workflow needs.</p>
    <Link className="start-reading" href="/docs">Read the documentation <ArrowRight size={18} aria-hidden="true" /></Link>
    <h2>Find your starting point</h2>
    <div className="reading-paths">{paths.map((path) => <Link key={path.href} href={path.href}><strong>{path.title}</strong><span>{path.description}</span></Link>)}</div>
    <p className="documentation-status">This documentation describes the current development source. Supported capabilities are discovered from your installed runtime and harness. Package namespaces do not imply a published release.</p>
  </main>;
}
