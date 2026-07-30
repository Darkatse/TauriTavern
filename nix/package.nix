{
  lib,
  stdenv,
  rustPlatform,
  cargo-tauri,
  fetchPnpmDeps,
  glib-networking,
  gst_all_1,
  gtk3,
  libayatana-appindicator,
  librsvg,
  nodejs_22,
  openssl,
  pkg-config,
  pnpm_10,
  pnpmConfigHook,
  webkitgtk_4_1,
  wrapGAppsHook4,
  src,
  gitBranch ? "main",
  gitRevision ? "",
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "tauritavern";
  version = "2.2.0";

  inherit src;

  cargoRoot = "src-tauri";
  buildAndTestSubdir = "src-tauri/crates/tauritavern";
  cargoLock = {
    lockFile = ../src-tauri/Cargo.lock;
  };

  pnpmDeps = fetchPnpmDeps {
    inherit (finalAttrs)
      pname
      version
      src
      ;
    pnpm = pnpm_10;
    fetcherVersion = 3;
    hash = "sha256-aDxsMBQcMWYJl4FPTo+cReYnkqbiMuSvKRzslwmkGVM=";
  };

  nativeBuildInputs = [
    cargo-tauri.hook
    nodejs_22
    pkg-config
    pnpm_10
    pnpmConfigHook
    wrapGAppsHook4
  ];

  buildInputs = [
    glib-networking
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-good
    gtk3
    libayatana-appindicator
    librsvg
    openssl
    webkitgtk_4_1
  ];

  tauriBundleType = "deb";

  # The repository harness owns the intentional feature matrix. The generic
  # buildRustPackage check phase enables custom-protocol while compiling lib
  # tests, which is not a supported test configuration for this workspace.
  doCheck = false;

  env = {
    TAURITAVERN_BUILD_BRANCH = gitBranch;
    TAURITAVERN_BUILD_REVISION = gitRevision;
  };

  meta = {
    description = "SillyTavern frontend and ecosystem rebuilt as a native Tauri application";
    homepage = "https://github.com/Darkatse/TauriTavern";
    license = lib.licenses.agpl3Only;
    mainProgram = "tauritavern";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
  };
})
