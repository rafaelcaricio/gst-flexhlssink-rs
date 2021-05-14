# GStreamer HTTP Live Streaming Plugin
A highly configurable GStreamer HLS sink plugin. Based on the [`hlssink2`](https://gstreamer.freedesktop.org/documentation/hls/hlssink2.html?gi-language=c) element. The `flexhlssink` is written in Rust and has various options to configure the HLS output playlist generation.

## Development status

The plugin is in **active development**. The first release objective is to have full feature parity with the `hlssink2` plugin.

Progress:
- [x] Support all properties exposed by the `hlssink2` plugin;
- [x] Write TS segments to segment stream (defaults to filesystem);
- [x] Write HLS playlist m3u8 file;
- [ ] Signal to acquire segment stream;
- [ ] Signal to acquire HLS playlist stream;
- [ ] Delete old segments;

## Example Usage

After [installing GStreamer](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs#installation)
, it is possible to compile and run the `flexhlsplugin`.

```bash
cargo build --release
```

On MacOS it might be necessary to set the `PKG_CONFIG_PATH` environment variable:
```bash
export PKG_CONFIG_PATH="/Library/Frameworks/GStreamer.framework/Versions/Current/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
```

An example pipeline:
```bash
export PROJECT_DIR=`pwd`
gst-launch-1.0 videotestsrc is-live=true ! \
    x264enc ! h264parse ! flexhlssink target-duration=4 \
    --gst-plugin-load=${PROJECT_DIR}/target/release/libflexhlssink.dylib
```

In another terminal run a simple HTTP server:
```bash
cd $PROJECT_DIR
python simple_http.py
```

Open the example player site https://hls-js.netlify.app/demo/ and play the `http://localhost:8000/playlist.m3u8` playback URL.
