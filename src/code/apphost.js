define(["modules/apphost.js"], function (mod) {
  var inner = mod && mod.default ? mod.default : mod;
  // The web apphost derives its identity from the browser user-agent (so the
  // server shows "Safari" / "Emby Web" / the web bundle's version) and ignores
  // appStartInfo. Override the identity getters to report this native app
  // instead: Client="Emby Theater", DeviceName=<hostname>, Version=<app ver>.
  var info = window.appStartInfo || {};
  if (info.appName) inner.appName = function () { return info.appName; };
  if (info.deviceName) inner.deviceName = function () { return info.deviceName; };
  if (info.appVersion) inner.appVersion = function () { return info.appVersion; };
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
