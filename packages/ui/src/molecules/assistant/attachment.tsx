import { useCallback, useEffect, useId, useRef, useState } from "react";
import type { ComponentPropsWithRef } from "react";

import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import { cn } from "../../classes";
import { MediaPlayer } from "./structured";
import { AssistantModal } from "./thread";

export type AttachmentImage = { src: string; alt: string };
export type AttachmentStatus = "ready" | "uploading" | "failed";
export type FileAttachmentProps = Omit<ComponentPropsWithRef<"div">, "children"> & {
  name: string;
  detail: string;
  source?: "composer" | "message";
  image?: AttachmentImage | undefined;
  status?: AttachmentStatus | undefined;
  progress?: number | undefined;
  removeLabel?: string;
  onRemove?: () => void;
  previewLabel?: string | undefined;
  closeLabel?: string | undefined;
  href?: string;
  downloadLabel?: string;
  mimeType?: string;
  captions?: string;
};

function PreviewImage({ src, alt }: AttachmentImage) {
  const [loaded, setLoaded] = useState(false);
  return (
    <img
      src={src}
      alt={alt}
      onLoad={() => {
        setLoaded(true);
      }}
      className={cn(
        "mx-auto block h-auto max-h-[80dvh] w-auto max-w-full rounded-panel object-contain transition-opacity duration-300 motion-reduce:transition-none",
        loaded ? "opacity-100" : "opacity-0",
      )}
    />
  );
}

function AttachmentThumbnail({
  image,
  status,
}: {
  image: AttachmentImage | undefined;
  status: AttachmentStatus;
}) {
  const [failedImage, setFailedImage] = useState<string | null>(null);
  return (
    <>
      {image !== undefined && failedImage !== image.src ? (
        <img
          src={image.src}
          alt=""
          onError={() => {
            setFailedImage(image.src);
          }}
          className="size-full object-cover"
        />
      ) : (
        <Icon name="file" className="size-6 text-text-muted" />
      )}
      {status === "ready" ? null : <AttachmentStatusOverlay status={status} />}
    </>
  );
}

function AttachmentStatusOverlay({ status }: { status: "uploading" | "failed" }) {
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    const element = ref.current;
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    if (!element || preference.matches) {
      return undefined;
    }
    const animation = element.animate([{ opacity: 0 }, { opacity: 1 }], {
      duration: 150,
      easing: "ease",
      fill: "both",
    });
    const reduce = () => {
      if (preference.matches) {
        animation.cancel();
      }
    };
    preference.addEventListener("change", reduce);
    return () => {
      preference.removeEventListener("change", reduce);
      animation.cancel();
    };
  }, []);
  return (
    <span
      ref={ref}
      aria-hidden="true"
      className="absolute inset-0 flex items-center justify-center bg-surface/70 backdrop-blur-xs"
    >
      <Icon
        name={status === "failed" ? "circle-attention" : "loading"}
        className={cn(
          "size-4",
          status === "failed" ? "text-danger" : "text-text-muted motion-safe:animate-spin",
        )}
      />
    </span>
  );
}

function AttachmentDownload({
  href,
  label,
}: {
  href: string | undefined;
  label: string | undefined;
}) {
  if (href === undefined || label === undefined) {
    return null;
  }
  return (
    <Button asChild variant="link" size="menu">
      <a href={href} download>
        {label}
      </a>
    </Button>
  );
}

function AttachmentUploadProgress({
  value,
  active,
  label,
}: {
  value: number | undefined;
  active: boolean;
  label: string;
}) {
  if (!active || value === undefined) {
    return null;
  }
  return (
    <progress
      className="mt-1 h-1 w-14 overflow-hidden rounded-pill accent-primary"
      aria-label={label}
      max={100}
      value={value}
    />
  );
}

