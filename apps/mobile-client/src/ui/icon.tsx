import type { ColorValue } from "react-native";
import Svg, { Path } from "react-native-svg";

const paths = {
  conversations: "M4 4h16v12H9l-5 4V4Z",
  routines: "M12 8v5l3 2M21 12a9 9 0 1 1-3-7M21 3v5h-5",
  settings:
    "M12 3v3m0 12v3M3 12h3m12 0h3M5.6 5.6l2.1 2.1m8.6 8.6 2.1 2.1M5.6 18.4l2.1-2.1m8.6-8.6 2.1-2.1M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z",
  eye: "M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12ZM15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z",
  eyeOff:
    "M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12ZM15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0ZM3 3l18 18",
};

export function Icon({
  name,
  color,
  size = 24,
}: {
  name: keyof typeof paths;
  color: ColorValue | undefined;
  size?: number;
}) {
  return (
    <Svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
    >
      <Path
        d={paths[name]}
        fill="none"
        stroke={color ?? "currentColor"}
        strokeWidth={1.7}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </Svg>
  );
}
