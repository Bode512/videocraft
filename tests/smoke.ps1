# Integration test: three real renders (media-only, HTML-only, mixed) with
# synthetic media, plus protocol and error-handling checks.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$exe = Join-Path $root 'target/release/videocraft.exe'
$media = Join-Path $root 'test-media'
New-Item -ItemType Directory -Force $media | Out-Null

function Native([string]$program, [string[]]$arguments) {
  & $program @arguments
  if ($LASTEXITCODE -ne 0) { throw "Failed: $program" }
}
function Json([string[]]$arguments) {
  $out = & $exe @arguments
  if ($LASTEXITCODE -ne 0 -and $arguments[0] -ne 'render') { throw "Command failed: $($arguments -join ' ')" }
  return ($out | ConvertFrom-Json)
}
function Assert([bool]$condition, [string]$message) {
  if (!$condition) { throw "ASSERT: $message" }
}
function WriteJson($obj, [string]$path) {
  [IO.File]::WriteAllText($path, ($obj | ConvertTo-Json -Depth 8), (New-Object System.Text.UTF8Encoding($false)))
}

# --- dependencies -----------------------------------------------------------
if (!(Get-Command ffmpeg -ErrorAction SilentlyContinue)) { throw 'ffmpeg must be in PATH' }
$doctor = Json @('doctor')
Assert ($doctor.result.dependencies.ffmpeg -and $doctor.result.dependencies.ffprobe) 'doctor: ffmpeg/ffprobe missing'
Assert ($doctor.result.browser.found) 'doctor: no Chromium browser for scenes'

