#![windows_subsystem = "windows"]

use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicU32, Ordering},
    },
    thread,
};

use slint::winit_030::{
    EventResult, WinitWindowAccessor,
    winit::{event::WindowEvent, window::ResizeDirection},
};
use slint::{Color, ComponentHandle, Model, ModelRc, VecModel};

slint::include_modules!();

const RESOLUTIONS: [(u32, u32); 3] = [(1280, 720), (1920, 1080), (3840, 2160)];
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp"];
/// Index order matches the `Grip { dir: N }` elements in the UI.
const RESIZE_DIRS: [ResizeDirection; 8] = [
    ResizeDirection::East,
    ResizeDirection::North,
    ResizeDirection::NorthEast,
    ResizeDirection::NorthWest,
    ResizeDirection::South,
    ResizeDirection::SouthEast,
    ResizeDirection::SouthWest,
    ResizeDirection::West,
];
/// Shared-library build: smallest GPL download (≈83 MB) that includes libx264. Stable URL.
const FFMPEG_URL: &str =
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl-shared.zip";
const FFMPEG_ZIP_ROOT: &str = "ffmpeg-master-latest-win64-gpl-shared";
/// Same relative blur strength for the preview and every output resolution.
const BLUR: &str = "boxblur=luma_radius=min(h\\,w)/20:luma_power=3";

// ───────────── ffmpeg discovery & management ─────────────

/// Where rmpyou keeps its own FFmpeg download (survives app updates).
fn managed_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("rmpyou").join("ffmpeg"))
}

/// Finds a tool: next to rmpyou.exe (portable), then rmpyou's managed copy, then PATH.
fn find_tool(name: &str) -> Option<(PathBuf, &'static str)> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join(&file)));
    let managed = managed_dir().map(|d| d.join("bin").join(&file));
    let on_path = std::env::var_os("PATH")
        .and_then(|p| std::env::split_paths(&p).map(|d| d.join(&file)).find(|p| p.is_file()));
    [(beside, "next to rmpyou"), (managed, "managed by rmpyou"), (on_path, "system PATH")]
        .into_iter()
        .find_map(|(p, src)| p.filter(|p| p.is_file()).map(|p| (p, src)))
}

