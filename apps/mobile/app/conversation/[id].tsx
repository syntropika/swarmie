import { useLocalSearchParams } from "expo-router";
import { FlatList, Text, View } from "react-native";
import { KeyboardAvoidingView } from "react-native-keyboard-controller";
import { withUniwind } from "uniwind";

import { Approval } from "../../src/conversations/approval";
import { Composer } from "../../src/conversations/composer";
import { useConversation } from "../../src/conversations/controller";
import { Notice, Page } from "../../src/ui/controls";

const KeyboardView = withUniwind(KeyboardAvoidingView);

export default function ConversationScreen() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const conversation = useConversation(id);
  const { projected, transcript, session } = conversation;
  return (
    <Page>
      <KeyboardView className="flex-1" behavior="padding" keyboardVerticalOffset={96}>
        <FlatList
          data={projected.rows}
          keyExtractor={(row) => row.id}
          contentContainerClassName="gap-6 p-5"
          keyboardShouldPersistTaps="handled"
          ListHeaderComponent={
            <View className="gap-2">
              <Text className="text-base font-semibold text-text-muted">
                {session.data?.options.harness} · {session.data?.state.replaceAll("_", " ")}
              </Text>
              {transcript.isError ? (
                <Notice>
                  Could not read the transcript. Reopen this conversation to reconnect.
                </Notice>
              ) : null}
            </View>
          }
          ListEmptyComponent={
            <Notice>
              {transcript.isPending
                ? "Loading conversation…"
                : "Send a message to start this conversation."}
            </Notice>
          }
          renderItem={({ item }) => (
            <View className="gap-2">
              <Text className="text-sm font-semibold text-text-muted">{item.role}</Text>
              <Text selectable className="text-base leading-7 text-ink">
                {item.text}
              </Text>
            </View>
          )}
          ListFooterComponent={
            <View className="gap-3">
              {projected.pending.map((request) => (
                <Approval key={request.id} request={request} sessionId={id} />
              ))}
            </View>
          }
        />
        <Composer conversation={conversation} />
      </KeyboardView>
    </Page>
  );
}
