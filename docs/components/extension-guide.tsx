import guide from '@/lib/extension-guide.json';

export function SlotIndex() {
  return <nav className="slot-index" aria-label="Contribution slot index">{guide.slots.map((slot) => <a key={slot.id} href={`#slot-${slot.id}`}>{slot.id}</a>)}</nav>;
}
export function AnnotatedCapture({ name, title }: { name: string; title: string }) {
  const capture = guide.captures.find((entry) => entry.name === name);
  if (!capture) throw new Error(`Unknown documentation capture: ${name}`);
  const names = [...new Set(capture.slots.map((slot) => slot.slot))];
  return <figure className="extension-capture"><div className="annotated">
    <img src={`/extensions/${name}.png`} width={capture.width} height={capture.height} alt={`${title}: implemented Swarmie interface with fixture workspace data`} loading="lazy" />
    {capture.slots.map((slot, index) => <a key={`${slot.slot}:${index}`} data-slot-name={slot.slot} className="hotspot" href={`/docs/extensions/slots/#slot-${slot.slot}`} aria-label={`View the ${slot.slot} contract`} style={{ left: `${slot.x / capture.width * 100}%`, top: `${slot.y / capture.height * 100}%`, width: `${slot.width / capture.width * 100}%`, height: `${slot.height / capture.height * 100}%` }}><span className={`hint ${slot.x / capture.width > 0.75 ? 'right' : ''}`}>{slot.slot}</span></a>)}
  </div><figcaption><div className="legend">{names.map((slot) => <a key={slot} href={`/docs/extensions/slots/#slot-${slot}`}>{slot}</a>)}</div><a href={`/extensions/${name}.png`} target="_blank" rel="noopener noreferrer">Open full-size capture</a></figcaption></figure>;
}
