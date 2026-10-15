import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, router } from "expo-router";
import { useState } from "react";
import { FlatList, Pressable, Text, View } from "react-native";

import { useRuntime } from "~/src/connection/provider";
import { useLocale } from "~/src/platform/i18n/provider";
import { statusLabel } from "~/src/platform/i18n/translate";
import { Action, Field, Notice, Page } from "~/src/ui/controls";

export default function ConversationsScreen() {
  const { t } = useLocale();
  const connection = useRuntime();
  const cache = useQueryClient();
  const [creating, setCreating] = useState(false);
  const [harness, setHarness] = useState("");
  const [cwd, setCwd] = useState("");
  const sessions = useQuery({
    queryKey: ["sessions", connection.scope],
    queryFn: async ({ signal }) => await connection.client.call("session/list", {}, signal),
    refetchInterval: 5000,
  });
  const harnesses = useQuery({
    queryKey: ["harnesses", connection.scope],
    queryFn: async ({ signal }) => await connection.client.call("runtime/discover", {}, signal),
    enabled: creating,
  });
  const create = useMutation({
    mutationFn: async () =>
      await connection.client.call("session/create", {
        harness,
        ...(cwd.trim() ? { cwd: cwd.trim() } : {}),
      }),
    onSuccess: async (session) => {
      setCreating(false);
      await cache.invalidateQueries({ queryKey: ["sessions"] });
      router.push({ pathname: "/conversation/[id]", params: { id: session.id } });
    },
  });
  return (
    <Page>
      <FlatList
        data={sessions.data?.filter((session) => !session.metadata.archived)}
        keyExtractor={(session) => session.id}
        contentContainerClassName="gap-1 px-5 pb-8"
        refreshing={sessions.isRefetching}
        onRefresh={() => {
          sessions.refetch().catch(() => {
            console.warn("Conversation refresh failed");
          });
        }}
        ListHeaderComponent={
          <View className="gap-4 py-5">
            <Text className="text-base leading-7 text-text-muted">{t("conversations.intro")}</Text>
            <Action
              label={creating ? t("conversations.closeNew") : t("conversations.new")}
              onPress={() => {
                setCreating(!creating);
              }}
              selected
            />
            {creating ? (
              <View className="gap-4">
                <Text className="text-base text-ink">{t("conversations.chooseAgent")}</Text>
                <View className="gap-2">
                  {harnesses.data
                    ?.filter((item) => item.available)
                    .map((item) => (
                      <Action
                        key={item.harness}
                        label={item.harness}
                        selected={harness === item.harness}
                        onPress={() => {
                          setHarness(item.harness);
                        }}
                      />
                    ))}
                </View>
                <Field
                  label={t("conversations.directory")}
                  value={cwd}
                  onChangeText={setCwd}
                  autoCapitalize="none"
                  autoCorrect={false}
                />
                <Action
                  label={create.isPending ? t("conversations.creating") : t("conversations.create")}
                  disabled={!harness || create.isPending}
                  onPress={() => {
                    create.mutate();
                  }}
                />
                {create.isError || harnesses.isError ? (
                  <Notice>{t("conversations.createError")}</Notice>
                ) : null}
              </View>
            ) : null}
            {sessions.isError ? <Notice>{t("conversations.loadError")}</Notice> : null}
          </View>
        }
        ListEmptyComponent={
          <Notice>
            {sessions.isPending ? t("conversations.loading") : t("conversations.empty")}
          </Notice>
        }
        renderItem={({ item }) => (
          <Link href={{ pathname: "/conversation/[id]", params: { id: item.id } }} asChild>
            <Pressable
              className="min-h-20 gap-2 border-b border-border px-1 py-5"
              accessibilityRole="button"
            >
              <Text className="text-lg font-semibold text-ink">
                {item.metadata.title ?? item.options.harness}
              </Text>
              <Text className="text-sm text-text-muted">
                {item.options.harness} · {statusLabel(t, item.state)}
              </Text>
            </Pressable>
          </Link>
        )}
      />
    </Page>
  );
}
