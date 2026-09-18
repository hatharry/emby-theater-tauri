define(["modules/common/inputmanager.js"], function (mod) {
  var im = mod && mod.default ? mod.default : mod;
  return function () {
    this.id = "cecinput";
    this.name = "cec";
    this.type = "input";
    this.getRoutes = function () {
      return [
        {
          path: "cec/cec.html",
          transition: "slide",
          controller: "embyhost://host/cec/cec.js",
          type: "settings",
          title: "HDMI-CEC",
          category: "Playback",
          thumbImage: "",
          icon: "tv",
          settingsTheme: true,
          adjustHeaderForEmbeddedScroll: true,
        },
      ];
    };
    // Apply the saved HDMI port and CEC adapter (if any) at startup.
    try {
      var p = localStorage.getItem("cec-hdmiport") || "";
      window.__TAURI_INTERNALS__.invoke("cec_set_hdmi_port", { port: p });
      var d = localStorage.getItem("cec-device") || "";
      window.__TAURI_INTERNALS__.invoke("cec_set_device", { device: d });
    } catch (e) {}
    var failures = 0;
    function poll() {
      try {
        window.__TAURI_INTERNALS__.invoke("cec_poll").then(
          function (keys) {
            failures = 0;
            for (var i = 0; i < keys.length; i++) {
              try {
                im.trigger(keys[i]);
              } catch (e) {}
            }
            setTimeout(poll, 100);
          },
          function () {
            if (++failures < 50) setTimeout(poll, 1000);
          }
        );
      } catch (e) {
        if (++failures < 50) setTimeout(poll, 1000);
      }
    }
    poll();
  };
});
