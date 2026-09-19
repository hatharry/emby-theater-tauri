define(
  [
    "modules/htmlvideoplayer/basehtmlplayer.js",
    "modules/htmlvideoplayer/plugin.js",
  ],
  function (baseMod, playerMod) {
    // Static device profile for the desktop (amd64) native app. The
    // DirectPlayProfiles below were derived EMPIRICALLY: every container x
    // codec combination the client's own browserdeviceprofile builder can
    // emit was generated with ffmpeg and played through this exact media
    // engine (WebKitGTK 2.52 + GStreamer, the same stack the Tauri binary
    // embeds) at both 320x240 and 1280x720. A combination is listed as
    // direct-playable only if <video> reached 'ended' at >= ~1x real time
    // with no MediaError. canPlayType was NOT used (it returns "maybe" for
    // formats GStreamer cannot actually decode).
    //
    // Findings that shaped this profile:
    // - The desktop decodes H.264/HEVC/AV1/VP8/VP9/MPEG-4/MPEG-2 in software
    //   at real time, so all of them direct-play (unlike the Pi, which has no
    //   HEVC/AV1 block and must transcode those).
    // - Native HLS (m3u8/ts) FAILS (MediaError 4) in this webview, so the
    //   first video TranscodingProfile is a progressive Matroska stream, and
    //   no HLS video profile is offered.
    // - AC3/E-AC3 direct-play only in Matroska (ffmpeg cannot mux them into
    //   MP4); in MP4 they need the transcode path.
    // - One entry per container: each lists the UNION of the video and audio
    //   codecs that passed for that container. HEVC and AV1 direct-play here
    //   (software decode) — unlike the Pi profile, which omits them so the
    //   server transcodes to H.264.
    var PROFILE = {
  MaxStaticBitrate: 200000000,
  MaxStreamingBitrate: 200000000,
  MusicStreamingTranscodingBitrate: 192000,
  DirectPlayProfiles: [
    {
      Container: "mp4,m4v",
      Type: "Video",
      VideoCodec: "h264,hevc,mpeg2video,mpeg4",
      AudioCodec: "aac,ac3,alac,flac,mp3"
    },
    {
      Container: "mkv",
      Type: "Video",
      VideoCodec: "h264,hevc,av1,vp8,vp9,mpeg2video,mpeg4",
      AudioCodec: "aac,ac3,eac3,flac,mp3,opus,pcm_s16le,vorbis"
    },
    {
      Container: "webm",
      Type: "Video",
      VideoCodec: "vp8,vp9,av1",
      AudioCodec: "opus,vorbis"
    },
    {
      Container: "ts",
      Type: "Video",
      VideoCodec: "h264,hevc,mpeg2video",
      AudioCodec: "aac,ac3,mp3"
    },
    {
      Container: "mov",
      Type: "Video",
      VideoCodec: "h264,hevc,mpeg4",
      AudioCodec: "aac,alac,pcm_s16le"
    },
    {
      Container: "flv",
      Type: "Video",
      VideoCodec: "h264",
      AudioCodec: "aac,mp3"
    },
    {
      Container: "avi",
      Type: "Video",
      VideoCodec: "h264,mpeg2video,mpeg4",
      AudioCodec: "mp3,pcm_s16le"
    },
    {
      Container: "3gp",
      Type: "Video",
      VideoCodec: "h264,mpeg4",
      AudioCodec: "aac"
    },
    {
      Container: "mp3",
      Type: "Audio",
      AudioCodec: "mp3"
    },
    {
      Container: "aac",
      Type: "Audio",
      AudioCodec: "aac"
    },
    {
      Container: "m4a",
      Type: "Audio",
      AudioCodec: "aac,alac"
    },
    {
      Container: "opus",
      Type: "Audio",
      AudioCodec: "opus"
    },
    {
      Container: "webma,webm",
      Type: "Audio",
      AudioCodec: "opus,vorbis"
    },
    {
      Container: "ogg",
      Type: "Audio",
      AudioCodec: "vorbis"
    },
    {
      Container: "flac",
      Type: "Audio",
      AudioCodec: "flac"
    },
    {
      Container: "wav",
      Type: "Audio",
      AudioCodec: "pcm_s16le,pcm_s24le"
    },
    {
      Container: "mp4",
      Type: "Audio",
      AudioCodec: "aac,alac"
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
      Container: "mkv",
      Type: "Video",
      AudioCodec: "ac3,eac3,mp3,aac,opus,flac,vorbis",
      VideoCodec: "h264,vp8,vp9,av1,hevc",
      Context: "Static",
      MaxAudioChannels: "2",
      CopyTimestamps: true
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
      Conditions: [
        {
          Condition: "EqualsAny",
          Property: "VideoProfile",
          Value: "main|main 10|main still picture",
          IsRequired: false
        }
      ]
    },
    {
      Type: "VideoAudio",
      Codec: "ac3",
      Conditions: [
        {
          Condition: "LessThanEqual",
          Property: "AudioChannels",
          Value: "6",
          IsRequired: false
        }
      ]
    },
    {
      Type: "VideoAudio",
      Codec: "eac3",
      Conditions: [
        {
          Condition: "LessThanEqual",
          Property: "AudioChannels",
          Value: "16",
          IsRequired: false
        }
      ]
    }
  ],
  SubtitleProfiles: [
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
    },
    {
      Format: "srt",
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
      this.id = "defaultdeviceprofile";
      this.name = "Desktop Device Profile";
      this.type = "defaultdeviceprofile";
    };
  }
);
