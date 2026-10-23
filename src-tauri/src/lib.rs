use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// Modèle de progression passé à yt-dlp pour obtenir des lignes faciles à parser.
const PROGRESS_TEMPLATE: &str =
    "PROG|%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s";

// --- État partagé : PID du téléchargement / de la conversion en cours (pour l'annulation). ---
struct AppState {
    current_pid: Mutex<Option<u32>>,
    convert_pid: Mutex<Option<u32>>,
}

// --- Types échangés avec le frontend ---
#[derive(serde::Serialize)]
struct EngineStatus {
    yt_dlp: Option<String>,
    ffmpeg: Option<String>,
}

#[derive(serde::Serialize)]
struct VideoInfo {
    title: String,
    uploader: String,
    thumbnail: String,
    duration: Option<f64>,
    heights: Vec<i64>,
}

#[derive(serde::Deserialize)]
struct DownloadOpts {
    url: String,
    out_dir: String,
    file_name: String, // nom de fichier choisi (sans extension)
    mode: String,      // "video" | "audio"
    max_height: i64,
    audio_quality: String,
    video_format: String, // conteneur vidéo : "mp4" | "mkv" | "webm" | "mov"
    audio_format: String, // format audio : "mp3" | "m4a" | "opus" | "flac" | "wav"
    nvenc: String,   // "none" | "h264_nvenc" | "hevc_nvenc"
    upscale: String, // "none" | "fast" | "ai2" | "ai4"
}

#[derive(serde::Serialize)]
struct MediaInfo {
    name: String, // nom du fichier sans extension
    dir: String,
    ext: String,
    size: u64,
    duration: Option<f64>,
    has_video: bool,
    has_audio: bool,
    width: Option<i64>,
    height: Option<i64>,
}

#[derive(serde::Deserialize)]
struct ConvertOpts {
    input: String,
    out_dir: String,
    file_name: String,     // sans extension
    format: String,        // "mp3" | "m4a" | … | "mp4" | "mkv" | … | "gif"
    audio_quality: String, // "0" | "5" | "9" (même échelle que yt-dlp)
    video_codec: String,   // "auto" | "copy" | "h264_nvenc" | "hevc_nvenc"
}

#[derive(Clone, serde::Serialize)]
struct ConvertDone {
    success: bool,
    message: String,
    path: String,
}

#[derive(Clone, serde::Serialize)]
struct Progress {
    percent: Option<f64>,
    speed: Option<String>,
    eta: Option<String>,
    phase: Option<String>,
}

#[derive(Clone, serde::Serialize)]
struct Done {
    success: bool,
    message: String,
}

// --- Localisation des binaires (yt-dlp.exe / ffmpeg.exe) ---
// En dev : src-tauri/binaries. En release : ressources empaquetées.
fn binaries_dir(app: &AppHandle) -> PathBuf {
    #[cfg(debug_assertions)]
    {
        let _ = app;
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries")
    }
    #[cfg(not(debug_assertions))]
    {
        app.path()
            .resource_dir()
            .map(|d| d.join("binaries"))
            .unwrap_or_else(|_| PathBuf::from("binaries"))
    }
}

fn yt_dlp_path(app: &AppHandle) -> PathBuf {
    binaries_dir(app).join("yt-dlp.exe")
}

// Construit une commande en évitant l'ouverture d'une fenêtre console sous Windows.
fn base_command<P: AsRef<std::ffi::OsStr>>(program: P) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

fn run_version(program: &Path, args: &[&str]) -> Option<String> {
    let out = base_command(program).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().next().map(|l| l.trim().to_string())
}

// --- Commandes Tauri ---

#[tauri::command]
fn check_engines(app: AppHandle) -> EngineStatus {
    let dir = binaries_dir(&app);
    let yt = run_version(&dir.join("yt-dlp.exe"), &["--version"]);
    let ffmpeg = run_version(&dir.join("ffmpeg.exe"), &["-version"]).map(|line| {
        // "ffmpeg version n7.1 ..." -> "n7.1"
        line.split_whitespace()
            .nth(2)
            .unwrap_or("ok")
            .to_string()
    });
    EngineStatus {
        yt_dlp: yt,
        ffmpeg,
    }
}

