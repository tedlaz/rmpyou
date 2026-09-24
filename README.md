# rmpyou

Turn an **MP3** and a **cover image** into a video ready to upload to YouTube.

- Drag and drop an MP3 or image anywhere on the window
- MP3 cover art (ID3) is used as the image automatically
- Background fill around the image: blurred artwork, the image's average color, black, white, any hex color, or a color picked from [Color Hunt](https://colorhunt.co) palettes (popular, new, random)
- Live 16:9 preview; the image is letterboxed, never cropped
- 720p / 1080p / 4K output: H.264 still image + your original MP3 audio copied bit-for-bit (no re-encode, no quality loss). The video ends up only slightly bigger than the MP3 (~1.1×)
- Settings drawer: 4 themes (Midnight, Carbon, Daylight, Sakura), update controls, FFmpeg manager, credits
- Updates itself from GitHub Releases
- Custom frameless title bar
- Pure CPU: Slint software renderer, no GPU needed
- Small: ~9 MB exe (~4.8 MB zipped). Uses Windows' own TLS and `tar.exe` (Windows 10 1803+) instead of bundling them

## Download

Grab `rmpyou-x86_64-pc-windows-msvc.zip` from [Releases](../../releases/latest), unzip, and run `rmpyou.exe`.

On first run, rmpyou offers to download FFmpeg once (≈ 83 MB, from [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds)) into `%LOCALAPPDATA%\rmpyou\ffmpeg`. It's kept outside the app, so app updates stay small.

rmpyou looks for FFmpeg in this order:
1. next to `rmpyou.exe` (portable setups)
2. its own managed copy in `%LOCALAPPDATA%\rmpyou\ffmpeg`
3. your `PATH`

## Build from source

```sh
cargo run --release
```

The UI lives in `ui/app.slint`. Local builds don't auto-update.
To test the updater locally, build with `GITHUB_REPOSITORY=<owner>/<repo>` set.

## Releasing

1. Bump `version` in `Cargo.toml` and commit.
2. `git tag v0.2.0 && git push origin main --tags`.

The tag must match the Cargo version. The `Release` workflow builds the Windows zip and publishes it.

## License

MIT © Ted Lazaros. FFmpeg, which rmpyou downloads separately, is licensed under the GPL.
