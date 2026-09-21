define(
  ["modules/htmlvideoplayer/basehtmlplayer.js", "modules/htmlvideoplayer/plugin.js"],
  function (baseMod, playerMod) {
    // Forward-seek fix for WebKitGTK/GStreamer.
    //
    // Root cause (measured on WebKitGTK 2.52.6 / GStreamer 1.28.2): reading
    // HTMLMediaElement.seekable while the demuxer is still scanning for the
    // container index permanently aborts that scan. The element then stays
    // in "streaming" mode for the rest of playback: video.duration tracks
    // the download buffer (seconds on an hour-long file) and video.seekable
    // collapses to that buffer. Backward seeks still land (the target is
    // inside the buffer); forward seeks clamp to the fake end, so
    // fast-forward and chapter jumps look broken. Reloading the src does NOT
    // recover the index afterwards.
    //
    // The client touches .seekable on the very first 'waiting'/'loadstart'
    // events - long before loadedmetadata reports the real duration - via
    // BaseHtmlPlayer.prototype.seekable() and getSeekableRanges() (the
    // playback-progress report path). That early read is the poison.
    //
    // Fix: while the element has not yet reported a finite positive duration
    // (index still loading), these two accessors return an empty result
    // WITHOUT touching mediaElement.seekable. Once the index has loaded,
    // reads pass straight through, so normal seekability checks are
    // unaffected. Verified in a standalone test page: guarding the read
    // keeps duration at the full file length and forward seeks work.
    var base = baseMod && baseMod.default ? baseMod.default : baseMod;
    var player = playerMod && playerMod.default ? playerMod.default : playerMod;

    // True once the element's duration looks like a real container index
    // rather than the download buffer. duration is NaN until loadedmetadata;
    // in a healthy load it then equals the full file length.
    function indexed(elem) {
      if (!elem) return false;
      var d = elem.duration;
      return isFinite(d) && d > 0;
    }

    function install(proto) {
      if (!proto || proto._seekfixInstalled) return;
      proto._seekfixInstalled = true;

      // BaseHtmlPlayer.prototype.seekable() -> boolean (is there a seek range)
      if (typeof proto.seekable === "function") {
        var origSeekable = proto.seekable;
        proto.seekable = function () {
          if (!indexed(this._mediaElement)) return false;
          return origSeekable.apply(this, arguments);
        };
      }

      // BaseHtmlPlayer.prototype.getSeekableRanges() -> [{start,end}] ticks
      if (typeof proto.getSeekableRanges === "function") {
        var origRanges = proto.getSeekableRanges;
        proto.getSeekableRanges = function () {
          if (!indexed(this._mediaElement)) return [];
          return origRanges.apply(this, arguments);
        };
      }
    }

    // The concrete player copies BaseHtmlPlayer.prototype onto its own at
    // load time, so patch both prototypes.
    install(base && base.prototype);
    install(player && player.prototype);

    return function () {
      this.id = "seekfix";
      this.name = "Seek Clamp Fix";
      this.type = "seekfix";
    };
  },
);
