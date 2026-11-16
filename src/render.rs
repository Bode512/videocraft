//! Edit plan construction: FFmpeg arguments for the assembly engine,
//! frame-by-frame pre-rendering of HTML scenes, execution and verification.

use crate::browser::{find_browser, to_file_url, SceneRenderer};
use crate::project::{validate, Project, Segment};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

type R<T> = std::result::Result<T, String>;

pub fn probe(path: &Path) -> R<Value> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_format", "-show_streams", "-of", "json"])
        .arg(path)
        .output()
        .map_err(|e| format!("ffprobe: {e} (is FFmpeg installed and in PATH?)"))?;
    if !out.status.success() {
        return Err(format!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("ffprobe output not JSON: {e}"))
}

pub fn duration(v: &Value) -> R<f64> {
    v["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or("Media has no finite duration".into())
}

fn has_stream(v: &Value, kind: &str) -> bool {
    v["streams"]
        .as_array()
        .is_some_and(|a| a.iter().any(|s| s["codec_type"] == kind))
}

fn resolve(base: &Path, file: &str) -> R<PathBuf> {
    let p = base
        .join(file)
        .canonicalize()
        .map_err(|e| format!("{file}: {e}"))?;
    if !p.is_file() {
        return Err(format!("Not a file: {file}"));
    }
    Ok(p)
}

/// Number of whole frames a scene contributes (scenes quantize to the frame grid).
pub fn scene_frame_count(duration: f64, fps: u32) -> u32 {
    (((duration * fps as f64).round() as i64).max(1)) as u32
}

/// Temporary workspace for frames and scene segments; removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> R<TempDir> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("videocraft-{}-{millis}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create temp dir: {e}"))?;
        Ok(TempDir(dir))
    }
    fn frames(&self, index: usize) -> PathBuf {
        let d = self.0.join(format!("frames{index}"));
        let _ = std::fs::create_dir_all(&d);
        d
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Build the final FFmpeg command: normalize every video input, concat video,
/// mix audio (with music ducking under the voice) and encode H.264/AAC.
/// `input_sets` contains the video inputs first, then music (if any) and then
/// voice (if any); `video_inputs` is how many of the leading entries are video.
pub fn build_final_args(
    width: u32,
    height: u32,
    fps: u32,
    total: f64,
    input_sets: &[Vec<String>],
    video_inputs: usize,
    has_music: bool,
    has_voice: bool,
    music_gain: Option<f64>,
    output: &str,
) -> Vec<String> {
    let n = video_inputs;
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostdin".into(),
        "-n".into(),
    ];
    for set in input_sets {
        args.extend(set.iter().cloned());
    }
    let mut filters: Vec<String> = Vec::new();
    for i in 0..n {
        filters.push(format!(
            "[{i}:v]scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height},setsar=1,fps={fps},format=yuv420p,setpts=PTS-STARTPTS[v{i}]"
        ));
    }
    filters.push(format!(
        "{}concat=n={n}:v=1:a=0[v]",
        (0..n).map(|i| format!("[v{i}]")).collect::<String>()
    ));
    let mut next = n;
    if has_music {
        let gain = music_gain.unwrap_or(0.0);
        let fade_start = (total - 1.0).max(0.0);
        filters.push(format!(
            "[{next}:a]atrim=duration={total},asetpts=PTS-STARTPTS,aresample=48000,aformat=channel_layouts=stereo,volume={gain},afade=t=out:st={fade_start}:d={}[music]",
            total.min(1.0)
        ));
        next += 1;
    }
    if has_voice {
        filters.push(format!(
            "[{next}:a]aresample=48000,aformat=channel_layouts=stereo,apad,atrim=duration={total},asetpts=PTS-STARTPTS[voice]"
        ));
    }
    match (has_music, has_voice) {
        (true, true) => filters.push(
            "[voice]asplit=2[sc][vo];[music][sc]sidechaincompress=threshold=0.03:ratio=8:attack=20:release=250[duck];[duck][vo]amix=inputs=2:normalize=0,loudnorm=I=-16:TP=-1.5:LRA=11[a]".into(),
        ),
        (true, false) => filters.push("[music]loudnorm=I=-16:TP=-1.5:LRA=11[a]".into()),
        (false, true) => filters.push("[voice]loudnorm=I=-16:TP=-1.5:LRA=11[a]".into()),
        (false, false) => {}
    }
    args.extend(["-filter_complex".into(), filters.join(";"), "-map".into(), "[v]".into()]);
    if has_music || has_voice {
        args.extend([
            "-map".into(),
            "[a]".into(),
            "-c:a".into(),
            "aac".into(),
            "-ar".into(),
            "48000".into(),
            "-b:a".into(),
            "192k".into(),
        ]);
    }
    args.extend([
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "medium".into(),
        "-crf".into(),
        "18".into(),
        "-t".into(),
        total.to_string(),
        "-movflags".into(),
        "+faststart".into(),
        output.into(),
    ]);
    args
}