function AttachmentTooltip({
  id,
  visible,
  name,
  detail,
}: {
  id: string;
  visible: boolean;
  name: string;
  detail: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = ref.current;
    if (!element) {
      return undefined;
    }
    if (visible && !element.matches(":popover-open")) {
      element.showPopover();
    }
    if (!element.matches(":popover-open")) {
      return undefined;
    }
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    const frames = visible
      ? [
          { opacity: 0, transform: "translateY(8px) scale(0.95)" },
          { opacity: 1, transform: "translateY(0) scale(1)" },
        ]
      : [
          { opacity: 1, transform: "scale(1)" },
          { opacity: 0, transform: "scale(0.95)" },
        ];
    const animation = element.animate(frames, {
      duration: preference.matches ? 0 : 150,
      easing: "ease",
      fill: "both",
    });
    animation.onfinish = () => {
      if (!visible && element.matches(":popover-open")) {
        element.hidePopover();
      }
    };
    const reduce = () => {
      if (preference.matches) {
        animation.finish();
      }
    };
    preference.addEventListener("change", reduce);
    return () => {
      preference.removeEventListener("change", reduce);
      animation.onfinish = null;
      animation.cancel();
    };
  }, [visible]);
  return (
    <div
      ref={ref}
      id={id}
      role="tooltip"
      popover="manual"
      className="pointer-events-none fixed inset-auto m-0 mb-2 w-max max-w-[min(18rem,calc(100vw-3rem))] rounded-control border-0 bg-inverse px-3 py-2 text-ui text-inverse-text [position-anchor:--assistant-attachment] [position-area:top] [position-try-fallbacks:flip-block,flip-inline]"
    >
      <span className="block break-all">{name}</span>
      <span className="block text-xs">{detail}</span>
    </div>
  );
}

function AttachmentMedia({
  source,
  href,
  mimeType,
  name,
  captions,
}: {
  source: string;
  href: string | undefined;
  mimeType: string | undefined;
  name: string;
  captions: string | undefined;
}) {
  if (source !== "message" || href === undefined || mimeType === undefined) {
    return null;
  }
  if (!mimeType.startsWith("audio/") && !mimeType.startsWith("video/")) {
    return null;
  }
  return (
    <MediaPlayer
      kind={mimeType.startsWith("audio/") ? "audio" : "video"}
      src={href}
      label={name}
      {...(captions === undefined ? {} : { captions })}
    />
  );
}

/** Runtime-neutral attachment tile. The host owns upload state, URLs, progress, and removal. */
export function FileAttachment({
  name,
  detail,
  source = "composer",
  image,
  status = "ready",
  progress,
  removeLabel,
  onRemove,
  previewLabel,
  closeLabel,
  href,
  downloadLabel,
  mimeType,
  captions,
  className,
  ref,
  onMouseEnter,
  onMouseLeave,
  ...props
}: FileAttachmentProps) {
  const [open, setOpen] = useState(false);
  const [tooltip, setTooltip] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const attachRoot = useCallback(
    (element: HTMLDivElement | null) => {
      root.current = element;
      if (typeof ref === "function") {
        return ref(element);
      }
      if (ref !== undefined && ref !== null) {
        ref.current = element;
      }
      return undefined;
    },
    [ref],
  );
  const tooltipId = useId();
  const state =
    progress !== undefined && progress < 100 && status === "ready" ? "uploading" : status;
  const canPreview = image !== undefined && previewLabel !== undefined && closeLabel !== undefined;
  useEffect(() => {
    const element = root.current;
    if (
      !element ||
      source !== "composer" ||
      window.matchMedia("(prefers-reduced-motion: reduce)").matches
    ) {
      return undefined;
    }
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    const animation = element.animate(
      [
        { opacity: 0, transform: "scale(0.95)" },
        { opacity: 1, transform: "scale(1)" },
      ],
      { duration: 200, easing: "ease", fill: "both" },
    );
    const stop = () => {
      if (preference.matches) {
        animation.cancel();
      }
    };
    preference.addEventListener("change", stop);
    return () => {
      preference.removeEventListener("change", stop);
      animation.cancel();
    };
  }, [source]);
  const tileClassName =
    "relative flex size-14 shrink-0 items-center justify-center overflow-hidden rounded-panel bg-surface-muted p-0 text-ink transition-transform duration-150 hover:bg-surface-hover active:scale-[0.96] motion-reduce:transition-none";
  return (
    <div
      {...props}
      ref={attachRoot}
      data-source={source}
      data-status={state}
      className={cn("relative w-fit shrink-0 [anchor-scope:all]", className)}
      onMouseEnter={(event) => {
        onMouseEnter?.(event);
        setTooltip(true);
      }}
      onMouseLeave={(event) => {
        onMouseLeave?.(event);
        setTooltip(false);
      }}
    >
      <Button
        variant="ghost"
        size="icon"
        aria-label={canPreview ? previewLabel : name}
        aria-describedby={tooltipId}
        aria-busy={state === "uploading"}
        className={cn(
          tileClassName,
          "[anchor-name:--assistant-attachment]",
          canPreview && "cursor-zoom-in",
        )}
        onFocus={() => {
          setTooltip(true);
        }}
        onBlur={() => {
          setTooltip(false);
        }}
        onClick={() => {
          if (canPreview) {
            setOpen(true);
          }
        }}
      >
        <AttachmentThumbnail image={image} status={state} />
      </Button>
      {source === "composer" && onRemove !== undefined && removeLabel !== undefined ? (
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`${removeLabel}: ${name}`}
          className="group absolute top-0 right-0 size-5 translate-x-1/2 -translate-y-1/2 text-inverse-text hover:bg-transparent active:scale-[0.96] pointer-coarse:size-control-compact"
          onClick={onRemove}
        >
          <span className="inline-flex size-4 items-center justify-center rounded-full bg-inverse/70 transition-colors duration-150 group-hover:bg-inverse motion-reduce:transition-none">
            <Icon name="close" className="size-2.5" />
          </span>
        </Button>
      ) : null}
      <AttachmentTooltip id={tooltipId} visible={tooltip && !open} name={name} detail={detail} />
      <AttachmentUploadProgress value={progress} active={state === "uploading"} label={detail} />
      {canPreview &&
      image !== undefined &&
      previewLabel !== undefined &&
      closeLabel !== undefined ? (
        <AssistantModal
          open={open}
          onOpenChange={setOpen}
          title={previewLabel}
          closeLabel={closeLabel}
        >
          {open ? <PreviewImage key={image.src} {...image} /> : null}
        </AssistantModal>
      ) : null}
      <AttachmentDownload href={href} label={downloadLabel} />
      <AttachmentMedia
        source={source}
        href={href}
        mimeType={mimeType}
        name={name}
        captions={captions}
      />
    </div>
  );
}

