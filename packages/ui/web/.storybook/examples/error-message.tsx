// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/error-message/basic.tsx
"use client";

import { useMemo, useState } from "react";

import type { Key } from "../../src/index";
import { Description, ErrorMessage, Label, Tag, TagGroup } from "../../src/index";

export function OfficialExample() {
  const [selected, setSelected] = useState<Iterable<Key>>(new Set());
  const isInvalid = useMemo(() => [...selected].length === 0, [selected]);

  return (
    <TagGroup
      selectedKeys={selected}
      selectionMode="multiple"
      onSelectionChange={(keys) => {
        setSelected(keys);
      }}
    >
      <Label>Required Categories</Label>
      <TagGroup.List>
        <Tag id="news">News</Tag>
        <Tag id="travel">Travel</Tag>
        <Tag id="gaming">Gaming</Tag>
        <Tag id="shopping">Shopping</Tag>
      </TagGroup.List>
      <Description>Select at least one category</Description>
      <ErrorMessage>{isInvalid && <>Please select at least one category</>}</ErrorMessage>
    </TagGroup>
  );
}
