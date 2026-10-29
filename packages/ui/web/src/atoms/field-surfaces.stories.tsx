import type { Meta, StoryObj } from "@storybook/react-vite";

import {
  Autocomplete,
  Calendar,
  Checkbox,
  CheckboxGroup,
  ColorArea,
  ColorField,
  ColorPicker,
  ColorSlider,
  ColorSwatch,
  ColorSwatchPicker,
  ComboBox,
  DateField,
  DatePicker,
  DateRangePicker,
  Description,
  FieldError,
  Input,
  InputGroup,
  InputOTP,
  Label,
  ListBox,
  NumberField,
  Radio,
  RadioGroup,
  RangeCalendar,
  SearchField,
  Select,
  Switch,
  TextArea,
  TextField,
  TimeField,
} from "../index";

const meta = {
  title: "Foundations/Field surfaces",
  parameters: { layout: "fullscreen" },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

function Fields() {
  return (
    <div className="grid min-w-0 gap-4">
      <TextField fullWidth className="min-w-0" defaultValue="Documents">
        <Label>Server name</Label>
        <Input fullWidth className="min-w-0" />
      </TextField>
      <SearchField fullWidth className="min-w-0" aria-label="Search servers">
        <SearchField.Group className="w-full min-w-0">
          <SearchField.SearchIcon />
          <SearchField.Input placeholder="Search servers" />
          <SearchField.ClearButton />
        </SearchField.Group>
      </SearchField>
      <Select fullWidth className="min-w-0" defaultValue="current">
        <Label>Protocol version</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>
            <ListBox.Item id="current" textValue="2026-07-28">
              2026-07-28
              <ListBox.ItemIndicator />
            </ListBox.Item>
          </ListBox>
        </Select.Popover>
      </Select>
      <TextField fullWidth className="min-w-0" defaultValue="Summarize the requested changes.">
        <Label>Instructions</Label>
        <TextArea fullWidth className="min-w-0" rows={3} />
      </TextField>
    </div>
  );
}

export const CanvasAndPanel: Story = {
  render: () => (
    <div className="@container w-full min-w-0 bg-canvas text-ink">
      <div className="grid min-w-0 gap-8 @2xl:grid-cols-2">
        <section className="min-w-0 p-4">
          <h2 className="mb-5 font-medium">Canvas</h2>
          <Fields />
        </section>
        <section className="min-w-0 rounded-3xl bg-surface p-4">
          <h2 className="mb-5 font-medium">Panel</h2>
          <Fields />
        </section>
      </div>
    </div>
  ),
};

export const SelectionAndValidation: Story = {
  render: () => (
    <div className="grid w-full max-w-lg min-w-0 grid-cols-1 gap-6">
      <TextField fullWidth className="min-w-0" isInvalid defaultValue="Invalid address">
        <Label>Email address</Label>
        <Input fullWidth className="min-w-0" type="email" />
        <FieldError>Enter a valid email address.</FieldError>
      </TextField>
      <TextField fullWidth className="min-w-0" isDisabled defaultValue="Unavailable field">
        <Label>Unavailable</Label>
        <Input fullWidth className="min-w-0" />
      </TextField>
      <TextField fullWidth className="min-w-0">
        <Label>Username</Label>
        <InputGroup fullWidth className="min-w-0">
          <InputGroup.Prefix>@</InputGroup.Prefix>
          <InputGroup.Input className="min-w-0" placeholder="username" />
        </InputGroup>
      </TextField>
      <CheckboxGroup defaultValue={["replies"]}>
        <Label>Notifications</Label>
        {["replies", "mentions"].map((value) => (
          <Checkbox key={value} value={value}>
            <Checkbox.Content>
              <Checkbox.Control>
                <Checkbox.Indicator />
              </Checkbox.Control>
              {value === "replies" ? "Replies" : "Mentions"}
            </Checkbox.Content>
          </Checkbox>
        ))}
      </CheckboxGroup>
      <RadioGroup defaultValue="brief">
        <Label>Response length</Label>
        {["brief", "detailed"].map((value) => (
          <Radio key={value} value={value}>
            <Radio.Content>
              <Radio.Control>
                <Radio.Indicator />
              </Radio.Control>
              {value === "brief" ? "Brief" : "Detailed"}
            </Radio.Content>
          </Radio>
        ))}
      </RadioGroup>
      <Switch defaultSelected aria-label="Enable notifications">
        <Switch.Content>
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
        </Switch.Content>
        <Description>Control notifications for this workspace.</Description>
      </Switch>
      <Switch defaultSelected isDisabled>
        <Switch.Content>
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
          Unavailable notifications
        </Switch.Content>
      </Switch>
      <ComboBox fullWidth className="min-w-0">
        <Label>Project</Label>
        <ComboBox.InputGroup className="w-full min-w-0">
          <Input fullWidth className="min-w-0" placeholder="Choose a project" />
          <ComboBox.Trigger />
        </ComboBox.InputGroup>
        <ComboBox.Popover>
          <ProjectOptions />
        </ComboBox.Popover>
      </ComboBox>
      <Autocomplete fullWidth className="min-w-0" placeholder="Choose a project">
        <Label>Destination</Label>
        <Autocomplete.Trigger>
          <Autocomplete.Value />
          <Autocomplete.Indicator />
        </Autocomplete.Trigger>
        <Autocomplete.Popover>
          <ProjectOptions />
        </Autocomplete.Popover>
      </Autocomplete>
    </div>
  ),
};

function ProjectOptions() {
  return (
    <ListBox>
      <ListBox.Item id="web" textValue="Website">
        Website
        <ListBox.ItemIndicator />
      </ListBox.Item>
      <ListBox.Item id="desktop" textValue="Desktop">
        Desktop
        <ListBox.ItemIndicator />
      </ListBox.Item>
      <ListBox.Item id="unavailable" textValue="Unavailable" isDisabled>
        Unavailable
        <ListBox.ItemIndicator />
      </ListBox.Item>
    </ListBox>
  );
}

function DateCalendar() {
  return (
    <Calendar aria-label="Event date">
      <Calendar.Header>
        <Calendar.Heading />
        <Calendar.NavButton slot="previous" />
        <Calendar.NavButton slot="next" />
      </Calendar.Header>
      <Calendar.Grid>
        <Calendar.GridHeader>
          {(day) => <Calendar.HeaderCell>{day}</Calendar.HeaderCell>}
        </Calendar.GridHeader>
        <Calendar.GridBody>{(date) => <Calendar.Cell date={date} />}</Calendar.GridBody>
      </Calendar.Grid>
    </Calendar>
  );
}

function DatesCalendar() {
  return (
    <RangeCalendar aria-label="Trip dates">
      <RangeCalendar.Header>
        <RangeCalendar.Heading />
        <RangeCalendar.NavButton slot="previous" />
        <RangeCalendar.NavButton slot="next" />
      </RangeCalendar.Header>
      <RangeCalendar.Grid>
        <RangeCalendar.GridHeader>
          {(day) => <RangeCalendar.HeaderCell>{day}</RangeCalendar.HeaderCell>}
        </RangeCalendar.GridHeader>
        <RangeCalendar.GridBody>
          {(date) => <RangeCalendar.Cell date={date} />}
        </RangeCalendar.GridBody>
      </RangeCalendar.Grid>
    </RangeCalendar>
  );
}

export const NumericAndTemporal: Story = {
  render: () => (
    <div className="grid w-full max-w-lg min-w-0 grid-cols-1 gap-6">
      <NumberField fullWidth className="min-w-0" defaultValue={3} minValue={0} maxValue={10}>
        <Label>Attempts</Label>
        <NumberField.Group>
          <NumberField.DecrementButton />
          <NumberField.Input />
          <NumberField.IncrementButton />
        </NumberField.Group>
      </NumberField>
      <DateField fullWidth className="min-w-0">
        <Label>Date</Label>
        <DateField.Group fullWidth className="min-w-0">
          <DateField.Input>{(segment) => <DateField.Segment segment={segment} />}</DateField.Input>
        </DateField.Group>
      </DateField>
      <TimeField fullWidth className="min-w-0">
        <Label>Time</Label>
        <TimeField.Group fullWidth className="min-w-0">
          <TimeField.Input>{(segment) => <TimeField.Segment segment={segment} />}</TimeField.Input>
        </TimeField.Group>
      </TimeField>
      <DatePicker className="min-w-0">
        <Label>Event date</Label>
        <DateField.Group fullWidth className="min-w-0">
          <DateField.Input>{(segment) => <DateField.Segment segment={segment} />}</DateField.Input>
          <DateField.Suffix>
            <DatePicker.Trigger>
              <DatePicker.TriggerIndicator />
            </DatePicker.Trigger>
          </DateField.Suffix>
        </DateField.Group>
        <DatePicker.Popover>
          <DateCalendar />
        </DatePicker.Popover>
      </DatePicker>
      <DateRangePicker className="min-w-0">
        <Label>Trip dates</Label>
        <DateField.Group fullWidth className="min-w-0">
          <DateField.Input slot="start">
            {(segment) => <DateField.Segment segment={segment} />}
          </DateField.Input>
          <DateRangePicker.RangeSeparator />
          <DateField.Input slot="end">
            {(segment) => <DateField.Segment segment={segment} />}
          </DateField.Input>
          <DateField.Suffix>
            <DateRangePicker.Trigger>
              <DateRangePicker.TriggerIndicator />
            </DateRangePicker.Trigger>
          </DateField.Suffix>
        </DateField.Group>
        <DateRangePicker.Popover>
          <DatesCalendar />
        </DateRangePicker.Popover>
      </DateRangePicker>
      <InputOTP aria-label="Verification code" maxLength={6}>
        <InputOTP.Group>
          {[0, 1, 2, 3, 4, 5].map((index) => (
            <InputOTP.Slot key={index} index={index} />
          ))}
        </InputOTP.Group>
      </InputOTP>
      <div className="min-w-0 overflow-x-auto">
        <DateCalendar />
      </div>
      <div className="min-w-0 overflow-x-auto">
        <DatesCalendar />
      </div>
    </div>
  ),
};

export const ColorControls: Story = {
  render: () => (
    <div className="grid w-full max-w-lg min-w-0 grid-cols-1 gap-6">
      <ColorField fullWidth className="min-w-0" defaultValue="#dda99f">
        <Label>Color</Label>
        <ColorField.Group fullWidth className="min-w-0">
          <ColorField.Input />
        </ColorField.Group>
      </ColorField>
      <ColorPicker defaultValue="#dda99f">
        <ColorPicker.Trigger>
          <ColorSwatch />
          <Label>Pick a color</Label>
        </ColorPicker.Trigger>
        <ColorPicker.Popover>
          <ColorArea
            aria-label="Color area"
            colorSpace="hsb"
            xChannel="saturation"
            yChannel="brightness"
          >
            <ColorArea.Thumb />
          </ColorArea>
          <ColorSlider channel="hue" colorSpace="hsb">
            <Label>Hue</Label>
            <ColorSlider.Output />
            <ColorSlider.Track>
              <ColorSlider.Thumb />
            </ColorSlider.Track>
          </ColorSlider>
        </ColorPicker.Popover>
      </ColorPicker>
      <ColorArea aria-label="Saturation and brightness" defaultValue="#dda99f">
        <ColorArea.Thumb />
      </ColorArea>
      <ColorSlider channel="hue" defaultValue="hsl(0, 100%, 50%)">
        <Label>Hue</Label>
        <ColorSlider.Output />
        <ColorSlider.Track>
          <ColorSlider.Thumb />
        </ColorSlider.Track>
      </ColorSlider>
      <ColorSwatchPicker aria-label="Color presets" defaultValue="#dda99f">
        {["#dda99f", "#262626", "#ffffff"].map((color) => (
          <ColorSwatchPicker.Item key={color} color={color}>
            <ColorSwatchPicker.Swatch />
            <ColorSwatchPicker.Indicator />
          </ColorSwatchPicker.Item>
        ))}
      </ColorSwatchPicker>
    </div>
  ),
};
