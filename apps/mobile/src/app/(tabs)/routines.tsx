import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { ScrollView, Text, View } from "react-native";

import { useRuntime } from "~/src/connection/provider";
import { useLocale } from "~/src/platform/i18n/provider";
import { statusLabel } from "~/src/platform/i18n/translate";
import { routinesSchema, scheduleInstant } from "~/src/routines/contracts";
import { Action, Field, Notice, Page } from "~/src/ui/controls";

export default function RoutinesScreen() {
  const { t, formatDate, formatNumber } = useLocale();
  const connection = useRuntime();
  const supported = connection.resources.has("schedules.put");
  const cache = useQueryClient();
  const [sessionId, setSessionId] = useState("");
  const [text, setText] = useState("");
  const [time, setTime] = useState("");
  const [interval, setInterval] = useState("");
  const sessions = useQuery({
    queryKey: ["sessions", connection.scope],
    queryFn: async ({ signal }) => await connection.client.call("session/list", {}, signal),
  });
  const routines = useQuery({
    queryKey: ["routines", connection.scope],
    queryFn: async ({ signal }) =>
      routinesSchema.parse(
        await connection.client.call(
          "extension/invoke",
          { extensionId: "resources", command: "schedules.list" },
          signal,
        ),
      ),
    enabled: supported,
    refetchInterval: 5000,
  });
  const create = useMutation({
    mutationFn: async () => {
      const intervalSeconds = interval ? Number(interval) : null;
      if (
        intervalSeconds !== null &&
        (!Number.isSafeInteger(intervalSeconds) ||
          intervalSeconds < 1 ||
          intervalSeconds > 31_536_000)
      ) {
        throw new Error("Invalid interval");
      }
      return await connection.client.call("extension/invoke", {
        extensionId: "resources",
        command: "schedules.put",
        arguments: {
          sessionId,
          text: text.trim(),
          nextRunAtMs: scheduleInstant(time),
          intervalSeconds,
          enabled: true,
        },
      });
    },
    onSuccess: async () => {
      setText("");
      await cache.invalidateQueries({ queryKey: ["routines"] });
    },
  });
  const change = useMutation({
    mutationFn: async ({ id, enabled }: { id: string; enabled: boolean }) =>
      await connection.client.call("extension/invoke", {
        extensionId: "resources",
        command: "schedules.enabled",
        arguments: { id, enabled },
      }),
    onSuccess: async () => {
      await cache.invalidateQueries({ queryKey: ["routines"] });
    },
  });
  const remove = useMutation({
    mutationFn: async (id: string) =>
      await connection.client.call("extension/invoke", {
        extensionId: "resources",
        command: "schedules.cancel",
        arguments: { id },
      }),
    onSuccess: async () => {
      await cache.invalidateQueries({ queryKey: ["routines"] });
    },
  });
  return (
    <Page>
      <ScrollView keyboardShouldPersistTaps="handled" contentContainerClassName="gap-5 p-5">
        <Text className="text-base leading-7 text-text-muted">{t("routines.intro")}</Text>
        {supported ? (
          <>
            {routines.data?.schedules.map((routine) => (
              <View key={routine.id} className="gap-3 border-b border-border pb-6">
                <Text className="text-lg font-semibold text-ink">{routine.text}</Text>
                <Text className="text-base text-text-muted">
                  {routine.enabled ? t("routines.enabled") : t("routines.paused")} ·{" "}
                  {statusLabel(t, routine.status)}
                </Text>
                <Text className="text-sm text-text-muted">
                  {t("routines.next", {
                    date: formatDate(routine.nextRunAtMs),
                    recurrence:
                      routine.intervalSeconds === null
                        ? t("routines.once")
                        : t("routines.every", { seconds: formatNumber(routine.intervalSeconds) }),
                  })}
                </Text>
                <View className="flex-row flex-wrap gap-3">
                  {connection.resources.has("schedules.enabled") ? (
                    <Action
                      label={routine.enabled ? t("routines.pause") : t("routines.resume")}
                      disabled={
                        change.isPending ||
                        routine.status === "running" ||
                        routine.status === "interrupted" ||
                        routine.status === "cancelled"
                      }
                      onPress={() => {
                        change.mutate({ id: routine.id, enabled: !routine.enabled });
                      }}
                    />
                  ) : null}
                  <Action
                    label={t("routines.cancel")}
                    disabled={
                      remove.isPending ||
                      routine.status === "running" ||
                      routine.status === "interrupted" ||
                      routine.status === "cancelled"
                    }
                    onPress={() => {
                      remove.mutate(routine.id);
                    }}
                  />
                </View>
                {routine.attempts.map((attempt) => (
                  <Text key={attempt.id} className="text-sm text-text-muted">
                    {formatDate(attempt.startedAtMs)} · {statusLabel(t, attempt.status)}
                  </Text>
                ))}
              </View>
            ))}
            <Text accessibilityRole="header" className="text-xl font-semibold text-ink">
              {t("routines.new")}
            </Text>
            <Field label={t("routines.prompt")} value={text} onChangeText={setText} multiline />
            <Text className="text-base text-ink">{t("routines.destination")}</Text>
            {sessions.data?.map((session) => (
              <Action
                key={session.id}
                label={
                  session.metadata.title ?? `${session.options.harness} · ${session.id.slice(0, 8)}`
                }
                selected={sessionId === session.id}
                onPress={() => {
                  setSessionId(session.id);
                }}
              />
            ))}
            <Field
              label={t("routines.start")}
              placeholder="2026-11-01T09:00:00+00:00"
              value={time}
              onChangeText={setTime}
              autoCapitalize="none"
            />
            <Field
              label={t("routines.interval")}
              value={interval}
              onChangeText={setInterval}
              keyboardType="number-pad"
            />
            <Text className="text-sm leading-6 text-text-muted">{t("routines.timing")}</Text>
            <Action
              label={create.isPending ? t("routines.saving") : t("routines.save")}
              selected
              disabled={!text.trim() || !sessionId || !time || create.isPending}
              onPress={() => {
                create.mutate();
              }}
            />
            {create.isError || change.isError || remove.isError || routines.isError ? (
              <Notice>{t("routines.error")}</Notice>
            ) : null}
          </>
        ) : (
          <Notice>{t("routines.unavailable")}</Notice>
        )}
      </ScrollView>
    </Page>
  );
}