fn tool(name: &str) -> Command {
    let mut cmd = Command::new(find_tool(name).map_or_else(|| PathBuf::from(name), |t| t.0));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

fn ffmpeg_version() -> Option<String> {
    let out = tool("ffmpeg").arg("-version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    Some(text.split_whitespace().nth(2)?.to_string()).filter(|_| out.status.success())
}

fn refresh_ffmpeg(ui: &AppWindow) {
    let version = ffmpeg_version();
    let ok = version.is_some() && find_tool("ffprobe").is_some();
    let found = find_tool("ffmpeg");
    ui.set_ffmpeg_ok(ok);
    ui.set_ffmpeg_version(version.unwrap_or_default().into());
    ui.set_ffmpeg_source(found.as_ref().map_or("", |t| t.1).into());
    ui.set_ffmpeg_path(found.map(|t| t.0.to_string_lossy().into_owned()).unwrap_or_default().into());
}

/// Downloads FFmpeg into the managed dir. Swaps in atomically-ish: the old copy is only
/// removed once the new one is fully downloaded and extracted.
fn install_ffmpeg(on_progress: impl Fn(u64, Option<u64>) + Send + Sync + 'static) -> Result<(), String> {
    let dir = managed_dir().ok_or("LOCALAPPDATA is not set")?;
    let staging = dir.with_file_name("ffmpeg-staging");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let zip = staging.join("ffmpeg.zip");
    let file = fs::File::create(&zip).map_err(|e| e.to_string())?;
    self_update::Download::from_url(FFMPEG_URL)
        .progress_callback(on_progress)
        .download_to(file)
        .map_err(|e| format!("download failed: {e}"))?;
    self_update::Extract::from_source(&zip)
        .archive(self_update::ArchiveKind::Zip)
        .extract_into(&staging)
        .map_err(|e| format!("unzip failed: {e}"))?;
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let root = staging.join(FFMPEG_ZIP_ROOT);
    fs::rename(root.join("bin"), dir.join("bin")).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(dir.join("bin").join("ffplay.exe")); // player, not needed
    let _ = fs::rename(root.join("LICENSE.txt"), dir.join("LICENSE.txt"));
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

// ───────────── media helpers ─────────────

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

/// A fresh temp path per call: Slint caches images by path, so reusing a name would show stale art.
fn temp_png(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("rmpyou-{tag}-{}-{n}.png", std::process::id()))
}

/// Saves the MP3's embedded cover art (ID3 picture) as a PNG, if it has one.
fn extract_cover(audio: &Path) -> Option<PathBuf> {
    let out = temp_png("cover");
    tool("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(audio)
        .args(["-map", "0:v:0", "-frames:v", "1"])
        .arg(&out)
        .status()
        .ok()?
        .success()
        .then_some(out)
}

/// The image's average color, used by the "Auto" fill.
fn average_color(image: &Path) -> Option<Color> {
    let out = tool("ffmpeg")
        .args(["-loglevel", "error", "-i"])
        .arg(image)
        .args(["-vf", "scale=1:1:flags=area", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .ok()?;
    match out.stdout[..] {
        [r, g, b, ..] => Some(Color::from_rgb_u8(r, g, b)),
        _ => None,
    }
}

/// Small blurred 16:9 background for the preview, same look as the render's blur fill.
fn blur_preview(image: &Path) -> Option<PathBuf> {
    let out = temp_png("blur");
    let vf = format!("scale=480:270:force_original_aspect_ratio=increase,crop=480:270,{BLUR}");
    tool("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(image)
        .args(["-vf", &vf, "-frames:v", "1"])
        .arg(&out)
        .status()
        .ok()?
        .success()
        .then_some(out)
}

fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).ok().filter(|_| s.len() == 6)?;
    Some(Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// `fill`: None = blurred copy of the image, Some(0xRRGGBB) = solid color.
fn video_filter((w, h): (u32, u32), fill: Option<&str>) -> String {
    match fill {
        Some(color) => format!(
            "scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color={color},setsar=1,format=yuv420p"
        ),
        None => format!(
            "split[a][b];[a]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},{BLUR}[bg];\
             [b]scale={w}:{h}:force_original_aspect_ratio=decrease[fg];\
             [bg][fg]overlay=(W-w)/2:(H-h)/2,setsar=1,format=yuv420p"
        ),
    }
}

/// Encode still image + audio into a YouTube-ready MP4. The MP3 stream is copied untouched (no quality
/// loss, no size growth) and the still is encoded at 2 fps, so the file ends up barely larger than the MP3.
fn render(
    image: &Path,
    audio: &Path,
    out: &Path,
    res: (u32, u32),
    fill: Option<&str>,
    duration: f64,
    on_progress: impl Fn(f32),
) -> Result<(), String> {
    let vf = video_filter(res, fill);
    let mut cmd = tool("ffmpeg");
    cmd.args(["-y", "-hide_banner", "-loglevel", "error", "-loop", "1", "-framerate", "2", "-i"])
        .arg(image)
        .arg("-i")
        .arg(audio)
        .args(["-map", "0:v", "-map", "1:a", "-vf", &vf])
        .args(["-c:v", "libx264", "-tune", "stillimage", "-preset", "medium", "-crf", "18", "-profile:v", "high"])
        .args(["-c:a", "copy", "-movflags", "+faststart"]);
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
        .map_err(|e| format!("could not start ffmpeg ({e})"))?;

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

// ───────────── Color Hunt palettes ─────────────

/// Fetches one page of palettes from colorhunt.co's public feed (the one its own site uses).
/// sort: 0 popular (all time), 1 new, 2 random.
fn fetch_palettes(sort: i32, step: u32) -> Result<Vec<[String; 4]>, String> {
    let sort = ["popular", "new", "random"][sort.clamp(0, 2) as usize];
    let step = step.to_string();
    let body = ureq::post("https://colorhunt.co/php/feed.php")
        .send_form([("step", step.as_str()), ("sort", sort), ("tags", ""), ("timeframe", "4000")])
        .map_err(|e| format!("Couldn't reach colorhunt.co ({e})"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    Ok(parse_palettes(&body))
}

/// Pulls every `"code":"<24 hex>"` out of the feed JSON; each code is 4 colors of 6 hex digits.
fn parse_palettes(json: &str) -> Vec<[String; 4]> {
    json.split("\"code\":\"")
        .skip(1)
        .filter_map(|s| s.get(..24))
        .filter(|code| code.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(|code| std::array::from_fn(|i| code[i * 6..i * 6 + 6].to_uppercase()))
        .collect()
}

fn to_hunt_palette(hexes: &[String; 4]) -> HuntPalette {
    let colors: Vec<Color> = hexes.iter().filter_map(|h| parse_hex(h)).collect();
    let hexes: Vec<slint::SharedString> = hexes.iter().map(|h| h.as_str().into()).collect();
    HuntPalette {
        colors: ModelRc::new(VecModel::from(colors)),
        hexes: ModelRc::new(VecModel::from(hexes)),
    }
}

// ───────────── settings ─────────────

fn settings_file() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("rmpyou").join("settings.ini"))
}

fn load_settings(ui: &AppWindow) {
    let Some(text) = settings_file().and_then(|f| fs::read_to_string(f).ok()) else { return };
    for (key, value) in text.lines().filter_map(|l| l.split_once('=')) {
        match (key.trim(), value.trim()) {
            ("theme", v) => {
                if let Some(i) = v.parse().ok().filter(|i| (0..4).contains(i)) {
                    ui.global::<Theme>().set_index(i);
                }
            }
            ("fill", v) => {
                if let Some(i) = v.parse().ok().filter(|i| (0..5).contains(i)) {
                    ui.set_fill_kind(i);
                }
            }
            ("custom_color", v) => {
                if let Some(c) = parse_hex(v) {
                    ui.set_custom_color(c);
                    ui.set_custom_hex(v.into());
                }
            }
            ("auto_update", v) => ui.set_auto_update(v != "false"),
            _ => {}
        }
    }
}

fn save_settings(ui: &AppWindow) {
    let Some(f) = settings_file() else { return };
    let _ = fs::create_dir_all(f.parent().unwrap());
    let c = ui.get_custom_color();
    let _ = fs::write(
        f,
        format!(
            "theme={}\nfill={}\ncustom_color=#{:02X}{:02X}{:02X}\nauto_update={}\n",
            ui.global::<Theme>().get_index(),
            ui.get_fill_kind(),
            c.red(),
            c.green(),
            c.blue(),
            ui.get_auto_update()
        ),
    );
}

// ───────────── updates ─────────────

/// Only enabled in CI builds, where GitHub sets GITHUB_REPOSITORY=owner/repo.
fn repo() -> Option<&'static str> {
    option_env!("GITHUB_REPOSITORY")
}

fn updater() -> Option<self_update::backends::github::Update> {
    let (owner, name) = repo()?.split_once('/')?;
    self_update::backends::github::Update::configure()
        .repo_owner(owner)
        .repo_name(name)
        .bin_name("rmpyou")
        .current_version(self_update::cargo_crate_version!())
        .no_confirm(true)
        .show_output(false)
        .show_download_progress(false)
        .build()
        .ok()
}

fn check_for_update(weak: slint::Weak<AppWindow>) {
    thread::spawn(move || {
        let result = updater().map(|u| u.is_update_available());
        let _ = weak.upgrade_in_event_loop(move |ui| match result {
            Some(Ok(Some(release))) => {
                ui.set_update_version(release.version().into());
                ui.set_update_state("available".into());
            }
            Some(Ok(None)) => ui.set_update_state("latest".into()),
            _ => ui.set_update_state("failed".into()),
        });
    });
}

// ───────────── UI glue ─────────────

/// Windows 11 rounded corners (+ native border) for our frameless window. Returns false where
/// unsupported (Windows 10), so the UI draws its own outline instead.
#[cfg(windows)]
fn round_corners(window: &slint::winit_030::winit::window::Window) -> bool {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(hwnd: isize, attr: u32, value: *const u32, size: u32) -> i32;
    }
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: u32 = 2;
    if let Ok(RawWindowHandle::Win32(h)) = window.window_handle().map(|h| h.as_raw()) {
        // Safety: valid HWND from winit; attribute value is a u32 as documented. Fails harmlessly on Windows 10.
        let hr = unsafe { DwmSetWindowAttribute(h.hwnd.get(), DWMWA_WINDOW_CORNER_PREFERENCE, &DWMWCP_ROUND, 4) };
        return hr == 0;
    }
    false
}

fn open(target: &str) {
    #[cfg(windows)]
    let _ = Command::new("explorer").arg(target).spawn();
    #[cfg(not(windows))]
    let _ = target;
}

fn set_output(ui: &AppWindow, path: &Path) {
    ui.set_out_path(path.to_string_lossy().as_ref().into());
    ui.set_out_name(path.file_name().unwrap_or_default().to_string_lossy().as_ref().into());
    ui.set_out_dir(path.parent().unwrap_or(Path::new("")).to_string_lossy().as_ref().into());
}

fn load_image(ui: &AppWindow, path: &Path) {
    match slint::Image::load_from_path(path) {
        Ok(img) => {
            ui.set_cover(img);
            ui.set_has_cover(true);
            ui.set_image_path(path.to_string_lossy().as_ref().into());
            ui.set_done(false);
            ui.set_status("".into());
            if let Some(c) = average_color(path) {
                ui.set_auto_color(c);
            }
            match blur_preview(path).and_then(|p| slint::Image::load_from_path(&p).ok()) {
                Some(bg) => {
                    ui.set_blur_bg(bg);
                    ui.set_has_blur(true);
                }
                None => ui.set_has_blur(false),
            }
        }
        Err(_) => ui.set_status("Couldn't read that image.".into()),
    }
}

fn load_audio(ui: &AppWindow, path: &Path) {
    let duration = probe_duration(path);
    let mb = fs::metadata(path).map(|m| m.len() as f64 / 1_048_576.0).unwrap_or(0.0);
    ui.set_audio_path(path.to_string_lossy().as_ref().into());
    ui.set_audio_name(path.file_name().unwrap_or_default().to_string_lossy().as_ref().into());
    ui.set_duration(duration.unwrap_or(0.0) as f32);
    ui.set_audio_info(
        match duration {
            Some(d) => format!("{} · {mb:.1} MB", fmt_time(d)),
            None => format!("{mb:.1} MB"),
        }
        .into(),
    );
    set_output(ui, &path.with_extension("mp4"));
    ui.set_done(false);
    ui.set_status("".into());
    if let Some(cover) = extract_cover(path) {
        load_image(ui, &cover);
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let app = AppWindow::new()?;

    let author = env!("CARGO_PKG_AUTHORS").split(':').next().unwrap_or_default();
    app.set_app_version(env!("CARGO_PKG_VERSION").into());
    app.set_author(author.into());
    app.set_repo_url(repo().map(|r| format!("https://github.com/{r}")).unwrap_or_default().into());
    load_settings(&app);
    if let Some(c) = parse_hex(&app.get_custom_hex()) {
        app.set_custom_color(c);
    }
    refresh_ffmpeg(&app);

    let weak = app.as_weak();
    app.on_settings_changed(move || save_settings(&weak.unwrap()));

    let weak = app.as_weak();
    app.on_custom_hex_edited(move |text| {
        let ui = weak.unwrap();
        if let Some(c) = parse_hex(&text) {
            ui.set_custom_color(c);
            save_settings(&ui);
        }
    });

    // Color Hunt: pages load in a thread; a generation counter drops responses from a
    // tab the user already switched away from.
    let hunt_model = std::rc::Rc::new(VecModel::<HuntPalette>::default());
    app.set_hunt_palettes(hunt_model.clone().into());
    let hunt_gen = std::sync::Arc::new(AtomicU32::new(0));
    let hunt_step = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let weak = app.as_weak();
    app.on_hunt_load(move |sort, append| {
        let ui = weak.unwrap();
        let step = if append { hunt_step.get() + 1 } else { 0 };
        hunt_step.set(step);
        if !append {
            hunt_model.set_vec(Vec::new());
        }
        let generation = hunt_gen.fetch_add(1, Ordering::SeqCst) + 1;
        ui.set_hunt_loading(true);
        ui.set_hunt_error("".into());
        let (weak, hunt_gen) = (weak.clone(), hunt_gen.clone());
        thread::spawn(move || {
            let result = fetch_palettes(sort, step);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                if hunt_gen.load(Ordering::SeqCst) != generation {
                    return; // stale
                }
                ui.set_hunt_loading(false);
                match result {
                    Ok(list) if list.is_empty() => ui.set_hunt_error("No more palettes.".into()),
                    Ok(list) => {
                        let model = ui.get_hunt_palettes();
                        let model = model.as_any().downcast_ref::<VecModel<HuntPalette>>().unwrap();
                        model.extend(list.iter().map(to_hunt_palette));
                    }
                    Err(e) => ui.set_hunt_error(e.into()),
                }
            });
        });
    });

    let weak = app.as_weak();
    app.on_hunt_pick(move |hex| {
        let ui = weak.unwrap();
        if let Some(c) = parse_hex(&hex) {
            ui.set_custom_color(c);
            ui.set_custom_hex(format!("#{hex}").into());
            ui.set_fill_kind(4);
            ui.set_hunt_open(false);
            save_settings(&ui);
        }
    });

    let weak = app.as_weak();
    app.on_pick_image(move || {
        if let Some(path) = rfd::FileDialog::new().add_filter("Images", IMAGE_EXTS).pick_file() {
            load_image(&weak.unwrap(), &path);
        }
    });

    let weak = app.as_weak();
    app.on_pick_audio(move || {
        if let Some(path) = rfd::FileDialog::new().add_filter("MP3 audio", &["mp3"]).pick_file() {
            load_audio(&weak.unwrap(), &path);
        }
    });

    // Drag-and-drop and maximize state come straight from winit window events.
    let weak = app.as_weak();
    app.window().on_winit_window_event(move |window, event| {
        let Some(ui) = weak.upgrade() else { return EventResult::Propagate };
        match event {
            WindowEvent::HoveredFile(_) => ui.set_dragging(true),
            WindowEvent::HoveredFileCancelled => ui.set_dragging(false),
            WindowEvent::DroppedFile(path) => {
                ui.set_dragging(false);
                let ext = path.extension().unwrap_or_default().to_string_lossy().to_lowercase();
                if ext == "mp3" {
                    load_audio(&ui, path);
                } else if IMAGE_EXTS.contains(&ext.as_str()) {
                    load_image(&ui, path);
                } else {
                    ui.set_status("Drop an MP3 or a PNG/JPG/WEBP/BMP image.".into());
                }
            }
            WindowEvent::Resized(_) => {
                ui.set_is_max(window.is_maximized());
                #[cfg(windows)]
                ui.set_native_frame(window.with_winit_window(round_corners).unwrap_or(false));
            }
            _ => {}
        }
        EventResult::Propagate
    });

    let weak = app.as_weak();
    app.on_start_drag(move || {
        weak.unwrap().window().with_winit_window(|w| w.drag_window().ok());
    });
    let weak = app.as_weak();
    app.on_start_resize(move |dir| {
        let dir = RESIZE_DIRS[dir.clamp(0, 7) as usize];
        weak.unwrap().window().with_winit_window(|w| w.drag_resize_window(dir).ok());
    });
    let weak = app.as_weak();
    app.on_minimize(move || weak.unwrap().window().set_minimized(true));
    let weak = app.as_weak();
    app.on_toggle_maximize(move || {
        let ui = weak.unwrap();
        let max = !ui.window().is_maximized();
        ui.window().set_maximized(max);
        ui.set_is_max(max);
    });
    app.on_close_window(|| {
        let _ = slint::quit_event_loop();
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
        let c = ui.get_fill_color();
        let fill = (ui.get_fill_kind() != 0).then(|| format!("0x{:02X}{:02X}{:02X}", c.red(), c.green(), c.blue()));
        let duration = ui.get_duration() as f64;
        ui.set_rendering(true);
        ui.set_done(false);
        ui.set_progress(0.0);
        ui.set_status(format!("Encoding {}x{} video, keeping your original MP3 audio…", res.0, res.1).into());
        let weak = ui.as_weak();
        thread::spawn(move || {
            let w = weak.clone();
            let result = render(&image, &audio, &out, res, fill.as_deref(), duration, move |p| {
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

    app.on_open_url(|url| open(&url));

    let weak = app.as_weak();
    app.on_open_ffmpeg_folder(move || {
        let path = PathBuf::from(weak.unwrap().get_ffmpeg_path().as_str());
        if let Some(dir) = path.parent() {
            open(&dir.to_string_lossy());
        }
    });

    let weak = app.as_weak();
    app.on_install_ffmpeg(move || {
        let ui = weak.unwrap();
        ui.set_ffmpeg_busy(true);
        ui.set_ffmpeg_progress(0.0);
        ui.set_ffmpeg_msg("Connecting to GitHub…".into());
        let weak = weak.clone();
        thread::spawn(move || {
            // Throttle UI updates to whole percents; the callback fires for every chunk.
            let last = AtomicU32::new(u32::MAX);
            let progress_ui = Mutex::new(weak.clone());
            let result = install_ffmpeg(move |done, total| {
                let Some(total) = total.filter(|t| *t > 0) else { return };
                let pct = (done * 100 / total) as u32;
                if last.swap(pct, Ordering::Relaxed) != pct {
                    let msg = format!("Downloading FFmpeg… {} / {} MB", done >> 20, total >> 20);
                    let _ = progress_ui.lock().unwrap().upgrade_in_event_loop(move |ui| {
                        ui.set_ffmpeg_progress(pct as f32 / 100.0);
                        ui.set_ffmpeg_msg(if pct == 100 { "Unpacking…".into() } else { msg.into() });
                    });
                }
            });
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_ffmpeg_busy(false);
                refresh_ffmpeg(&ui);
                ui.set_ffmpeg_msg(match result {
                    Ok(()) => "FFmpeg is installed and ready.".into(),
                    Err(e) => format!("FFmpeg setup failed: {e}").into(),
                });
            });
        });
    });

    // Updates
    if updater().is_none() {
        app.set_update_state("disabled".into());
    } else if app.get_auto_update() {
        check_for_update(app.as_weak());
    }

    let weak = app.as_weak();
    app.on_check_update(move || {
        weak.unwrap().set_update_state("checking".into());
        check_for_update(weak.clone());
    });

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
        for fill in [None, Some("0x1E293B")] {
            let last = std::cell::Cell::new(0.0);
            render(&img, &mp3, &out, (1920, 1080), fill, dur, |p| last.set(p)).unwrap();
            assert!(last.get() > 0.9);
            let info = tool("ffprobe")
                .args(["-v", "error", "-show_entries", "stream=codec_name,width,height,pix_fmt,sample_rate", "-of", "csv=p=0"])
                .arg(&out)
                .output()
                .unwrap();
            let info = String::from_utf8_lossy(&info.stdout);
            assert!(info.contains("h264,1920,1080,yuv420p"), "{info}");
            assert!(info.contains("mp3,"), "audio must be stream-copied: {info}");
            assert!((probe_duration(&out).unwrap() - dur).abs() < 0.2);
        }

        // No embedded art -> None; with art -> extracted PNG.
        assert!(extract_cover(&mp3).is_none());
        let tagged = dir.join("tagged.mp3");
        assert!(tool("ffmpeg")
            .args(["-y", "-loglevel", "error", "-i"]).arg(&mp3).arg("-i").arg(&img)
            .args(["-map", "0:a", "-map", "1:v", "-c", "copy", "-disposition:v", "attached_pic"])
            .arg(&tagged).status().unwrap().success());
        let cover = extract_cover(&tagged).unwrap();
        assert!(slint::Image::load_from_path(&cover).is_ok());
        assert!(average_color(&cover).is_some());
        assert!(blur_preview(&cover).is_some());
    }

    #[test]
    fn parses_hex_colors() {
        assert_eq!(parse_hex("#1E293B"), Some(Color::from_rgb_u8(0x1e, 0x29, 0x3b)));
        assert_eq!(parse_hex("ff0000"), Some(Color::from_rgb_u8(255, 0, 0)));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#GGGGGG"), None);
    }

    #[test]
    fn parses_colorhunt_feed() {
        let json = r#"[{"code":"fff5f5f7d6d0e2b4bd4a4a4a","likes":"2550","date":"4 weeks"},{"code":"bad","likes":"1"},{"code":"8b9a6ef7f2ebeae2d6eeeeee","likes":"1237"}]"#;
        let p = parse_palettes(json);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0], ["FFF5F5", "F7D6D0", "E2B4BD", "4A4A4A"].map(String::from));
        assert_eq!(p[1][3], "EEEEEE");
    }
}

#[cfg(test)]
mod live {
    /// Hits colorhunt.co; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn fetches_colorhunt() {
        for sort in 0..3 {
            let p = super::fetch_palettes(sort, 0).unwrap();
            assert!(p.len() >= 10, "sort {sort}: {} palettes", p.len());
        }
        assert_ne!(super::fetch_palettes(1, 0).unwrap()[0], super::fetch_palettes(1, 1).unwrap()[0]);
    }
}
