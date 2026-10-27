import { Select, ListBox } from "../atoms/select";
import { Label } from "../atoms/text-field";

/** UI projections of models discovered by the consumer; selection does not open a provider. */
export type ModelOptionView = { id: string; label: string };
export type ModelSelectorProps = {
  models: readonly ModelOptionView[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  label?: string;
  disabled?: boolean;
};
export function ModelSelector({
  models,
  selectedId,
  onSelect,
  label = "Model",
  disabled = false,
}: ModelSelectorProps) {
  return (
    <Select
      aria-label={label}
      value={selectedId}
      isDisabled={disabled}
      onChange={(id) => {
        if (typeof id === "string") {
          onSelect(id);
        }
      }}
    >
      <Label>{label}</Label>
      <Select.Trigger>
        <Select.Value />
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover>
        <ListBox>
          {models.map((model) => (
            <ListBox.Item key={model.id} id={model.id} textValue={model.label}>
              {model.label}
              <ListBox.ItemIndicator />
            </ListBox.Item>
          ))}
        </ListBox>
      </Select.Popover>
    </Select>
  );
}