#[tauri::command]
fn fetch_info(app: AppHandle, url: String) -> Result<VideoInfo, String> {
    let yt = yt_dlp_path(&app);
    let out = base_command(&yt)
        .args(["-J", "--no-warnings", "--no-playlist", &url])
        .output()
        .map_err(|e| format!("yt-dlp introuvable : {e}"))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.trim().lines().last().unwrap_or("Erreur inconnue").to_string());
    }

    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("JSON illisible : {e}"))?;

    let mut heights: Vec<i64> = Vec::new();
    if let Some(formats) = v["formats"].as_array() {
        for f in formats {
            if f["vcodec"].as_str().unwrap_or("none") != "none" {
                if let Some(h) = f["height"].as_i64() {
                    if h > 0 {
                        heights.push(h);
                    }
                }
            }
        }
    }
    heights.sort_unstable();
    heights.dedup();
    heights.reverse();

    Ok(VideoInfo {
        title: v["title"].as_str().unwrap_or("").to_string(),
        uploader: v["uploader"].as_str().unwrap_or("").to_string(),
        thumbnail: v["thumbnail"].as_str().unwrap_or("").to_string(),
        duration: v["duration"].as_f64(),
        heights,
    })
}

#[tauri::command]
fn start_download(
    app: AppHandle,
    state: State<AppState>,
    opts: DownloadOpts,
) -> Result<(), String> {
    let dir = binaries_dir(&app);
    let yt = dir.join("yt-dlp.exe");
    let ffmpeg_location = dir.to_string_lossy().to_string();
    // Nom de fichier : celui choisi par l'utilisateur, sinon le titre.
    let safe_name = sanitize_filename(&opts.file_name);
    let name_part = if safe_name.is_empty() {
        "%(title)s [%(id)s]".to_string()
    } else {
        safe_name.clone()
    };
    let out_template = Path::new(&opts.out_dir)
        .join(format!("{name_part}.%(ext)s"))
        .to_string_lossy()
        .to_string();

    let container = valid_video_format(&opts.video_format);
    let mut args: Vec<String> = Vec::new();

    if opts.mode == "audio" {
        let audio_format = valid_audio_format(&opts.audio_format);
        args.extend([
            "-x".into(),
            "--audio-format".into(),
            audio_format.into(),
            "--audio-quality".into(),
            opts.audio_quality.clone(),
        ]);
    } else {
        let fmt = if opts.max_height >= 9999 || opts.max_height <= 0 {
            "bv*+ba/b".to_string()
        } else {
            format!(
                "bv*[height<={h}]+ba/b[height<={h}]/b[height<={h}]/b",
                h = opts.max_height
            )
        };
        args.extend([
            "-f".into(),
            fmt,
            "--merge-output-format".into(),
            container.into(),
        ]);
        // Option : ré-encodage GPU via NVENC (3060). On recode vers le conteneur choisi.
        if opts.nvenc != "none" {
            args.extend([
                "--recode-video".into(),
                container.into(),
                "--ppa".into(),
                format!("VideoConvertor:-c:v {} -preset p5 -cq 23", opts.nvenc),
            ]);
        }
    }

    args.extend([
        "-o".into(),
        out_template,
        "--ffmpeg-location".into(),
        ffmpeg_location,
        // --- Accélération : contourne le bridage par flux de YouTube ---
        // Télécharge jusqu'à 16 fragments en parallèle (gros gain de vitesse).
        "--concurrent-fragments".into(),
        "16".into(),
        // Buffer réseau plus large + agrandissement dynamique.
        "--buffer-size".into(),
        "16M".into(),
        "--http-chunk-size".into(),
        "10M".into(),
        "--newline".into(),
        "--no-playlist".into(),
        "--no-warnings".into(),
        "--progress-template".into(),
        PROGRESS_TEMPLATE.into(),
        opts.url.clone(),
    ]);

    // --- Préparation de l'agrandissement (post-traitement, vidéo uniquement) ---
    // On ne peut prédire le chemin du fichier que si un nom explicite est fourni.
    // WEBM est exclu : les encodeurs NVENC (H.264/HEVC) n'y sont pas valides.
    let want_upscale = opts.mode == "video"
        && opts.upscale != "none"
        && !safe_name.is_empty()
        && container != "webm";
    if opts.mode == "video" && opts.upscale != "none" && container == "webm" {
        let _ = app.emit(
            "dl:log",
            "⚠ Agrandissement ignoré : incompatible avec le conteneur WEBM.".to_string(),
        );
    }
    let upscale_input: Option<PathBuf> = want_upscale
        .then(|| Path::new(&opts.out_dir).join(format!("{safe_name}.{container}")));
    let upscale_mode = opts.upscale.clone();
    let upscale_nvenc = opts.nvenc.clone();
    let upscale_bin_dir = dir.clone();

    let mut child = base_command(&yt)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Lancement impossible : {e}"))?;

    *state.current_pid.lock().unwrap() = Some(child.id());

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Thread stderr -> journal
    {
        let app_err = app.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    let _ = app_err.emit("dl:log", line);
                }
            }
        });
    }

    // Thread stdout -> progression + journal, puis attente de la fin
    {
        let app_out = app.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(rest) = line.strip_prefix("PROG|") {
                    let parts: Vec<&str> = rest.split('|').collect();
                    let percent = parts
                        .first()
                        .and_then(|s| s.trim().trim_end_matches('%').parse::<f64>().ok());
                    let progress = Progress {
                        percent,
                        speed: clean(parts.get(1)),
                        eta: clean(parts.get(2)),
                        phase: None,
                    };
                    let _ = app_out.emit("dl:progress", progress);
                } else {
                    if let Some(phase) = phase_of(&line) {
                        let _ = app_out.emit(
                            "dl:progress",
                            Progress {
                                percent: None,
                                speed: None,
                                eta: None,
                                phase: Some(phase),
                            },
                        );
                    }
                    if !line.trim().is_empty() {
                        let _ = app_out.emit("dl:log", line);
                    }
                }
            }

            let dl_ok = child.wait().map(|s| s.success()).unwrap_or(false);
            if let Some(st) = app_out.try_state::<AppState>() {
                *st.current_pid.lock().unwrap() = None;
            }

            let mut success = dl_ok;
            let mut message = if dl_ok {
                String::new()
            } else {
                "Téléchargement interrompu ou en échec.".into()
            };

            // --- Agrandissement (upscaling) si demandé ---
            if dl_ok {
                if let Some(input) = upscale_input {
                    match run_upscale(
                        &app_out,
                        &upscale_bin_dir,
                        &input,
                        &upscale_mode,
                        container,
                        &upscale_nvenc,
                    ) {
                        Ok(()) => {
                            let _ = app_out.emit("dl:log", "✓ Agrandissement terminé.".to_string());
                        }
                        Err(e) => {
                            success = false;
                            message = format!("Agrandissement échoué : {e}");
                            let _ = app_out.emit("dl:log", format!("✗ Agrandissement : {e}"));
                        }
                    }
                    if let Some(st) = app_out.try_state::<AppState>() {
                        *st.current_pid.lock().unwrap() = None;
                    }
                }
            }

            let _ = app_out.emit("dl:done", Done { success, message });
        });
    }

    Ok(())
}

