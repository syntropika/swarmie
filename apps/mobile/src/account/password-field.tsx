import { NativeField, NativeIconAction } from "@swarmie/ui-native";
import { useState } from "react";
import { useCSSVariable } from "uniwind";

import { useLocale } from "~/src/platform/i18n/provider";
import { Icon } from "~/src/ui/icon";

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
  const { t } = useLocale();
  const [visible, setVisible] = useState(false);
  const muted = useCSSVariable("--color-native-muted");
  return (
    <NativeField
      label={creating ? t("account.createPassword") : t("account.password")}
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
          accessibilityLabel={visible ? t("account.hidePassword") : t("account.showPassword")}
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
