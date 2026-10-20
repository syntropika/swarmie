// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/toolbar/basic.tsx
import { Bold, Copy, Italic, Scissors, Underline } from "@gravity-ui/icons";

import {
  Button,
  ButtonGroup,
  Separator,
  ToggleButton,
  ToggleButtonGroup,
  Toolbar,
} from "../../src/index";

export function OfficialExample() {
  return (
    <Toolbar aria-label="Text formatting">
      <ToggleButtonGroup aria-label="Text style" selectionMode="multiple">
        <ToggleButton isIconOnly aria-label="Bold" id="bold">
          <Bold />
        </ToggleButton>
        <ToggleButton isIconOnly aria-label="Italic" id="italic">
          <ToggleButtonGroup.Separator />
          <Italic />
        </ToggleButton>
        <ToggleButton isIconOnly aria-label="Underline" id="underline">
          <ToggleButtonGroup.Separator />
          <Underline />
        </ToggleButton>
      </ToggleButtonGroup>
      <Separator />
      <ButtonGroup variant="tertiary">
        <Button isIconOnly aria-label="Copy">
          <Copy />
        </Button>
        <Button isIconOnly aria-label="Cut">
          <ButtonGroup.Separator />
          <Scissors />
        </Button>
      </ButtonGroup>
    </Toolbar>
  );
}
