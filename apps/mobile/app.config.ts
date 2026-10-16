import type { ExpoConfig } from "expo/config";

import languageNames from "./locales.json";

export function createAppConfig(environment: Record<string, string | undefined>): ExpoConfig {
  const variant = environment.APP_VARIANT ?? "development";
  if (variant !== "development" && variant !== "production") {
    throw new Error("APP_VARIANT must be development or production");
  }
  const identifier = variant === "development" ? "app.swarmie.mobile.dev" : "app.swarmie.mobile";
  const scheme = variant === "development" ? "swarmie-dev" : "swarmie";
  const buildNumber = environment.IOS_BUILD_NUMBER ?? "1";
  if (!/^[1-9]\d*(?:\.\d+){0,2}$/u.test(buildNumber) || buildNumber.length > 18) {
    throw new Error(
      "IOS_BUILD_NUMBER must be a positive numeric version with at most three components",
    );
  }
  let apiBaseUrl = environment.MOBILE_API_BASE_URL;
  if (apiBaseUrl !== undefined) {
    if (!URL.canParse(apiBaseUrl)) {
      throw new Error("MOBILE_API_BASE_URL must be a valid HTTPS URL");
    }
    const url = new URL(apiBaseUrl);
    if (url.protocol !== "https:" || url.username || url.password || url.search || url.hash) {
      throw new Error(
        "MOBILE_API_BASE_URL must be an HTTPS URL without credentials, query or fragment",
      );
    }
    apiBaseUrl = url.href.replace(/\/$/u, "");
  }

  return {
    name: "Swarmie",
    slug: "swarmie-mobile",
    icon: "../desktop/assets/icon.png",
    scheme,
    version: "0.1.0",
    orientation: "default",
    userInterfaceStyle: "light",
    // SDK 57 uses Hermes and the New Architecture without a legacy engine switch.
    ios: { bundleIdentifier: identifier, buildNumber, supportsTablet: true },
    android: {
      package: identifier,
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
    extra: {
      appVariant: variant,
      ...(apiBaseUrl === undefined ? {} : { apiBaseUrl }),
    },
    web: { bundler: "metro", output: "single" },
  };
}

export default function appConfig(): ExpoConfig {
  return createAppConfig(process.env);
}
