import { createElement, useId } from "react";

/** Hooks deliberately verify that the renderer and loaded contribution share React identity. */
export function Widget(props: { readonly label: string }) {
  const id = useId();
  return createElement("p", { id, "data-contribution": "widget" }, props.label);
}
