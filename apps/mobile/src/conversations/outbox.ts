import AsyncStorage from "@react-native-async-storage/async-storage";
import { randomUUID } from "expo-crypto";

import { createOutbox } from "~/src/conversations/pending-input";

export { inputContent } from "~/src/conversations/pending-input";
export type { PendingInput } from "~/src/conversations/pending-input";
export const { loadPending, prepareInput, clearPending } = createOutbox(AsyncStorage, randomUUID);
