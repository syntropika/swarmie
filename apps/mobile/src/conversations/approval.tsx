import type { PermissionRequest } from "@swarmie/contracts";
import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { Text, View } from "react-native";

import { useRuntime } from "~/src/connection/provider";
import { useLocale } from "~/src/platform/i18n/provider";
import { Action, Field, Notice } from "~/src/ui/controls";

export function Approval({
  request,
  sessionId,
}: {
  request: PermissionRequest;
  sessionId: string;
}) {
  const { t } = useLocale();
  const connection = useRuntime();
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const response = useMutation({
    mutationFn: async (option: string) =>
      await connection.client.call("permission/respond", {
        sessionId,
        id: request.id,
        option,
        ...((request.questions?.length ?? 0) > 0 ? { answers } : {}),
      }),
  });
  const answerable =
    request.questions?.some(
      (question) => question.multiple || (!question.allowText && question.options.length === 0),
    ) !== true;
  const missing = Boolean(
    request.questions?.some((question) => (answers[question.id] ?? "").length === 0),
  );
  return (
    <View className="gap-3 rounded-panel bg-attention-soft p-4">
      <Text accessibilityRole="header" className="text-lg font-semibold text-ink">
        {request.title}
      </Text>
      {request.description === null ? null : (
        <Text className="text-base text-ink">{request.description}</Text>
      )}
      {request.questions?.map((question) => (
        <View key={question.id} className="gap-2">
          <Text className="text-base text-ink">{question.prompt}</Text>
          {question.options.map((option) => (
            <Action
              key={option.id}
              label={option.label}
              selected={answers[question.id] === option.id}
              onPress={() => {
                setAnswers({ ...answers, [question.id]: option.id });
              }}
            />
          ))}
          {question.allowText ? (
            <Field
              label={t("approval.answer")}
              value={answers[question.id] ?? ""}
              secureTextEntry={question.secret}
              onChangeText={(value) => {
                setAnswers({ ...answers, [question.id]: value });
              }}
            />
          ) : null}
        </View>
      ))}
      {answerable ? (
        request.options.map((option) => (
          <Action
            key={option.id}
            label={option.label}
            disabled={response.isPending || response.isSuccess || missing}
            onPress={() => {
              response.mutate(option.id);
            }}
          />
        ))
      ) : (
        <Notice>{t("approval.unsupported")}</Notice>
      )}
      {response.isError ? <Notice>{t("approval.error")}</Notice> : null}
    </View>
  );
}