#[tauri::command]
fn cancel_download(state: State<AppState>) {
    let pid = state.current_pid.lock().unwrap().take();
    if let Some(pid) = pid {
        kill_tree(pid);
    }
}

// Tue un processus et toute son arborescence (yt-dlp -> ffmpeg inclus).
fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = base_command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = base_command("kill").arg(pid.to_string()).output();
    }
}

// --- Conversion de fichiers locaux (ffmpeg) ---

// Lit les infos d'un fichier média local via ffprobe.
#[tauri::command]
fn probe_file(app: AppHandle, path: String) -> Result<MediaInfo, String> {
    let p = Path::new(&path);
    if !p.is_file() {
        return Err("Fichier introuvable.".into());
    }
    let ffprobe = binaries_dir(&app).join("ffprobe.exe");
    let out = base_command(&ffprobe)
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(p)
        .output()
        .map_err(|e| format!("ffprobe introuvable : {e}"))?;
    if !out.status.success() {
        return Err("Fichier illisible : ce n'est pas un média reconnu par ffmpeg.".into());
    }
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("JSON illisible : {e}"))?;

    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    // Une pochette d'album (attached_pic) n'est pas une « vraie » vidéo.
    let video = streams.iter().find(|s| {
        s["codec_type"] == "video" && s["disposition"]["attached_pic"].as_i64() != Some(1)
    });
    let has_audio = streams.iter().any(|s| s["codec_type"] == "audio");
    if video.is_none() && !has_audio {
        return Err("Aucune piste audio ou vidéo dans ce fichier.".into());
    }

    Ok(MediaInfo {
        name: p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        dir: p.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default(),
        ext: p.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default(),
        size: std::fs::metadata(p).map(|m| m.len()).unwrap_or(0),
        duration: media_duration(&v),
        has_video: video.is_some(),
        has_audio,
        width: video.and_then(|s| s["width"].as_i64()),
        height: video.and_then(|s| s["height"].as_i64()),
    })
}

