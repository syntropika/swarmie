import type { ReactNode } from "react";

import { Skeleton } from "../atoms/skeleton";
import { cn } from "../classes";

type LoadingProps = { readonly label?: string; readonly className?: string };

function Frame({ label, className, children }: LoadingProps & { readonly children: ReactNode }) {
  return (
    <div
      // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- Loading status is not a calculation result.
      role="status"
      aria-busy="true"
      aria-label={label}
      className={className}
      data-loading-skeleton
    >
      <span className="sw-loading-label">{label}</span>
      <div aria-hidden="true" className="sw-loading-shapes">
        {children}
      </div>
    </div>
  );
}

function Line({
  width = "100%",
  className,
}: {
  readonly width?: string;
  readonly className?: string;
}) {
  return (
    <Skeleton
      animationType="none"
      className={cn("sw-loading-line", className)}
      data-width={width}
    />
  );
}

function Rows({ rows = 3, detail = false }: { readonly rows?: number; readonly detail?: boolean }) {
  return (
    <div className="sw-loading-rows">
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="sw-loading-row">
          <Line width={index % 2 === 0 ? "72%" : "54%"} />
          {detail ? <Line width="42%" className="sw-loading-secondary" /> : null}
        </div>
      ))}
    </div>
  );
}

/** Reserve list geometry only while initial data or another page is unavailable. */
export function LoadingRows({
  label = "Loading items",
  className,
  rows = 3,
  detail = false,
}: LoadingProps & { readonly rows?: number; readonly detail?: boolean }) {
  return (
    <Frame label={label} className={cn("sw-loading-list", className)}>
      <Rows rows={rows} detail={detail} />
    </Frame>
  );
}

export function ContentSkeleton({ label = "Loading content", className }: LoadingProps) {
  return (
    <Frame label={label} className={cn("sw-loading-document", className)}>
      <Line width="38%" className="sw-loading-heading" />
      <Rows rows={4} detail />
    </Frame>
  );
}

export function MessageSkeleton({
  label = "Loading conversation history",
  className,
}: LoadingProps) {
  return (
    <Frame label={label} className={cn("sw-loading-history", className)}>
      <Line width="24%" />
      <Line width="68%" />
      <div className="sw-loading-answer">
        <Line width="18%" />
        <Line />
        <Line width="92%" />
        <Line width="76%" />
      </div>
    </Frame>
  );
}

function ConversationShapes({ empty = false }: { readonly empty?: boolean }) {
  return (
    <>
      <div className="sw-loading-conversation-body">
        {empty ? (
          <div className="sw-loading-intro">
            <Line width="68%" className="sw-loading-heading" />
            <Line />
            <Line width="80%" />
          </div>
        ) : (
          <Rows rows={3} detail />
        )}
      </div>
      <div className="sw-loading-composer">
        <Line width="30%" />
        <Line width="42%" />
        <div className="sw-loading-composer-controls">
          <Line width="20px" />
          <Line width="95px" />
        </div>
      </div>
    </>
  );
}

export function ConversationSkeleton({ label = "Loading conversation", className }: LoadingProps) {
  return (
    <Frame label={label} className={cn("sw-loading-conversation", className)}>
      <ConversationShapes />
    </Frame>
  );
}

function NavigationShapes() {
  return (
    <>
      <div className="sw-loading-navigation-header">
        <Line width="58%" />
      </div>
      <div className="sw-loading-navigation-body">
        <Line width="34%" />
        <Rows rows={2} />
        <Line width="44%" />
        <Rows rows={4} />
      </div>
    </>
  );
}

export function NavigationSkeleton({ label = "Loading navigation", className }: LoadingProps) {
  return (
    <Frame label={label} className={cn("sw-loading-navigation", className)}>
      <NavigationShapes />
    </Frame>
  );
}

export function SettingsSkeleton() {
  return (
    <Frame label="Loading settings" className="sw-loading-settings">
      <div className="sw-loading-settings-navigation">
        <Rows rows={5} />
      </div>
      <div className="sw-loading-settings-content">
        <Line width="32%" className="sw-loading-heading" />
        <Rows rows={5} detail />
      </div>
    </Frame>
  );
}

/** The same placeholder is rendered into the initial HTML and the React connection boundary. */
export function ApplicationSkeleton() {
  return (
    <Frame label="Loading Swarmie" className="sw-loading-app">
      <div className="sw-loading-shell">
        <div className="sw-loading-rail">
          <Line width="22px" className="sw-loading-mark" />
          <Line width="18px" />
          <Line width="18px" />
          <div className="sw-loading-rail-footer">
            <Line width="18px" />
          </div>
        </div>
        <div className="sw-loading-topbar">
          <Line width="110px" />
          <Line width="175px" />
        </div>
        <div className="sw-loading-navigation sw-loading-sidebar">
          <NavigationShapes />
        </div>
        <div className="sw-loading-conversation">
          <ConversationShapes empty />
        </div>
      </div>
    </Frame>
  );
}

/** Keep known content in place; only missing initial content receives a placeholder. */
export function PendingContent({
  pending,
  hasContent = false,
  failed = false,
  label,
  children,
}: {
  readonly pending: boolean;
  readonly hasContent?: boolean;
  readonly failed?: boolean;
  readonly label: string;
  readonly children: ReactNode;
}): ReactNode {
  if (!pending || failed) {
    return null;
  }
  if (hasContent) {
    return <output className="sw-loading-label">{label}</output>;
  }
  return children;
}
