# VideoCraft CLI

Herramienta de línea de comandos en Rust para que agentes de IA produzcan y editen vídeos y anuncios sin interfaz gráfica. La IA escribe un proyecto JSON; VideoCraft lo valida, muestra el plan de edición, renderiza con FFmpeg y comprueba el resultado.

Dos motores comparten el mismo protocolo JSON y pueden mezclarse en una misma línea de tiempo:

1. **Montaje multimedia (FFmpeg):** recorta y une clips, integra imágenes fijas, mezcla música y locución, baja automáticamente la música cuando habla la voz y normaliza el audio.
2. **Escenas HTML (navegador headless):** la IA escribe HTML/CSS/JS para textos animados, gráficos, composiciones de producto y transiciones. Un navegador Chromium automatizado renderiza cada escena fotograma a fotograma y FFmpeg las convierte en vídeo.

Todo el procesamiento es local. VideoCraft no incluye ningún modelo de IA, no llama a servicios de IA y no necesita claves ni cuentas. La calidad creativa depende del material y de la revisión humana: la validación técnica no garantiza un resultado «10/10».

## Requisitos

| Dependencia | Para qué | Cómo instalarla |
|---|---|---|
| FFmpeg y ffprobe en PATH | Todos los renders | <https://ffmpeg.org/download.html> o `choco install ffmpeg` |
| Navegador Chromium (Edge o Chrome) | Solo para items `scene` | Windows y macOS suelen traer Edge; en Linux, `chromium` o `chrome`. Se puede fijar uno concreto con la variable de entorno `VIDEOCRAFT_BROWSER` (ruta al ejecutable) |

`videocraft doctor` comprueba las tres dependencias y devuelve JSON.