#[tauri::command]
fn start_convert(app: AppHandle, state: State<AppState>, opts: ConvertOpts) -> Result<(), String> {
    let input = PathBuf::from(&opts.input);
    if !input.is_file() {
        return Err("Fichier source introuvable.".into());
    }
    let dir = binaries_dir(&app);
    let ffmpeg = dir.join("ffmpeg.exe");
    let ffprobe = dir.join("ffprobe.exe");

    let format = valid_convert_format(&opts.format);
    let name = {
        let n = sanitize_filename(&opts.file_name);
        if n.is_empty() {
            input.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("converti".into())
        } else {
            n
        }
    };
    let out_dir = if opts.out_dir.trim().is_empty() {
        input.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        PathBuf::from(&opts.out_dir)
    };
    // Jamais d'écrasement (ni de la source, ni d'un fichier existant).
    let output = unique_path(&out_dir, &name, format);

    let duration = base_command(&ffprobe)
        .args(["-v", "error", "-print_format", "json", "-show_format"])
        .arg(&input)
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| media_duration(&v));

    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-loglevel".into(), "error".into(),
        "-progress".into(), "pipe:1".into(),
        "-nostats".into(),
        "-i".into(), input.to_string_lossy().to_string(),
    ];
    args.extend(convert_codec_args(format, &opts.audio_quality, &opts.video_codec));
    args.push(output.to_string_lossy().to_string());

    let _ = app.emit(
        "dl:log",
        format!("▶ Conversion : {} → {}", input.display(), output.display()),
    );

    let mut child = base_command(&ffmpeg)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Lancement de ffmpeg impossible : {e}"))?;

    *state.convert_pid.lock().unwrap() = Some(child.id());

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Thread stderr -> journal (ffmpeg n'y écrit que les erreurs avec -loglevel error).
    let app_err = app.clone();
    let err_thread = std::thread::spawn(move || {
        let mut last = String::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if !line.trim().is_empty() {
                let _ = app_err.emit("dl:log", line.clone());
                last = line;
            }
        }
        last
    });

    // Thread stdout -> progression (-progress pipe:1 : lignes clé=valeur), puis attente.
    let app_out = app.clone();
    std::thread::spawn(move || {
        let mut speed: Option<String> = None;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Some((key, val)) = line.split_once('=') else { continue };
            match key {
                "speed" => speed = clean(Some(&val)).filter(|s| s != "N/A"),
                // Malgré son nom, out_time_ms est en microsecondes.
                "out_time_ms" | "out_time_us" => {
                    let (Some(total), Ok(us)) = (duration, val.trim().parse::<f64>()) else {
                        continue;
                    };
                    let done = us / 1_000_000.0;
                    let percent = (done / total * 100.0).clamp(0.0, 100.0);
                    let eta = speed
                        .as_deref()
                        .and_then(|s| s.trim_end_matches('x').parse::<f64>().ok())
                        .filter(|x| *x > 0.0)
                        .map(|x| fmt_hms((total - done).max(0.0) / x));
                    let _ = app_out.emit(
                        "cv:progress",
                        Progress {
                            percent: Some(percent),
                            speed: speed.clone(),
                            eta,
                            phase: Some("Conversion".into()),
                        },
                    );
                }
                _ => {}
            }
        }

        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        let last_err = err_thread.join().unwrap_or_default();
        let cancelled = match app_out.try_state::<AppState>() {
            Some(st) => st.convert_pid.lock().unwrap().take().is_none(),
            None => false,
        };
        if !ok {
            // Fichier partiel inutilisable : on le supprime.
            let _ = std::fs::remove_file(&output);
        }
        let message = if ok {
            String::new()
        } else if cancelled {
            "Conversion annulée.".into()
        } else if last_err.is_empty() {
            "Conversion en échec.".into()
        } else {
            format!("Conversion en échec : {last_err}")
        };
        let _ = app_out.emit(
            "cv:done",
            ConvertDone {
                success: ok,
                message,
                path: output.to_string_lossy().to_string(),
            },
        );
    });

    Ok(())
}

