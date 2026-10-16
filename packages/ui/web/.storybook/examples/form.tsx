import { Check } from "@gravity-ui/icons";
// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/form/basic.tsx
import type { ComponentProps } from "react";

import { Button, Description, FieldError, Form, Input, Label, TextField } from "../../src/index";

const onSubmit: NonNullable<ComponentProps<typeof Form>["onSubmit"]> = (e) => {
  e.preventDefault();
  const formData = new FormData(e.currentTarget);
  const data: Record<string, string> = {};

  // Convert FormData to plain object
  for (const [key, value] of formData) {
    data[key] = typeof value === "string" ? value : value.name;
  }

  // oxlint-disable-next-line eslint/no-alert -- Preserve the upstream demo feedback for sample submissions.
  alert(`Form submitted with: ${JSON.stringify(data, null, 2)}`);
};

export function OfficialExample() {
  return (
    <Form className="flex w-96 flex-col gap-4" onSubmit={onSubmit}>
      <TextField
        isRequired
        name="email"
        type="email"
        validate={(value) => {
          if (!/^[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}$/iu.test(value)) {
            return "Please enter a valid email address";
          }

          return null;
        }}
      >
        <Label>Email</Label>
        <Input placeholder="john@example.com" />
        <FieldError />
      </TextField>

      <TextField
        isRequired
        minLength={8}
        name="password"
        type="password"
        validate={(value) => {
          if (value.length < 8) {
            return "Password must be at least 8 characters";
          }
          if (!/[A-Z]/u.test(value)) {
            return "Password must contain at least one uppercase letter";
          }
          if (!/[0-9]/u.test(value)) {
            return "Password must contain at least one number";
          }

          return null;
        }}
      >
        <Label>Password</Label>
        <Input placeholder="Enter your password" />
        <Description>Must be at least 8 characters with 1 uppercase and 1 number</Description>
        <FieldError />
      </TextField>

      <div className="flex gap-2">
        <Button type="submit">
          <Check />
          Submit
        </Button>
        <Button type="reset" variant="secondary">
          Reset
        </Button>
      </div>
    </Form>
  );
}
