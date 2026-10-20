// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/field-error/basic.tsx
"use client";

import { useState } from "react";

import { FieldError, Input, Label, TextField } from "../../src/index";

export function OfficialExample() {
  const [value, setValue] = useState("jr");
  const isInvalid = value.length > 0 && value.length < 3;

  return (
    <TextField className="w-64" isInvalid={isInvalid}>
      <Label htmlFor="username">Username</Label>
      <Input
        id="username"
        placeholder="Enter username"
        value={value}
        onChange={(e) => {
          setValue(e.target.value);
        }}
      />
      <FieldError>Username must be at least 3 characters</FieldError>
    </TextField>
  );
}