#[tauri::command]
fn cancel_convert(state: State<AppState>) {
    if let Some(pid) = state.convert_pid.lock().unwrap().take() {
        kill_tree(pid);
    }
}

// Durée (secondes) lue dans la sortie JSON de ffprobe.
fn media_duration(v: &serde_json::Value) -> Option<f64> {
    v["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|d| *d > 0.0)
}

// Valide le format de sortie d'une conversion ; défaut « mp3 » si inconnu.
fn valid_convert_format(fmt: &str) -> &'static str {
    match fmt {
        "mp4" => "mp4",
        "mkv" => "mkv",
        "webm" => "webm",
        "mov" => "mov",
        "avi" => "avi",
        "gif" => "gif",
        other => valid_audio_format(other),
    }
}

// Arguments ffmpeg (sélection des pistes + codecs) selon le format visé.
fn convert_codec_args(format: &str, audio_quality: &str, video_codec: &str) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
    // Même échelle que yt-dlp : 0 = meilleure, 5 = moyenne, 9 = légère.
    let (br, br_opus) = match audio_quality {
        "9" => ("64k", "48k"),
        "5" => ("128k", "96k"),
        _ => ("320k", "192k"),
    };

    // --- Sorties audio : on ignore toute piste vidéo (et les pochettes). ---
    let audio = match format {
        "mp3" => Some(s(&["-c:a", "libmp3lame", "-b:a", br])),
        "m4a" => Some(s(&["-c:a", "aac", "-b:a", br])),
        "opus" => Some(s(&["-c:a", "libopus", "-b:a", br_opus])),
        "flac" => Some(s(&["-c:a", "flac"])),
        "wav" => Some(s(&["-c:a", "pcm_s16le"])),
        _ => None,
    };
    if let Some(codec) = audio {
        let mut a = s(&["-map", "0:a:0", "-vn"]);
        a.extend(codec);
        return a;
    }

    if format == "gif" {
        return s(&[
            "-map", "0:v:0",
            "-an",
            "-vf", "fps=12,scale='min(480,iw)':-2:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse",
            "-loop", "0",
        ]);
    }

    // --- Sorties vidéo : première piste vidéo + première piste audio (si présentes). ---
    let mut a = s(&["-map", "0:v:0?", "-map", "0:a:0?"]);
    if video_codec == "copy" {
        // Remux sans perte : instantané, mais le conteneur doit accepter les codecs source.
        a.extend(s(&["-c", "copy"]));
    } else {
        match format {
            "webm" => a.extend(s(&[
                "-c:v", "libvpx-vp9", "-crf", "32", "-b:v", "0",
                "-row-mt", "1", "-deadline", "good", "-cpu-used", "4",
                "-c:a", "libopus", "-b:a", "128k",
            ])),
            "avi" => a.extend(s(&[
                "-c:v", "mpeg4", "-q:v", "3",
                "-c:a", "libmp3lame", "-b:a", "192k",
            ])),
            _ => {
                match video_codec {
                    "h264_nvenc" | "hevc_nvenc" => {
                        a.extend(s(&["-c:v", video_codec, "-preset", "p5", "-cq", "23"]));
                        if video_codec == "hevc_nvenc" && format != "mkv" {
                            // Balise attendue par les lecteurs Apple / Windows.
                            a.extend(s(&["-tag:v", "hvc1"]));
                        }
                    }
                    _ => a.extend(s(&["-c:v", "libx264", "-crf", "20", "-preset", "medium"])),
                }
                a.extend(s(&["-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k"]));
            }
        }
    }
    if matches!(format, "mp4" | "mov") {
        a.extend(s(&["-movflags", "+faststart"]));
    }
    a
}

