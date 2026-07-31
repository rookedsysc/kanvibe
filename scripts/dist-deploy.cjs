#!/usr/bin/env node

const { execFileSync } = require("node:child_process");
const { closeSync, existsSync, openSync, readFileSync, readdirSync, readSync, statSync } = require("node:fs");
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

// electron-builder가 실행 환경에서 패키지 매니저를 추론할 때 참고하는 값들이다.
const packageManagerHintKeys = ["npm_config_user_agent", "npm_execpath", "npm_lifecycle_event", "npm_lifecycle_script"];

// 앱 기동에 필요하지만 잘못된 수집 경로에서 누락된 적이 있는 전이 의존성이다.
const requiredPackagedModules = ["ms", "scheduler", "ansi-regex", "decimal.js", "string_decoder", "util-deprecate", "wrappy", "typeorm", "better-sqlite3"];

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

/**
 * electron-builder는 락 파일이 여러 개면 패키지 매니저를 판정하지 못하고 실행 환경 변수를 보고
 * 결정한다. 이 저장소에는 pnpm-lock.yaml과 package-lock.json이 함께 있어, pnpm이 띄운 프로세스에서
 * 패키징하면 pnpm 수집기가 선택되고 전이 의존성 일부가 asar에서 누락된다. 패키징에만 쓰는 환경에서
 * 패키지 매니저 힌트를 지워 항상 같은 수집 경로를 타도록 고정한다.
 *
 * @param {NodeJS.ProcessEnv} buildEnvironment 빌드용 환경
 * @returns {NodeJS.ProcessEnv} 패키지 매니저 힌트를 제거한 패키징 전용 환경
 */
function createPackagingEnvironment(buildEnvironment) {
  const packagingEnvironment = { ...buildEnvironment };

  for (const key of packageManagerHintKeys) {
    delete packagingEnvironment[key];
  }

  return packagingEnvironment;
}

/**
 * 패키징 결과물이 앱 실행에 필요한 의존성을 실제로 담고 있는지 검사한다. 서명·공증이 모두 성공해도
 * 의존성이 빠진 채로 배포될 수 있어, 발행 전에 여기서 끊는다.
 *
 * @param {string} appBundlePath 검사할 앱 번들 경로
 */
function ensurePackagedDependencies(appBundlePath) {
  const asarPath = path.join(appBundlePath, "Contents", "Resources", "app.asar");
  const packaged = readAsarNodeModuleNames(asarPath);
  const missing = requiredPackagedModules.filter((name) => !packaged.has(name));

  if (missing.length > 0) {
    throw new Error(
      `packaged app.asar is missing required modules: ${missing.join(", ")}. ` +
        "This usually means electron-builder collected dependencies with the wrong package manager."
    );
  }

  console.log(`\n[kanvibe] packaged node_modules verified: ${packaged.size} packages`);
}

/**
 * asar 헤더만 읽어 최상위 node_modules 패키지 이름을 모은다.
 *
 * @param {string} asarPath 대상 asar 경로
 * @returns {Set<string>} 패키징된 최상위 패키지 이름 집합
 */
function readAsarNodeModuleNames(asarPath) {
  const descriptor = openSync(asarPath, "r");

  try {
    const sizeBuffer = Buffer.alloc(16);
    readSync(descriptor, sizeBuffer, 0, 16, 0);

    const headerSize = sizeBuffer.readUInt32LE(12);
    const headerBuffer = Buffer.alloc(headerSize);
    readSync(descriptor, headerBuffer, 0, headerSize, 16);

    const header = JSON.parse(headerBuffer.toString("utf8").replace(/\0+$/, ""));
    const names = new Set();

    for (const [name, entry] of Object.entries(header.files?.node_modules?.files ?? {})) {
      if (name.startsWith("@")) {
        for (const scoped of Object.keys(entry.files ?? {})) {
          names.add(`${name}/${scoped}`);
        }
        continue;
      }

      names.add(name);
    }

    return names;
  } finally {
    closeSync(descriptor);
  }
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

    runCommand("pnpm", ["db:prepare"], { env: buildEnvironment });
    runCommand("pnpm", ["build"], { env: buildEnvironment });
    runCommand("pnpm", ["rebuild:native:electron"], { env: buildEnvironment });

    // electron-builder는 pnpm이 띄운 프로세스에서 실행하면 전이 의존성을 누락시키므로 직접 호출한다.
    runCommand(path.join(projectRoot, "node_modules", ".bin", "electron-builder"), ["--mac", "dmg"], {
      env: createPackagingEnvironment(buildEnvironment),
    });

    const { appBundlePath, dmgPath } = ensureBuildArtifacts(version);

    ensurePackagedDependencies(appBundlePath);
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