/// Verify the rendered file: duration, dimensions, fps and codecs.
fn verify_render(path: &str, width: u32, height: u32, fps: u32, total: f64, expect_audio: bool) -> R<Value> {
    let media = probe(Path::new(path))?;
    let rendered = duration(&media)?;
    if (rendered - total).abs() > 0.25 {
        return Err(format!(
            "Rendered duration {rendered:.2}s does not match timeline {total:.2}s"
        ));
    }
    let video = media["streams"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["codec_type"] == "video"))
        .ok_or("Rendered file has no video stream")?;
    let (w, h) = (
        video["width"].as_u64().unwrap_or(0),
        video["height"].as_u64().unwrap_or(0),
    );
    if w != width as u64 || h != height as u64 {
        return Err(format!("Rendered size {w}x{h} does not match project {width}x{height}"));
    }
    if video["codec_name"] != "h264" {
        return Err(format!("Rendered video codec is not h264: {}", video["codec_name"]));
    }
    let fps_seen = video["avg_frame_rate"]
        .as_str()
        .and_then(|s| {
            let mut it = s.splitn(2, '/');
            let num: f64 = it.next()?.parse().ok()?;
            let den: f64 = it.next()?.parse().ok()?;
            if den == 0.0 { None } else { Some(num / den) }
        })
        .unwrap_or(0.0);
    if (fps_seen - fps as f64).abs() > 1.0 {
        return Err(format!(
            "Rendered fps {fps_seen} does not match project fps {fps}"
        ));
    }
    let audio: Vec<&Value> = media["streams"]
        .as_array()
        .map(|a| a.iter().filter(|s| s["codec_type"] == "audio").collect())
        .unwrap_or_default();
    if expect_audio {
        if audio.len() != 1 {
            return Err(format!(
                "Expected exactly one audio stream, found {}",
                audio.len()
            ));
        }
        if audio[0]["codec_name"] != "aac" {
            return Err(format!(
                "Audio codec is not aac: {}",
                audio[0]["codec_name"]
            ));
        }
    } else if !audio.is_empty() {
        return Err("No audio was requested but the output has an audio stream".into());
    }
    Ok(json!({
        "duration_ok": true,
        "dimensions": format!("{width}x{height}"),
        "fps": fps,
        "fps_seen": fps_seen,
        "video_codec": video["codec_name"],
        "audio_codec": audio.first().map(|a| a["codec_name"].clone()).unwrap_or(json!(null)),
    }))
}

fn run_ffmpeg(args: &[String], what: &str) -> R<()> {
    let status = Command::new("ffmpeg")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| format!("ffmpeg: {e} (is FFmpeg installed and in PATH?)"))?;
    if !status.success() {
        return Err(format!("FFmpeg failed during {what}; any partial output must be reviewed or removed"));
    }
    Ok(())
}