// Chemin de sortie libre : « nom.ext », sinon « nom (1).ext », « nom (2).ext »…
fn unique_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{name}.{ext}"));
    if !first.exists() {
        return first;
    }
    (1..)
        .map(|i| dir.join(format!("{name} ({i}).{ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

// 75.4 -> "01:15" ; 3725 -> "1:02:05"
fn fmt_hms(sec: f64) -> String {
    let t = sec.round() as u64;
    let (h, m, s) = (t / 3600, (t % 3600) / 60, t % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

// Ouvre l'explorateur de fichiers sur le dossier (ou le fichier) donné.
#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    #[cfg(windows)]
    {
        if p.is_file() {
            // `raw_arg` : on contrôle nous-mêmes les guillemets. Explorer exige
            // /select,"chemin" (avec guillemets) sinon il ignore la sélection
            // dès que le chemin contient des espaces.
            base_command("explorer")
                .raw_arg(format!("/select,\"{}\"", path))
                .spawn()
                .map_err(|e| e.to_string())?;
        } else if p.is_dir() {
            base_command("explorer")
                .arg(&path)
                .spawn()
                .map_err(|e| e.to_string())?;
        } else {
            return Err("Chemin introuvable.".into());
        }
    }
    #[cfg(not(windows))]
    {
        let target = if p.is_file() {
            p.parent().map(|d| d.to_path_buf()).unwrap_or_default()
        } else {
            p.to_path_buf()
        };
        base_command("xdg-open")
            .arg(target)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// Valide le conteneur vidéo demandé ; défaut « mp4 » si inconnu.
fn valid_video_format(fmt: &str) -> &'static str {
    match fmt {
        "mkv" => "mkv",
        "webm" => "webm",
        "mov" => "mov",
        _ => "mp4",
    }
}

// Valide le format audio demandé ; défaut « mp3 » si inconnu.
fn valid_audio_format(fmt: &str) -> &'static str {
    match fmt {
        "m4a" => "m4a",
        "opus" => "opus",
        "flac" => "flac",
        "wav" => "wav",
        _ => "mp3",
    }
}

// Détecte la fréquence d'images d'une vidéo via ffprobe (ex. "30000/1001").
fn ffprobe_fps(ffprobe: &Path, input: &Path) -> String {
    let out = base_command(ffprobe)
        .args([
            "-v", "0",
            "-select_streams", "v:0",
            "-show_entries", "stream=r_frame_rate",
            "-of", "csv=p=0",
        ])
        .arg(input)
        .output();
    if let Ok(o) = out {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !s.is_empty() && s != "0/0" {
            return s;
        }
    }
    "30".to_string()
}

// Extrait un pourcentage d'une ligne de progression (ex. "  3.45%" -> 3.45).
fn parse_percent(s: &str) -> Option<f64> {
    let tok = s.split_whitespace().find(|t| t.ends_with('%'))?;
    tok.trim_end_matches('%')
        .parse::<f64>()
        .ok()
        .filter(|p| (0.0..=100.0).contains(p))
}

// Lance une étape externe (ffmpeg / Real-ESRGAN), enregistre son PID pour
// l'annulation, et remonte la progression (phase + pourcentage si présent).
fn run_stage(app: &AppHandle, program: &Path, args: &[String], phase: &str) -> Result<(), String> {
    let _ = app.emit(
        "dl:progress",
        Progress {
            percent: Some(0.0),
            speed: None,
            eta: None,
            phase: Some(phase.to_string()),
        },
    );

    let mut child = base_command(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{phase} : lancement impossible ({e})"))?;

    if let Some(st) = app.try_state::<AppState>() {
        *st.current_pid.lock().unwrap() = Some(child.id());
    }

    let stderr = child.stderr.take().unwrap();
    let app2 = app.clone();
    let phase_owned = phase.to_string();
    let err_thread = std::thread::spawn(move || {
        // ffmpeg et Real-ESRGAN écrivent leur progression avec des '\r'.
        for chunk in BufReader::new(stderr).split(b'\r').map_while(Result::ok) {
            let text = String::from_utf8_lossy(&chunk);
            if let Some(p) = parse_percent(text.trim()) {
                let _ = app2.emit(
                    "dl:progress",
                    Progress {
                        percent: Some(p),
                        speed: None,
                        eta: None,
                        phase: Some(phase_owned.clone()),
                    },
                );
            }
        }
    });

    // On draine stdout pour éviter un blocage sur le tampon.
    let stdout = child.stdout.take().unwrap();
    let out_thread =
        std::thread::spawn(move || for _ in BufReader::new(stdout).lines().map_while(Result::ok) {});

    let status = child.wait().map_err(|e| e.to_string())?;
    let _ = err_thread.join();
    let _ = out_thread.join();

    if let Some(st) = app.try_state::<AppState>() {
        *st.current_pid.lock().unwrap() = None;
    }

    if status.success() {
        Ok(())
    } else {
        Err(format!("étape « {phase} » interrompue ou en échec"))
    }
}

// Remplace `target` par `src` (rename impossible par-dessus un fichier existant
// sous Windows : on supprime d'abord).
fn replace_file(target: &Path, src: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(target);
    std::fs::rename(src, target).map_err(|e| format!("remplacement du fichier : {e}"))
}

// Agrandit la vidéo `input` sur place.
//   mode : "fast" (ffmpeg lanczos ×2) | "ai2" (Real-ESRGAN, net ×2) | "ai4" (×4)
// Pour l'IA on passe par le modèle x4plus (×4) puis on redescend à ×2 pour "ai2"
// (sur-échantillonnage : meilleur rendu). Le résultat garde le conteneur d'origine.
fn run_upscale(
    app: &AppHandle,
    bin_dir: &Path,
    input: &Path,
    mode: &str,
    container: &str,
    nvenc: &str,
) -> Result<(), String> {
    let ffmpeg = bin_dir.join("ffmpeg.exe");
    let ffprobe = bin_dir.join("ffprobe.exe");
    let enc = if nvenc != "none" { nvenc } else { "h264_nvenc" };
    let out_tmp = input.with_extension(format!("up.{container}"));
    let path = |p: &Path| p.to_string_lossy().to_string();

    // --- Mode rapide (non-IA) : simple mise à l'échelle ×2 par ffmpeg. ---
    if mode == "fast" {
        let args = vec![
            "-y".into(),
            "-i".into(), path(input),
            "-vf".into(), "scale=iw*2:ih*2:flags=lanczos".into(),
            "-c:v".into(), enc.to_string(),
            "-preset".into(), "p5".into(),
            "-cq".into(), "23".into(),
            "-pix_fmt".into(), "yuv420p".into(),
            "-c:a".into(), "copy".into(),
            path(&out_tmp),
        ];
        run_stage(app, &ffmpeg, &args, "Agrandissement rapide")?;
        return replace_file(input, &out_tmp);
    }

    // --- Mode IA : extraction des images -> Real-ESRGAN -> réassemblage. ---
    let realesr = bin_dir.join("realesrgan").join("realesrgan-ncnn-vulkan.exe");
    if !realesr.exists() {
        return Err("Real-ESRGAN introuvable — lance scripts/setup-binaries.ps1".into());
    }
    let models = bin_dir.join("realesrgan").join("models");
    let scale_down = mode == "ai2";
    let fps = ffprobe_fps(&ffprobe, input);

    let tmp = input
        .parent()
        .ok_or("dossier parent introuvable")?
        .join(".ytdl_upscale_tmp");
    let frames = tmp.join("in");
    let up = tmp.join("out");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&frames).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&up).map_err(|e| e.to_string())?;

    // Nettoyage garanti du dossier temporaire, quel que soit le résultat.
    let cleanup = |r: Result<(), String>| {
        let _ = std::fs::remove_dir_all(&tmp);
        r
    };

    // 1) Extraction des images en PNG.
    let extract = vec![
        "-y".into(),
        "-i".into(), path(input),
        path(&frames.join("frame%08d.png")),
    ];
    if let Err(e) = run_stage(app, &ffmpeg, &extract, "Extraction des images") {
        return cleanup(Err(e));
    }

    // 2) Agrandissement IA (modèle général x4plus, ×4).
    let ai = vec![
        "-i".into(), path(&frames),
        "-o".into(), path(&up),
        "-n".into(), "realesrgan-x4plus".into(),
        "-s".into(), "4".into(),
        "-f".into(), "png".into(),
        "-m".into(), path(&models),
    ];
    if let Err(e) = run_stage(app, &realesr, &ai, "Agrandissement IA") {
        return cleanup(Err(e));
    }

    // 3) Réassemblage des images + audio d'origine, ré-encodage GPU.
    let mut enc_args = vec![
        "-y".into(),
        "-framerate".into(), fps.clone(),
        "-i".into(), path(&up.join("frame%08d.png")),
        "-i".into(), path(input),
        "-map".into(), "0:v:0".into(),
        "-map".into(), "1:a?".into(),
        "-c:a".into(), "copy".into(),
        "-c:v".into(), enc.to_string(),
        "-preset".into(), "p5".into(),
        "-cq".into(), "20".into(),
        "-pix_fmt".into(), "yuv420p".into(),
        "-r".into(), fps.clone(),
    ];
    if scale_down {
        enc_args.push("-vf".into());
        enc_args.push("scale=iw/2:ih/2:flags=lanczos".into());
    }
    enc_args.push(path(&out_tmp));
    if let Err(e) = run_stage(app, &ffmpeg, &enc_args, "Ré-encodage final") {
        return cleanup(Err(e));
    }

    cleanup(replace_file(input, &out_tmp))
}

// Nettoie un nom de fichier des caractères interdits sous Windows (et %).
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '%') && !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(180)
        .collect()
}

// Nettoie une valeur de progression (vide / inconnue -> None).
fn clean(s: Option<&&str>) -> Option<String> {
    let t = s.map(|x| x.trim()).unwrap_or("");
    if t.is_empty() || t.contains("Unknown") || t == "NA" {
        None
    } else {
        Some(t.to_string())
    }
}

// Détecte la phase courante à partir des messages de yt-dlp.
fn phase_of(line: &str) -> Option<String> {
    if line.contains("Destination") && line.contains("[download]") {
        Some("Téléchargement".into())
    } else if line.contains("[Merger]") {
        Some("Fusion audio/vidéo".into())
    } else if line.contains("[ExtractAudio]") {
        Some("Extraction audio".into())
    } else if line.contains("[VideoConvertor]") || line.contains("Recoding") {
        Some("Ré-encodage GPU".into())
    } else {
        None
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            current_pid: Mutex::new(None),
            convert_pid: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            check_engines,
            fetch_info,
            start_download,
            cancel_download,
            probe_file,
            start_convert,
            cancel_convert,
            reveal_path
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de l'application");
}
