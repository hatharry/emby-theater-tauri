window.appStartInfo = Object.assign({
  environment: "emby-theater",
  appVersion: "{version}",
  deviceId: "{did}",
  deviceName: "{device_name}",
  platform: "linux",
  architecture: "x64",
  canUpdate: false,
  canRestart: false,
  canQuit: true,
  devToolsEnabled: true,
  supportedCommands: [],
  paths: {
    serverdiscovery: "embyhost://host/serverdiscovery.js",
    wakeonlan: "embyhost://host/wakeonlan.js",
    apphost: "embyhost://host/apphost.js",
  },
  plugins: ["embyhost://host/cec.js"{profile_plugin}],
}, window.appStartInfo || {});
(function startEmby() {
  if (window.Emby && window.Emby.App && typeof window.Emby.App.start === "function") {
    window.Emby.App.start(window.appStartInfo);
  } else {
    setTimeout(startEmby, 50);
  }})();
// Report the client's persisted view mode (settings -> "View mode", stored by
// layoutmanager as the "layout" key) so the window starts fullscreen for the
// TV layout. Only an explicit "tv" counts: empty/auto resolves to the
// desktop/mobile layout, which runs in a normal window.
(function () {
  function send() {
    try {
      var l = localStorage.getItem("layout");
      window.__TAURI_INTERNALS__.invoke("set_layout_mode", {
        mode: l === "tv" ? "tv" : "normal",
      });
      return true;
    } catch (e) {
      return false;
    }
  }
  if (!send()) setTimeout(send, 200);
  // The settings page can change the view mode without a page reload; catch
  // the write so the window state follows immediately. Must patch the
  // prototype: assigning localStorage.setItem would just store an ITEM named
  // "setItem" (Storage is an exotic object with a named-property setter).
  var orig = Storage.prototype.setItem;
  Storage.prototype.setItem = function (key, value) {
    orig.call(this, key, value);
    if (key === "layout") send();
  };
})();