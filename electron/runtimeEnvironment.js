const path = require("node:path");

function getRequestedAppDataDirectory(env = process.env) {
  const value = env.KANVIBE_APP_DATA_DIR;
  if (typeof value !== "string") return null;

  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function applyAppDataDirectoryOverride(app, env = process.env) {
  const requestedAppDataDir = getRequestedAppDataDirectory(env);
  if (requestedAppDataDir) {
    app.setPath("userData", requestedAppDataDir);
    return requestedAppDataDir;
  }

  if (env.KANVIBE_QA_MODE === "1") {
    throw new Error("Automated QA requires an isolated KANVIBE_APP_DATA_DIR");
  }

  const isManualQa = app.commandLine.hasSwitch("qa");
  if (!app.isPackaged || isManualQa) {
    const directory = env.KANVIBE_RENDERER_URL && !isManualQa ? "kanvibe-dev" : "kanvibe-qa";
    const appDataDirectory = path.join(app.getPath("appData"), directory);
    app.setPath("userData", appDataDirectory);
    return appDataDirectory;
  }

  return app.getPath("userData");
}

module.exports = {
  applyAppDataDirectoryOverride,
  getRequestedAppDataDirectory,
};
