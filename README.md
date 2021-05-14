# GStreamer HTTP Live Streaming Plugin
A highly configurable GStreamer HLS sink plugin. Based on the [`hlssink2`](https://gstreamer.freedesktop.org/documentation/hls/hlssink2.html?gi-language=c) element. The `flexhlssink` is written in Rust and has various options to configure the HLS output playlist generation.

## Development status

The plugin is in **active development**. The first release objective is to have full feature parity with the `hlssink2` plugin.

Progress:
- [x] Support all properties exposed by the `hlssink2` plugin;
- [x] Write TS segments to segment stream (defaults to filesystem);
- [ ] Write HLS playlist m3u8 file;
- [ ] Signal to acquire segment stream;
- [ ] Signal to acquire HLS playlist stream;
