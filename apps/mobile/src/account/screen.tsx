import {
  NativeButton,
  NativeCodeField,
  NativeField,
  NativeNotice,
  NativeTextAction,
} from "@swarmie/ui-native";
import { useEffect, useState } from "react";
import { BackHandler, Platform, Text, View } from "react-native";

import type { AccountAccess } from "./contracts";
import { emailSchema, stepEmail } from "./flow";
import { AccountFrame } from "./frame";
import { PasswordField } from "./password-field";
import { useAccountFlow } from "./use-account-flow";
import type { AccountFlow } from "./use-account-flow";

const unlockCopy =
  "Sign in to Swarmie the same way you unlock your device, with Face ID, your fingerprint or a PIN.";

function Body({ children }: { children: string }) {
  return <Text className="font-native text-base leading-6 text-native-muted">{children}</Text>;
}

function Legal({ creating = false }: { creating?: boolean }) {
  return (
    <Text className="text-center font-native text-xs leading-5 text-native-muted">
      {creating
        ? "By creating an account, you agree to our Terms and Privacy Policy."
        : "By continuing, you agree to our Terms and Privacy Policy."}
    </Text>
  );
}

function Feedback({ flow }: { flow: AccountFlow }) {
  return (
    <>
      {flow.error === undefined ? null : <NativeNotice>{flow.error}</NativeNotice>}
      {flow.notice === undefined ? null : <NativeNotice>{flow.notice}</NativeNotice>}
    </>
  );
}

function EmailScreen({ flow }: { flow: AccountFlow }) {
  const [email, setEmail] = useState(stepEmail(flow.step));
  const normalized = email.trim();
  const valid = emailSchema.safeParse(normalized).success;
  function submit() {
    if (valid) {
      flow.identify(normalized);
    }
  }
  return (
    <AccountFrame
      title={"Your space.\nYour way."}
      subtitle="Sign in or create your account."
      footer={
        <>
          <NativeButton
            label={flow.pending ? "Continuing…" : "Continue"}
            loading={flow.pending}
            disabled={!valid}
            onPress={submit}
          />
          <Legal />
        </>
      }
    >
      <NativeField
        label="Email address"
        value={email}
        onChangeText={setEmail}
        placeholder="you@example.com"
        autoCapitalize="none"
        autoCorrect={false}
        keyboardType="email-address"
        inputMode="email"
        autoComplete="email"
        textContentType="username"
        editable={!flow.pending}
        returnKeyType="go"
        onSubmitEditing={submit}
      />
      <Feedback flow={flow} />
    </AccountFrame>
  );
}

function CredentialScreen({ flow }: { flow: AccountFlow }) {
  const [password, setPassword] = useState("");
  const { step, pending } = flow;
  if (step.kind !== "passkey" && step.kind !== "password" && step.kind !== "signup") {
    return null;
  }
  const creating = step.kind === "signup";
  const passkey = step.kind === "passkey";
  // Preserve password whitespace; policy is owned by the identity service, not by presentation.
  const canSubmit = password.length > 0 && password.length <= 1024;
  function submitPassword() {
    if (canSubmit) {
      flow.password(password);
      setPassword("");
    }
  }
  const credentialLabel = passkey ? "Sign in with passkey" : "Sign in";
  const mainLabel = creating ? "Create account" : credentialLabel;
  return (
    <AccountFrame
      title={creating ? "Create your\naccount." : "Welcome back."}
      email={step.email}
      onChangeEmail={flow.changeEmail}
      footer={
        <>
          <NativeButton
            label={pending ? "Please wait…" : mainLabel}
            loading={pending}
            disabled={!passkey && !canSubmit}
            onPress={passkey ? flow.passkey : submitPassword}
          />
          {!creating && (passkey ? step.methods.password : step.methods.passkey) ? (
            <NativeButton
              label={passkey ? "Use a password" : "Use a passkey"}
              tone="secondary"
              onPress={() => {
                flow.navigate({ ...step, kind: passkey ? "password" : "passkey" });
              }}
            />
          ) : null}
          {creating ? <Legal creating /> : null}
        </>
      }
    >
      {passkey ? (
        <Body>{unlockCopy}</Body>
      ) : (
        <>
          <PasswordField
            value={password}
            onChange={setPassword}
            creating={creating}
            disabled={pending}
            onSubmit={submitPassword}
          />
          {creating ? null : (
            <View className="items-end">
              <NativeTextAction
                label="Forgot password?"
                onPress={() => {
                  flow.navigate({ kind: "reset", email: step.email });
                }}
              />
            </View>
          )}
        </>
      )}
      <Feedback flow={flow} />
    </AccountFrame>
  );
}

