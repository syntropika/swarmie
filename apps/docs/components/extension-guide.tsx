import guide from "~/lib/extension-guide.json";

export function SlotIndex() {
  return (
    <nav className="slot-index" aria-label="Contribution slot index">
      {guide.slots.map((slot) => (
        <a key={slot.id} href={`#slot-${slot.id}`}>
          {slot.id}
        </a>
      ))}
    </nav>
  );
}
export function AnnotatedCapture({ name, title }: { name: string; title: string }) {
  const capture = guide.captures.find((entry) => entry.name === name);
  if (!capture) {
    throw new Error(`Unknown documentation capture: ${name}`);
  }
  const names = [...new Set(capture.slots.map((slot) => slot.slot))];
  return (
    <figure className="extension-capture">
      <div className="annotated">
        <img
          src={`/extensions/${name}.png`}
          width={capture.width}
          height={capture.height}
          alt={`${title}: implemented Swarmie interface with fixture workspace data`}
          loading="lazy"
        />
        <svg
          className="absolute inset-0 h-full w-full"
          viewBox={`0 0 ${capture.width} ${capture.height}`}
          preserveAspectRatio="none"
          aria-label="Contribution slot links"
        >
          {capture.slots.map((slot) => (
            <a
              key={`${slot.slot}:${slot.x}:${slot.y}:${slot.width}:${slot.height}`}
              data-slot-name={slot.slot}
              className="group"
              href={`/docs/extensions/slots/#slot-${slot.slot}`}
              aria-label={`View the ${slot.slot} contract`}
            >
              <title>{slot.slot}</title>
              <rect
                x={slot.x}
                y={slot.y}
                width={slot.width}
                height={slot.height}
                fill="transparent"
                className="stroke-transparent group-hover:stroke-current group-focus:stroke-current"
              />
              <text
                x={slot.x + 3}
                y={slot.y + 14}
                fontSize={12}
                className="fill-current opacity-0 group-hover:opacity-100 group-focus:opacity-100"
              >
                {slot.slot}
              </text>
            </a>
          ))}
        </svg>
      </div>
      <figcaption>
        <div className="legend">
          {names.map((slot) => (
            <a key={slot} href={`/docs/extensions/slots/#slot-${slot}`}>
              {slot}
            </a>
          ))}
        </div>
        <a href={`/extensions/${name}.png`} target="_blank" rel="noopener noreferrer">
          Open full-size capture
        </a>
      </figcaption>
    </figure>
  );
}
