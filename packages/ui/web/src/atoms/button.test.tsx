import { expect, test } from "bun:test";

import { renderToStaticMarkup } from "react-dom/server";

import { Button } from "./button";

test("preserves workflow-owned busy state while loading state takes precedence", () => {
  const idle = renderToStaticMarkup(<Button aria-busy={false}>Save</Button>);
  const externallyBusy = renderToStaticMarkup(<Button aria-busy>Save</Button>);
  const loading = renderToStaticMarkup(
    <Button aria-busy={false} loading loadingLabel="Saving changes">
      Save
    </Button>,
  );
  expect(idle).toContain('aria-busy="false"');
  expect(externallyBusy).toContain('aria-busy="true"');
  expect(loading).toContain('aria-busy="true"');
  expect(loading).toContain('disabled=""');
  expect(loading).toContain("Saving changes");
});

test("preserves custom rendering and render children through the shared root", () => {
  const markup = renderToStaticMarkup(
    <Button.Root
      loading
      loadingLabel="Saving changes"
      title="Save changes"
      render={(props) => <button {...props} type="button" data-custom-render="true" />}
    >
      {({ isPending }) => (isPending ? "Saving" : "Save")}
    </Button.Root>,
  );
  expect(markup).toContain('data-custom-render="true"');
  expect(markup).toContain('title="Save changes"');
  expect(markup).toContain('aria-busy="true"');
  expect(markup).toContain("Saving");
  expect(markup).toContain("Saving changes");
});
