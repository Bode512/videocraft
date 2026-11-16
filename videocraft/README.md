# VideoCraft CLI

Herramienta Rust sin interfaz grafica para agentes de IA. Recorta y concatena video, adapta el encuadre con recorte central, mezcla musica y locucion, reduce musica durante la voz y exporta MP4 H.264/AAC.

## Uso

Requiere ffmpeg y ffprobe en PATH (https://ffmpeg.org/download.html). El exe no incluye FFmpeg ni un modelo de IA. Compatible con una IA que pueda ejecutar comandos locales.

```powershell
videocraft.exe help
videocraft.exe doctor
videocraft.exe skills ads
videocraft.exe example
videocraft.exe check project.json
videocraft.exe render project.json anuncio.mp4 --dry-run
videocraft.exe render project.json anuncio.mp4
videocraft.exe serve
```

Todos los resultados usan JSON: {id,ok,result} o {id,ok,error}. Los errores de comandos terminan con codigo 1. Los mensajes de FFmpeg van a stderr. En serve, enviar un objeto por linea: `{"id":1,"args":["skills","music"]}`. No es MCP ni un chat integrado: el agente externo interpreta el lenguaje natural y llama a esta herramienta.

Guardar el campo result de example como project.json, cambiar las rutas por archivos reales y sustituir la licencia de ejemplo. Las rutas de medios se resuelven respecto al archivo de proyecto; la ruta de salida respecto al directorio de ejecucion. No se sobrescriben salidas. Para iterar, elegir otro nombre.

## Contrato del proyecto v1

- version: 1; width/height: pares entre 16 y 3840; fps: 1 a 60.
- mood: etiqueta descriptiva.
- clips: lista de {file,start,duration}, segundos; maximo 100 clips y 1 hora.
- music: null o {file,bpm,mood,license,gain}; BPM 20 a 300 y gain 0 a 1.
- voice: null o ruta de locucion.

El audio de los clips se descarta. La musica corta se repite. La locucion corta se rellena con silencio; el audio largo se recorta. Los cortes son directos. Esta version no incluye subtitulos, overlays, generacion de imagen/video/musica, reconocimiento semantico ni deteccion automatica de BPM. Usar clips ya rotulados si se necesita texto.

Skills incorporadas en el exe: ads, music, quality. Son instrucciones consultables por la IA, no plugins instalados en ChatGPT. Los avisos de ritmo usan una cuadricula BPM que empieza en cero; no analizan el audio. Revisar visualmente y escuchar antes de entregar: ninguna puntuacion automatica garantiza calidad creativa.

## Desarrollo y release

```powershell
cargo test --locked
cargo build --release --locked
```

El workflow de GitHub compila Windows, ejecuta pruebas y adjunta el exe y su SHA256 al publicar un tag v*. FFmpeg se instala por separado y conserva su licencia correspondiente. LICENSE cubre solo el codigo VideoCraft.

Filtros de referencia: https://ffmpeg.org/ffmpeg-filters.html

