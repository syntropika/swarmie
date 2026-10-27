/** HTTP local development can copy through a user-initiated selection without a secure-context API. */
export async function copyText(text: string): Promise<void> {
  if (navigator.clipboard !== undefined) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const previous = document.activeElement;
  const input = document.createElement("textarea");
  input.value = text;
  input.readOnly = true;
  input.className = "fixed -left-[10000px] top-0";
  // oxlint-disable-next-line unicorn/prefer-dom-node-append -- Worker consumers also declare HTMLRewriter Element.append; appendChild retains the DOM Node signature.
  document.body.appendChild(input);
  try {
    input.select();
    // oxlint-disable-next-line typescript/no-deprecated -- HTTP local origins lack the Clipboard API; this user-initiated browser fallback is the available copy mechanism.
    if (!document.execCommand("copy")) {
      throw new Error("The browser did not allow clipboard access.");
    }
  } finally {
    input.remove();
    if (previous instanceof HTMLElement) {
      previous.focus({ preventScroll: true });
    }
  }
}
