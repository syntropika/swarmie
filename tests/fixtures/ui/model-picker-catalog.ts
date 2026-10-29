import type { ModelPickerAccount } from "../../../packages/ui/web/src/elements/model-picker";

const efforts = ["low", "medium", "high", "xhigh"];
export const modelPickerAccounts: ModelPickerAccount[] = [
  {
    id: "personal", label: "Personal", providerId: "openai", providerLabel: "OpenAI",
    models: Array.from({ length: 12 }, (_, index) => ({ id: `example-${index + 1}`, label: `Example model ${index + 1}`, efforts: index === 3 ? [] : efforts })),
  },
  { id: "work", label: "Work", providerId: "openai", providerLabel: "OpenAI", models: [{ id: "work-model", label: "Work model", efforts: ["low", "high"] }] },
  { id: "second", label: "Personal", providerId: "claude", providerLabel: "Claude", models: [{ id: "another-model", label: "Another model", efforts: ["low", "medium", "high"] }] },
];
