# Ejemplos

Tres proyectos completos que cubren los tres casos de uso:

| Proyecto | Motor | Encuadre | Contenido |
|---|---|---|---|
| `html.json` | Solo escenas HTML | Vertical 1080x1920 | Intro animada + CTA, con música |
| `media.json` | Solo montaje multimedia | Horizontal 1920x1080 | Dos clips + imagen fija, música y locución |
| `mixed.json` | Ambos motores mezclados | Vertical 1080x1920 | Escena + clip + imagen + clip + escena |

## Reproducir

```powershell
powershell -ExecutionPolicy Bypass -File make-media.ps1   # requiere ffmpeg en PATH
cd ..
videocraft render examples/html.json  demo-html.mp4
videocraft render examples/media.json demo-media.mp4
videocraft render examples/mixed.json demo-mixed.mp4
```

`make-media.ps1` genera medios sintéticos (patrones de prueba y tonos puros) en `examples/media/`. No se suben al repositorio. Para anuncios reales, sustituye el material y declara la licencia real de la música en el campo `license`.

Las escenas de `scenes/intro.html` y `scenes/cta.html` implementan el contrato `window.vcSeek(t)` documentado en `videocraft skills web`.
