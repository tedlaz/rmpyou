#![windows_subsystem = "windows"]

use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

use slint::ComponentHandle;

slint::slint! {
    struct Palette {
        bg1: color, bg2: color, card: color, border: color, field: color,
        text: color, muted: color, accent: color, accent2: color,
    }

    export global Theme {
        in-out property <int> index: 0;
        out property <[string]> names: ["Midnight", "Carbon", "Daylight", "Sakura"];
        out property <[Palette]> palettes: [
            { bg1: #0f0c29, bg2: #302b63, card: #ffffff12, border: #ffffff24, field: #00000038,
              text: #f5f3ff, muted: #a5a1c9, accent: #8b5cf6, accent2: #ec4899 },
            { bg1: #0b0f14, bg2: #1c2530, card: #ffffff0f, border: #ffffff1f, field: #00000045,
              text: #e6edf3, muted: #8b98a5, accent: #14b8a6, accent2: #22d3ee },
            { bg1: #f5f7ff, bg2: #dbe4ff, card: #ffffffd0, border: #1d4ed826, field: #eef2ff,
              text: #0f172a, muted: #64748b, accent: #2f63eb, accent2: #7c3aed },
            { bg1: #fff5f7, bg2: #ffdde8, card: #ffffffd0, border: #be185d26, field: #fff1f5,
              text: #3b0a1f, muted: #9d5c77, accent: #e11d48, accent2: #f97316 },
        ];
        out property <Palette> p: palettes[index];
    }

    component Label inherits Text {
        color: Theme.p.muted;
        font-size: 11px;
        font-weight: 700;
        letter-spacing: 1.4px;
    }

    component Card inherits Rectangle {
        background: Theme.p.card;
        border-radius: 20px;
        border-width: 1px;
        border-color: Theme.p.border;
        drop-shadow-blur: 30px;
        drop-shadow-offset-y: 10px;
        drop-shadow-color: #00000030;
        animate background, border-color { duration: 250ms; }
    }

    component Button inherits Rectangle {
        in property <string> text;
        in property <bool> primary;
        in property <bool> enabled: true;
        callback clicked;
        min-height: primary ? 54px : 38px;
        border-radius: primary ? 14px : 10px;
        background: primary
            ? Theme.p.accent
            : (ta.has-hover && enabled ? Theme.p.border : Theme.p.field);
        border-width: primary ? 0px : 1px;
        border-color: Theme.p.border;
        opacity: !enabled ? 0.4 : ta.pressed ? 0.8 : 1;
        drop-shadow-blur: primary && enabled ? (ta.has-hover ? 28px : 18px) : 0px;
        drop-shadow-color: Theme.p.accent2.transparentize(35%);
        animate drop-shadow-blur { duration: 200ms; }
        animate opacity { duration: 150ms; }
        accessible-role: button;
        accessible-label: text;
        accessible-action-default => { if enabled { clicked(); } }
        ta := TouchArea {
            enabled: root.enabled;
            mouse-cursor: root.enabled ? pointer : default;
            clicked => { root.clicked(); }
        }
        HorizontalLayout {
            padding-left: 18px;
            padding-right: 18px;
            Text {
                text: root.text;
                color: root.primary ? white : Theme.p.text;
                font-size: root.primary ? 17px : 13px;
                font-weight: root.primary ? 700 : 600;
                horizontal-alignment: center;
                vertical-alignment: center;
            }
        }
    }

    component Field inherits Rectangle {
        in property <string> title;
        in property <string> subtitle;
        in property <string> action;
        callback clicked;
        min-height: 62px;
        border-radius: 12px;
        background: ta.has-hover ? Theme.p.border : Theme.p.field;
        border-width: 1px;
        border-color: ta.has-hover ? Theme.p.accent : Theme.p.border;
        animate border-color { duration: 150ms; }
        accessible-role: button;
        accessible-label: action + " " + title;
        accessible-action-default => { clicked(); }
        ta := TouchArea { mouse-cursor: pointer; clicked => { root.clicked(); } }
        HorizontalLayout {
            padding: 14px;
            spacing: 12px;
            VerticalLayout {
                alignment: center;
                horizontal-stretch: 1;
                spacing: 3px;
                Text { text: root.title; color: Theme.p.text; font-size: 14px; font-weight: 600; overflow: elide; }
                if root.subtitle != "": Text { text: root.subtitle; color: Theme.p.muted; font-size: 12px; overflow: elide; }
            }
            Text {
                text: root.action;
                color: Theme.p.accent;
                font-size: 13px;
                font-weight: 700;
                vertical-alignment: center;
                horizontal-stretch: 0;
            }
        }
    }

    export component AppWindow inherits Window {
        title: "rmpyou — MP3 to YouTube video";
        preferred-width: 1000px;
        preferred-height: 660px;
        min-width: 820px;
        min-height: 580px;
        background: @linear-gradient(135deg, Theme.p.bg1 0%, Theme.p.bg2 100%);

        in property <image> cover;
        in property <bool> has-cover;
        in-out property <string> image-path;
        in-out property <string> audio-path;
        in property <string> audio-name;
        in property <string> audio-info;
        in-out property <float> duration;
        in-out property <int> resolution: 1;
        in-out property <string> out-path;
        in property <string> out-name;
        in property <string> out-dir;
        in property <float> progress;
        in property <bool> rendering;
        in property <bool> done;
        in property <string> status;
        in property <string> update-version;
        in property <string> update-state; // "", available, downloading, ready, failed

        callback pick-image();
        callback pick-audio();
        callback pick-output();
        callback render();
        callback open-folder();
        callback do-update();
        callback restart();
        callback theme-changed(int);

        property <[string]> res-labels: ["720p", "1080p", "4K"];
        property <bool> can-render: has-cover && audio-path != "" && out-path != "" && !rendering;

        VerticalLayout {
            padding: 28px;
            spacing: 22px;

            // Header
            HorizontalLayout {
                spacing: 14px;
                Rectangle {
                    width: 46px;
                    height: 46px;
                    border-radius: 13px;
                    background: Theme.p.accent;
                    drop-shadow-blur: 18px;
                    drop-shadow-color: Theme.p.accent2.transparentize(40%);
                    Path {
                        width: 16px;
                        height: 18px;
                        x: 17px;
                        fill: white;
                        commands: "M 0 0 L 16 9 L 0 18 Z";
                    }
                }
                VerticalLayout {
                    alignment: center;
                    Text { text: "rmpyou"; color: Theme.p.text; font-size: 22px; font-weight: 800; }
                    Text { text: "MP3 + artwork → YouTube-ready video"; color: Theme.p.muted; font-size: 13px; }
                }
                Rectangle { horizontal-stretch: 1; }

                if root.update-state != "": Rectangle {
                    border-radius: 12px;
                    background: Theme.p.field;
                    border-width: 1px;
                    border-color: Theme.p.accent;
                    HorizontalLayout {
                        padding: 5px;
                        padding-left: 14px;
                        spacing: 10px;
                        Text {
                            vertical-alignment: center;
                            color: Theme.p.text;
                            font-size: 13px;
                            text: root.update-state == "available" ? "v" + root.update-version + " is available"
                                : root.update-state == "downloading" ? "Downloading update…"
                                : root.update-state == "ready" ? "Update installed"
                                : "Update failed";
                        }
                        if root.update-state == "available" || root.update-state == "ready": Button {
                            text: root.update-state == "available" ? "Update" : "Restart";
                            clicked => { if root.update-state == "available" { root.do-update(); } else { root.restart(); } }
                        }
                    }
                }

                VerticalLayout {
                    alignment: center;
                    spacing: 6px;
                    Label { text: "THEME · " + Theme.names[Theme.index]; horizontal-alignment: right; }
                    HorizontalLayout {
                        spacing: 8px;
                        alignment: end;
                        for name[i] in Theme.names: Rectangle {
                            width: 26px;
                            height: 26px;
                            border-radius: 13px;
                            background: Theme.palettes[i].accent;
                            border-width: Theme.index == i ? 3px : 0px;
                            border-color: Theme.p.text;
                            accessible-role: button;
                            accessible-label: name + " theme";
                            accessible-action-default => { Theme.index = i; root.theme-changed(i); }
                            Rectangle {
                                width: 10px;
                                height: 10px;
                                border-radius: 5px;
                                background: Theme.palettes[i].bg1;
                            }
                            TouchArea {
                                mouse-cursor: pointer;
                                clicked => { Theme.index = i; root.theme-changed(i); }
                            }
                        }
                    }
                }
            }

            HorizontalLayout {
                spacing: 22px;

                // Artwork / live 16:9 preview
                Card {
                    horizontal-stretch: 3;
                    preferred-width: 1px;
                    VerticalLayout {
                        padding: 22px;
                        spacing: 14px;
                        HorizontalLayout {
                            spacing: 12px;
                            Label { text: "ARTWORK"; }
                            Label {
                                text: root.has-cover ? "PREVIEW · " + root.res-labels[root.resolution] : "";
                                horizontal-alignment: right;
                            }
                        }
                        Rectangle {
                            vertical-stretch: 1;
                            min-height: 200px;
                            frame := Rectangle {
                                width: min(parent.width, parent.height * 16 / 9);
                                height: self.width * 9 / 16;
                                x: (parent.width - self.width) / 2;
                                y: (parent.height - self.height) / 2;
                                border-radius: 14px;
                                clip: true;
                                background: root.has-cover ? black : Theme.p.field;
                                border-width: root.has-cover ? 0px : 2px;
                                border-color: cover-ta.has-hover ? Theme.p.accent : Theme.p.border;
                                animate border-color { duration: 150ms; }
                                if root.has-cover: Image {
                                    width: parent.width;
                                    height: parent.height;
                                    source: root.cover;
                                    image-fit: contain;
                                }
                                if !root.has-cover: VerticalLayout {
                                    alignment: center;
                                    spacing: 8px;
                                    Text { text: "+"; color: Theme.p.accent; font-size: 44px; font-weight: 300; horizontal-alignment: center; }
                                    Text { text: "Choose cover image"; color: Theme.p.text; font-size: 17px; font-weight: 600; horizontal-alignment: center; }
                                    Text { text: "PNG · JPG · WEBP · BMP — letterboxed to 16:9"; color: Theme.p.muted; font-size: 12px; horizontal-alignment: center; }
                                }
                                cover-ta := TouchArea {
                                    mouse-cursor: pointer;
                                    clicked => { root.pick-image(); }
                                }
                            }
                        }
                    }
                }

                // Settings + render
                Card {
                    horizontal-stretch: 2;
                    preferred-width: 1px;
                    min-width: 330px;
                    VerticalLayout {
                        padding: 22px;
                        spacing: 10px;

                        Label { text: "AUDIO"; }
                        Field {
                            title: root.audio-name == "" ? "Choose an MP3 file" : root.audio-name;
                            subtitle: root.audio-info;
                            action: root.audio-name == "" ? "Browse" : "Change";
                            clicked => { root.pick-audio(); }
                        }

                        Rectangle { height: 6px; }
                        Label { text: "RESOLUTION"; }
                        HorizontalLayout {
                            spacing: 8px;
                            for label[i] in root.res-labels: Rectangle {
                                height: 40px;
                                border-radius: 10px;
                                background: root.resolution == i
                                    ? Theme.p.accent
                                    : (seg-ta.has-hover ? Theme.p.border : Theme.p.field);
                                border-width: root.resolution == i ? 0px : 1px;
                                border-color: Theme.p.border;
                                accessible-role: button;
                                accessible-label: label;
                                accessible-action-default => { root.resolution = i; }
                                seg-ta := TouchArea { mouse-cursor: pointer; clicked => { root.resolution = i; } }
                                Text {
                                    text: label;
                                    color: root.resolution == i ? white : Theme.p.text;
                                    font-size: 13px;
                                    font-weight: 700;
                                }
                            }
                        }

                        Rectangle { height: 6px; }
                        Label { text: "SAVE TO"; }
                        Field {
                            title: root.out-name == "" ? "Pick where to save" : root.out-name;
                            subtitle: root.out-dir;
                            action: root.out-name == "" ? "Browse" : "Change";
                            clicked => { root.pick-output(); }
                        }

                        Rectangle { vertical-stretch: 1; }

                        if root.status != "": Text {
                            text: root.status;
                            color: Theme.p.muted;
                            font-size: 13px;
                            wrap: word-wrap;
                        }
                        if root.rendering || root.done: Rectangle {
                            height: 8px;
                            border-radius: 4px;
                            background: Theme.p.field;
                            Rectangle {
                                x: 0;
                                width: parent.width * root.progress;
                                border-radius: 4px;
                                background: Theme.p.accent;
                                animate width { duration: 300ms; easing: ease-out; }
                            }
                        }
                        Button {
                            primary: true;
                            enabled: root.can-render;
                            text: root.rendering ? "Rendering…  " + Math.round(root.progress * 100) + "%" : "Render video";
                            clicked => { root.render(); }
                        }
                        if root.done: Button {
                            text: "Show in folder";
                            clicked => { root.open-folder(); }
                        }
                    }
                }
            }
        }
    }
}

const RESOLUTIONS: [(u32, u32); 3] = [(1280, 720), (1920, 1080), (3840, 2160)];

/// ffmpeg/ffprobe next to our exe (release zip bundles them), else from PATH.
fn tool(name: &str) -> Command {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))))
        .filter(|p| p.exists());
    let mut cmd = Command::new(bundled.unwrap_or_else(|| name.into()));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

fn probe_duration(audio: &Path) -> Option<f64> {
    let out = tool("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(audio)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn fmt_time(secs: f64) -> String {
    let s = secs.round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Encode still image + audio into a YouTube-recommended MP4 (H.264 High yuv420p, AAC 48 kHz, faststart).
fn render(
    image: &Path,
    audio: &Path,
    out: &Path,
    (w, h): (u32, u32),
    duration: f64,
    on_progress: impl Fn(f32),
) -> Result<(), String> {
    let vf = format!(
        "scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:black,setsar=1,format=yuv420p"
    );
    let mut cmd = tool("ffmpeg");
    cmd.args(["-y", "-hide_banner", "-loglevel", "error", "-loop", "1", "-framerate", "1", "-i"])
        .arg(image)
        .arg("-i")
        .arg(audio)
        .args(["-map", "0:v", "-map", "1:a", "-vf", &vf, "-r", "30"])
        .args(["-c:v", "libx264", "-tune", "stillimage", "-preset", "medium", "-crf", "18", "-profile:v", "high"])
        .args(["-c:a", "aac", "-b:a", "320k", "-ar", "48000", "-movflags", "+faststart"]);
    if duration > 0.0 {
        cmd.args(["-t", &format!("{duration:.3}")]);
    } else {
        cmd.arg("-shortest");
    }
    let mut child = cmd
        .args(["-progress", "pipe:1", "-nostats"])
        .arg(out)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start ffmpeg ({e}). Is it installed?"))?;

    let mut stderr = child.stderr.take().unwrap();
    let errors = thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.parse::<f64>().ok())
            && duration > 0.0
        {
            on_progress((us / 1e6 / duration).clamp(0.0, 1.0) as f32);
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let errors = errors.join().unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        Err(errors.lines().last().unwrap_or("ffmpeg failed").to_string())
    }
}

fn theme_file() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("rmpyou").join("theme"))
}

