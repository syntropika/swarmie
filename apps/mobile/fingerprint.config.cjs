// Hash native source inputs rather than machine-dependent staged Rust binaries.
module.exports = {
  sourceSkips: ["PackageJsonAndroidAndIosScriptsIfNotContainRun", "ExpoConfigVersions"],
  ignorePaths: [
    "../../sdk/react-native/native/**/*",
    "../../sdk/react-native/android/src/main/jniLibs/**/*",
  ],
  extraSources: [
    { type: "dir", filePath: "../../crates/client", reasons: ["swarmieNativeClient"] },
    { type: "dir", filePath: "../../crates/client-ffi", reasons: ["swarmieNativeClient"] },
    { type: "dir", filePath: "../../sdk/wit", reasons: ["swarmieNativeBoundary"] },
    { type: "dir", filePath: "certs", reasons: ["updateSigningCertificates"] },
    ...[
      "../../Cargo.toml",
      "../../Cargo.lock",
      "../../mise.toml",
      "../../sdk/tools/build_mobile.py",
      "../../sdk/tools/build_apple.py",
    ].map((filePath) => ({ type: "file", filePath, reasons: ["swarmieNativeBuild"] })),
  ],
};
