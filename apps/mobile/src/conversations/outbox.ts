import AsyncStorage from "@react-native-async-storage/async-storage";
import { randomUUID } from "expo-crypto";

import { createOutbox } from "./pending-input";

export { inputContent } from "./pending-input";
export type { PendingInput } from "./pending-input";
export const { loadPending, prepareInput, clearPending } = createOutbox(AsyncStorage, randomUUID);
