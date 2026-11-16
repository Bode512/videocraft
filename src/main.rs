//! VideoCraft: an agent-first CLI that turns a JSON project into an MP4.
//!
//! Two engines share one JSON protocol:
//! - assembly: trim/concat clips and images, mix music and voice via FFmpeg
//! - scenes: HTML pages rendered frame by frame by a headless browser
//!
//! Everything is local: no AI services, no API keys.

mod browser;
mod project;
mod render;

use project::Project;
use serde_json::{json, Value};
use std::{
    env, fs, io::{self, BufRead},
    process::Command,
};

type Result<T> = std::result::Result<T, String>;

fn read_project(path: &str) -> Result<Project> {
    let bytes = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_slice(project::strip_bom(&bytes)).map_err(|e| format!("Project JSON: {e}"))
}

fn example() -> Value {
    json!({
        "version": 1, "width": 1080, "height": 1920, "fps": 30, "mood": "energetic",
        "timeline": [
            {"kind": "scene", "file": "scenes/intro.html", "duration": 2.0},
            {"kind": "clip", "file": "hook.mp4", "start": 0.0, "duration": 2.0},
            {"kind": "image", "file": "product.png", "duration": 1.0},
            {"kind": "clip", "file": "product.mp4", "start": 0.0, "duration": 3.0},
            {"kind": "scene", "file": "scenes/cta.html", "duration": 2.0}
        ],
        "music": {"file": "music.wav", "bpm": 120.0, "mood": "energetic",
                  "license": "REPLACE with the real license or ownership", "gain": 0.25},
        "voice": "voice.wav"
    })
}

fn example_media() -> Value {
    let mut v = example();
    v["timeline"] = json!([
        {"kind": "scene", "file": "scenes/intro.html", "duration": 0.0},
        {"kind": "clip", "file": "hook.mp4", "start": 0.0, "duration": 2.0},
        {"kind": "clip", "file": "product.mp4", "start": 0.0, "duration": 4.0},
        {"kind": "image", "file": "product.png", "duration": 1.0},
        {"kind": "scene", "file": "scenes/cta.html", "duration": 0.0}
    ]);
    // Remove the zero-length placeholders to keep it a pure media example.
    let items = v["timeline"].as_array_mut().unwrap();
    items.retain(|i| i["duration"].as_f64().unwrap_or(0.0) > 0.0);
    v
}

fn example_html() -> Value {
    let mut v = example();
    v["timeline"] = json!([
        {"kind": "scene", "file": "scenes/intro.html", "duration": 3.0},
        {"kind": "scene", "file": "scenes/cta.html", "duration": 2.0}
    ]);
    v
}

fn skill(name: &str) -> Result<&'static str> {
    match name {
        "ads" => Ok(include_str!("../skills/ads.md")),
        "music" => Ok(include_str!("../skills/music.md")),
        "quality" => Ok(include_str!("../skills/quality.md")),
        "web" => Ok(include_str!("../skills/web.md")),
        _ => Err("Unknown skill; use ads, music, quality or web".into()),
    }
}

/// Static scene-contract review used by `check`: does the HTML mention vcSeek?
fn scene_warnings(p: &Project, base: &std::path::Path) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Ok(segments) = p.segments() {
        for s in segments.iter().filter(|s| s.kind() == "scene") {
            let file = s.file();
            let html = fs::read_to_string(base.join(file)).unwrap_or_default();
            if html.is_empty() {
                warnings.push(format!(
                    "Scene {file} is empty or unreadable; check runs before render resolves paths"
                ));
            } else if !html.contains("vcSeek") {
                warnings.push(format!(
                    "Scene {file} does not mention vcSeek; render will fail until it defines \
                     window.vcSeek(t) (see skills web)"
                ));
            }
        }
    }
    warnings
}

fn help() -> Value {
    json!({
        "name": "videocraft",
        "version": env!("CARGO_PKG_VERSION"),
        "role": "Local video editing for external AI agents. Assembly and HTML scene engines, rendered with FFmpeg. No model, no API key, no network services.",
        "protocol": "One JSON object per invocation: {id, ok, result} or {id, ok, error}. Commands exit 0 on ok and 1 on error. `serve` reads one request per line ({id, args:[command, ...]}) and answers one response per line.",
        "commands": [
            {"name": "doctor", "description": "Check dependencies: ffmpeg, ffprobe and a Chromium browser for scenes"},
            {"name": "example [mixed|media|html]", "description": "Print a full valid project JSON to edit and save"},
            {"name": "skills [ads|music|quality|web]", "description": "Built-in agent instructions; `web` documents the HTML scene contract"},
            {"name": "probe FILE", "description": "Inspect any media file: streams, durations, dimensions (ffprobe JSON)"},
            {"name": "check PROJECT.json", "description": "Validate a project without media access; returns warnings and total duration"},
            {"name": "render PROJECT.json OUTPUT.mp4 [--dry-run]", "description": "Preview the edit plan (--dry-run) or render the final MP4 and verify duration, size, fps and codecs"},
            {"name": "serve", "description": "JSONL request/response mode for long-lived agent sessions"}
        ],
        "engines": "Timeline item kinds: clip (video range), image (still), scene (HTML page; must define window.vcSeek(t) and is rendered frame by frame by a headless browser).",
        "dependencies": "FFmpeg and ffprobe in PATH for all rendering. A Chromium-based browser (Edge or Chrome) only for scene items; set VIDEOCRAFT_BROWSER to point at one.",
        "project": "Run `example` for a full project. Media paths resolve relative to the project file; outputs are never overwritten."
    })
}

