import { View } from "react-native";

import type { useConversation } from "~/src/conversations/controller";
import { useLocale } from "~/src/platform/i18n/provider";
import { Action, Field, Notice } from "~/src/ui/controls";

type Props = { conversation: ReturnType<typeof useConversation> };
export function Composer({ conversation }: Props) {
  const { t } = useLocale();
  const { session, outbox, send, reconcile, attach, text, setText, images, setImages } =
    conversation;
  return (
    <View className="gap-3 border-t border-border bg-surface p-4">
      {outbox.data ? (
        <View className="gap-2">
          <Notice>{t("composer.unresolved")}</Notice>
          <Action
            label={reconcile.isPending ? t("composer.checking") : t("composer.check")}
            disabled={reconcile.isPending}
            onPress={() => {
              reconcile.mutate();
            }}
          />
        </View>
      ) : null}
      <Field
        label={t("composer.message")}
        multiline
        value={outbox.data?.text ?? text}
        onChangeText={setText}
        editable={!send.isPending && !outbox.data}
      />
      {images.length ? (
        <Action
          label={t("composer.removeImage")}
          onPress={() => {
            setImages([]);
          }}
        />
      ) : null}
      <View className="flex-row flex-wrap gap-3">
        {session.data?.capabilities.images.availability === "available" ? (
          <Action
            label={attach.isPending ? t("composer.attaching") : t("composer.attach")}
            disabled={attach.isPending || Boolean(outbox.data)}
            onPress={() => {
              attach.mutate();
            }}
          />
        ) : null}
        <Action
          label={send.isPending ? t("composer.sending") : t("composer.send")}
          selected
          disabled={
            !text.trim() ||
            send.isPending ||
            outbox.isPending ||
            Boolean(outbox.data) ||
            session.data?.state !== "idle"
          }
          onPress={() => {
            send.mutate();
          }}
        />
        <SessionLifecycle conversation={conversation} />
      </View>
      <ComposerNotices conversation={conversation} />
    </View>
  );
}
function ComposerNotices({ conversation }: Props) {
  const { t } = useLocale();
  const { send, reconcile, cancel, resume, attach } = conversation;
  return (
    <>
      {send.isError ||
      send.data?.delivery === "rejected" ||
      reconcile.isError ||
      cancel.isError ||
      resume.isError ? (
        <Notice>{t("composer.operationError")}</Notice>
      ) : null}
      {reconcile.isSuccess && (!reconcile.data || reconcile.data.recoveryRequired) ? (
        <Notice>{t("composer.deliveryError")}</Notice>
      ) : null}
      {attach.isError ? <Notice>{t("composer.imageError")}</Notice> : null}
    </>
  );
}

function SessionLifecycle({ conversation }: Props) {
  const { t } = useLocale();
  const { session, cancel, resume } = conversation;
  const snapshot = session.data;
  if (!snapshot) {
    return null;
  }
  if (snapshot.state === "running" && snapshot.capabilities.cancel.availability === "available") {
    return (
      <Action
        label={t("composer.stop")}
        disabled={cancel.isPending}
        onPress={() => {
          cancel.mutate();
        }}
      />
    );
  }
  if (
    (snapshot.state === "closed" || snapshot.state === "failed") &&
    snapshot.capabilities.resume.availability === "available"
  ) {
    return (
      <Action
        label={resume.isPending ? t("composer.resuming") : t("composer.resume")}
        disabled={resume.isPending}
        onPress={() => {
          resume.mutate();
        }}
      />
    );
  }
  return null;
}
