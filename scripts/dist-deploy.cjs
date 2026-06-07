#!/usr/bin/env node

const { execFileSync } = require("node:child_process");
const { existsSync, readFileSync, readdirSync, statSync } = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const projectRoot = path.resolve(__dirname, "..");
const requiredEnvironmentKeys = ["CSC_NAME", "APPLE_API_KEY", "APPLE_API_KEY_ID", "APPLE_API_ISSUER"];
const notarizationEnvironmentKeys = [
  "APPLE_API_KEY",
  "APPLE_API_KEY_ID",
  "APPLE_API_ISSUER",
  "APPLE_ID",
  "APPLE_APP_SPECIFIC_PASSWORD",
  "APPLE_TEAM_ID",
  "APPLE_KEYCHAIN",
  "APPLE_KEYCHAIN_PROFILE",
];

function stripInlineComment(value) {
  const commentIndex = value.search(/\s#/);
  return commentIndex === -1 ? value : value.slice(0, commentIndex);
}

function parseEnvironmentValue(rawValue) {
  const trimmedValue = rawValue.trim();
  const quote = trimmedValue[0];

  if ((quote === "\"" || quote === "'") && trimmedValue.endsWith(quote)) {
    return trimmedValue.slice(1, -1);
  }

  return stripInlineComment(trimmedValue).trim();
}

function loadEnvironmentFile() {
  const environmentPath = path.join(projectRoot, ".env");

  if (!existsSync(environmentPath)) {
    throw new Error("Missing .env. Copy .env.example to .env and fill the Apple signing values.");
  }

  const lines = readFileSync(environmentPath, "utf8").split(/\r?\n/);

  for (const line of lines) {
    const trimmedLine = line.trim();

    if (!trimmedLine || trimmedLine.startsWith("#")) {
      continue;
    }

    const match = trimmedLine.match(/^(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$/);

    if (!match) {
      continue;
    }

    process.env[match[1]] = parseEnvironmentValue(match[2]);
  }
}

function expandFilePath(filePath) {
  if (filePath.startsWith("~/")) {
    return path.join(os.homedir(), filePath.slice(2));
  }

  if (filePath.startsWith("$HOME/")) {
    return path.join(os.homedir(), filePath.slice("$HOME/".length));
  }

  if (filePath.startsWith("${HOME}/")) {
    return path.join(os.homedir(), filePath.slice("${HOME}/".length));
  }

  return path.isAbsolute(filePath) ? filePath : path.resolve(projectRoot, filePath);
}

function ensureRequiredEnvironment() {
  const missingKeys = requiredEnvironmentKeys.filter((key) => !process.env[key]);

  if (missingKeys.length > 0) {
    throw new Error(`Missing required .env values: ${missingKeys.join(", ")}`);
  }

  process.env.APPLE_API_KEY = expandFilePath(process.env.APPLE_API_KEY);

  if (!existsSync(process.env.APPLE_API_KEY)) {
    throw new Error(`APPLE_API_KEY does not point to an existing .p8 file: ${process.env.APPLE_API_KEY}`);
  }
}

function runCommand(command, args, options = {}) {
  const commandText = [command, ...args].join(" ");
  console.log(`\n[kanvibe] $ ${commandText}`);

  execFileSync(command, args, {
    cwd: projectRoot,
    stdio: "inherit",
    env: options.env ?? process.env,
  });
}

function getCommandOutput(command, args) {
  return execFileSync(command, args, {
    cwd: projectRoot,
    encoding: "utf8",
    env: process.env,
  });
}

function ensureMacOS() {
  if (process.platform !== "darwin") {
    throw new Error("deploy requires macOS because codesign, stapler, and notarytool are macOS tools.");
  }
}

function ensureCommandAvailable(command, args) {
  try {
    execFileSync(command, args, {
      cwd: projectRoot,
      stdio: "ignore",
      env: process.env,
    });
  } catch {
    throw new Error(`Required command is unavailable or not configured: ${command} ${args.join(" ")}`);
  }
}

function ensureSigningIdentity() {
  const identities = getCommandOutput("security", ["find-identity", "-v", "-p", "codesigning"]);

  if (!identities.includes(process.env.CSC_NAME)) {
    throw new Error(`CSC_NAME was not found in the macOS codesigning identities: ${process.env.CSC_NAME}`);
  }
}

function getPackageVersion() {
  const packageJson = JSON.parse(readFileSync(path.join(projectRoot, "package.json"), "utf8"));
  return packageJson.version;
}

function findAppBundle(directoryPath, depth = 0) {
  if (depth > 5 || !existsSync(directoryPath)) {
    return null;
  }

  for (const entry of readdirSync(directoryPath)) {
    const entryPath = path.join(directoryPath, entry);
    const stats = statSync(entryPath);

    if (stats.isDirectory() && entry === "KanVibe.app") {
      return entryPath;
    }

    if (stats.isDirectory()) {
      const appBundlePath = findAppBundle(entryPath, depth + 1);

      if (appBundlePath) {
        return appBundlePath;
      }
    }
  }

  return null;
}

function createBuildEnvironment() {
  const buildEnvironment = { ...process.env };

  for (const key of notarizationEnvironmentKeys) {
    delete buildEnvironment[key];
  }

  return buildEnvironment;
}

function ensureBuildArtifacts(version) {
  const dmgPath = path.join(projectRoot, "dist", `KanVibe-${version}.dmg`);
  const appBundlePath = findAppBundle(path.join(projectRoot, "dist"));

  if (!existsSync(dmgPath)) {
    throw new Error(`Expected DMG was not created: ${dmgPath}`);
  }

  if (!appBundlePath) {
    throw new Error("Expected KanVibe.app bundle was not created under dist/.");
  }

  return { appBundlePath, dmgPath };
}

function submitDmgForNotarization(dmgPath) {
  runCommand("xcrun", [
    "notarytool",
    "submit",
    dmgPath,
    "--key",
    process.env.APPLE_API_KEY,
    "--key-id",
    process.env.APPLE_API_KEY_ID,
    "--issuer",
    process.env.APPLE_API_ISSUER,
    "--wait",
  ]);
}

function printDmgSha256(dmgPath) {
  const checksum = getCommandOutput("shasum", ["-a", "256", dmgPath]).trim();
  console.log(`\n[kanvibe] DMG sha256:\n${checksum}`);
}

function main() {
  try {
    ensureMacOS();
    loadEnvironmentFile();
    ensureRequiredEnvironment();
    ensureCommandAvailable("xcrun", ["--find", "notarytool"]);
    ensureCommandAvailable("xcrun", ["--find", "stapler"]);
    ensureSigningIdentity();

    const version = getPackageVersion();
    const buildEnvironment = createBuildEnvironment();

    runCommand("pnpm", ["dist"], { env: buildEnvironment });

    const { appBundlePath, dmgPath } = ensureBuildArtifacts(version);

    runCommand("codesign", ["--verify", "--deep", "--strict", "--verbose=2", appBundlePath]);
    submitDmgForNotarization(dmgPath);
    runCommand("xcrun", ["stapler", "staple", dmgPath]);
    runCommand("xcrun", ["stapler", "validate", dmgPath]);
    printDmgSha256(dmgPath);
  } catch (error) {
    console.error(`\n[kanvibe] deploy failed: ${error.message}`);
    process.exit(1);
  }
}

main();
