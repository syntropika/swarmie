import { View } from "react-native";

import { Action, Field, Notice } from "../ui/controls";
import type { useConversation } from "./controller";

type Props = { conversation: ReturnType<typeof useConversation> };
export function Composer({ conversation }: Props) {
  const { session, outbox, send, reconcile, attach, text, setText, images, setImages } =
    conversation;
  return (
    <View className="gap-3 border-t border-border bg-surface p-4">
      {outbox.data ? (
        <View className="gap-2">
          <Notice>
            A message has unresolved delivery. Check its receipt before sending another.
          </Notice>
          <Action
            label={reconcile.isPending ? "Checking…" : "Check delivery"}
            disabled={reconcile.isPending}
            onPress={() => {
              reconcile.mutate();
            }}
          />
        </View>
      ) : null}
      <Field
        label="Message"
        multiline
        value={outbox.data?.text ?? text}
        onChangeText={setText}
        editable={!send.isPending && !outbox.data}
      />
      {images.length ? (
        <Action
          label="Remove attached image"
          onPress={() => {
            setImages([]);
          }}
        />
      ) : null}
      <View className="flex-row flex-wrap gap-3">
        {session.data?.capabilities.images.availability === "available" ? (
          <Action
            label={attach.isPending ? "Attaching…" : "Attach image"}
            disabled={attach.isPending || Boolean(outbox.data)}
            onPress={() => {
              attach.mutate();
            }}
          />
        ) : null}
        <Action
          label={send.isPending ? "Sending…" : "Send message"}
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
  const { send, reconcile, cancel, resume, attach } = conversation;
  return (
    <>
      {send.isError ||
      send.data?.delivery === "rejected" ||
      reconcile.isError ||
      cancel.isError ||
      resume.isError ? (
        <Notice>The operation did not finish. Check delivery before retrying.</Notice>
      ) : null}
      {reconcile.isSuccess && (!reconcile.data || reconcile.data.recoveryRequired) ? (
        <Notice>
          Delivery remains unresolved. Inspect this session from the runtime before retrying.
        </Notice>
      ) : null}
      {attach.isError ? (
        <Notice>Could not attach the image. Choose a PNG, JPEG or WebP smaller than 1 MiB.</Notice>
      ) : null}
    </>
  );
}

function SessionLifecycle({ conversation }: Props) {
  const { session, cancel, resume } = conversation;
  const snapshot = session.data;
  if (!snapshot) {
    return null;
  }
  if (snapshot.state === "running" && snapshot.capabilities.cancel.availability === "available") {
    return (
      <Action
        label="Stop response"
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
        label={resume.isPending ? "Resuming…" : "Resume conversation"}
        disabled={resume.isPending}
        onPress={() => {
          resume.mutate();
        }}
      />
    );
  }
  return null;
}
