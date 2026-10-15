import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { ComponentPropsWithRef } from "react";

import { cn } from "../../classes";
import { AssistantProgress } from "./shared";

const TRACK = 224;
const HEIGHT = 36;
const RADIUS = HEIGHT / 2;
const BUMP_H = 28;
const REST_W = 31.2;
const DRAG_W = 37.6;
const REST_H = 31.2;
const EDGE_PAD = (HEIGHT - REST_H) / 2;

/** A labeled discrete stop on the continuously draggable effort control. */
export type ReasoningEffortSliderOption = { value: string; label: string };

/** The caller owns the selected value, model name, and optional real usage measure. */
export type ReasoningEffortSliderProps = Omit<ComponentPropsWithRef<"div">, "onChange"> & {
  label: string;
  value: string;
  options: readonly ReasoningEffortSliderOption[];
  onChange: (value: string) => void;
  model?: string;
  disabled?: boolean;
  usage?: { value: number; max: number; label: string };
};

function clamp(n: number, min: number, max: number) {
  return Math.min(max, Math.max(min, n));
}

function centerFor(value: number, count: number) {
  const span = TRACK - RADIUS * 2;
  return RADIUS + (value / (count - 1)) * span;
}

function easeOut(t: number) {
  return 1 - (1 - t) ** 3;
}

const LEFT_SHOULDER = 23.572;
/** Space from each glyph edge to the foot of the label shoulder. */
const TEXT_SIDE = 20;
/** How far the outer foot walks down the cap once it passes the crown. */
const PHI_MAX = 0.6;
const PHI_REACH = 36;

function plateauFor(textW: number) {
  return Math.max(8, textW + TEXT_SIDE * 2 - LEFT_SHOULDER * 2);
}

function smoothstep(t: number) {
  const x = clamp(t, 0, 1);
  return x * x * (3 - 2 * x);
}

function capFoot(side: -1 | 1, phi: number) {
  const crown = side === -1 ? RADIUS : TRACK - RADIUS;
  return {
    x: crown + side * RADIUS * Math.sin(phi),
    y: BUMP_H + RADIUS * (1 - Math.cos(phi)),
  };
}

function shellGeom(cx: number, plateauW: number) {
  const crownL = RADIUS;
  const crownR = TRACK - RADIUS;
  let topL = cx - plateauW / 2;
  let topR = topL + plateauW;
  let phiL = 0;
  let phiR = 0;

  const overflowL = crownL - (topL - LEFT_SHOULDER);
  if (overflowL > 0) {
    phiL = PHI_MAX * smoothstep(overflowL / PHI_REACH);
    const foot = capFoot(-1, phiL);
    const minTopL = foot.x + LEFT_SHOULDER * Math.cos(phiL);
    if (topL < minTopL) {
      topR += minTopL - topL;
      topL = minTopL;
    }
  }

  const overflowR = topR + LEFT_SHOULDER - crownR;
  if (overflowR > 0) {
    phiR = PHI_MAX * smoothstep(overflowR / PHI_REACH);
    const foot = capFoot(1, phiR);
    const maxTopR = foot.x - LEFT_SHOULDER * Math.cos(phiR);
    if (topR > maxTopR) {
      topL -= topR - maxTopR;
      topR = maxTopR;
    }
  }

  // The tilted shoulder is shorter in x, which would pull the text onto the
  // outer edge. Grow the plateau inward by that loss and keep the label with it.
  const insetL = phiL > 0 ? LEFT_SHOULDER * (1 - Math.cos(phiL)) : 0;
  const insetR = phiR > 0 ? LEFT_SHOULDER * (1 - Math.cos(phiR)) : 0;
  topR += insetL;
  topL -= insetR;

  return { topL, topR, phiL, phiR, insetL, insetR };
}

/** Reference shoulder. phi tilts the foot onto the cap tangent; 0 is the flat scoop. */
function fmt(p: { x: number; y: number }) {
  return `${p.x.toFixed(2)} ${p.y.toFixed(2)}`;
}

