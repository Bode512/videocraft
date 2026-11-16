use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{self, BufRead},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

type Result<T> = std::result::Result<T, String>;
#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Clip {
    file: String,
    start: f64,
    duration: f64,
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Music {
    file: String,
    bpm: f64,
    mood: String,
    license: String,
    gain: f64,
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Project {
    version: u32,
    width: u32,
    height: u32,
    fps: u32,
    mood: String,
    clips: Vec<Clip>,
    music: Option<Music>,
    voice: Option<String>,
}

fn read_project(path: &str) -> Result<Project> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
fn probe(path: &Path) -> Result<Value> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|e| format!("ffprobe: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}
fn duration(v: &Value) -> Result<f64> {
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
fn resolve(base: &Path, file: &str) -> Result<PathBuf> {
    let p = base
        .join(file)
        .canonicalize()
        .map_err(|e| format!("{file}: {e}"))?;
    if !p.is_file() {
        return Err(format!("Not a file: {file}"));
    }
    Ok(p)
}
fn validate(p: &Project) -> Result<Vec<String>> {
    if p.version != 1 {
        return Err("Unsupported project version".into());
    }
    if p.width < 16
        || p.height < 16
        || p.width > 3840
        || p.height > 3840
        || !p.width.is_multiple_of(2)
        || !p.height.is_multiple_of(2)
        || !(1..=60).contains(&p.fps)
    {
        return Err("Use even dimensions 16..3840 and fps 1..60".into());
    }
    if p.clips.is_empty() || p.clips.len() > 100 {
        return Err("Use 1..100 clips".into());
    }
    if p.clips.iter().any(|c| {
        !c.start.is_finite() || c.start < 0.0 || !c.duration.is_finite() || c.duration < 0.1
    }) || p.clips.iter().map(|c| c.duration).sum::<f64>() > 3600.0
    {
        return Err("Invalid clip times or total duration exceeds 1 hour".into());
    }
    let mut warnings = vec![
        "Creative quality requires visual and listening review; no automatic 10/10 guarantee."
            .into(),
    ];
    if let Some(m) = &p.music {
        if !m.bpm.is_finite()
            || !(20.0..=300.0).contains(&m.bpm)
            || !m.gain.is_finite()
            || !(0.0..=1.0).contains(&m.gain)
            || m.license.trim().is_empty()
        {
            return Err("Music requires BPM 20..300, gain 0..1 and license attribution".into());
        }
        if !m.mood.eq_ignore_ascii_case(&p.mood) {
            warnings.push("Music mood differs from project mood".into());
        }
        let mut t = 0.0;
        for c in p.clips.iter().take(p.clips.len() - 1) {
            t += c.duration;
            let beat = t * m.bpm / 60.0;
            if (beat - beat.round()).abs() > 0.15 {
                warnings.push(format!("Cut at {t:.2}s is off the declared beat grid"));
            }
        }
    } else {
        warnings.push("No music selected".into());
    }
    Ok(warnings)
}
fn render(path: &str, output: &str, dry: bool) -> Result<Value> {
    let p = read_project(path)?;
    let warnings = validate(&p)?;
    let base = Path::new(path).parent().unwrap_or(Path::new("."));
    if Path::new(output).exists() {
        return Err("Output already exists; choose a new filename".into());
    }
    if Path::new(output).extension().and_then(|s| s.to_str()) != Some("mp4") {
        return Err("Output must use .mp4".into());
    }
    let total: f64 = p.clips.iter().map(|c| c.duration).sum();
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostdin".into(),
        "-n".into(),
    ];
    let mut filters = Vec::new();
    for (i, c) in p.clips.iter().enumerate() {
        let file = resolve(base, &c.file)?;
        let info = probe(&file)?;
        if !has_stream(&info, "video") || c.start + c.duration > duration(&info)? + 0.05 {
            return Err(format!(
                "Clip {i}: missing video or range exceeds source duration"
            ));
        }
        args.extend([
            "-ss".into(),
            c.start.to_string(),
            "-t".into(),
            c.duration.to_string(),
            "-i".into(),
            file.to_string_lossy().into_owned(),
        ]);
        filters.push(format!("[{i}:v]scale={}:{}:force_original_aspect_ratio=increase,crop={}:{},setsar=1,fps={},format=yuv420p,setpts=PTS-STARTPTS[v{i}]",p.width,p.height,p.width,p.height,p.fps));
    }
    filters.push(format!(
        "{}concat=n={}:v=1:a=0[v]",
        (0..p.clips.len())
            .map(|i| format!("[v{i}]"))
            .collect::<String>(),
        p.clips.len()
    ));
    let mut next = p.clips.len();
    if let Some(m) = &p.music {
        let file = resolve(base, &m.file)?;
        if !has_stream(&probe(&file)?, "audio") {
            return Err("Music has no audio".into());
        }
        args.extend([
            "-stream_loop".into(),
            "-1".into(),
            "-i".into(),
            file.to_string_lossy().into_owned(),
        ]);
        filters.push(format!("[{next}:a]atrim=duration={total},asetpts=PTS-STARTPTS,aresample=48000,aformat=channel_layouts=stereo,volume={},afade=t=out:st={}:d={}[music]",m.gain,(total-1.0).max(0.0),total.min(1.0)));
        next += 1;
    }
    if let Some(voice) = &p.voice {
        let file = resolve(base, voice)?;
        if !has_stream(&probe(&file)?, "audio") {
            return Err("Voice has no audio".into());
        }
        args.extend(["-i".into(), file.to_string_lossy().into_owned()]);
        filters.push(format!("[{next}:a]aresample=48000,aformat=channel_layouts=stereo,apad,atrim=duration={total},asetpts=PTS-STARTPTS[voice]"));
    }
    match (p.music.is_some(),p.voice.is_some()) {
        (true,true)=>filters.push("[voice]asplit=2[sc][vo];[music][sc]sidechaincompress=threshold=0.03:ratio=8:attack=20:release=250[duck];[duck][vo]amix=inputs=2:normalize=0,loudnorm=I=-16:TP=-1.5:LRA=11[a]".into()),
        (true,false)=>filters.push("[music]loudnorm=I=-16:TP=-1.5:LRA=11[a]".into()),
        (false,true)=>filters.push("[voice]loudnorm=I=-16:TP=-1.5:LRA=11[a]".into()),
        _=>{}
    }
    args.extend([
        "-filter_complex".into(),
        filters.join(";"),
        "-map".into(),
        "[v]".into(),
    ]);
    if p.music.is_some() || p.voice.is_some() {
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
    if dry {
        return Ok(json!({"program":"ffmpeg","args":args,"warnings":warnings}));
    }
    let status = Command::new("ffmpeg")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("FFmpeg failed; any partial output must be reviewed or removed".into());
    }
    let result = probe(Path::new(output))?;
    if (duration(&result)? - total).abs() > 0.25 {
        return Err("Rendered duration does not match timeline".into());
    }
    Ok(json!({"output":output,"duration":total,"warnings":warnings,"media":result}))
}
fn skill(name: &str) -> Result<&'static str> {
    match name {
        "ads" => Ok(include_str!("../skills/ads.md")),
        "music" => Ok(include_str!("../skills/music.md")),
        "quality" => Ok(include_str!("../skills/quality.md")),
        _ => Err("Unknown skill; use ads, music or quality".into()),
    }
}
fn dispatch(a: &[String]) -> Result<Value> {
    let get = |n: usize| {
        a.get(n)
            .map(String::as_str)
            .ok_or_else(|| "Missing argument; run help".to_string())
    };
    match a.first().map(String::as_str).unwrap_or("help") {
        "help" | "--help" => Ok(
            json!({"name":"videocraft","version":env!("CARGO_PKG_VERSION"),"commands":["doctor","example","skills [ads|music|quality]","probe FILE","check PROJECT.json","render PROJECT.json OUTPUT.mp4 [--dry-run]","serve"],"protocol":"serve accepts one JSON object per line: {id, args: [command, ...]}; returns {id, ok, result|error}","role":"Local editing tool for external AI agents. No model or API key required."}),
        ),
        "--version" => Ok(json!({"version":env!("CARGO_PKG_VERSION")})),
        "doctor" => {
            let mut results = serde_json::Map::new();
            for tool in ["ffmpeg", "ffprobe"] {
                let ok = Command::new(tool)
                    .arg("-version")
                    .output()
                    .is_ok_and(|o| o.status.success());
                results.insert(tool.into(), json!(ok));
            }
            Ok(json!({"dependencies":results}))
        }
        "example" => Ok(
            json!({"version":1,"width":1080,"height":1920,"fps":30,"mood":"energetic","clips":[{"file":"hook.mp4","start":0.0,"duration":2.0},{"file":"product.mp4","start":0.0,"duration":4.0},{"file":"cta.mp4","start":0.0,"duration":2.0}],"music":{"file":"music.wav","bpm":120.0,"mood":"energetic","license":"REPLACE with actual license or ownership","gain":0.25},"voice":"voice.wav"}),
        ),
        "skills" => {
            if a.len() == 1 {
                Ok(json!(["ads", "music", "quality"]))
            } else {
                Ok(json!({"name":get(1)?,"instructions":skill(get(1)?)?}))
            }
        }
        "probe" => probe(Path::new(get(1)?)),
        "check" => {
            let p = read_project(get(1)?)?;
            Ok(
                json!({"warnings":validate(&p)?,"duration":p.clips.iter().map(|c|c.duration).sum::<f64>(),"scope":"Timeline validation only. render --dry-run also verifies input media."}),
            )
        }
        "render" => {
            if a.len() > 4 || a.get(3).is_some_and(|x| x != "--dry-run") {
                return Err("Unknown render option".into());
            }
            render(
                get(1)?,
                get(2)?,
                a.get(3).map(String::as_str) == Some("--dry-run"),
            )
        }
        _ => Err("Unknown command; run help".into()),
    }
}
fn envelope(id: Value, r: Result<Value>) -> Value {
    match r {
        Ok(v) => json!({"id":id,"ok":true,"result":v}),
        Err(e) => json!({"id":id,"ok":false,"error":e}),
    }
}
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("serve") {
        for line in io::stdin().lock().lines() {
            let request = line
                .map_err(|e| e.to_string())
                .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()));
            let (id, result) = match request {
                Ok(v) => {
                    let id = v["id"].clone();
                    let r = v["args"]
                        .as_array()
                        .ok_or("args must be an array".into())
                        .and_then(|a| {
                            a.iter()
                                .map(|s| {
                                    s.as_str()
                                        .map(str::to_owned)
                                        .ok_or("Arguments must be strings".into())
                                })
                                .collect::<Result<Vec<_>>>()
                        })
                        .and_then(|a| dispatch(&a));
                    (id, r)
                }
                Err(e) => (Value::Null, Err(e)),
            };
            println!("{}", envelope(id, result));
        }
    } else {
        let result = dispatch(&args);
        let failed = result.is_err();
        println!("{}", envelope(Value::Null, result));
        if failed {
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project() -> Project {
        serde_json::from_value(dispatch(&["example".into()]).unwrap()).unwrap()
    }
    #[test]
    fn invalid_times_rejected() {
        let mut p = project();
        p.clips[0].duration = -1.0;
        assert!(validate(&p).is_err());
    }
    #[test]
    fn beat_warnings() {
        let mut p = project();
        assert_eq!(validate(&p).unwrap().len(), 1);
        p.clips[0].duration = 2.24;
        assert!(validate(&p).unwrap().len() > 1);
    }
    #[test]
    fn music_mismatch() {
        let mut p = project();
        p.music.as_mut().unwrap().mood = "calm".into();
        assert!(validate(&p).unwrap().iter().any(|s| s.contains("mood")));
    }
    #[test]
    fn rejects_unknown_fields() {
        let mut v = serde_json::to_value(project()).unwrap();
        v["typo"] = json!(true);
        assert!(serde_json::from_value::<Project>(v).is_err());
    }
    #[test]
    fn empty_timeline() {
        let mut p = project();
        p.clips.clear();
        assert!(validate(&p).is_err());
    }
}
