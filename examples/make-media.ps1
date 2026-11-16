# Genera los medios sinteticos para los ejemplos (requiere ffmpeg en PATH).
$ErrorActionPreference = 'Stop'
$dir = Join-Path $PSScriptRoot 'media'
New-Item -ItemType Directory -Force $dir | Out-Null
function Native([string]$program, [string[]]$arguments) {
  & $program @arguments
  if ($LASTEXITCODE -ne 0) { throw "Failed: $program" }
}
# Clips de prueba: patrones de video de 5 y 6 segundos.
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','testsrc2=s=1080x1920:r=30:d=5','-c:v','libx264','-pix_fmt','yuv420p',"$dir/hook.mp4")
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','smptebars=s=1280x720:r=30:d=6','-c:v','libx264','-pix_fmt','yuv420p',"$dir/product.mp4")
# Imagen fija.
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','color=c=steelblue:s=1080x1080','-frames:v','1',"$dir/product.png")
# Musica y locucion sinteticas (tonos puros; sustituir por material real con licencia).
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','sine=frequency=220:duration=10','-c:a','pcm_s16le',"$dir/music.wav")
Native 'ffmpeg' @('-hide_banner','-loglevel','error','-y','-f','lavfi','-i','sine=frequency=700:duration=8','-c:a','pcm_s16le',"$dir/voice.wav")
Write-Output "Medios listos en $dir"
