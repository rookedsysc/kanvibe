const { writeJson } = require("./httpBody.js");

const LOCAL_HOOK_ADDRESSES = new Set(["127.0.0.1", "::1", "::ffff:127.0.0.1"]);

function rejectRemoteHookRequest(request, response) {
  if (LOCAL_HOOK_ADDRESSES.has(request.socket.remoteAddress)) {
    return false;
  }

  writeJson(response, 404, { success: false, error: "Not found" });
  return true;
}

module.exports = { rejectRemoteHookRequest };
