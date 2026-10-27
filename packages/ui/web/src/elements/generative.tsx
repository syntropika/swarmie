import { renderGenerativeUI } from "@assistant-ui/generative-ui/react";
import type { GenerativeUIDispatch } from "@assistant-ui/generative-ui/react";
import type { ReactNode } from "react";

import { styledGenerativeUILibrary } from "./generative-library";

export type GenerativePresentationProps = { tree: unknown; onAction?: GenerativeUIDispatch };
/** Render validated UI data. The consumer owns action handling and the application runtime. */
export function GenerativePresentation({ tree, onAction }: GenerativePresentationProps): ReactNode {
  return (
    <div className="swarmie-generative">
      {renderGenerativeUI(tree, styledGenerativeUILibrary, {
        status: "done",
        ...(onAction === undefined ? {} : { dispatch: onAction }),
      })}
    </div>
  );
}