Descarga el exe compilado para Windows x64 en la sección [Releases](https://github.com/Bode512/videocraft/releases); cada release incluye el binario y su SHA-256. El exe no incluye FFmpeg ni el navegador: instálalos por separado y respeta sus licencias.

## Comandos

```
videocraft help                          Ayuda completa en JSON
videocraft doctor                        Comprueba ffmpeg, ffprobe y el navegador
videocraft example [mixed|media|html]    Proyecto JSON completo listo para editar
videocraft skills [ads|music|quality|web]  Instrucciones integradas para la IA
videocraft probe ARCHIVO                 Duraciones, pistas y dimensiones de un medio
videocraft check PROYECTO.json           Valida el proyecto sin tocar medios
videocraft render PROYECTO.json SALIDA.mp4 [--dry-run]
videocraft serve                         Modo JSONL petición/respuesta para sesiones largas
```

Salida siempre JSON: `{"id":null,"ok":true,"result":{...}}` o `{"id":null,"ok":false,"error":"..."}`. Los errores terminan con código de salida 1; los mensajes de FFmpeg van a stderr. En `serve`, enviar un objeto por línea: `{"id":1,"args":["check","proyecto.json"]}` y se responde un objeto por línea.

Las rutas de medios se resuelven respecto al archivo de proyecto; la salida respecto al directorio de ejecución. Nunca se sobrescribe un archivo existente: para iterar, cambia el nombre de salida.

## Protocolo del proyecto (versión 1)

Ejecuta `videocraft example` para un proyecto completo. Estructura:

```json
{
  "version": 1,
  "width": 1080, "height": 1920, "fps": 30,
  "mood": "energetic",
  "timeline": [
    { "kind": "scene", "file": "scenes/intro.html", "duration": 2.0 },
    { "kind": "clip",  "file": "media/hook.mp4", "start": 0.0, "duration": 2.0 },
    { "kind": "image", "file": "media/product.png", "duration": 1.0 },
    { "kind": "scene", "file": "scenes/cta.html", "duration": 2.0 }
  ],
  "music": { "file": "media/music.wav", "bpm": 120.0, "mood": "energetic",
             "license": "Describe aquí la licencia o propiedad real", "gain": 0.25 },
  "voice": "media/voice.wav"
}
```

- `version`: 1. `width`/`height`: pares entre 16 y 3840 (vertical 1080x1920, horizontal 1920x1080). `fps`: 1 a 60.
- `timeline`: lista ordenada de items, máximo 100 y 1 hora en total.
  - `clip`: rango `{file, start, duration}` de un vídeo. Su audio original **se descarta**.
  - `image`: imagen `{file, duration}` mostrada con recorte central.
  - `scene`: página HTML `{file, duration}` renderizada por el navegador. Sin audio propio.
- Formas simples: en lugar de `timeline`, `clips` y/o `scenes` (las escenas van primero). No mezclar ambas formas.
- `music`: `{file, bpm, mood, license, gain}` con BPM 20–300 y gain 0–1. Se repite si es corta y tiene fundido final de 1 s.
- `voice`: ruta de la locución. Se rellena con silencio si es corta y se recorta si es larga.

### Audio automático

Con música y voz, VideoCraft aplica: recorte de música a la duración de la línea de tiempo, compresor lateral (ducking) que baja la música cuando entra la voz, y normalización loudnorm (I=-16, TP=-1.5, LRA=11) sobre la mezcla. El audio de los clips siempre se descarta: si el material original tiene diálogos, aporta una locución aparte.

## Motor de escenas HTML

Cada escena es un HTML autocompleto y **sin red** (sin CDNs ni recursos externos) que debe definir:

```js
window.vcSeek = function (t) { /* pinta el estado exacto del segundo t */ };
```

- `vcSeek(t)` es determinista: la misma `t` produce siempre el mismo fotograma. Todo el movimiento se calcula a partir de `t`; no uses `requestAnimationFrame`, `setInterval` ni animaciones CSS (VideoCraft las desactiva al cargar).
- El navegador se abre al tamaño del proyecto; diseña con unidades relativas (`vw`, `%`).
- Las escenas se cuantizan a fotogramas: 2.0 s a 30 fps son 60 fotogramas exactos.
- No llevan audio: la música y la locución son las únicas fuentes de sonido.

`videocraft skills web` incluye el contrato completo y una plantilla. Los ejemplos de `examples/scenes/` son puntos de partida funcionales.

## Qué es automático y qué requiere revisión humana

**Decide y lo hace VideoCraft (automático):**
- Escalar y recortar cada clip/imagen/escena al encuadre del proyecto, fps uniformes, unión en orden, `yuv420p`, H.264 CRF 18 + AAC 192k, `+faststart`.
- Bucle de música, recorte/pad de la voz, ducking de música bajo la voz, fundido final de la música, loudnorm de la mezcla.
- Validación de límites (dimensiones, fps, duraciones, campos desconocidos, licencia declarada, coherencia de `mood`).
- Verificación del archivo renderizado: duración ±0.25 s, dimensiones exactas, fps, H.264 y AAC esperados. Si falla, el render devuelve error en vez de entregar un archivo roto.
- Avisos de ritmo: los cortes se comparan con la cuadrícula BPM declarada (que asume el primer beat en el segundo 0; no analiza el audio).

**Requiere revisión humana (no lo garantiza la herramienta):**
- Elección creativa de música, ritmo, estructura del anuncio y textos. La CLI compara etiquetas; no entiende el contenido de audio ni detecta BPM reales.
- La licencia de la música: el campo `license` debe declararse con datos reales.
- Ortografía, legibilidad y veracidad de los textos de las escenas HTML.
- El visionado final: VideoCraft no puede ver ni escuchar el resultado. Ninguna puntuación automática garantiza calidad creativa; la skill `quality` enumera qué revisar antes de entregar.

## Instrucciones integradas para la IA

`skills ads`, `skills music`, `skills quality` y `skills web` devuelven instrucciones consultables: cómo planificar un anuncio, cómo elegir música coherente con el mensaje, cómo revisar la calidad y cómo escribir escenas HTML válidas. Son texto, no plugins ni integraciones.

## Ejemplos reproducibles

```powershell
cd examples
powershell -ExecutionPolicy Bypass -File make-media.ps1   # genera medios sintéticos con ffmpeg
cd ..
videocraft render examples/html.json  demo-html.mp4    # solo escenas HTML (vertical)
videocraft render examples/media.json demo-media.mp4   # solo clips + imagen (horizontal)
videocraft render examples/mixed.json demo-mixed.mp4   # escenas + clips + imagen mezclados
```

Los tres proyectos usan medios sintéticos generados localmente; sustitúyelos por material real y actualiza `license`. Prueba de humo de integración: `powershell tests/smoke.ps1` renderiza los tres motores y comprueba protocolo, errores y verificación.

## Desarrollo y release

```powershell
cargo test --locked              # pruebas unitarias (esquema, validación, contrato, plan)
cargo build --release --locked
powershell tests/smoke.ps1       # renders reales con medios sintéticos
```

Estructura del repositorio: `Cargo.toml`, `src/` (CLI, esquema, FFmpeg, navegador), `skills/`, `examples/`, `tests/`, `README.md` en la raíz. El workflow de GitHub Actions compila y prueba en Windows en cada push, y al publicar una etiqueta `v*` adjunta `videocraft.exe` y su SHA-256 a la release.

## Limitaciones actuales

- Cortes directos entre items; las transiciones se dibujan dentro de cada escena HTML.
- Sin subtítulos automáticos, TTS, generación de imagen/vídeo/música, análisis semántico ni detección automática de BPM.
- El audio de los clips se descarta; las escenas son mudas.
- Máximo 100 items y 1 hora por proyecto; salida siempre MP4.

Licencia: MIT para el código de VideoCraft (`LICENSE`). FFmpeg y el navegador se instalan aparte y conservan sus propias licencias.
