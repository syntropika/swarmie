export const settingsSections = ["general", "appearance", "security", "account"] as const;
export type SettingsSection = (typeof settingsSections)[number];
