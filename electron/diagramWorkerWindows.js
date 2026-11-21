const diagramWorkerWindows = new WeakSet();

function registerDiagramWorkerWindow(window) {
  diagramWorkerWindows.add(window);
}

function isDiagramWorkerWindow(window) {
  return diagramWorkerWindows.has(window);
}

function getUserWindows(BrowserWindow) {
  return BrowserWindow.getAllWindows().filter((window) => !isDiagramWorkerWindow(window));
}

module.exports = { registerDiagramWorkerWindow, isDiagramWorkerWindow, getUserWindows };
