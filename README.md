# rmpyou

Turn an **MP3** and a **cover image** into a video ready to upload to YouTube.

- Live 16:9 preview; the image is letterboxed, never cropped
- 720p / 1080p / 4K output: H.264 High, yuv420p, AAC 320k 48 kHz, faststart MP4 (YouTube's recommended settings)
- 4 themes (Midnight, Carbon, Daylight, Sakura), remembered between runs
- Updates itself from GitHub Releases
- Pure CPU: Slint software renderer, no GPU needed

## Download

Grab `rmpyou-x86_64-pc-windows-msvc.zip` from [Releases](../../releases/latest), unzip, and run `rmpyou.exe`.
The zip includes `ffmpeg.exe` and `ffprobe.exe`.

## Build from source

```sh
cargo run --release
```

This needs `ffmpeg` and `ffprobe` on your `PATH`. Local builds don't auto-update.
To test the updater locally, build with `GITHUB_REPOSITORY=<owner>/<repo>` set.

## Releasing

1. Bump `version` in `Cargo.toml` and commit.
2. `git tag v0.2.0 && git push origin main --tags`.

The tag must match the Cargo version. The `Release` workflow builds the Windows zip and publishes it.

## License

MIT. Release zips bundle an [FFmpeg](https://ffmpeg.org) build from [gyan.dev](https://www.gyan.dev/ffmpeg/builds/), licensed under the GPL (see `FFMPEG-LICENSE.txt`). Its source is available from ffmpeg.org.
