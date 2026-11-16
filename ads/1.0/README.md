# VideoCraft 1.0 — Announcement Ads

The VideoCraft 1.0 announcement, produced **with VideoCraft itself** (dogfooding).
Five kinetic-typography scenes in the "data-story" Shorts style: one idea per beat,
giant numbers, a different background color per scene, everything driven by the
deterministic `vcSeek(t)` contract.

Four language versions: **Spanish (es), English (en), French (fr), Arabic (ar, RTL)**.

## Files

| Path | What it is |
|---|---|
| `scenes/{es,en,fr,ar}/*.html` | The 5 scenes per language (hook → json → numbers → result → cta) |
| `project-{es,en,fr,ar}.json` | One VideoCraft project per language (1080×1920, 30 fps, 14 s, cuts on the 120 BPM grid) |
| `voice.wav` / `voice-en.wav` / `voice-fr.wav` / `voice-ar.wav` | Neural voice-overs (edge-tts: `es-ES-ElviraNeural`, `en-US-JennyNeural`, `fr-FR-DeniseNeural`, `ar-EG-SalmaNeural`) |
| `sfx.wav` | Sound-effects bed used by en/fr/ar: whooshes on the cuts, booms on the beat, ticks on the counters, chime on the result, pop on the CTA (all synthesized with FFmpeg, see below) |
| `music.wav` | Synth music bed used by the Spanish version (kick + hats + bass + pad, FFmpeg synthesis) |
| `render/` | Final rendered MP4s, verified by the CLI at render time |

## Timeline (all languages)

| Scene | Time | Content |
|---|---|---|
| `hook` | 0.0–3.0 s | "How long does your AI take to edit a video?" (black/violet) |
| `json` | 3.0–6.0 s | "One JSON. One command." (code green, neon braces) |
| `numbers` | 6.0–9.0 s | Counters: **2** engines · **1** command · **0** GUIs (electric blue) |
| `result` | 9.0–11.5 s | "11 s later… REAL MP4" + verified chips (magenta) |
| `cta` | 11.5–14.0 s | Inverted cream background, green GitHub button |

## Re-render locally

```powershell
videocraft check   ads/1.0/project-en.json
videocraft render  ads/1.0/project-en.json  ad-en.mp4
```

Requires only FFmpeg + Edge/Chrome (the same ones the CLI already needs).
The CLI applies voice/music ducking (`sidechaincompress`), loudnorm and
verifies the output (dimensions, fps, codecs, duration).

## Sound design (no third-party material)

Every sound here is synthesized from scratch with FFmpeg `aevalsrc`/`sine`/`anoisesrc`
and mixed into one 14 s stereo bed:

- Boom on the opening beat and on every scene cut (55–66 Hz exponential decay).
- Filtered pink-noise whoosh rising into each cut.
- Ticks when each counter settles.
- Two-note chime when the result lands.
- Pop on the CTA button.

The Spanish version instead uses a synth music bed (`music.wav`, gain 0.35).

## Voice notes

Voices generated with [edge-tts](https://github.com/rany2/edge-tts) (Microsoft neural
voices, free, no API key) — an **online** service used only to generate these WAVs.
The VideoCraft CLI itself is 100% local; audio was produced offline afterwards.
Texts were shortened/accelerated (+12% to +30%) to fit the 14 s timeline.
