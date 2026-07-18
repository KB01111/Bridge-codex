import { readFileSync } from "node:fs";
import { resolve } from "node:path";

export const EXPECTED_WIX_UPGRADE_CODE =
  "a76106d9-e397-575e-b340-78f9ca968434";

export function validateWixUpgradeCode(upgradeCode) {
  if (upgradeCode !== EXPECTED_WIX_UPGRADE_CODE) {
    throw new Error(
      `WiX upgradeCode must remain pinned to ${EXPECTED_WIX_UPGRADE_CODE}`,
    );
  }
}

export function wixVersionFor(releaseVersion) {
  const semanticNumber = "(0|[1-9]\\d*)";
  const stableVersion = releaseVersion.match(
    new RegExp(`^${semanticNumber}\\.${semanticNumber}\\.${semanticNumber}$`),
  );
  const releaseCandidateVersion = releaseVersion.match(
    new RegExp(
      `^${semanticNumber}\\.${semanticNumber}\\.${semanticNumber}-rc\\.${semanticNumber}$`,
    ),
  );
  if (!stableVersion && !releaseCandidateVersion) {
    throw new Error(
      `VERSION must be a stable or numbered RC semantic version: ${releaseVersion}`,
    );
  }

  const versionParts = stableVersion ?? releaseCandidateVersion;
  const [, major, minor, patch, releaseCandidate] = versionParts;
  const wixPatchStride = 100;
  const wixStableSlot = wixPatchStride - 1;
  const wixReleaseCandidate = releaseCandidate
    ? Number(releaseCandidate)
    : wixStableSlot;
  if (
    releaseCandidate &&
    (wixReleaseCandidate === 0 || wixReleaseCandidate >= wixStableSlot)
  ) {
    throw new Error(
      `MSI version mapping supports RC numbers 1 through ${wixStableSlot - 1}`,
    );
  }
  const wixBuildLimit = 65_535;
  const wixStableBuild = Number(patch) * wixPatchStride + wixStableSlot;
  if (wixStableBuild > wixBuildLimit) {
    throw new Error(
      `MSI version mapping cannot reserve a stable slot for ${releaseVersion}`,
    );
  }
  const wixBuild = Number(patch) * wixPatchStride + wixReleaseCandidate;
  const wixVersion = `${major}.${minor}.${wixBuild}`;
  const wixVersionLimits = [255, 255, wixBuildLimit];
  for (const [index, part] of wixVersion.split(".").entries()) {
    if (Number(part) > wixVersionLimits[index]) {
      throw new Error(`MSI version field is out of range: ${wixVersion}`);
    }
  }
  return wixVersion;
}

const root = resolve(import.meta.dirname, "..");
const releaseVersion = readFileSync(resolve(root, "VERSION"), "utf8").trim();
const packageJson = JSON.parse(
  readFileSync(resolve(root, "package.json"), "utf8"),
);
const tauriConfig = JSON.parse(
  readFileSync(resolve(root, "src-tauri", "tauri.conf.json"), "utf8"),
);
const cargoToml = readFileSync(
  resolve(root, "src-tauri", "Cargo.toml"),
  "utf8",
);
const cargoVersion = cargoToml.match(/^version = "([^"]+)"$/m)?.[1];
const cargoLock = readFileSync(resolve(root, "..", "Cargo.lock"), "utf8");
const lockedDesktopPackage = cargoLock
  .split("[[package]]")
  .find((entry) => /^\s*name = "codex-desktop"\s*$/m.test(entry));
const lockedCargoVersion = lockedDesktopPackage?.match(
  /^\s*version = "([^"]+)"\s*$/m,
)?.[1];
const releaseNotes = readFileSync(resolve(root, "RELEASE_NOTES.md"), "utf8");
const wixVersion = wixVersionFor(releaseVersion);

const versions = new Map([
  ["package.json", packageJson.version],
  ["src-tauri/tauri.conf.json", tauriConfig.version],
  ["src-tauri/Cargo.toml", cargoVersion],
  ["../Cargo.lock (codex-desktop)", lockedCargoVersion],
]);

const mismatches = [...versions].filter(
  ([, version]) => version !== releaseVersion,
);
if (mismatches.length > 0) {
  const details = mismatches
    .map(([file, version]) => `${file}: ${version ?? "missing"}`)
    .join("\n");
  throw new Error(
    `Bridge desktop version must match VERSION (${releaseVersion}):\n${details}`,
  );
}

if (packageJson.name !== "@kb01111/bridge-codex-desktop") {
  throw new Error("package.json must use the KB-owned npm scope");
}

const releaseNotesVersion = releaseNotes.match(
  /^# Bridge Codex ([^\r\n]+)\r?$/m,
)?.[1];
if (releaseNotesVersion !== releaseVersion) {
  throw new Error("RELEASE_NOTES.md heading must match VERSION");
}

if (
  tauriConfig.bundle?.windows?.webviewInstallMode?.type !== "offlineInstaller"
) {
  throw new Error(
    "Windows releases must bundle the offline WebView2 installer",
  );
}

if (tauriConfig.bundle?.windows?.wix?.version !== wixVersion) {
  throw new Error(
    `WiX version must map ${releaseVersion} to Windows installer version ${wixVersion}`,
  );
}

validateWixUpgradeCode(tauriConfig.bundle?.windows?.wix?.upgradeCode);

console.log(`Bridge Codex version ${releaseVersion} is aligned.`);
