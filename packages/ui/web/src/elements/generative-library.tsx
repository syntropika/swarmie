import { defaultGenerativeUILibrary } from "@assistant-ui/generative-ui/react";
import type { GenerativeUIDispatch, GenerativeUILibrary } from "@assistant-ui/generative-ui/react";
import { Children } from "react";
import type { ReactNode } from "react";

import { GeoMap } from "../../target/assistant-elements/packages/ui/src/components/react/assistant-ui/elements/geo-map";
import { MermaidDiagram } from "../../target/assistant-elements/packages/ui/src/components/react/assistant-ui/elements/mermaid-diagram";
import { Button } from "../atoms/button";
import { Card } from "../atoms/card";
import { Checkbox } from "../atoms/checkbox";
import { CheckboxGroup } from "../atoms/checkbox-group";
import { Form } from "../atoms/form";
import { Input } from "../atoms/input";
import { Radio, RadioGroup } from "../atoms/radio-group";
import { Select, ListBox } from "../atoms/select";
import { Slider } from "../atoms/slider";
import { Tabs } from "../atoms/tabs";
import { Label } from "../atoms/text-field";
import { TextArea } from "../atoms/textarea";
import { MarkdownText } from "./markdown";

type Action = Parameters<GenerativeUIDispatch>[0];
type ActionView = { $action?: Action; $dispatch?: GenerativeUIDispatch };
type Choice = string | { value: string; label?: string; description?: string };
type ChoiceView = ActionView & {
  label?: string;
  name?: string;
  value?: string;
  defaultValue?: string;
  placeholder?: string;
  options?: readonly Choice[];
  children?: ReactNode;
};
type InputRenderProps = {
  label?: string;
  name?: string;
  placeholder?: string;
  value?: string;
  type?: string;
  multiline?: boolean;
  $bindings?: { value?: (value: string) => void };
};
type FooterAction = { label?: string; $action?: Action };
type CardRenderProps = ActionView & {
  title?: string;
  children?: ReactNode;
  confirm?: FooterAction;
  cancel?: FooterAction;
  asForm?: boolean;
};

function component(name: string) {
  const entry = defaultGenerativeUILibrary[name];
  if (!entry) {
    throw new Error(`Missing generative presentation component: ${name}`);
  }
  return entry;
}
function choices(options: readonly Choice[] = []) {
  return options.map((option) =>
    typeof option === "string"
      ? { value: option, label: option }
      : { ...option, label: option.label ?? option.value },
  );
}
function dispatch(view: ActionView, value: unknown) {
  if (view.$action && view.$dispatch) {
    view.$dispatch({ ...view.$action, value });
  }
}
function CardView({ title, children, confirm, cancel, asForm, ...action }: CardRenderProps) {
  const footer =
    confirm || cancel ? (
      <Card.Footer>
        {confirm ? (
          <Button
            type={asForm === true ? "submit" : "button"}
            onPress={() => {
              if (asForm !== true) {
                dispatch({ ...action, ...confirm }, undefined);
              }
            }}
          >
            {confirm.label ?? "Confirm"}
          </Button>
        ) : null}
        {cancel ? (
          <Button
            variant="tertiary"
            onPress={() => {
              dispatch({ ...action, ...cancel }, undefined);
            }}
          >
            {cancel.label ?? "Cancel"}
          </Button>
        ) : null}
      </Card.Footer>
    ) : null;
  const content = (
    <Card>
      {title === undefined ? null : (
        <Card.Header>
          <Card.Title>{title}</Card.Title>
        </Card.Header>
      )}
      <Card.Content className="grid gap-4">{children}</Card.Content>
      {footer}
    </Card>
  );
  if (asForm !== true) {
    return content;
  }
  return (
    <Form
      onSubmit={(event) => {
        event.preventDefault();
        const values: Record<string, string> = {};
        for (const [key, value] of new FormData(event.currentTarget)) {
          if (typeof value === "string") {
            values[key] = value;
          }
        }
        dispatch({ ...action, ...confirm }, values);
      }}
    >
      {content}
    </Form>
  );
}
function InputView({
  label,
  name,
  placeholder,
  value,
  type,
  multiline,
  $bindings,
}: InputRenderProps) {
  const props = {
    ...(name === undefined ? {} : { name }),
    "aria-label": label ?? name ?? "Value",
    ...(placeholder === undefined ? {} : { placeholder }),
    ...(value === undefined ? {} : { defaultValue: value }),
    "data-aui-field-name": name,
  };
  if (multiline === true || type === "textarea") {
    return <TextArea {...props} onChange={(event) => $bindings?.value?.(event.target.value)} />;
  }
  return (
    <Input
      {...props}
      {...(type === undefined ? {} : { type })}
      onChange={(event) => $bindings?.value?.(event.target.value)}
    />
  );
}

