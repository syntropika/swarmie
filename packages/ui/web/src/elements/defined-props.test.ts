import { describe, expect, test } from "bun:test";

import { definedProps } from "./defined-props";

const callback = () => "selected";

describe("native control prop adaptation", () => {
  test("omits only undefined and preserves values, callbacks and the source object", () => {
    const ref = { current: null };
    const source = {
      value: "",
      disabled: false,
      count: 0,
      title: undefined,
      nullable: null,
      onClick: callback,
      ref,
    };
    const adapted = definedProps(source);
    expect(Object.hasOwn(adapted, "title")).toBe(false);
    expect(adapted.value).toBe("");
    expect(adapted.disabled).toBe(false);
    expect(adapted.count).toBe(0);
    expect(adapted.nullable).toBeNull();
    expect(adapted.onClick).toBe(callback);
    expect(adapted.ref).toBe(ref);
    expect(Object.hasOwn(source, "title")).toBe(true);
  });
});