function shoulderCmds(
  footX: number,
  footY: number,
  platX: number,
  platY: number,
  phi: number,
  dir: 1 | -1,
) {
  const xScale = Math.abs(platX - footX) / LEFT_SHOULDER;
  const yScale = Math.max(0.001, (footY - platY) / 20);
  const tangent = { x: dir * Math.cos(phi), y: -Math.sin(phi) };
  const map = (lx: number, ly: number) => ({
    x: footX + dir * lx * xScale,
    y: footY + (ly - 20) * yScale,
  });
  const h1 = 5.398 * xScale;
  const p0 = { x: footX, y: footY };
  const c1 = { x: footX + tangent.x * h1, y: footY + tangent.y * h1 };
  const c2 = map(10.012, 16.114);
  const p1 = map(10.93, 10.794);
  const c3 = map(11.916, 4.577);
  const c4 = map(17.277, 0);
  const p3 = { x: platX, y: platY };
  if (dir === 1) {
    return `C${fmt(c1)} ${fmt(c2)} ${fmt(p1)}C${fmt(c3)} ${fmt(c4)} ${fmt(p3)}`;
  }
  return `C${fmt(c4)} ${fmt(c3)} ${fmt(p1)}C${fmt(c2)} ${fmt(c1)} ${fmt(p0)}`;
}

/** One outline: the pill, with the label shoulder grown out of its top edge. */
function shellPath(cx: number, open: number, plateauW: number) {
  const trackTop = BUMP_H;
  const trackBot = BUMP_H + HEIGHT;
  const crownL = RADIUS;
  const crownR = TRACK - RADIUS;
  if (open < 0.012) {
    return `M${crownL} ${trackTop}H${crownR}A${RADIUS} ${RADIUS} 0 0 1 ${crownR} ${trackBot}H${crownL}A${RADIUS} ${RADIUS} 0 0 1 ${crownL} ${trackTop}Z`;
  }

  const bumpTop = BUMP_H * (1 - open);
  const { topL, topR, phiL, phiR } = shellGeom(cx, plateauW);
  const cmds = [`M${crownL} ${trackBot}`];

  if (phiL > 0.001) {
    const foot = capFoot(-1, phiL);
    cmds.push(
      `A${RADIUS} ${RADIUS} 0 0 1 ${foot.x.toFixed(2)} ${foot.y.toFixed(2)}`,
      shoulderCmds(foot.x, foot.y, topL, bumpTop, phiL, 1),
    );
  } else {
    cmds.push(`A${RADIUS} ${RADIUS} 0 0 1 ${crownL} ${trackTop}`);
    const footL = topL - LEFT_SHOULDER;
    if (footL > crownL + 0.4) {
      cmds.push(`H${footL.toFixed(2)}`);
    }
    cmds.push(shoulderCmds(footL, trackTop, topL, bumpTop, 0, 1));
  }

  if (topR > topL + 0.4) {
    cmds.push(`H${topR.toFixed(2)}`);
  }

  if (phiR > 0.001) {
    const foot = capFoot(1, phiR);
    cmds.push(shoulderCmds(foot.x, foot.y, topR, bumpTop, phiR, -1));
  } else {
    const footR = topR + LEFT_SHOULDER;
    cmds.push(shoulderCmds(footR, trackTop, topR, bumpTop, 0, -1));
    if (footR < crownR - 0.4) {
      cmds.push(`H${crownR}`);
    }
  }

  cmds.push(`A${RADIUS} ${RADIUS} 0 0 1 ${crownR} ${trackBot}`, `H${crownL}Z`);
  return cmds.join("");
}

function pointerValue(element: HTMLInputElement, clientX: number, count: number) {
  const rect = element.getBoundingClientRect();
  const x = ((clientX - rect.left) / rect.width) * TRACK;
  return clamp(((x - RADIUS) / (TRACK - RADIUS * 2)) * (count - 1), 0, count - 1);
}

type SliderGeometry = { display: number; open: number; reveal: number; plateau: number };

/** Independent motion channels share preference handling without restarting one another. */
function animateGeometry(duration: number, delay: number, update: (progress: number) => void) {
  const media = window.matchMedia("(prefers-reduced-motion: reduce)");
  let frame = 0;
  const finish = () => {
    cancelAnimationFrame(frame);
    update(1);
  };
  const start = performance.now();
  const step = (now: number) => {
    const elapsed = now - start - delay;
    if (elapsed >= 0) {
      update(easeOut(Math.min(1, elapsed / duration)));
    }
    if (elapsed < duration) {
      frame = requestAnimationFrame(step);
    }
  };
  if (media.matches) {
    finish();
  } else {
    frame = requestAnimationFrame(step);
  }
  const onPreference = () => {
    if (media.matches) {
      finish();
    }
  };
  media.addEventListener("change", onPreference);
  return () => {
    cancelAnimationFrame(frame);
    media.removeEventListener("change", onPreference);
  };
}

