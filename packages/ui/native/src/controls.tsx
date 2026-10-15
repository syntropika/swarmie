import { Button } from "heroui-native/button";
import { Input } from "heroui-native/input";
import { Label } from "heroui-native/label";
import { TextField } from "heroui-native/text-field";
import type { ComponentProps, ReactNode } from "react";
import { useId, useRef, useState } from "react";
import { Image, Text, View } from "react-native";
import type { ImageSourcePropType, PressableProps, TextInput, TextInputProps } from "react-native";
import { useCSSVariable } from "uniwind";

type ButtonProps = Omit<PressableProps, "children"> & {
  label: string;
  tone?: "primary" | "secondary";
  loading?: boolean;
};

/** Platform controls share neutral tokens without loading the DOM component entry. */
export function NativeButton({
  label,
  tone = "primary",
  loading = false,
  disabled,
  ...props
}: ButtonProps) {
  const unavailable = (disabled ?? false) || loading;
  return (
    <Button
      {...props}
      accessibilityRole="button"
      accessibilityLabel={props.accessibilityLabel ?? label}
      accessibilityState={{ ...props.accessibilityState, disabled: unavailable, busy: loading }}
      isDisabled={unavailable}
      variant={tone}
      size="lg"
      background={null}
      className="h-auto min-h-native-control gap-3 rounded-full px-6 py-4 focus:outline-2 focus:outline-offset-4 focus:outline-native-ink"
    >
      <Button.Label
        className={
          tone === "primary"
            ? "text-center font-native text-base font-semibold text-native-canvas"
            : "text-center font-native text-base font-semibold text-native-ink"
        }
      >
        {label}
      </Button.Label>
    </Button>
  );
}

export function NativeIconAction({
  children,
  disabled,
  ...props
}: PressableProps & { accessibilityLabel: string; children: ReactNode }) {
  return (
    <Button
      {...props}
      accessibilityRole="button"
      accessibilityState={{ ...props.accessibilityState, disabled: Boolean(disabled) }}
      isDisabled={Boolean(disabled)}
      variant="ghost"
      isIconOnly
      className="h-12 w-12 rounded-full focus:outline-2 focus:outline-offset-2 focus:outline-native-muted"
    >
      {children}
    </Button>
  );
}

export function NativeTextAction({
  label,
  disabled,
  ...props
}: Omit<PressableProps, "children"> & { label: string }) {
  return (
    <Button
      {...props}
      accessibilityRole="button"
      accessibilityLabel={props.accessibilityLabel ?? label}
      accessibilityState={{ ...props.accessibilityState, disabled: Boolean(disabled) }}
      isDisabled={Boolean(disabled)}
      variant="ghost"
      feedbackVariant="none"
      className="h-auto min-h-11 min-w-11 rounded-full px-1 focus:outline-2 focus:outline-offset-2 focus:outline-native-ink"
    >
      <Button.Label className="font-native text-sm font-normal text-native-muted underline">
        {label}
      </Button.Label>
    </Button>
  );
}

type FieldProps = TextInputProps & {
  label: string;
  error?: string;
  helper?: string;
  ref?: ComponentProps<typeof Input>["ref"];
  trailing?: ReactNode;
};

export function NativeField({
  label,
  error,
  helper,
  ref: inputRef,
  trailing,
  ...props
}: FieldProps) {
  const id = useId();
  const muted = useCSSVariable("--color-native-muted");
  const hasTrailing = trailing !== undefined && trailing !== null;
  return (
    <TextField
      isDisabled={props.editable === false}
      isInvalid={error !== undefined}
      className="gap-2"
    >
      <Label className="self-start p-0">
        <Label.Text
          nativeID={`${id}-label`}
          className="font-native text-xl font-medium text-native-ink"
        >
          {label}
        </Label.Text>
      </Label>
      <View className="relative">
        <Input
          {...(typeof muted === "string" ? { placeholderTextColor: muted } : {})}
          {...props}
          ref={inputRef}
          accessibilityLabel={label}
          accessibilityLabelledBy={`${id}-label`}
          accessibilityHint={error ?? helper}
          underlineColorAndroid="transparent"
          background={null}
          selectionColorClassName="accent-native-ink"
          placeholderColorClassName="accent-native-muted"
          className={
            hasTrailing
              ? "ios:shadow-none ios:outline-none ios:focus:outline-none android:border-0 android:shadow-none min-h-native-field rounded-full border-0 bg-native-field py-4 pr-20 pl-6 font-native text-xl text-native-ink shadow-none outline-none focus:bg-native-field-focus"
              : "ios:shadow-none ios:outline-none ios:focus:outline-none android:border-0 android:shadow-none min-h-native-field rounded-full border-0 bg-native-field px-6 py-4 font-native text-xl text-native-ink shadow-none outline-none focus:bg-native-field-focus"
          }
        />
        {hasTrailing ? (
          <View className="absolute inset-y-0 right-3 justify-center">{trailing}</View>
        ) : null}
      </View>
      {helper === undefined ? null : <Text className="text-sm text-native-muted">{helper}</Text>}
      {error === undefined ? null : <NativeNotice>{error}</NativeNotice>}
    </TextField>
  );
}

export function NativeCodeField({
  value,
  onChangeText,
  disabled = false,
  accessibilityLabel,
}: {
  value: string;
  onChangeText: (value: string) => void;
  disabled?: boolean;
  accessibilityLabel: string;
}) {
  const [focused, setFocused] = useState(false);
  const input = useRef<TextInput>(null);
  return (
    <View className="relative">
      <View
        className="flex-row gap-2"
        pointerEvents="none"
        accessibilityElementsHidden
        importantForAccessibility="no-hide-descendants"
      >
        {[0, 1, 2, 3, 4, 5].map((index) => (
          <View
            key={index}
            className={
              focused && index === Math.min(value.length, 5)
                ? "min-h-native-control flex-1 items-center justify-center rounded-2xl border border-native-ink bg-native-control"
                : "min-h-native-control flex-1 items-center justify-center rounded-2xl border border-transparent bg-native-control"
            }
          >
            <Text className="text-xl font-semibold text-native-ink">{value[index] ?? ""}</Text>
          </View>
        ))}
      </View>
      <Input
        ref={input}
        value={value}
        editable={!disabled}
        onChangeText={(text) => {
          onChangeText(text.replaceAll(/\D/gu, "").slice(0, 6));
        }}
        accessibilityLabel={accessibilityLabel}
        autoComplete="one-time-code"
        textContentType="oneTimeCode"
        keyboardType="number-pad"
        inputMode="numeric"
        maxLength={6}
        autoFocus
        caretHidden
        background={null}
        onFocus={() => {
          setFocused(true);
        }}
        onBlur={() => {
          setFocused(false);
        }}
        className="ios:shadow-none ios:outline-none ios:focus:outline-none android:border-0 android:shadow-none absolute inset-0 min-h-0 rounded-none border-0 bg-transparent p-0 text-transparent shadow-none outline-none"
      />
    </View>
  );
}

export function NativeNotice({ children }: { children: ReactNode }) {
  return (
    <Text
      accessibilityRole="alert"
      accessibilityLiveRegion="polite"
      className="font-native text-sm leading-6 text-native-ink"
    >
      {children}
    </Text>
  );
}

/** The owner supplies a bundled canonical asset; the primitive owns its display geometry. */
export function NativeBrand({ source }: { source: ImageSourcePropType }) {
  return (
    <Image
      source={source}
      resizeMode="contain"
      accessibilityLabel="Swarmie"
      className="h-native-brand-height w-native-brand-width max-w-full"
    />
  );
}
