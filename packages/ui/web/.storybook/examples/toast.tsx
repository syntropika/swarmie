// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/toast/default.tsx
"use client";

import { Persons } from "@gravity-ui/icons";

import { Button, toast } from "../../src/index";

function showToast() {
  const id = toast("You have been invited to join a team", {
    actionProps: {
      children: "Dismiss",
      onPress: () => {
        toast.close(id);
      },
      variant: "tertiary",
    },
    description: "Bob sent you an invitation to join HeroUI team",
    indicator: <Persons />,
    variant: "default",
  });
}

export function OfficialExample() {
  return (
    <div className="flex h-full max-w-xl flex-col items-center justify-center">
      <Button size="sm" variant="secondary" onPress={showToast}>
        Show toast
      </Button>
    </div>
  );
}
