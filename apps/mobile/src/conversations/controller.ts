import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as DocumentPicker from "expo-document-picker";
import { File } from "expo-file-system";
import { useState } from "react";

import { useRuntime } from "~/src/connection/provider";
import { projectTranscript } from "~/src/conversations/journal";
import { clearPending, inputContent, loadPending, prepareInput } from "~/src/conversations/outbox";
import type { PendingInput } from "~/src/conversations/outbox";
import { useTranscript } from "~/src/conversations/stream";
import { useLocale } from "~/src/platform/i18n/provider";

export function useConversation(id: string) {
  const { t } = useLocale();
  const connection = useRuntime();
  const cache = useQueryClient();
  const [text, setText] = useState("");
  const [images, setImages] = useState<PendingInput["images"]>([]);
  const transcript = useTranscript(id);
  const projected = projectTranscript(transcript.data ?? [], {
    user: t("conversation.user"),
    agent: t("conversation.agent"),
    runtime: t("conversation.runtime"),
    attachment: t("conversation.attachment"),
    agentError: t("conversation.agentError"),
  });
  const session = useQuery({
    queryKey: ["sessions", connection.scope, id],
    queryFn: async ({ signal }) =>
      await connection.client.call("session/get", { sessionId: id }, signal),
    refetchInterval: 5000,
  });
  const outboxKey = ["outbox", connection.scope, id];
  const outbox = useQuery({
    queryKey: outboxKey,
    queryFn: async () => await loadPending(connection.scope, id),
    staleTime: Infinity,
  });
  const send = useMutation({
    mutationFn: async () => {
      const input = await prepareInput(connection.scope, id, text.trim(), images);
      await cache.invalidateQueries({ queryKey: outboxKey });
      const receipt = await connection.client.call("session/prompt", {
        sessionId: id,
        content: inputContent(input),
        idempotencyKey: input.key,
      });
      if (receipt.delivery === "accepted" || receipt.delivery === "rejected") {
        await clearPending(connection.scope, id);
      }
      return receipt;
    },
    onSuccess: (receipt) => {
      if (receipt.delivery === "accepted") {
        setText("");
        setImages([]);
      }
    },
    onSettled: async () => {
      await cache.invalidateQueries({ queryKey: outboxKey });
      await cache.invalidateQueries({ queryKey: ["sessions"] });
    },
  });
  const reconcile = useMutation({
    mutationFn: async () => {
      if (!outbox.data) {
        return null;
      }
      const receipt = await connection.client.call("session/submission/reconcile", {
        sessionId: id,
        idempotencyKey: outbox.data.key,
      });
      if (receipt?.delivery === "accepted" || receipt?.delivery === "rejected") {
        await clearPending(connection.scope, id);
      }
      return receipt;
    },
    onSuccess: (receipt) => {
      if (receipt?.delivery === "accepted") {
        setText("");
        setImages([]);
      }
    },
    onSettled: async () => {
      await cache.invalidateQueries({ queryKey: outboxKey });
    },
  });
  const attach = useMutation({
    mutationFn: async () => {
      const selection = await DocumentPicker.getDocumentAsync({
        type: ["image/png", "image/jpeg", "image/webp"],
        copyToCacheDirectory: true,
      });
      const asset = selection.assets?.[0];
      if (!asset) {
        return null;
      }
      const file = new File(asset.uri);
      try {
        if (file.size > 1024 * 1024) {
          throw new Error("Choose an image smaller than 1 MiB");
        }
        return {
          type: "image" as const,
          data: { data: await file.base64(), mime_type: asset.mimeType ?? "image/jpeg" },
        };
      } finally {
        file.delete();
      }
    },
    onSuccess: (image) => {
      if (image) {
        setImages([image]);
      }
    },
  });
  const cancel = useMutation({
    mutationFn: async () => await connection.client.call("session/cancel", { sessionId: id }),
    onSuccess: async () => {
      await cache.invalidateQueries({ queryKey: ["sessions"] });
    },
  });
  const resume = useMutation({
    mutationFn: async () => await connection.client.call("session/resume", { sessionId: id }),
    onSuccess: async () => {
      await cache.invalidateQueries({ queryKey: ["sessions"] });
    },
  });
  return {
    projected,
    transcript,
    session,
    outbox,
    send,
    reconcile,
    attach,
    cancel,
    resume,
    text,
    setText,
    images,
    setImages,
  };
}
