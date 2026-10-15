import { cn } from "@swarmie/ui-web";
import type { ReactNode } from "react";

import type { AuthenticationMethod } from "~/src/identity/shared/authentication-method";

const routes: { method: AuthenticationMethod; path: string; node: ReactNode }[] = [
  {
    method: "password",
    path: "M64 31H119C161 31 161 75 203 75",
    node: <circle className="fill-mint" cx="52" cy="31" r="11" />,
  },
  {
    method: "magic",
    path: "M66 75H203",
    node: <rect className="fill-peach" x="38" y="65" width="28" height="20" rx="10" />,
  },
  {
    method: "passkey",
    path: "M65 119H119C161 119 161 75 203 75",
    node: (
      <rect
        className="fill-lilac"
        x="42"
        y="109"
        width="20"
        height="20"
        rx="5"
        transform="rotate(45 52 119)"
      />
    ),
  },
];

/** Three quiet routes share one threshold; the selected method receives the blue path. */
export function EntryPaths({ method }: { method: AuthenticationMethod }) {
  return (
    <svg
      className="block h-auto w-full"
      viewBox="0 0 388 150"
      width="388"
      height="150"
      fill="none"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect className="fill-surface" x="285" y="29" width="70" height="92" rx="14" />
      <rect className="fill-primary-soft" x="307" y="42" width="34" height="66" rx="8" />
      <path
        className="stroke-primary"
        d="M285 63V43A14 14 0 0 1 299 29H341A14 14 0 0 1 355 43V107A14 14 0 0 1 341 121H299A14 14 0 0 1 285 107V87"
        strokeWidth="1.3"
      />
      <path
        className="stroke-drawing-line"
        d="M307 56V50A8 8 0 0 1 315 42H333A8 8 0 0 1 341 50V100A8 8 0 0 1 333 108H315A8 8 0 0 1 307 100V94"
        strokeWidth="1"
      />
      {routes.map((route) => (
        <g
          key={route.method}
          className={cn(
            "stroke-drawing-line stroke-1",
            method === route.method && "stroke-primary stroke-2",
          )}
        >
          <path d={route.path} />
          {route.node}
        </g>
      ))}
      <path className="stroke-primary" d="M203 75H318" strokeWidth="1.3" />
      <circle className="fill-butter stroke-primary" cx="324" cy="75" r="6" strokeWidth="1.3" />
    </svg>
  );
}
