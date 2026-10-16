import type { ExpoConfig } from "expo/config";

import languageNames from "./locales.json";

const updateApplications = {
  development: {
    appId: "cbde1ef2-b30c-4203-ad25-e993e6b1d2bd",
    channel: "development",
    certificate: "./certs/development.pem",
  },
  production: {
    appId: "1a235ef9-ac36-46a0-b2dc-8b37e8747fea",
    channel: "production",
    certificate: "./certs/production.pem",
  },
};

export function createAppConfig(environment: Record<string, string | undefined>): ExpoConfig {
  const variant = environment.APP_VARIANT ?? "development";
  if (variant !== "development" && variant !== "production") {
    throw new Error("APP_VARIANT must be development or production");
  }
  const updateApplication = updateApplications[variant];
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
    icon: "./assets/app-icon.png",
    scheme,
    version: "0.1.0",
    orientation: "default",
    userInterfaceStyle: "light",
    // SDK 57 uses Hermes and the New Architecture without a legacy engine switch.
    ios: {
      bundleIdentifier: identifier,
      buildNumber,
      supportsTablet: true,
      ...iosEncryptionConfiguration(environment, variant),
    },
    android: {
      package: identifier,
      adaptiveIcon: {
        foregroundImage: "./assets/adaptive-icon-foreground.png",
        monochromeImage: "./assets/adaptive-icon-foreground.png",
        backgroundColor: "#ffffff",
      },
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
    runtimeVersion: { policy: "fingerprint" },
    updates: {
      enabled: true,
      url: "https://ota.syntropika.ai/manifest",
      requestHeaders: {
        "expo-app-id": updateApplication.appId,
        "expo-channel-name": updateApplication.channel,
        "xprem-branch": "",
      },
      codeSigningCertificate: updateApplication.certificate,
      codeSigningMetadata: { keyid: "main", alg: "rsa-v1_5-sha256" },
      checkAutomatically: "ON_LOAD",
      fallbackToCacheTimeout: 0,
      useEmbeddedUpdate: true,
    },
    extra: {
      appVariant: variant,
      ...(apiBaseUrl === undefined ? {} : { apiBaseUrl }),
    },
    web: { bundler: "metro", output: "single" },
  };
}

function iosEncryptionConfiguration(
  environment: Record<string, string | undefined>,
  variant: "development" | "production",
): Pick<NonNullable<ExpoConfig["ios"]>, "infoPlist"> {
  if (variant !== "production") {
    // Development distribution excludes France and is exempt from documentation.
    return { infoPlist: { ITSAppUsesNonExemptEncryption: false } };
  }
  const code = environment.IOS_PRODUCTION_ENCRYPTION_EXPORT_COMPLIANCE_CODE?.trim();
  if (code === undefined) {
    return {};
  }
  if (code.length === 0) {
    throw new Error("IOS_PRODUCTION_ENCRYPTION_EXPORT_COMPLIANCE_CODE must not be blank");
  }
  return {
    infoPlist: {
      ITSAppUsesNonExemptEncryption: true,
      ITSEncryptionExportComplianceCode: code,
    },
  };
}

export default function appConfig(): ExpoConfig {
  return createAppConfig(process.env);
}
