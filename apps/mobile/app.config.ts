import type { ExpoConfig } from "expo/config";

import languageNames from "./locales.json";

const config: ExpoConfig = {
  name: "Swarmie",
  slug: "swarmie-mobile",
  icon: "../desktop/assets/icon.png",
  scheme: "swarmie",
  version: "0.1.0",
  orientation: "default",
  userInterfaceStyle: "light",
  // SDK 57 uses Hermes and the New Architecture without a legacy engine switch.
  ios: { bundleIdentifier: "app.swarmie.mobile", supportsTablet: true },
  android: {
    package: "app.swarmie.mobile",
    // Functional modules are installed ahead of their UI. Ask only at the action that needs access.
    blockedPermissions: [
      "android.permission.RECORD_AUDIO",
      "android.permission.CAMERA",
      "android.permission.READ_MEDIA_IMAGES",
      "android.permission.READ_MEDIA_VIDEO",
      "android.permission.READ_MEDIA_AUDIO",
      "android.permission.READ_EXTERNAL_STORAGE",
      "android.permission.WRITE_EXTERNAL_STORAGE",
      "android.permission.READ_MEDIA_VISUAL_USER_SELECTED",
    ],
  },
  plugins: [
    [
      "expo-localization",
      { supportedLocales: Object.keys(languageNames), allowDynamicLocaleChangesAndroid: true },
    ],
    "expo-router",
    "expo-secure-store",
    "expo-splash-screen",
    ["expo-dev-client", { addGeneratedScheme: false }],
    [
      "expo-image-picker",
      { cameraPermission: false, microphonePermission: false, photosPermission: false },
    ],
    [
      "expo-audio",
      {
        microphonePermission: false,
        enableBackgroundRecording: false,
        enableBackgroundPlayback: false,
        recordAudioAndroid: false,
      },
    ],
    [
      "expo-media-library",
      {
        photosPermission: false,
        savePhotosPermission: false,
        granularPermissions: [],
        isAccessMediaLocationEnabled: false,
      },
    ],
    ["expo-video", { supportsBackgroundPlayback: false, supportsPictureInPicture: false }],
    [
      "expo-build-properties",
      {
        android: { usesCleartextTraffic: false, buildArchs: ["arm64-v8a", "x86_64"] },
        ios: { deploymentTarget: "16.4" },
      },
    ],
  ],
  experiments: { typedRoutes: true },
  updates: { enabled: false },
  web: { bundler: "metro", output: "single" },
};

export default config;
