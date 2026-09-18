define(
  [
    "modules/htmlvideoplayer/basehtmlplayer.js",
    "modules/htmlvideoplayer/plugin.js",
  ],
  function (baseMod, playerMod) {
    // Static device profile for the Raspberry Pi. Captured from the client's own
    // browserdeviceprofile builder running on this exact WebKitGTK build, then
    // adjusted for the Pi's hardware: H.264/VP8/VP9 direct-play (vc4 decodes
    // them), but NO HEVC and NO AV1 (the Pi 4 has no block for either, so the
    // server transcodes them to H.264). The first video TranscodingProfile is a
    // progressive Matroska stream, which is what the server emits and GStreamer
    // plays in hardware via a plain video.src (HLS is unplayable in this webview).
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
      VideoCodec: "h264,vp8,vp9",
      AudioCodec: "ac3,eac3,mp3,aac,opus,flac,vorbis"
    },
    {
      Container: "mkv",
      Type: "Video",
      VideoCodec: "h264,vp8,vp9",
      AudioCodec: "ac3,eac3,mp3,aac,opus,flac,vorbis"
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
      VideoCodec: "",
      AudioCodec: "ac3,eac3,mp3,aac,opus,flac,vorbis"
    },
    {
      Container: "mov",
      Type: "Video",
      VideoCodec: "h264",
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
      Container: "webm",
      Type: "Video",
      AudioCodec: "vorbis,opus",
      VideoCodec: "VP8,VP9"
    }
  ],
  TranscodingProfiles: [
    {
      Container: "mkv",
      Type: "Video",
      VideoCodec: "h264",
      AudioCodec: "ac3,eac3,mp3,aac,opus,flac,vorbis",
      Context: "Streaming",
      Protocol: "http",
      MaxAudioChannels: "6"
    },
    {
      Container: "aac",
      Type: "Audio",
      AudioCodec: "aac",
      Context: "Streaming",
      Protocol: "hls",
      MaxAudioChannels: "2",
      MinSegments: "1",
      BreakOnNonKeyFrames: true
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
    },
    {
      Container: "opus",
      Type: "Audio",
      AudioCodec: "opus",
      Context: "Static",
      Protocol: "http",
      MaxAudioChannels: "2"
    },
    {
      Container: "mp3",
      Type: "Audio",
      AudioCodec: "mp3",
      Context: "Static",
      Protocol: "http",
      MaxAudioChannels: "2"
    },
    {
      Container: "aac",
      Type: "Audio",
      AudioCodec: "aac",
      Context: "Static",
      Protocol: "http",
      MaxAudioChannels: "2"
    },
    {
      Container: "wav",
      Type: "Audio",
      AudioCodec: "wav",
      Context: "Static",
      Protocol: "http",
      MaxAudioChannels: "2"
    },
    {
      Container: "mkv",
      Type: "Video",
      AudioCodec: "ac3,eac3,mp3,aac,opus,flac,vorbis",
      VideoCodec: "h264,vp8,vp9",
      Context: "Static",
      MaxAudioChannels: "2",
      CopyTimestamps: true
    },
    {
      Container: "ts",
      Type: "Video",
      AudioCodec: "ac3,mp3,aac",
      VideoCodec: "h264",
      Context: "Streaming",
      Protocol: "hls",
      MaxAudioChannels: "2",
      MinSegments: "1",
      BreakOnNonKeyFrames: true,
      ManifestSubtitles: "vtt"
    },
    {
      Container: "webm",
      Type: "Video",
      AudioCodec: "vorbis",
      VideoCodec: "vpx",
      Context: "Streaming",
      Protocol: "http",
      MaxAudioChannels: "2"
    },
    {
      Container: "mp4",
      Type: "Video",
      AudioCodec: "ac3,eac3,mp3,aac,opus,flac,vorbis",
      VideoCodec: "h264",
      Context: "Static",
      Protocol: "http"
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