fn doctor() -> Value {
    let mut tools = serde_json::Map::new();
    for tool in ["ffmpeg", "ffprobe"] {
        let ok = Command::new(tool)
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success());
        tools.insert(tool.into(), json!(ok));
    }
    let browser = match browser::find_browser() {
        Some(b) => json!({"found": true, "name": b.name, "path": b.path.to_string_lossy()}),
        None => json!({"found": false, "hint": "Install Microsoft Edge or Google Chrome, or set VIDEOCRAFT_BROWSER to a Chromium executable. Only needed for scene items."}),
    };
    json!({"dependencies": tools, "browser": browser})
}

fn check(path: &str) -> Result<Value> {
    let p = read_project(path)?;
    let mut warnings = project::validate(&p)?;
    let base = std::path::Path::new(path).parent().unwrap_or(std::path::Path::new("."));
    warnings.extend(scene_warnings(&p, base));
    let total: f64 = p.segments()?.iter().map(|s| s.duration()).sum();
    Ok(json!({
        "warnings": warnings,
        "duration": total,
        "scope": "Static validation only: no media probing, no browser. `render --dry-run` also probes the media files and checks the browser."
    }))
}

fn dispatch(a: &[String]) -> Result<Value> {
    let get = |n: usize| {
        a.get(n)
            .map(String::as_str)
            .ok_or_else(|| "Missing argument; run help".to_string())
    };
    match a.first().map(String::as_str).unwrap_or("help") {
        "help" | "--help" | "-h" => Ok(help()),
        "--version" | "version" => Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "protocol": 1
        })),
        "doctor" => Ok(doctor()),
        "example" => Ok(match a.get(1).map(String::as_str).unwrap_or("mixed") {
            "media" => example_media(),
            "html" => example_html(),
            "mixed" => example(),
            other => return Err(format!("Unknown example '{other}'; use mixed, media or html")),
        }),
        "skills" => {
            if a.len() == 1 {
                Ok(json!(["ads", "music", "quality", "web"]))
            } else {
                Ok(json!({"name": get(1)?, "instructions": skill(get(1)?)?}))
            }
        }
        "probe" => {
            let path = get(1)?;
            let p = std::path::Path::new(path);
            if !p.is_file() {
                return Err(format!("{path}: not a file"));
            }
            render::probe(p)
        }
        "check" => check(get(1)?),
        "render" => {
            if a.len() > 4 || a.get(3).is_some_and(|x| x != "--dry-run") {
                return Err("Usage: render PROJECT.json OUTPUT.mp4 [--dry-run]".into());
            }
            render::render(
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
        Ok(v) => json!({"id": id, "ok": true, "result": v}),
        Err(e) => json!({"id": id, "ok": false, "error": e}),
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

    fn project_from_example() -> Project {
        // dispatch returns the bare command result; the envelope is added in main().
        let v = dispatch(&["example".into()]).unwrap();
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn examples_parse_as_projects() {
        for name in ["mixed", "media", "html"] {
            let v = dispatch(&["example".into(), name.into()]).unwrap();
            let p: Project = serde_json::from_value(v).unwrap();
            assert!(project::validate(&p).is_ok(), "example {name} must validate");
        }
    }

    #[test]
    fn help_lists_all_commands() {
        let h = help();
        let names: Vec<&str> = h["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        for cmd in ["doctor", "example", "skills", "probe", "check", "render", "serve"] {
            assert!(names.iter().any(|n| n.starts_with(cmd)), "missing {cmd}");
        }
        assert!(h["engines"].as_str().unwrap().contains("vcSeek"));
    }

    #[test]
    fn check_reports_scene_contract() {
        let p = project_from_example();
        let warnings = scene_warnings(&p, std::path::Path::new("."));
        assert!(
            warnings.iter().any(|w| w.contains("vcSeek") || w.contains("unreadable")),
            "missing scenes must warn about the contract"
        );
    }

    #[test]
    fn skills_cover_four_topics() {
        assert!(skill("ads").unwrap().contains("anuncio") || skill("ads").unwrap().contains("ad"));
        assert!(skill("web").unwrap().contains("vcSeek"));
        assert!(skill("quality").is_ok());
        assert!(skill("music").is_ok());
        assert!(skill("nope").is_err());
    }

    #[test]
    fn unknown_command_and_missing_args_are_clear_errors() {
        assert!(dispatch(&["nope".into()]).unwrap_err().contains("help"));
        assert!(dispatch(&["probe".into()]).unwrap_err().contains("help"));
    }

    #[test]
    fn envelope_shape() {
        let ok = envelope(json!(7), Ok(json!({"a": 1})));
        assert_eq!(ok["id"], 7);
        assert_eq!(ok["ok"], true);
        let err = envelope(Value::Null, Err("boom".into()));
        assert_eq!(err["ok"], false);
        assert_eq!(err["error"], "boom");
    }
}
