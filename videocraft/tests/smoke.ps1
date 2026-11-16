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
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','color=c=blue:s=320x240:r=24:d=2','-c:v','libx264',"$media/blue.mp4")
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','color=c=red:s=640x360:r=30:d=2','-c:v','libx264',"$media/red.mp4")
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','sine=frequency=220:duration=1','-c:a','pcm_s16le',"$media/music.wav")
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','sine=frequency=700:duration=2.5','-c:a','pcm_s16le',"$media/voice.wav")
$project = @{
 version=1; width=320; height=240; fps=30; mood='energetic'
 clips=@(@{file='blue.mp4';start=0.0;duration=1.0},@{file='red.mp4';start=0.5;duration=1.0})
 music=@{file='music.wav';bpm=120.0;mood='energetic';license='Synthetic test tone';gain=0.2}
 voice='voice.wav'
}
$project | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 "$media/project.json"
$dry = & $exe render "$media/project.json" "$media/dry.mp4" --dry-run | ConvertFrom-Json
if (!$dry.ok -or (Test-Path "$media/dry.mp4")) { throw 'Dry run failed' }
foreach ($mode in @('both','voice','music','silent')) {
  $copy = $project.Clone()
  if ($mode -eq 'voice' -or $mode -eq 'silent') { $copy.music=$null }
  if ($mode -eq 'music' -or $mode -eq 'silent') { $copy.voice=$null }
  $copy | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 "$media/$mode.json"
  $out = "$media/$mode-$([guid]::NewGuid().ToString('N')).mp4"
  $render = & $exe render "$media/$mode.json" $out | ConvertFrom-Json
  if (!$render.ok) { throw "Render failed: $($render.error)" }
  $streams = $render.result.media.streams
  $v = $streams | Where-Object codec_type -eq video
  if ($v.width -ne 320 -or $v.height -ne 240 -or $v.codec_name -ne 'h264') { throw 'Wrong video format' }
  $audio = @($streams | Where-Object codec_type -eq audio)
  if (($mode -eq 'silent' -and $audio.Count -ne 0) -or ($mode -ne 'silent' -and $audio.Count -ne 1)) { throw 'Wrong audio streams' }
  $repeat = & $exe render "$media/$mode.json" $out | ConvertFrom-Json
  if ($repeat.ok -or $LASTEXITCODE -ne 1) { throw 'Overwrite protection failed' }
}
$project.clips[0].duration=500.0
$project | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 "$media/invalid.json"
$bad = & $exe render "$media/invalid.json" "$media/invalid.mp4" --dry-run | ConvertFrom-Json
if ($bad.ok -or $LASTEXITCODE -ne 1) { throw 'Out of bounds accepted' }
$responses = @('{"id":1,"args":["skills","music"]}','not-json','{"id":3,"args":["doctor"]}') | & $exe serve
$parsed = @($responses | ForEach-Object { $_ | ConvertFrom-Json })
if ($parsed.Count -ne 3 -or !$parsed[0].ok -or $parsed[1].ok -or !$parsed[2].ok -or $parsed[2].id -ne 3) { throw 'Serve recovery failed' }
Write-Output 'PASS: four render/audio modes, dry run, bounds validation, overwrite protection and JSONL error recovery'

