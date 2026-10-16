import { describe, expect, test } from "bun:test";
import { X509Certificate } from "node:crypto";
import { readFileSync } from "node:fs";

import { createAppConfig } from "~/app.config";

describe("mobile build variants", () => {
  test("development has an independent identity in release builds", () => {
    const config = createAppConfig({
      APP_VARIANT: "development",
      MOBILE_API_BASE_URL: "https://api-dev.swarmie.ai/",
      IOS_BUILD_NUMBER: "42.2",
      NODE_ENV: "production",
    });
    expect(config.name).toBe("Swarmie");
    expect(config.ios?.bundleIdentifier).toBe("app.swarmie.mobile.dev");
    expect(config.ios?.buildNumber).toBe("42.2");
    expect(config.android?.package).toBe("app.swarmie.mobile.dev");
    expect(config.scheme).toBe("swarmie-dev");
    expect(config.extra).toEqual({
      appVariant: "development",
      apiBaseUrl: "https://api-dev.swarmie.ai",
    });
    expect(config.updates?.enabled).toBe(true);
  });

  test("production keeps a separate identifier, links and explicit endpoint", () => {
    const config = createAppConfig({
      APP_VARIANT: "production",
      MOBILE_API_BASE_URL: "https://api.example.com",
    });
    expect(config.ios?.bundleIdentifier).toBe("app.swarmie.mobile");
    expect(config.android?.package).toBe("app.swarmie.mobile");
    expect(config.scheme).toBe("swarmie");
    expect(config.extra).toEqual({
      appVariant: "production",
      apiBaseUrl: "https://api.example.com",
    });
  });

  test("local commands default to development without inventing a backend", () => {
    const config = createAppConfig({});
    expect(config.ios?.bundleIdentifier).toBe("app.swarmie.mobile.dev");
    expect(config.extra).toEqual({ appVariant: "development" });
  });

  test("development keeps its own manual export declaration", () => {
    const config = createAppConfig({
      APP_VARIANT: "development",
      IOS_PRODUCTION_ENCRYPTION_EXPORT_COMPLIANCE_CODE: "fixture-approval-reference",
    });
    expect(config.ios?.infoPlist?.ITSEncryptionExportComplianceCode).toBeUndefined();
    expect(config.ios?.infoPlist?.ITSAppUsesNonExemptEncryption).toBeUndefined();
  });

  test("production embeds its supplied approval without inventing an exemption", () => {
    const pending = createAppConfig({ APP_VARIANT: "production" });
    expect(pending.ios?.infoPlist?.ITSEncryptionExportComplianceCode).toBeUndefined();
    expect(pending.ios?.infoPlist?.ITSAppUsesNonExemptEncryption).toBeUndefined();
    const approved = createAppConfig({
      APP_VARIANT: "production",
      IOS_PRODUCTION_ENCRYPTION_EXPORT_COMPLIANCE_CODE: " fixture-approval-reference ",
    });
    expect(approved.ios?.infoPlist).toEqual({
      ITSAppUsesNonExemptEncryption: true,
      ITSEncryptionExportComplianceCode: "fixture-approval-reference",
    });
    expect(() =>
      createAppConfig({
        APP_VARIANT: "production",
        IOS_PRODUCTION_ENCRYPTION_EXPORT_COMPLIANCE_CODE: " ",
      }),
    ).toThrow("IOS_PRODUCTION_ENCRYPTION_EXPORT_COMPLIANCE_CODE");
  });

  test("invalid environment choices fail before generating a native project", () => {
    expect(() => createAppConfig({ APP_VARIANT: "staging" })).toThrow("APP_VARIANT");
    for (const number of ["0", "abc", "1.2.3.4", "-1", "1;echo secret"]) {
      expect(() => createAppConfig({ IOS_BUILD_NUMBER: number })).toThrow("IOS_BUILD_NUMBER");
    }
    for (const url of [
      "http://api.example.com",
      "https://user:password@api.example.com",
      "https://api.example.com?token=value",
      "https://api.example.com#value",
    ]) {
      expect(() => createAppConfig({ MOBILE_API_BASE_URL: url })).toThrow("MOBILE_API_BASE_URL");
    }
  });
});

describe("mobile OTA trust boundaries", () => {
  test("each variant binds its own app, channel and signing certificate", () => {
    const development = createAppConfig({ APP_VARIANT: "development" });
    const production = createAppConfig({ APP_VARIANT: "production" });
    const certificates = [];
    for (const [variant, config] of [
      ["development", development],
      ["production", production],
    ] as const) {
      expect(config.updates?.url).toBe("https://ota.syntropika.ai/manifest");
      expect(config.updates?.requestHeaders?.["expo-channel-name"]).toBe(variant);
      expect(config.updates?.requestHeaders?.["xprem-branch"]).toBe("");
      expect(config.runtimeVersion).toEqual({ policy: "fingerprint" });
      expect(config.updates?.codeSigningMetadata).toEqual({
        keyid: "main",
        alg: "rsa-v1_5-sha256",
      });
      const path = config.updates?.codeSigningCertificate;
      if (path === undefined || path.length === 0) {
        throw new Error("OTA builds require a signing certificate");
      }
      const pem = readFileSync(new URL(`../${path}`, import.meta.url), "utf-8");
      expect(pem).not.toContain("PRIVATE KEY");
      const certificate = new X509Certificate(pem);
      expect(certificate.publicKey.asymmetricKeyType).toBe("rsa");
      certificates.push(certificate.fingerprint256);
      expect(config.updates?.checkAutomatically).toBe("ON_LOAD");
      expect(config.updates?.fallbackToCacheTimeout).toBe(0);
      expect(config.updates?.useEmbeddedUpdate).toBe(true);
    }
    expect(development.updates?.requestHeaders?.["expo-app-id"]).not.toBe(
      production.updates?.requestHeaders?.["expo-app-id"],
    );
    expect(certificates[0]).not.toBe(certificates[1]);
  });
});