export type MessageAttachmentItem = {
  id: string;
  name: string;
  detail: string;
  kind: "image" | "document" | "file";
  image?: AttachmentImage | undefined;
};

/** Received-file rows; opening a file remains a host callback. */
export function MessageAttachments({
  attachments,
  onOpen,
  className,
  ...props
}: Omit<ComponentPropsWithRef<"div">, "children"> & {
  attachments: readonly MessageAttachmentItem[];
  onOpen?: (id: string) => void;
}) {
  if (attachments.length === 0) {
    return null;
  }
  return (
    <div {...props} className={cn("flex w-full max-w-sm flex-col gap-1.5", className)}>
      {attachments.map((item) => {
        const content = (
          <>
            <span
              aria-hidden="true"
              className={cn(
                "flex shrink-0 items-center justify-center overflow-hidden rounded-control bg-surface-muted text-text-muted",
                item.kind === "image"
                  ? "size-12 transition-transform duration-300 group-hover/file:scale-[1.04] motion-reduce:transition-none"
                  : "size-8",
              )}
            >
              {item.image === undefined ? (
                <Icon name="file" className="size-4" />
              ) : (
                <img src={item.image.src} alt="" className="size-full object-cover" />
              )}
            </span>
            <span className="flex min-w-0 flex-1 flex-col gap-0.5">
              <span className="truncate text-ui">{item.name}</span>
              <span className="text-xs text-text-muted">{item.detail}</span>
            </span>
          </>
        );
        return onOpen === undefined ? (
          <div
            key={item.id}
            className="flex items-center gap-3 rounded-panel bg-surface-muted px-3 py-2 motion-safe:animate-assistant-receipt"
          >
            {content}
          </div>
        ) : (
          <Button
            key={item.id}
            variant="quiet"
            className="group/file flex w-full items-center justify-start gap-3 rounded-panel bg-surface-muted px-3 py-2 text-left transition-colors duration-300 motion-safe:animate-assistant-receipt motion-reduce:transition-none"
            onClick={() => {
              onOpen(item.id);
            }}
          >
            {content}
          </Button>
        );
      })}
    </div>
  );
}

/** Sent-message tiles omit composer removal; one image uses the reference's larger thumbnail. */
export function UserMessageAttachments({
  attachments,
  previewLabel,
  closeLabel,
  label,
}: {
  attachments: readonly MessageAttachmentItem[];
  previewLabel: (name: string) => string;
  closeLabel: string;
  label: string;
}) {
  if (attachments.length === 0) {
    return null;
  }
  return (
    <ul aria-label={label} className="flex w-full flex-wrap justify-end gap-2">
      {attachments.map((item) => (
        <li
          key={item.id}
          className={cn(
            attachments.length === 1 && item.kind === "image" && "[&>div>button]:size-24",
          )}
        >
          <FileAttachment
            name={item.name}
            detail={item.detail}
            source="message"
            image={item.image}
            previewLabel={previewLabel(item.name)}
            closeLabel={closeLabel}
          />
        </li>
      ))}
    </ul>
  );
}
