import { expect, test } from "bun:test";

import * as HeroUI from "@heroui/react";

import { Button } from "./button";
import { Chip } from "./chip";
import { DatePicker } from "./date-picker";
import { DateRangePicker } from "./date-range-picker";
import { Link } from "./link";
import { ListBox } from "./list-box";
import { Modal } from "./modal";
import { Popover } from "./popover";
import { Tabs } from "./tabs";
import { TextArea } from "./textarea";
import { ToggleButtonGroup } from "./toggle-button-group";

test("base component facades preserve the vendor component and its compound API", () => {
  const facades = {
    Button,
    Chip,
    DatePicker,
    DateRangePicker,
    Link,
    ListBox,
    Modal,
    Popover,
    Tabs,
    TextArea,
    ToggleButtonGroup,
  };
  for (const name of Object.keys(facades)) {
    expect(Reflect.get(facades, name)).toBe(Reflect.get(HeroUI, name));
  }
});
