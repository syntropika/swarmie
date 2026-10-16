import { describe, expect, test } from "bun:test";

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
    expect(config.updates?.enabled).toBe(false);
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
