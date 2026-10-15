import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "~/src/app";

import "~/src/styles.css";

const root = document.querySelector("#root");
if (root === null) {
  throw new Error("The desktop root is missing.");
}
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
