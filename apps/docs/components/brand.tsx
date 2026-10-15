export function SwarmieLogo() {
  return (
    <span className="swarmie-brand">
      <img
        className="brand-logo-light"
        src="/brand/swarmie-mono-ink.svg"
        width={130}
        height={36}
        alt=""
        aria-hidden="true"
      />
      <img
        className="brand-logo-dark"
        src="/brand/swarmie-mono-white.svg"
        width={130}
        height={36}
        alt=""
        aria-hidden="true"
      />
      <span className="sr-only">Swarmie</span>
    </span>
  );
}