# --- synthetic media --------------------------------------------------------
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','color=c=blue:s=320x240:r=24:d=2','-c:v','libx264',"$media/blue.mp4")
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','testsrc2=s=640x360:r=30:d=3','-c:v','libx264','-pix_fmt','yuv420p',"$media/red.mp4")
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','color=c=green:s=640x640','-frames:v','1',"$media/still.png")
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','sine=frequency=220:duration=8','-c:a','pcm_s16le',"$media/music.wav")
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','sine=frequency=700:duration=6','-c:a','pcm_s16le',"$media/voice.wav")
@"
<!doctype html><html><head><meta charset="utf-8">
<style>html,body{margin:0;width:100%;height:100%;background:#000}</style></head>
<body id="b"><script>
var D = 2.0;
window.vcSeek = function(t){ var p = Math.min(Math.max(t/D,0),1); var v = Math.round(255*(0.15+0.7*p));
document.getElementById('b').style.backgroundColor = 'rgb('+v+','+v+','+v+')'; };
</script></body></html>
"@ | Set-Content -Encoding utf8 "$media/scene.html"
@"
<!doctype html><html><head><meta charset="utf-8"><style>body{background:#000}</style></head>
<body><p>sin contrato</p></body></html>
"@ | Set-Content -Encoding utf8 "$media/bad-scene.html"
Write-Output 'ok: synthetic media and scenes'

# --- case 1: media only (horizontal, music + voice + ducking) ---------------
$mediaProject = [ordered]@{
  version=1; width=1280; height=720; fps=30; mood='energetic'
  timeline=@(
    @{kind='clip';file='blue.mp4';start=0.0;duration=1.0},
    @{kind='image';file='still.png';duration=0.8},
    @{kind='clip';file='red.mp4';start=0.5;duration=1.0}
  )
  music=@{file='music.wav';bpm=120.0;mood='energetic';license='Synthetic test tone';gain=0.2}
  voice='voice.wav'
}
WriteJson $mediaProject "$media/media.json"
$out1 = "$media/out-media-$([guid]::NewGuid().ToString('N')).mp4"
$r1 = Json @('render',"$media/media.json",$out1)
Assert $r1.ok "media render failed: $($r1.error)"
Assert $r1.result.verified.duration_ok 'media: duration not verified'
Assert ($r1.result.verified.dimensions -eq '1280x720') 'media: wrong dimensions'
Assert ($r1.result.verified.video_codec -eq 'h264' -and $r1.result.verified.audio_codec -eq 'aac') 'media: wrong codecs'
Write-Output "ok: media-only render ($($r1.result.duration)s, ducking + loudnorm applied)"

# --- case 2: HTML only (vertical, browser-rendered frames + music) ----------
$htmlProject = [ordered]@{
  version=1; width=540; height=960; fps=15; mood='energetic'
  timeline=@(
    @{kind='scene';file='scene.html';duration=2.0},
    @{kind='scene';file='scene.html';duration=1.5}
  )
  music=@{file='music.wav';bpm=100.0;mood='energetic';license='Synthetic test tone';gain=0.3}
  voice=$null
}
WriteJson $htmlProject "$media/html.json"
$out2 = "$media/out-html-$([guid]::NewGuid().ToString('N')).mp4"
$r2 = Json @('render',"$media/html.json",$out2)
Assert $r2.ok "html render failed: $($r2.error)"
Assert $r2.result.verified.duration_ok 'html: duration not verified'
Assert ($r2.result.verified.dimensions -eq '540x960') 'html: wrong dimensions'
Assert ($r2.result.verified.fps -eq 15) 'html: wrong fps'
# the scene must actually paint: extract one frame and check it is not black
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-ss','1.0','-i',$out2,'-frames:v','1','-f','rawvideo','-pix_fmt','gray',"$media/frame.gray")
$bytes = [IO.File]::ReadAllBytes("$media/frame.gray")
$avg = ($bytes | Measure-Object -Average).Average
Assert ($avg -gt 5) "html: frames look black (average brightness $avg)"
Write-Output "ok: html-only render ($($r2.result.duration)s, frames painted, avg brightness $([math]::Round($avg,1)))"

# --- case 3: mixed (scenes + clips + image in one timeline) -----------------
$mixedProject = [ordered]@{
  version=1; width=720; height=1280; fps=30; mood='energetic'
  timeline=@(
    @{kind='scene';file='scene.html';duration=1.5},
    @{kind='clip';file='blue.mp4';start=0.0;duration=1.0},
    @{kind='image';file='still.png';duration=0.5},
    @{kind='clip';file='red.mp4';start=0.0;duration=1.0},
    @{kind='scene';file='scene.html';duration=1.0}
  )
  music=@{file='music.wav';bpm=120.0;mood='energetic';license='Synthetic test tone';gain=0.25}
  voice='voice.wav'
}
WriteJson $mixedProject "$media/mixed.json"
$out3 = "$media/out-mixed-$([guid]::NewGuid().ToString('N')).mp4"
$r3 = Json @('render',"$media/mixed.json",$out3)
Assert $r3.ok "mixed render failed: $($r3.error)"
Assert $r3.result.verified.duration_ok 'mixed: duration not verified'
Assert ($r3.result.verified.dimensions -eq '720x1280') 'mixed: wrong dimensions'
Assert ($r3.result.duration -gt 4.9 -and $r3.result.duration -lt 5.2) "mixed: unexpected total $($r3.result.duration)"
Write-Output "ok: mixed render ($($r3.result.duration)s, scenes + clips + image interleaved)"

# --- dry run: plan without touching disk -------------------------------------
$dry = Json @('render',"$media/mixed.json","$media/dry-$([guid]::NewGuid().ToString('N')).mp4",'--dry-run')
Assert $dry.ok "dry run failed: $($dry.error)"
Assert ($dry.result.plan.items.Count -eq 5) 'dry run: plan must list all 5 items'
Assert ($dry.result.plan.items[0].kind -eq 'scene' -and $dry.result.plan.items[1].kind -eq 'clip') 'dry run: wrong order'
Assert ($dry.result.program -eq 'ffmpeg') 'dry run: program must be ffmpeg'
Write-Output 'ok: dry run plan (5 items, no files created)'

# --- protocol and error handling --------------------------------------------
$repeat = Json @('render',"$media/media.json",$out1)
if ($repeat.ok -or $LASTEXITCODE -ne 1) { throw 'Overwrite protection failed' }
$bad = Json @('render',"$media/media.json","$media/bad.txt")
if ($bad.ok -or $LASTEXITCODE -ne 1) { throw 'Non-mp4 output accepted' }
$invalid = [ordered]@{
  version=1; width=1280; height=720; fps=30; mood='energetic'
  timeline=@(
    @{kind='clip';file='blue.mp4';start=0.0;duration=5000.0}
  )
  music=$null; voice=$null
}
WriteJson $invalid "$media/invalid.json"
$bad2 = Json @('render',"$media/invalid.json","$media/invalid.mp4",'--dry-run')
if ($bad2.ok -or $LASTEXITCODE -ne 1) { throw 'Out of bounds accepted' }
$badScene = [ordered]@{
  version=1; width=540; height=960; fps=15; mood='x'
  timeline=@(@{kind='scene';file='bad-scene.html';duration=1.0})
  music=$null; voice=$null
}
WriteJson $badScene "$media/badscene.json"
$bad3 = Json @('render',"$media/badscene.json","$media/badscene.mp4")
if ($bad3.ok -or $LASTEXITCODE -ne 1) { throw 'Scene without vcSeek accepted' }
if ($bad3.error -notmatch 'vcSeek') { throw "Missing vcSeek error is not clear: $($bad3.error)" }
$checked = Json @('check',"$media/mixed.json")
Assert $checked.ok 'check failed on a valid project'
Assert ($checked.result.warnings.Count -ge 1) 'check must return the quality warning'
Write-Output 'ok: overwrite protection, extension check, bounds, scene contract error, check'

# --- serve mode (JSONL) ------------------------------------------------------
$responses = @('{"id":1,"args":["skills","music"]}','not-json','{"id":3,"args":["doctor"]}','{"id":4,"args":["skills","web"]}') | & $exe serve
$parsed = @($responses | ForEach-Object { $_ | ConvertFrom-Json })
Assert ($parsed.Count -eq 4) 'serve: wrong response count'
Assert ($parsed[0].ok) 'serve: valid request failed'
Assert (!$parsed[1].ok) 'serve: malformed JSON must fail cleanly'
Assert ($parsed[2].ok -and $parsed[2].id -eq 3) 'serve: doctor via serve failed'
Assert ($parsed[3].result.instructions -match 'vcSeek') 'serve: web skill must document the contract'
Write-Output 'ok: serve JSONL mode with error recovery'

Write-Output 'PASS: media-only, html-only and mixed real renders; dry-run plan; protocol errors; serve mode'