/// Render `PROJECT.json` to `output`, or with `dry` return the plan without
/// touching FFmpeg or the browser.
pub fn render(path: &str, output: &str, dry: bool) -> R<Value> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let p: Project = serde_json::from_slice(crate::project::strip_bom(&bytes))
        .map_err(|e| format!("Project JSON: {e}"))?;
    let warnings = validate(&p)?;
    let base = Path::new(path).parent().unwrap_or(Path::new(".")).to_path_buf();
    if Path::new(output).exists() {
        return Err("Output already exists; choose a new filename".into());
    }
    if Path::new(output).extension().and_then(|s| s.to_str()) != Some("mp4") {
        return Err("Output must use .mp4".into());
    }
    let segments = p.segments()?;
    let fps = p.fps;
    let (width, height) = (p.width, p.height);

    // Resolve and validate every segment, and prepare its FFmpeg input args.
    let mut input_sets: Vec<Vec<String>> = Vec::new();
    let mut scene_jobs: Vec<(usize, PathBuf, u32)> = Vec::new();
    let mut has_scenes = false;
    let mut plan_items: Vec<Value> = Vec::new();
    let mut total = 0.0;
    for (i, seg) in segments.iter().enumerate() {
        match seg {
            Segment::Clip { file, start, duration: dur } => {
                let file_path = resolve(&base, file)?;
                let info = probe(&file_path)?;
                if !has_stream(&info, "video") || start + dur > duration(&info)? + 0.05 {
                    return Err(format!(
                        "Clip {i} ({file}): missing video stream or range exceeds source duration"
                    ));
                }
                input_sets.push(vec![
                    "-ss".into(), start.to_string(),
                    "-t".into(), dur.to_string(),
                    "-i".into(), file_path.to_string_lossy().into_owned(),
                ]);
                total += dur;
                plan_items.push(json!({"index": i, "kind": "clip", "file": file, "start": start, "duration": dur}));
            }
            Segment::Image { file, duration } => {
                let file_path = resolve(&base, file)?;
                let info = probe(&file_path)?;
                if !has_stream(&info, "video") {
                    return Err(format!("Image {i} ({file}): not a readable image"));
                }
                input_sets.push(vec![
                    "-loop".into(), "1".into(),
                    "-t".into(), duration.to_string(),
                    "-i".into(), file_path.to_string_lossy().into_owned(),
                ]);
                total += duration;
                plan_items.push(json!({"index": i, "kind": "image", "file": file, "duration": duration}));
            }
            Segment::Scene { file, duration } => {
                let file_path = resolve(&base, file)?;
                let html = std::fs::read_to_string(&file_path)
                    .map_err(|e| format!("Scene {file}: {e}"))?;
                if !html.contains("vcSeek") {
                    return Err(format!(
                        "Scene {file} does not define window.vcSeek(t); every scene must implement \
                         the deterministic time contract. Run `videocraft skills web`"
                    ));
                }
                let frames = scene_frame_count(*duration, fps);
                let effective = frames as f64 / fps as f64;
                total += effective;
                has_scenes = true;
                plan_items.push(json!({
                    "index": i, "kind": "scene", "file": file,
                    "duration": duration, "frames": frames, "effective_duration": effective
                }));
                if dry {
                    input_sets.push(vec![
                        "-i".into(), format!("<scene-segment-{i}-frames.mp4>"),
                    ]);
                } else {
                    scene_jobs.push((i, file_path, frames));
                    input_sets.push(vec![]); // filled after pre-rendering
                }
            }
        }
    }
    let video_inputs = input_sets.len();

    // Music loops to fill the timeline; voice is padded or trimmed to it.
    if let Some(m) = &p.music {
        let file = resolve(&base, &m.file)?;
        if !has_stream(&probe(&file)?, "audio") {
            return Err(format!("Music ({}): has no audio stream", m.file));
        }
        input_sets.push(vec![
            "-stream_loop".into(),
            "-1".into(),
            "-i".into(),
            file.to_string_lossy().into_owned(),
        ]);
    }
    if let Some(voice) = &p.voice {
        let file = resolve(&base, voice)?;
        if !has_stream(&probe(&file)?, "audio") {
            return Err(format!("Voice ({voice}): has no audio stream"));
        }
        input_sets.push(vec!["-i".into(), file.to_string_lossy().into_owned()]);
    }

    if dry {
        let browser = find_browser().map(|b| (b.path.to_string_lossy().into_owned(), b.name));
        if has_scenes && browser.is_none() {
            return Err(
                "Project has scene items but no Chromium browser was found; install Edge or \
                 Chrome, or set VIDEOCRAFT_BROWSER"
                    .into(),
            );
        }
        let final_args = build_final_args(
            width, height, fps, total, &input_sets, video_inputs, p.music.is_some(), p.voice.is_some(),
            p.music.as_ref().map(|m| m.gain), output,
        );
        return Ok(json!({
            "program": "ffmpeg",
            "args": final_args,
            "warnings": warnings,
            "plan": {
                "items": plan_items,
                "total_duration": total,
                "scenes_to_pre_render": scene_jobs.len(),
                "browser": browser.as_ref().map(|(p, _)| p.clone()),
                "browser_name": browser.as_ref().map(|(_, n)| n.clone()),
                "note": "Scene items are rendered frame by frame by a headless browser and encoded before the final FFmpeg pass; <scene-segment-N-frames.mp4> is a temporary file created at render time."
            }
        }));
    }

    // Pre-render scene segments with the headless browser. The temp workspace
    // outlives the final assembly, which consumes the encoded segments; the
    // leading underscore silences the unused warning while Drop does the work.
    let _temp = if scene_jobs.is_empty() {
        None
    } else {
        let browser = find_browser().ok_or(
            "Project has scene items but no Chromium browser was found; install Edge or Chrome, \
             or set VIDEOCRAFT_BROWSER",
        )?;
        let temp = TempDir::new()?;
        let profile = temp.0.join("profile");
        {
            let mut scene = SceneRenderer::launch(&browser, width, height, &profile)?;
            for (index, html_path, frames) in scene_jobs.iter() {
                scene.load_scene(&to_file_url(html_path))?;
                let frames_dir = temp.frames(*index);
                for f in 0..*frames {
                    let t = f as f64 / fps as f64;
                    scene.capture_frame(
                        t,
                        &frames_dir.join(format!("frame_{f:06}.png")),
                        width,
                        height,
                    )?;
                }
                let seg_path = temp.0.join(format!("segment-{index}.mp4"));
                let encode_args = vec![
                    "-hide_banner".to_string(), "-loglevel".to_string(), "error".to_string(),
                    "-nostdin".to_string(), "-y".to_string(),
                    "-framerate".to_string(), fps.to_string(),
                    "-i".to_string(), frames_dir.join("frame_%06d.png").to_string_lossy().into_owned(),
                    "-c:v".to_string(), "libx264".to_string(),
                    "-preset".to_string(), "medium".to_string(),
                    "-crf".to_string(), "10".to_string(),
                    "-pix_fmt".to_string(), "yuv420p".to_string(),
                    "-an".to_string(),
                    seg_path.to_string_lossy().into_owned(),
                ];
                run_ffmpeg(&encode_args, &format!("scene {index} frame encoding"))?;
                input_sets[*index] = vec!["-i".into(), seg_path.to_string_lossy().into_owned()];
                plan_items[*index]["segment"] = json!(seg_path.to_string_lossy());
            }
        }
        Some(temp)
    };

    let final_args = build_final_args(
        width, height, fps, total, &input_sets, video_inputs, p.music.is_some(), p.voice.is_some(),
        p.music.as_ref().map(|m| m.gain), output,
    );
    run_ffmpeg(&final_args, "final assembly")?;
    let verified = verify_render(output, width, height, fps, total, p.music.is_some() || p.voice.is_some())?;
    let media = probe(Path::new(output))?;
    Ok(json!({
        "output": output,
        "duration": total,
        "warnings": warnings,
        "verified": verified,
        "media": media
    }))
}
