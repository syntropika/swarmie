import { NativeField, NativeIconAction } from "@swarmie/ui-native";
import { useState } from "react";
import { useCSSVariable } from "uniwind";

import { Icon } from "../ui/icon";

export function PasswordField({
  value,
  onChange,
  creating,
  disabled,
  onSubmit,
}: {
  value: string;
  onChange: (value: string) => void;
  creating: boolean;
  disabled: boolean;
  onSubmit: () => void;
}) {
  const [visible, setVisible] = useState(false);
  const muted = useCSSVariable("--color-native-muted");
  return (
    <NativeField
      label={creating ? "Create a password" : "Password"}
      value={value}
      onChangeText={onChange}
      secureTextEntry={!visible}
      autoFocus
      autoComplete={creating ? "new-password" : "current-password"}
      textContentType={creating ? "newPassword" : "password"}
      autoCapitalize="none"
      autoCorrect={false}
      editable={!disabled}
      returnKeyType="go"
      onSubmitEditing={onSubmit}
      trailing={
        <NativeIconAction
          accessibilityLabel={visible ? "Hide password" : "Show password"}
          disabled={disabled}
          onPress={() => {
            setVisible(!visible);
          }}
        >
          <Icon
            name={visible ? "eyeOff" : "eye"}
            color={typeof muted === "string" ? muted : undefined}
            size={28}
          />
        </NativeIconAction>
      }
    />
  );
}
