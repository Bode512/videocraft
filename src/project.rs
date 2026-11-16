//! Project JSON schema (version 1) and validation rules.
//!
//! The schema is additive-stable: fields are only added, never removed or
//! redefined. Unknown fields are rejected so agents notice typos.

use serde::{Deserialize, Serialize};

/// Strip a UTF-8 BOM if present: Windows tools often write JSON with one.
pub fn strip_bom(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    }
}

pub const VERSION: u32 = 1;
pub const MAX_ITEMS: usize = 100;
pub const MAX_TOTAL_DURATION: f64 = 3600.0;
pub const MIN_ITEM_DURATION: f64 = 0.1;

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Music {
    pub file: String,
    pub bpm: f64,
    pub mood: String,
    pub license: String,
    pub gain: f64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub file: String,
    pub start: f64,
    pub duration: f64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub file: String,
    pub duration: f64,
}

/// One timeline entry. `kind` selects the engine:
/// - "clip":  range of a video file (FFmpeg assembly engine)
/// - "image": still image shown for `duration` seconds (FFmpeg assembly engine)
/// - "scene": HTML page rendered frame by frame by a headless browser
#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub kind: String,
    pub file: String,
    #[serde(default)]
    pub start: f64,
    pub duration: f64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub mood: String,
    /// Canonical mixed timeline. When present, `clips`/`scenes` must be absent.
    #[serde(default)]
    pub timeline: Option<Vec<Item>>,
    /// Legacy/simple form: video clips. Used only when `timeline` is absent.
    #[serde(default)]
    pub clips: Option<Vec<Clip>>,
    /// Legacy/simple form: HTML scenes. Used only when `timeline` is absent
    /// and they run before the clips.
    #[serde(default)]
    pub scenes: Option<Vec<Scene>>,
    pub music: Option<Music>,
    pub voice: Option<String>,
}

/// A validated timeline segment with its engine resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Clip { file: String, start: f64, duration: f64 },
    Image { file: String, duration: f64 },
    Scene { file: String, duration: f64 },
}

impl Segment {
    pub fn kind(&self) -> &'static str {
        match self {
            Segment::Clip { .. } => "clip",
            Segment::Image { .. } => "image",
            Segment::Scene { .. } => "scene",
        }
    }
    pub fn file(&self) -> &str {
        match self {
            Segment::Clip { file, .. }
            | Segment::Image { file, .. }
            | Segment::Scene { file, .. } => file,
        }
    }
    pub fn duration(&self) -> f64 {
        match self {
            Segment::Clip { duration, .. }
            | Segment::Image { duration, .. }
            | Segment::Scene { duration, .. } => *duration,
        }
    }
}

impl Project {
    /// Resolve the timeline in playback order. Accepts either the canonical
    /// `timeline` array or the simple `clips`/`scenes` arrays (scenes first).
    pub fn segments(&self) -> Result<Vec<Segment>, String> {
        if self.version != VERSION {
            return Err(format!(
                "Unsupported project version {}: use version {VERSION}",
                self.version
            ));
        }
        let segments = if let Some(items) = &self.timeline {
            if self.clips.is_some() || self.scenes.is_some() {
                return Err("Use timeline, or clips/scenes, not both".into());
            }
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let seg = match item.kind.as_str() {
                    "clip" => Segment::Clip {
                        file: item.file.clone(),
                        start: item.start,
                        duration: item.duration,
                    },
                    "image" => Segment::Image {
                        file: item.file.clone(),
                        duration: item.duration,
                    },
                    "scene" => Segment::Scene {
                        file: item.file.clone(),
                        duration: item.duration,
                    },
                    other => {
                        return Err(format!(
                            "Unknown timeline kind '{other}'; use clip, image or scene"
                        ))
                    }
                };
                if !matches!(item.kind.as_str(), "clip") && item.start != 0.0 {
                    return Err(format!(
                        "Timeline kind '{}' does not use start; only clip items seek the source",
                        item.kind
                    ));
                }
                out.push(seg);
            }
            out
        } else {
            let mut out = Vec::new();
            for s in self.scenes.as_deref().unwrap_or(&[]) {
                out.push(Segment::Scene {
                    file: s.file.clone(),
                    duration: s.duration,
                });
            }
            for c in self.clips.as_deref().unwrap_or(&[]) {
                out.push(Segment::Clip {
                    file: c.file.clone(),
                    start: c.start,
                    duration: c.duration,
                });
            }
            if self.clips.is_none() && self.scenes.is_none() {
                return Err("Add a timeline (clip/image/scene items) or clips to the project".into());
            }
            out
        };
        Ok(segments)
    }
}

fn finite_duration(name: &str, index: usize, d: f64) -> Result<(), String> {
    if !d.is_finite() || d < MIN_ITEM_DURATION {
        return Err(format!(
            "Item {index} ({name}): duration must be a finite number >= {MIN_ITEM_DURATION}"
        ));
    }
    Ok(())
}

