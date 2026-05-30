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

  return app.getPath("userData");
}

module.exports = {
  applyAppDataDirectoryOverride,
  getRequestedAppDataDirectory,
};
