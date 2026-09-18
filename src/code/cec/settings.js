define([
  "modules/loading/loading.js",
  "modules/viewmanager/baseview.js",
  "modules/common/appsettings.js",
  "modules/emby-elements/emby-select/emby-select.js",
  "modules/emby-elements/emby-scroller/emby-scroller.js",
], function (loading, BaseView, appSettings) {
  // alameda hands the raw ES-module namespace to the factory; unwrap defaults.
  loading = loading.default || loading;
  BaseView = BaseView.default || BaseView;
  appSettings = appSettings.default || appSettings;
  function onSubmit(e) {
    e.preventDefault();
    return false;
  }
  function renderSettings(view) {
    view.querySelector(".hdmiPort").value = appSettings.get("cec-hdmiport") || "";
    // Populate the adapter select from the native probe (Auto + one option
    // per /dev/cecN, flagged when nothing is plugged into that port).
    var sel = view.querySelector(".cecDevice");
    var saved = appSettings.get("cec-device") || "";
    try {
      window.__TAURI_INTERNALS__.invoke("cec_devices").then(
        function (devs) {
          while (sel.options.length > 1) sel.removeChild(sel.options[1]);
          for (var i = 0; i < devs.length; i++) {
            var o = document.createElement("option");
            o.value = devs[i].path;
            o.textContent = devs[i].connected ? devs[i].path : devs[i].path + " (nothing attached)";
            sel.appendChild(o);
          }
          sel.value = saved;
          if (sel.value !== saved) sel.value = "";
        },
        function () {}
      );
    } catch (e) {}
  }
  function saveSettings(view) {
    var port = view.querySelector(".hdmiPort").value;
    if ((appSettings.get("cec-hdmiport") || "") !== port) {
      appSettings.set("cec-hdmiport", port);
      try {
        window.__TAURI_INTERNALS__.invoke("cec_set_hdmi_port", { port: port });
      } catch (e) {}
    }
    var device = view.querySelector(".cecDevice").value;
    if ((appSettings.get("cec-device") || "") !== device) {
      appSettings.set("cec-device", device);
      try {
        window.__TAURI_INTERNALS__.invoke("cec_set_device", { device: device });
      } catch (e) {}
    }
  }
  function SettingsView(view, params) {
    BaseView.apply(this, arguments);
    view.querySelector("form").addEventListener("submit", onSubmit);
  }
  Object.assign(SettingsView.prototype, BaseView.prototype);
  SettingsView.prototype.onResume = function (options) {
    BaseView.prototype.onResume.apply(this, arguments);
    loading.hide();
    if (options.refresh) {
      renderSettings(this.view);
    }
  };
  SettingsView.prototype.onPause = function () {
    saveSettings(this.view);
    BaseView.prototype.onPause.apply(this, arguments);
  };
  return SettingsView;
});
