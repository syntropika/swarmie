// Hash distribution inputs outside the Expo application's own dependency graph.
module.exports = {
  sourceSkips: ["PackageJsonAndroidAndIosScriptsIfNotContainRun", "ExpoConfigVersions"],
  extraSources: [
    { type: "dir", filePath: "certs", reasons: ["updateSigningCertificates"] },
    { type: "file", filePath: "../../mise.toml", reasons: ["swarmieToolchain"] },
  ],
};