/// Only enabled in CI builds, where GitHub sets GITHUB_REPOSITORY=owner/repo.
fn updater() -> Option<self_update::backends::github::Update> {
    let (owner, repo) = option_env!("GITHUB_REPOSITORY")?.split_once('/')?;
    self_update::backends::github::Update::configure()
        .repo_owner(owner)
        .repo_name(repo)
        .bin_name("rmpyou")
        .current_version(self_update::cargo_crate_version!())
        .no_confirm(true)
        .show_output(false)
        .show_download_progress(false)
        .build()
        .ok()
}

fn set_output(ui: &AppWindow, path: &Path) {
    ui.set_out_path(path.to_string_lossy().as_ref().into());
    ui.set_out_name(path.file_name().unwrap_or_default().to_string_lossy().as_ref().into());
    ui.set_out_dir(path.parent().unwrap_or(Path::new("")).to_string_lossy().as_ref().into());
}

fn main() -> Result<(), slint::PlatformError> {
    let app = AppWindow::new()?;

    if let Some(i) = theme_file()
        .and_then(|f| fs::read_to_string(f).ok())
        .and_then(|s| s.trim().parse::<i32>().ok())
        .filter(|i| (0..4).contains(i))
    {
        app.global::<Theme>().set_index(i);
    }
    app.on_theme_changed(|i| {
        if let Some(f) = theme_file() {
            let _ = fs::create_dir_all(f.parent().unwrap());
            let _ = fs::write(f, i.to_string());
        }
    });

    let weak = app.as_weak();
    app.on_pick_image(move || {
        let ui = weak.unwrap();
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"])
            .pick_file()
        else {
            return;
        };
        match slint::Image::load_from_path(&path) {
            Ok(img) => {
                ui.set_cover(img);
                ui.set_has_cover(true);
                ui.set_image_path(path.to_string_lossy().as_ref().into());
                ui.set_done(false);
                ui.set_status("".into());
            }
            Err(_) => ui.set_status("Couldn't read that image.".into()),
        }
    });

    let weak = app.as_weak();
    app.on_pick_audio(move || {
        let ui = weak.unwrap();
        let Some(path) = rfd::FileDialog::new().add_filter("MP3 audio", &["mp3"]).pick_file() else {
            return;
        };
        let duration = probe_duration(&path);
        let mb = fs::metadata(&path).map(|m| m.len() as f64 / 1_048_576.0).unwrap_or(0.0);
        ui.set_audio_path(path.to_string_lossy().as_ref().into());
        ui.set_audio_name(path.file_name().unwrap_or_default().to_string_lossy().as_ref().into());
        ui.set_duration(duration.unwrap_or(0.0) as f32);
        ui.set_audio_info(
            match duration {
                Some(d) => format!("{} · {mb:.1} MB", fmt_time(d)),
                None => "ffprobe not found — progress unavailable".into(),
            }
            .into(),
        );
        set_output(&ui, &path.with_extension("mp4"));
        ui.set_done(false);
        ui.set_status("".into());
    });

    let weak = app.as_weak();
    app.on_pick_output(move || {
        let ui = weak.unwrap();
        let current = PathBuf::from(ui.get_out_path().as_str());
        let mut dialog = rfd::FileDialog::new().add_filter("MP4 video", &["mp4"]);
        if let Some(dir) = current.parent().filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(name) = current.file_name() {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
        if let Some(path) = dialog.save_file() {
            set_output(&ui, &path.with_extension("mp4"));
            ui.set_done(false);
        }
    });

    let weak = app.as_weak();
    app.on_render(move || {
        let ui = weak.unwrap();
        let image = PathBuf::from(ui.get_image_path().as_str());
        let audio = PathBuf::from(ui.get_audio_path().as_str());
        let out = PathBuf::from(ui.get_out_path().as_str());
        let res = RESOLUTIONS[ui.get_resolution().clamp(0, 2) as usize];
        let duration = ui.get_duration() as f64;
        ui.set_rendering(true);
        ui.set_done(false);
        ui.set_progress(0.0);
        ui.set_status(format!("Encoding {}x{} H.264 + AAC…", res.0, res.1).into());
        let weak = ui.as_weak();
        thread::spawn(move || {
            let w = weak.clone();
            let result = render(&image, &audio, &out, res, duration, move |p| {
                let _ = w.upgrade_in_event_loop(move |ui| ui.set_progress(p));
            });
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_rendering(false);
                match result {
                    Ok(()) => {
                        ui.set_progress(1.0);
                        ui.set_done(true);
                        ui.set_status("Done! Your video is ready to upload to YouTube.".into());
                    }
                    Err(e) => ui.set_status(format!("Render failed: {e}").into()),
                }
            });
        });
    });

    let weak = app.as_weak();
    app.on_open_folder(move || {
        let out = weak.unwrap().get_out_path();
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("explorer").raw_arg(format!("/select,\"{out}\"")).spawn();
        }
        #[cfg(not(windows))]
        let _ = out;
    });

    if updater().is_some() {
        let weak = app.as_weak();
        thread::spawn(move || {
            if let Some(Ok(Some(release))) = updater().map(|u| u.is_update_available()) {
                let version = release.version().to_string();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_update_version(version.into());
                    ui.set_update_state("available".into());
                });
            }
        });
    }

    let weak = app.as_weak();
    app.on_do_update(move || {
        weak.unwrap().set_update_state("downloading".into());
        let weak = weak.clone();
        thread::spawn(move || {
            let ok = updater().is_some_and(|u| u.update().is_ok());
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_update_state(if ok { "ready" } else { "failed" }.into());
            });
        });
    });

    app.on_restart(|| {
        if let Ok(exe) = std::env::current_exe() {
            let _ = Command::new(exe).spawn();
        }
        let _ = slint::quit_event_loop();
    });

    app.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_youtube_mp4() {
        let dir = std::env::temp_dir().join("rmpyou-test");
        fs::create_dir_all(&dir).unwrap();
        let (img, mp3, out) = (dir.join("c.png"), dir.join("a.mp3"), dir.join("o.mp4"));
        for (input, file, extra) in [("sine=duration=3", &mp3, &[][..]), ("testsrc2=size=640x480", &img, &["-frames:v", "1"][..])] {
            assert!(tool("ffmpeg").args(["-y", "-loglevel", "error", "-f", "lavfi", "-i", input]).args(extra).arg(file).status().unwrap().success());
        }
        let dur = probe_duration(&mp3).unwrap();
        let last = std::cell::Cell::new(0.0);
        render(&img, &mp3, &out, (1920, 1080), dur, |p| last.set(p)).unwrap();
        assert!(last.get() > 0.9);
        let info = tool("ffprobe")
            .args(["-v", "error", "-show_entries", "stream=codec_name,width,height,pix_fmt,sample_rate", "-of", "csv=p=0"])
            .arg(&out)
            .output()
            .unwrap();
        let info = String::from_utf8_lossy(&info.stdout);
        assert!(info.contains("h264,1920,1080,yuv420p"), "{info}");
        assert!(info.contains("aac,48000"), "{info}");
        assert!((probe_duration(&out).unwrap() - dur).abs() < 0.2);
    }
}