function VerificationScreen({ flow }: { flow: AccountFlow }) {
  const [code, setCode] = useState("");
  if (flow.step.kind !== "verify") {
    return null;
  }
  return (
    <AccountFrame
      title="Verify your email."
      email={flow.step.email}
      onChangeEmail={flow.changeEmail}
      footer={
        <>
          <NativeButton
            label={flow.pending ? "Verifying…" : "Verify email"}
            loading={flow.pending}
            disabled={code.length !== 6}
            onPress={() => {
              flow.verify(code);
              setCode("");
            }}
          />
          <View className="items-center">
            <NativeTextAction
              label="Resend code"
              disabled={flow.pending}
              onPress={() => {
                flow.resend();
                setCode("");
              }}
            />
          </View>
        </>
      }
    >
      <Body>Enter the code we sent to your email.</Body>
      <NativeCodeField value={code} onChangeText={setCode} disabled={flow.pending} />
      <Feedback flow={flow} />
    </AccountFrame>
  );
}

function EnrollmentScreen({ flow }: { flow: AccountFlow }) {
  if (flow.step.kind !== "add-passkey") {
    return null;
  }
  const { account } = flow.step;
  return (
    <AccountFrame
      title={"Make next time\neasier."}
      email={account.email}
      footer={
        <>
          <NativeButton
            label={flow.pending ? "Creating passkey…" : "Create a passkey"}
            loading={flow.pending}
            onPress={flow.addPasskey}
          />
          <NativeButton
            label="Not now"
            tone="secondary"
            disabled={flow.pending}
            onPress={() => {
              flow.navigate({ kind: "complete", account });
            }}
          />
        </>
      }
    >
      <Body>Add a passkey to sign in with Face ID, your fingerprint or a PIN.</Body>
      <Body>You can also set this up later.</Body>
      <Feedback flow={flow} />
    </AccountFrame>
  );
}

function ResetScreen({ flow }: { flow: AccountFlow }) {
  const sent = flow.step.kind === "reset-sent";
  return (
    <AccountFrame
      title={sent ? "Check your email." : "Reset your password."}
      email={stepEmail(flow.step)}
      onChangeEmail={flow.changeEmail}
      footer={
        <NativeButton
          label={sent ? "Back to sign in" : "Send reset instructions"}
          loading={flow.pending}
          onPress={sent ? flow.changeEmail : flow.requestReset}
        />
      }
    >
      <Body>
        {sent
          ? "If that email has an account, you'll receive instructions to reset your password."
          : "We'll email you instructions to reset your password."}
      </Body>
      <Feedback flow={flow} />
    </AccountFrame>
  );
}

export function AccountScreen({
  access,
  preview = false,
}: {
  access: AccountAccess;
  preview?: boolean;
}) {
  const flow = useAccountFlow(access);
  useEffect(() => {
    if (Platform.OS !== "android") {
      return undefined;
    }
    const listener = BackHandler.addEventListener("hardwareBackPress", () => {
      if (flow.step.kind === "email" || flow.step.kind === "complete") {
        return false;
      }
      flow.changeEmail();
      return true;
    });
    return () => {
      listener.remove();
    };
  }, [flow]);
  const { kind } = flow.step;
  switch (kind) {
    case "email": {
      return <EmailScreen flow={flow} />;
    }
    case "password":
    case "passkey":
    case "signup": {
      return <CredentialScreen key={flow.step.kind} flow={flow} />;
    }
    case "verify": {
      return <VerificationScreen key={flow.step.challenge} flow={flow} />;
    }
    case "add-passkey": {
      return <EnrollmentScreen flow={flow} />;
    }
    case "reset":
    case "reset-sent": {
      return <ResetScreen flow={flow} />;
    }
    case "complete": {
      return (
        <AccountFrame
          title="You're all set."
          email={flow.step.account.email}
          footer={
            <NativeButton
              label="Back to start"
              onPress={() => {
                flow.navigate({ kind: "email", email: "" });
              }}
            />
          }
        >
          <Body>
            {preview
              ? "Preview complete. No account or device credential was created."
              : "Your account is ready. Provider connections will be available soon."}
          </Body>
        </AccountFrame>
      );
    }
    default: {
      const exhaustive: never = kind;
      return exhaustive;
    }
  }
}