/// Static validation. Returns warnings; media files are inspected later by
/// render (probe) because `check` must work without FFmpeg knowing more.
pub fn validate(p: &Project) -> Result<Vec<String>, String> {
    let segments = p.segments()?;
    if p.width < 16
        || p.height < 16
        || p.width > 3840
        || p.height > 3840
        || p.width % 2 != 0
        || p.height % 2 != 0
        || !(1..=60).contains(&p.fps)
    {
        return Err("Use even dimensions 16..3840 and fps 1..60".into());
    }
    if segments.is_empty() || segments.len() > MAX_ITEMS {
        return Err(format!("Use 1..{MAX_ITEMS} timeline items"));
    }
    let mut total = 0.0;
    for (i, s) in segments.iter().enumerate() {
        finite_duration(s.kind(), i, s.duration())?;
        if let Segment::Clip { start, .. } = s {
            if !start.is_finite() || *start < 0.0 {
                return Err(format!("Item {i} (clip): start must be a finite number >= 0"));
            }
        }
        total += s.duration();
    }
    if total > MAX_TOTAL_DURATION {
        return Err(format!("Total duration exceeds {MAX_TOTAL_DURATION:.0} seconds"));
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
        for s in segments.iter().take(segments.len() - 1) {
            t += s.duration();
            let beat = t * m.bpm / 60.0;
            if (beat - beat.round()).abs() > 0.15 {
                warnings.push(format!("Cut at {t:.2}s is off the declared beat grid"));
            }
        }
    } else {
        warnings.push("No music selected".into());
    }
    if p.voice.is_none() {
        warnings.push("No voice track; if the ad needs spoken message, provide a voice file".into());
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base() -> serde_json::Value {
        json!({
            "version": 1, "width": 320, "height": 240, "fps": 30, "mood": "energetic",
            "clips": [{"file":"a.mp4","start":0.0,"duration":1.0}],
            "music": {"file":"m.wav","bpm":120.0,"mood":"energetic","license":"test","gain":0.2}
        })
    }

    fn parse(v: serde_json::Value) -> Project {
        serde_json::from_value(v).expect("project must parse")
    }

    #[test]
    fn bom_is_stripped() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"{"version":1}"#);
        assert!(serde_json::from_slice::<serde_json::Value>(strip_bom(&bytes)).is_ok());
        assert_eq!(strip_bom(b"{}"), b"{}");
    }

    #[test]
    fn legacy_clips_still_parse() {
        let p = parse(base());
        let segs = p.segments().unwrap();
        assert_eq!(segs.len(), 1);
        assert!(matches!(segs[0], Segment::Clip { .. }));
        assert!(validate(&p).is_ok());
    }

    #[test]
    fn timeline_mixed_order_is_preserved() {
        let mut v = base();
        v["timeline"] = json!([
            {"kind":"scene","file":"intro.html","duration":2.0},
            {"kind":"clip","file":"a.mp4","start":0.5,"duration":1.0},
            {"kind":"image","file":"still.png","duration":0.5},
            {"kind":"scene","file":"cta.html","duration":1.5}
        ]);
        v["clips"] = serde_json::Value::Null;
        let segs = parse(v).segments().unwrap();
        assert_eq!(segs.iter().map(|s| s.kind()).collect::<Vec<_>>(), ["scene", "clip", "image", "scene"]);
    }

    #[test]
    fn scenes_run_before_clips_in_simple_form() {
        let mut v = base();
        v["scenes"] = json!([{"file":"intro.html","duration":1.0}]);
        let segs = parse(v).segments().unwrap();
        assert_eq!(segs[0].kind(), "scene");
        assert_eq!(segs[1].kind(), "clip");
    }

    #[test]
    fn timeline_and_clips_are_mutually_exclusive() {
        let mut v = base();
        v["timeline"] = json!([{"kind":"clip","file":"a.mp4","duration":1.0}]);
        assert!(parse(v).segments().is_err());
    }

    #[test]
    fn unknown_kind_rejected() {
        let mut v = base();
        v["clips"] = serde_json::Value::Null;
        v["timeline"] = json!([{"kind":"sticker","file":"x.png","duration":1.0}]);
        assert!(parse(v).segments().unwrap_err().contains("Unknown timeline kind"));
    }

    #[test]
    fn scene_with_start_rejected() {
        let mut v = base();
        v["clips"] = serde_json::Value::Null;
        v["timeline"] = json!([{"kind":"scene","file":"a.html","start":1.0,"duration":1.0}]);
        assert!(parse(v).segments().unwrap_err().contains("start"));
    }

    #[test]
    fn unknown_field_rejected() {
        let mut v = base();
        v["typo"] = json!(true);
        assert!(serde_json::from_value::<Project>(v).is_err());
    }

    #[test]
    fn invalid_times_rejected() {
        let mut v = base();
        v["clips"][0]["duration"] = json!(-1.0);
        assert!(validate(&parse(v)).is_err());
    }

    #[test]
    fn short_duration_rejected() {
        let mut v = base();
        v["clips"][0]["duration"] = json!(0.05);
        assert!(validate(&parse(v)).is_err());
    }

    #[test]
    fn beat_warnings() {
        let mut v = base();
        v["clips"][0]["duration"] = json!(2.24);
        assert!(validate(&parse(v)).unwrap().len() > 1);
    }

    #[test]
    fn music_mismatch_warns() {
        let mut v = base();
        v["music"]["mood"] = json!("calm");
        assert!(validate(&parse(v)).unwrap().iter().any(|s| s.contains("mood")));
    }

    #[test]
    fn empty_timeline_rejected() {
        let mut v = base();
        v["clips"] = json!([]);
        assert!(validate(&parse(v)).is_err());
    }

    #[test]
    fn total_over_one_hour_rejected() {
        let mut v = base();
        v["clips"][0]["duration"] = json!(3601.0);
        assert!(validate(&parse(v)).is_err());
    }

    #[test]
    fn music_rules() {
        let mut v = base();
        v["music"]["bpm"] = json!(5.0);
        assert!(validate(&parse(v)).is_err());
        let mut v = base();
        v["music"]["license"] = json!("");
        assert!(validate(&parse(v)).is_err());
    }
}
