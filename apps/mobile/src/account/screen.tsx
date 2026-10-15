import {
  NativeButton,
  NativeCodeField,
  NativeField,
  NativeNotice,
  NativeTextAction,
} from "@swarmie/ui-native";
import { useEffect, useState } from "react";
import { BackHandler, Platform, Text, View } from "react-native";

import type { AccountAccess } from "~/src/account/contracts";
import { emailSchema, stepEmail } from "~/src/account/flow";
import { AccountFrame } from "~/src/account/frame";
import { PasswordField } from "~/src/account/password-field";
import { useAccountFlow } from "~/src/account/use-account-flow";
import type { AccountFlow } from "~/src/account/use-account-flow";
import { useLocale } from "~/src/platform/i18n/provider";

function Body({ children }: { children: string }) {
  return <Text className="font-native text-base leading-6 text-native-muted">{children}</Text>;
}

function Legal({ creating = false }: { creating?: boolean }) {
  const { t } = useLocale();
  return (
    <Text className="text-center font-native text-xs leading-5 text-native-muted">
      {creating ? t("account.signupLegal") : t("account.legal")}
    </Text>
  );
}

function Feedback({ flow }: { flow: AccountFlow }) {
  const { t } = useLocale();
  return (
    <>
      {flow.error === undefined ? null : <NativeNotice>{t(flow.error)}</NativeNotice>}
      {flow.notice === undefined ? null : <NativeNotice>{t(flow.notice)}</NativeNotice>}
    </>
  );
}

function EmailScreen({ flow }: { flow: AccountFlow }) {
  const { t } = useLocale();
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
      title={t("account.welcome")}
      subtitle={t("account.intro")}
      footer={
        <>
          <NativeButton
            label={flow.pending ? t("account.continuing") : t("account.continue")}
            loading={flow.pending}
            disabled={!valid}
            onPress={submit}
          />
          <Legal />
        </>
      }
    >
      <NativeField
        label={t("account.email")}
        value={email}
        onChangeText={setEmail}
        placeholder={t("account.emailPlaceholder")}
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
  const { t } = useLocale();
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
  const credentialLabel = passkey ? t("account.passkeySignIn") : t("account.signIn");
  const mainLabel = creating ? t("account.create") : credentialLabel;
  return (
    <AccountFrame
      title={creating ? t("account.signupTitle") : t("account.returning")}
      email={step.email}
      onChangeEmail={flow.changeEmail}
      footer={
        <>
          <NativeButton
            label={pending ? t("account.wait") : mainLabel}
            loading={pending}
            disabled={!passkey && !canSubmit}
            onPress={passkey ? flow.passkey : submitPassword}
          />
          {!creating && (passkey ? step.methods.password : step.methods.passkey) ? (
            <NativeButton
              label={passkey ? t("account.usePassword") : t("account.usePasskey")}
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
        <Body>{t("account.unlock")}</Body>
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
                label={t("account.forgot")}
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
  const { t } = useLocale();
  const [code, setCode] = useState("");
  if (flow.step.kind !== "verify") {
    return null;
  }
  return (
    <AccountFrame
      title={t("account.verifyTitle")}
      email={flow.step.email}
      onChangeEmail={flow.changeEmail}
      footer={
        <>
          <NativeButton
            label={flow.pending ? t("account.verifying") : t("account.verify")}
            loading={flow.pending}
            disabled={code.length !== 6}
            onPress={() => {
              flow.verify(code);
              setCode("");
            }}
          />
          <View className="items-center">
            <NativeTextAction
              label={t("account.resend")}
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
      <Body>{t("account.codeIntro")}</Body>
      <NativeCodeField
        value={code}
        onChangeText={setCode}
        disabled={flow.pending}
        accessibilityLabel={t("account.codeLabel")}
      />
      <Feedback flow={flow} />
    </AccountFrame>
  );
}

function EnrollmentScreen({ flow }: { flow: AccountFlow }) {
  const { t } = useLocale();
  if (flow.step.kind !== "add-passkey") {
    return null;
  }
  const { account } = flow.step;
  return (
    <AccountFrame
      title={t("account.enrollTitle")}
      email={account.email}
      footer={
        <>
          <NativeButton
            label={flow.pending ? t("account.creatingPasskey") : t("account.createPasskey")}
            loading={flow.pending}
            onPress={flow.addPasskey}
          />
          <NativeButton
            label={t("account.notNow")}
            tone="secondary"
            disabled={flow.pending}
            onPress={() => {
              flow.navigate({ kind: "complete", account });
            }}
          />
        </>
      }
    >
      <Body>{t("account.enrollIntro")}</Body>
      <Body>{t("account.enrollLater")}</Body>
      <Feedback flow={flow} />
    </AccountFrame>
  );
}

function ResetScreen({ flow }: { flow: AccountFlow }) {
  const { t } = useLocale();
  const sent = flow.step.kind === "reset-sent";
  return (
    <AccountFrame
      title={sent ? t("account.checkEmail") : t("account.resetTitle")}
      email={stepEmail(flow.step)}
      onChangeEmail={flow.changeEmail}
      footer={
        <NativeButton
          label={sent ? t("account.backSignIn") : t("account.sendReset")}
          loading={flow.pending}
          onPress={sent ? flow.changeEmail : flow.requestReset}
        />
      }
    >
      <Body>{sent ? t("account.resetSent") : t("account.resetIntro")}</Body>
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
  const { t } = useLocale();
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
          title={t("account.complete")}
          email={flow.step.account.email}
          footer={
            <NativeButton
              label={t("account.backStart")}
              onPress={() => {
                flow.navigate({ kind: "email", email: "" });
              }}
            />
          }
        >
          <Body>{preview ? t("account.previewComplete") : t("account.ready")}</Body>
        </AccountFrame>
      );
    }
    default: {
      const exhaustive: never = kind;
      return exhaustive;
    }
  }
}