/** Native range keyboard semantics with the source's morphing shoulder, free drag, and snap. */
export function ReasoningEffortSlider({
  label,
  value,
  options,
  onChange,
  model,
  disabled = false,
  usage,
  className,
  ...props
}: ReasoningEffortSliderProps) {
  const selected = Math.max(
    0,
    options.findIndex((option) => option.value === value),
  );
  const [dragValue, setDragValue] = useState<number | null>(null);
  const dragging = useRef(false);
  const continuousValue = useRef(selected);
  const snapping = useRef(false);
  const [geometry, setGeometry] = useState<SliderGeometry>({
    display: selected,
    open: 0,
    reveal: 0,
    plateau: plateauFor(64),
  });
  const current = useRef(geometry);
  const updateGeometry = useCallback((patch: Partial<SliderGeometry>) => {
    const next = { ...current.current, ...patch };
    current.current = next;
    setGeometry(next);
  }, []);
  const labelRef = useRef<SVGTextElement>(null);
  const [textWidth, setTextWidth] = useState(64);
  const clipId = useId();
  const active = Math.round(dragValue ?? selected);
  const option = options[active];
  const labelText = [model, option?.label].filter(Boolean).join(" ");
  const engaged = dragValue !== null;
  const plateauTarget = plateauFor(textWidth);

  useLayoutEffect(() => {
    if (!labelText) {
      return;
    }
    const width = labelRef.current?.getBBox().width;
    if (width !== undefined && width > 0) {
      setTextWidth(width);
    }
  }, [labelText]);

  // Dragging opens the shoulder once. Only releasing a drag starts the discrete snap.
  useEffect(() => {
    const from = current.current;
    const targetOpen = engaged ? 1 : 0;
    const targetDisplay = Math.round(continuousValue.current);
    snapping.current = !engaged;
    if (from.open === targetOpen && (engaged || from.display === targetDisplay)) {
      snapping.current = false;
      return () => {
        // No motion frame was needed for an already settled control.
      };
    }
    return animateGeometry(220, 0, (progress) => {
      const patch: Partial<SliderGeometry> = {
        open: from.open + (targetOpen - from.open) * progress,
      };
      if (!engaged) {
        patch.display = from.display + (targetDisplay - from.display) * progress;
      }
      updateGeometry(patch);
      if (progress === 1) {
        snapping.current = false;
      }
    });
  }, [engaged, updateGeometry]);

  // A new stop label adjusts its shoulder width without restarting opening or reveal.
  useEffect(() => {
    const from = current.current.plateau;
    if (Math.abs(plateauTarget - from) < 0.3) {
      updateGeometry({ plateau: plateauTarget });
      return () => {
        // The measured width is already within the source's settling threshold.
      };
    }
    return animateGeometry(90, 0, (progress) => {
      updateGeometry({ plateau: from + (plateauTarget - from) * progress });
    });
  }, [plateauTarget, updateGeometry]);

  // The delayed label reveal is tied to drag boundaries, never pointer movement.
  useEffect(() => {
    const from = current.current.reveal;
    const target = engaged ? 1 : 0;
    return animateGeometry(engaged ? 132 : 90, engaged ? 85 : 0, (progress) => {
      updateGeometry({ reveal: from + (target - from) * progress });
    });
  }, [engaged, updateGeometry]);

  // Controlled changes outside a gesture settle directly, matching native keyboard entry.
  useEffect(() => {
    if (!dragging.current && !snapping.current) {
      continuousValue.current = selected;
      updateGeometry({ display: selected });
    }
  }, [selected, updateGeometry]);

  if (options.length < 2) {
    return null;
  }
  const center = centerFor(geometry.display, options.length);
  const thumbWidth = REST_W + (DRAG_W - REST_W) * geometry.open;
  const thumbLeft = clamp(center - thumbWidth / 2, EDGE_PAD, TRACK - EDGE_PAD - thumbWidth);
  const fillWidth = Math.min(TRACK, thumbLeft + thumbWidth + EDGE_PAD);
  const plateau = shellGeom(center, geometry.plateau);
  const labelX = (plateau.topL + plateau.topR + plateau.insetL - plateau.insetR) / 2;
  const commit = (next: number) => {
    const target = options[clamp(Math.round(next), 0, options.length - 1)];
    if (target && target.value !== value) {
      onChange(target.value);
    }
  };

  return (
    <div {...props} className={cn("flex flex-col gap-3", className)}>
      <div className="relative h-16 w-56 max-w-full select-none pointer-coarse:h-18">
        <svg
          aria-hidden="true"
          viewBox={`0 0 ${TRACK} ${BUMP_H + HEIGHT}`}
          className="pointer-events-none absolute inset-0 size-full overflow-visible"
        >
          <defs>
            <filter id={`${clipId}-blur`} x="-50%" y="-50%" width="200%" height="200%">
              <feGaussianBlur stdDeviation={3 * (1 - geometry.reveal)} />
            </filter>
            <clipPath id={clipId}>
              <path d={shellPath(center, geometry.open, geometry.plateau)} />
            </clipPath>
          </defs>
          <path
            d={shellPath(center, geometry.open, geometry.plateau)}
            className="fill-surface-muted"
          />
          <rect
            x={0}
            y={BUMP_H}
            width={fillWidth}
            height={HEIGHT}
            rx={RADIUS}
            clipPath={`url(#${clipId})`}
            className="fill-primary"
          />
          <rect
            x={thumbLeft}
            y={BUMP_H + EDGE_PAD}
            width={thumbWidth}
            height={REST_H}
            rx={REST_H / 2}
            className="fill-surface"
          />
          {options.map((stop, index) => {
            const x = centerFor(index, options.length);
            const height = 4.8 + index * 1.6;
            const onThumb = x >= thumbLeft && x <= thumbLeft + thumbWidth;
            return (
              <rect
                key={stop.value}
                x={x - 0.75}
                y={BUMP_H + HEIGHT / 2 - height / 2}
                width={1.5}
                height={height}
                rx={0.75}
                className={cn(onThumb ? "fill-text-muted" : "fill-text-muted/40")}
              />
            );
          })}
          <g
            opacity={geometry.reveal}
            filter={`url(#${clipId}-blur)`}
            transform={`translate(0 ${3 * (1 - geometry.open)})`}
          >
            <text
              key={labelText}
              ref={labelRef}
              x={labelX}
              y={19}
              textAnchor="middle"
              className="fill-ink text-ui font-medium motion-safe:animate-assistant-label-swap"
            >
              {model === undefined ? null : <tspan>{model} </tspan>}
              <tspan className="fill-text-muted">{option?.label}</tspan>
            </text>
          </g>
        </svg>
        <label className="absolute inset-x-0 bottom-0 h-control rounded-pill focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-primary pointer-coarse:h-control-touch">
          <span className="sr-only">{label}</span>
          <input
            type="range"
            min={0}
            max={options.length - 1}
            step={dragValue === null ? 1 : "any"}
            value={dragValue ?? selected}
            disabled={disabled}
            aria-valuetext={labelText}
            className="size-full cursor-grab touch-none opacity-0 active:cursor-grabbing disabled:cursor-not-allowed"
            onBlur={() => {
              dragging.current = false;
              setDragValue(null);
            }}
            onPointerDown={(event) => {
              if (event.button !== 0 || disabled) {
                return;
              }
              dragging.current = true;
              event.currentTarget.setPointerCapture(event.pointerId);
              const next = pointerValue(event.currentTarget, event.clientX, options.length);
              continuousValue.current = next;
              updateGeometry({ display: next });
              setDragValue(next);
              commit(next);
            }}
            onPointerMove={(event) => {
              if (dragging.current && !disabled) {
                const next = pointerValue(event.currentTarget, event.clientX, options.length);
                continuousValue.current = next;
                updateGeometry({ display: next });
                setDragValue(next);
                commit(next);
              }
            }}
            onPointerUp={(event) => {
              if (!dragging.current || disabled) {
                return;
              }
              dragging.current = false;
              if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                event.currentTarget.releasePointerCapture(event.pointerId);
              }
              const next = pointerValue(event.currentTarget, event.clientX, options.length);
              continuousValue.current = next;
              updateGeometry({ display: next });
              commit(next);
              setDragValue(null);
            }}
            onPointerCancel={() => {
              dragging.current = false;
              setDragValue(null);
            }}
            onChange={(event) => {
              const next = Number(event.currentTarget.value);
              if (!dragging.current) {
                continuousValue.current = next;
                updateGeometry({ display: next });
                commit(next);
              }
            }}
          />
        </label>
      </div>
      {usage ? <AssistantProgress {...usage} /> : null}
    </div>
  );
}
