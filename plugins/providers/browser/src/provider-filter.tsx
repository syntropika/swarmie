import { Header, Label, ListBox, ProviderLogo, Select } from "@swarmie/ui-web";

export type ProviderFilterOption = { id: string; label: string };

/** A stock multiple Select keeps keyboard, selection and dismissal behavior with the vendor. */
export function ProviderFilter({
  options,
  selected,
  onChange,
}: {
  options: ProviderFilterOption[];
  selected: ReadonlySet<string> | null;
  onChange: (selected: ReadonlySet<string>) => void;
}) {
  const ids = options
    .filter((option) => selected === null || selected.has(option.id))
    .map((option) => option.id);
  return (
    <Select
      aria-label="Filter providers"
      selectionMode="multiple"
      value={ids}
      onChange={(keys) => {
        onChange(
          new Set(options.filter((option) => keys.includes(option.id)).map((option) => option.id)),
        );
      }}
      isDisabled={options.length === 0}
      className="w-full @min-[560px]:w-(--spacing-menu-width)"
    >
      <Select.Trigger>
        <Select.Value>
          {() => `Providers · ${ids.length === options.length ? "All" : `${ids.length} selected`}`}
        </Select.Value>
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover>
        <ListBox aria-label="Available providers" data-action-menu="">
          <ListBox.Section>
            <Header>Filter providers</Header>
            {options.map((option) => (
              <ListBox.Item key={option.id} id={option.id} textValue={option.label}>
                <ProviderLogo providerId={option.id} />
                <Label>{option.label}</Label>
                <ListBox.ItemIndicator />
              </ListBox.Item>
            ))}
          </ListBox.Section>
        </ListBox>
      </Select.Popover>
    </Select>
  );
}
