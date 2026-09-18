define(["modules/apphost.js"], function (mod) {
  var inner = mod && mod.default ? mod.default : mod;
  var baseSupports = inner.supports;
  inner.supports = function (feature) {
    if (feature === "exit") return true;
    // Back-menu power items (TV layout): the client shows Shutdown/Restart
    // only when these report true, then calls appHost.shutdown()/restart().
    if (feature === "shutdown" || feature === "restart") return true;
    return baseSupports.call(inner, feature);
  };
  inner.exit = function () {
    try {
      window.__TAURI_INTERNALS__.invoke("quit_app");
    } catch (e) {
      window.close();
    }
    return Promise.resolve();
  };
  inner.shutdown = function () {
    try {
      window.__TAURI_INTERNALS__.invoke("shutdown_system");
    } catch (e) {}
    return Promise.resolve();
  };
  inner.restart = function () {
    try {
      window.__TAURI_INTERNALS__.invoke("restart_system");
    } catch (e) {}
    return Promise.resolve();
  };
  return inner;
});