/** The renderer validates the upstream UI schema. Controls and action callbacks remain host-owned. */
export const styledGenerativeUILibrary: GenerativeUILibrary = {
  ...defaultGenerativeUILibrary,
  Button: {
    ...component("Button"),
    render: (
      props: ActionView & {
        label?: string;
        text?: string;
        children?: ReactNode;
        disabled?: boolean;
        style?: "primary" | "secondary" | "danger";
      },
    ) => {
      let variant: "primary" | "secondary" | "danger" = "primary";
      if (props.style === "secondary" || props.style === "danger") {
        variant = props.style;
      }
      return (
        <Button
          variant={variant}
          {...(props.disabled === undefined ? {} : { disabled: props.disabled })}
          onPress={() => {
            dispatch(props, undefined);
          }}
        >
          {props.label ?? props.text ?? props.children ?? "Continue"}
        </Button>
      );
    },
  },
  Diagram: {
    ...component("Diagram"),
    render: ({ code, title }: { code?: string; title?: string }) => (
      <figure>
        <MermaidDiagram code={code ?? ""} />
        {title === undefined ? null : <figcaption>{title}</figcaption>}
      </figure>
    ),
  },
  Map: {
    ...component("Map"),
    render: ({
      places = [],
      route,
    }: {
      places?: readonly { label: string; lat: number; lng: number; detail?: string }[];
      route?: boolean;
    }) => {
      const pins = places.map((place, index) => ({
        id: String(index),
        label: place.label,
        lat: place.lat,
        lng: place.lng,
        ...(place.detail === undefined ? {} : { description: place.detail }),
      }));
      return (
        <GeoMap
          places={pins}
          {...(route === true && pins.length > 1
            ? {
                routes: [{ id: "route", points: pins.map(({ lat, lng }) => [lat, lng] as const) }],
              }
            : {})}
        />
      );
    },
  },
  Input: { ...component("Input"), render: InputView },
  DatePicker: {
    ...component("DatePicker"),
    render: (props: InputRenderProps) => <InputView {...props} type="date" />,
  },
  Card: { ...component("Card"), render: CardView },
  Form: {
    ...component("Form"),
    render: (props: CardRenderProps) => <CardView {...props} asForm />,
  },
  Select: {
    ...component("Select"),
    render: (props: ChoiceView) => (
      <Select
        aria-label={props.label ?? "Select an option"}
        {...(props.name === undefined ? {} : { name: props.name })}
        {...(props.defaultValue === undefined && props.value === undefined
          ? {}
          : { defaultValue: props.defaultValue ?? props.value })}
        onChange={(value) => {
          if (typeof value === "string") {
            dispatch(props, value);
          }
        }}
      >
        <Label>{props.label}</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>
            {choices(props.options).map((option) => (
              <ListBox.Item key={option.value} id={option.value} textValue={option.label}>
                {option.label}
                <ListBox.ItemIndicator />
              </ListBox.Item>
            ))}
          </ListBox>
        </Select.Popover>
      </Select>
    ),
  },
  RadioGroup: {
    ...component("RadioGroup"),
    render: (props: ChoiceView) => (
      <RadioGroup
        aria-label={props.label ?? "Choose an option"}
        {...(props.name === undefined ? {} : { name: props.name })}
        {...(props.defaultValue === undefined && props.value === undefined
          ? {}
          : { defaultValue: props.defaultValue ?? props.value })}
        onChange={(value) => {
          dispatch(props, value);
        }}
      >
        <Label>{props.label}</Label>
        {choices(props.options).map((option) => (
          <Radio key={option.value} value={option.value}>
            <Radio.Content>
              <Radio.Control>
                <Radio.Indicator />
              </Radio.Control>
              {option.label}
            </Radio.Content>
          </Radio>
        ))}
      </RadioGroup>
    ),
  },
  Checkbox: {
    ...component("Checkbox"),
    render: (props: ActionView & { label?: string; name?: string; value?: boolean }) => (
      <Checkbox
        {...(props.name === undefined ? {} : { name: props.name })}
        {...(props.value === undefined ? {} : { defaultSelected: props.value })}
        onChange={(value) => {
          dispatch(props, value);
        }}
      >
        <Checkbox.Content>
          <Checkbox.Control>
            <Checkbox.Indicator />
          </Checkbox.Control>
          {props.label ?? "Select"}
        </Checkbox.Content>
      </Checkbox>
    ),
  },
  CheckboxGroup: {
    ...component("CheckboxGroup"),
    render: (
      props: Omit<ChoiceView, "value" | "defaultValue"> & {
        value?: string[];
        defaultValue?: string[];
      },
    ) => (
      <CheckboxGroup
        aria-label={props.label ?? "Choose options"}
        {...(props.name === undefined ? {} : { name: props.name })}
        {...(props.defaultValue === undefined && props.value === undefined
          ? {}
          : { defaultValue: props.defaultValue ?? props.value })}
        onChange={(value) => {
          dispatch(props, value);
        }}
      >
        <Label>{props.label}</Label>
        {choices(props.options).map((option) => (
          <Checkbox key={option.value} value={option.value}>
            <Checkbox.Content>
              <Checkbox.Control>
                <Checkbox.Indicator />
              </Checkbox.Control>
              {option.label}
            </Checkbox.Content>
          </Checkbox>
        ))}
      </CheckboxGroup>
    ),
  },
  Slider: {
    ...component("Slider"),
    render: (
      props: ActionView & {
        label?: string;
        name?: string;
        value?: number;
        min?: number;
        max?: number;
        step?: number;
      },
    ) => (
      <Slider
        aria-label={props.label ?? "Value"}
        {...(props.name === undefined ? {} : { name: props.name })}
        {...(props.value === undefined ? {} : { defaultValue: props.value })}
        {...(props.min === undefined ? {} : { minValue: props.min })}
        {...(props.max === undefined ? {} : { maxValue: props.max })}
        {...(props.step === undefined ? {} : { step: props.step })}
        onChange={(value) => {
          dispatch(props, value);
        }}
      >
        <Label>{props.label}</Label>
        <Slider.Output />
        <Slider.Track>
          <Slider.Fill />
          <Slider.Thumb />
        </Slider.Track>
      </Slider>
    ),
  },
  Tabs: {
    ...component("Tabs"),
    render: ({
      tabs = [],
      children,
    }: {
      tabs?: readonly (string | { label?: string })[];
      children?: ReactNode;
    }) => {
      // oxlint-disable-next-line react/no-react-children -- The validated renderer supplies already-rendered React children; normalization preserves their keys.
      const panels = Children.toArray(children);
      const views = tabs.map((tab, index) => ({
        id: typeof tab === "string" ? tab : (tab.label ?? "View"),
        panel: panels[index],
      }));
      return (
        <Tabs>
          <Tabs.ListContainer>
            <Tabs.List aria-label="Views">
              {views.map((view) => (
                <Tabs.Tab key={view.id} id={view.id}>
                  {view.id}
                  <Tabs.Indicator />
                </Tabs.Tab>
              ))}
            </Tabs.List>
          </Tabs.ListContainer>
          {views.map((view) => (
            <Tabs.Panel key={view.id} id={view.id}>
              {view.panel}
            </Tabs.Panel>
          ))}
        </Tabs>
      );
    },
  },
  Markdown: {
    ...component("Markdown"),
    render: ({ value }: { value?: string }) => <MarkdownText text={value ?? ""} />,
  },
};
