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

// --- État partagé : PID du téléchargement en cours (pour l'annulation). ---
struct AppState {
    current_pid: Mutex<Option<u32>>,
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
        #[cfg(windows)]
        {
            // Tue yt-dlp et toute son arborescence (ffmpeg inclus).
            let _ = base_command("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .output();
        }
        #[cfg(not(windows))]
        {
            let _ = base_command("kill").arg(pid.to_string()).output();
        }
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
        })
        .invoke_handler(tauri::generate_handler![
            check_engines,
            fetch_info,
            start_download,
            cancel_download,
            reveal_path
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de l'application");
}
