export function SwarmieLogo() {
  return (
    <span className="swarmie-brand">
      <img
        className="brand-logo-light"
        src="/brand/swarmie-ink-coral.svg"
        width={130}
        height={36}
        alt=""
        aria-hidden="true"
      />
      <img
        className="brand-logo-dark"
        src="/brand/swarmie-white-coral.svg"
        width={130}
        height={36}
        alt=""
        aria-hidden="true"
      />
      <span className="sr-only">Swarmie</span>
    </span>
  );
}
