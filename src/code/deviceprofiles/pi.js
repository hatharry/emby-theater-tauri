define(
  [
    "modules/htmlvideoplayer/basehtmlplayer.js",
    "modules/htmlvideoplayer/plugin.js",
  ],
  function (baseMod, playerMod) {
    // Static device profile for the Raspberry Pi. Derived empirically on the
    // Pi 4 itself: every container x codec combination was generated with
    // ffmpeg and played through this exact WebKitGTK build (720p full matrix +
    // 1080p video pass), keeping only combinations that PRESENTED >= 90% of
    // their frames at real time with no MediaError. Excluded because they drop
    // frames on the Pi 4 (measured at 1080p):
    //   hevc        ~35% frames presented (no hw block; avdec_h265 can't keep up)
    //   av1          ~2% (no hw block)
    //   mpeg2video  ~47% (software decode too slow at 1080p)
    //   alac        MediaError 4 (no ALAC decoder in this GStreamer build)
    // Direct-play: h264 (vc4 hw), vp8/vp9/mpeg4 (verified at 1080p/720p).
    // The first video TranscodingProfile is a progressive Matroska stream,
    // which is what the server emits and GStreamer plays in hardware via a
    // plain video.src (HLS is unplayable in this webview).
    //
    // This replaces the builder wholesale, so no canPlayType probing and no
    // Emby.importModule wrapping is needed. The server must have transcode
    // THROTTLING enabled (Dashboard -> Playback -> Transcoding): unthrottled,
    // ffmpeg writes the whole film to a temp file at ~16x realtime and the
    // webview buffers it into swap until the kernel OOM-kills the web process.
    var PROFILE = {
  MaxStaticBitrate: 200000000,
  MaxStreamingBitrate: 200000000,
  MusicStreamingTranscodingBitrate: 192000,
  DirectPlayProfiles: [
    {
      Container: "mp4,m4v",
      Type: "Video",
      VideoCodec: "h264,vp8,vp9,mpeg4",
      AudioCodec: "ac3,eac3,mp3,aac,opus,flac,vorbis,dts"
    },
    {
      Container: "mkv",
      Type: "Video",
      VideoCodec: "h264,vp8,vp9,mpeg4",
      AudioCodec: "ac3,eac3,mp3,aac,opus,flac,vorbis,dts,pcm_s16le"
    },
    {
      Container: "webm",
      Type: "Video",
      VideoCodec: "vp8,vp9",
      AudioCodec: "opus,vorbis"
    },
    {
      Container: "ts",
      Type: "Video",
      VideoCodec: "h264",
      AudioCodec: "ac3,mp3,aac,dts"
    },
    {
      Container: "mov",
      Type: "Video",
      VideoCodec: "h264,mpeg4",
      AudioCodec: "ac3,eac3,mp3,aac,opus,flac,vorbis,dts,pcm_s16le"
    },
    {
      Container: "flv",
      Type: "Video",
      VideoCodec: "h264",
      AudioCodec: "aac,mp3"
    },
    {
      Container: "3gp",
      Type: "Video",
      VideoCodec: "h264,mpeg4",
      AudioCodec: "ac3,eac3,mp3,aac,opus,flac,vorbis"
    },
    {
      Container: "opus",
      Type: "Audio"
    },
    {
      Container: "mp3",
      Type: "Audio",
      AudioCodec: "mp3"
    },
    {
      Container: "mp2,mp3",
      Type: "Audio",
      AudioCodec: "mp2"
    },
    {
      Container: "aac",
      Type: "Audio",
      AudioCodec: "aac"
    },
    {
      Container: "m4a",
      AudioCodec: "aac",
      Type: "Audio"
    },
    {
      Container: "mp4",
      AudioCodec: "aac",
      Type: "Audio"
    },
    {
      Container: "flac",
      Type: "Audio"
    },
    {
      Container: "webma,webm",
      Type: "Audio"
    },
    {
      Container: "wav",
      Type: "Audio",
      AudioCodec: "PCM_S16LE,PCM_S24LE"
    },
    {
      Container: "ogg",
      Type: "Audio"
    },
    {
      Container: "dts",
      Type: "Audio",
      AudioCodec: "dts"
    }
  ],
  TranscodingProfiles: [
    {
      Container: "mkv",
      Type: "Video",
      VideoCodec: "h264",
      AudioCodec: "ac3,eac3,mp3,aac,opus,flac,vorbis,dts",
      Context: "Streaming",
      Protocol: "http",
      MaxAudioChannels: "6"
    },
    {
      Container: "aac",
      Type: "Audio",
      AudioCodec: "aac",
      Context: "Streaming",
      Protocol: "http",
      MaxAudioChannels: "2"
    },
    {
      Container: "mp3",
      Type: "Audio",
      AudioCodec: "mp3",
      Context: "Streaming",
      Protocol: "http",
      MaxAudioChannels: "2"
    },
    {
      Container: "opus",
      Type: "Audio",
      AudioCodec: "opus",
      Context: "Streaming",
      Protocol: "http",
      MaxAudioChannels: "2"
    },
    {
      Container: "wav",
      Type: "Audio",
      AudioCodec: "wav",
      Context: "Streaming",
      Protocol: "http",
      MaxAudioChannels: "2"
    }
  ],
  ContainerProfiles: [],
  CodecProfiles: [
    {
      Type: "Video",
      Codec: "h264",
      Conditions: [
        {
          Condition: "EqualsAny",
          Property: "VideoProfile",
          Value: "high|main|baseline|constrained baseline|high 10",
          IsRequired: false
        },
        {
          Condition: "LessThanEqual",
          Property: "VideoLevel",
          Value: "62",
          IsRequired: false
        }
      ]
    },
    {
      Type: "Video",
      Codec: "hevc",
      Conditions: []
    }
  ],
  SubtitleProfiles: [
    {
      Format: "vtt",
      Method: "Hls"
    },
    {
      Format: "eia_608",
      Method: "VideoSideData",
      Protocol: "hls"
    },
    {
      Format: "eia_708",
      Method: "VideoSideData",
      Protocol: "hls"
    },
    {
      Format: "vtt",
      Method: "External",
      AllowChunkedResponse: true
    },
    {
      Format: "ass",
      Method: "External",
      AllowChunkedResponse: true
    },
    {
      Format: "ssa",
      Method: "External",
      AllowChunkedResponse: true
    }
  ],
  ResponseProfiles: [
    {
      Type: "Video",
      Container: "m4v",
      MimeType: "video/mp4"
    }
  ]
};

    function fresh() {
      // New copy per call: the client mutates the returned profile (e.g.
      // playbackmanager clears DirectPlayProfiles for live TV).
      return Promise.resolve(JSON.parse(JSON.stringify(PROFILE)));
    }

    // The concrete player (HtmlVideoPlayer) copies BaseHtmlPlayer.prototype
    // onto its own prototype via Object.assign at load time, so patch the
    // player class prototype (what instances resolve through) and the base
    // prototype too, in case the hierarchy differs across client versions.
    var base = baseMod && baseMod.default ? baseMod.default : baseMod;
    var player = playerMod && playerMod.default ? playerMod.default : playerMod;
    if (base && base.prototype) base.prototype.getDeviceProfile = fresh;
    if (player && player.prototype) player.prototype.getDeviceProfile = fresh;

    return function () {
      this.id = "pideviceprofile";
      this.name = "Pi Device Profile";
      this.type = "pideviceprofile";
    };
  }
);
