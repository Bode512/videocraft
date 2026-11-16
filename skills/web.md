# Escenas HTML

Usar al escribir items `scene` de un proyecto VideoCraft.

## Contrato (obligatorio)

- Cada escena es un HTML autocompleto: sin peticiones de red, sin CDNs, sin imagenes externas. Solo fuentes del sistema o texto vectorial/CSS.
- Debe definir `window.vcSeek(t)`: una funcion determinista que pinta la pagina tal como debe verse en el segundo `t` desde el inicio de la escena. Llamarla dos veces con el mismo t produce exactamente el mismo fotograma.
- El navegador headless abre la pagina al tamano del proyecto (por ejemplo 1080x1920), inyecta un estilo que desactiva animaciones y transiciones CSS, y para cada fotograma llama a `vcSeek(t)` y captura un PNG. FFmpeg convierte los fotogramas en video.
- No usar requestAnimationFrame, setInterval, ni CSS keyframes para animar: todo movimiento se calcula dentro de vcSeek a partir de t.
- Las escenas son mudas: el audio lo aportan music y voice del proyecto.

## Plantilla de partida

```html
<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
  html, body { margin: 0; width: 100%; height: 100%; overflow: hidden; background: #0b1020; color: #fff; font-family: system-ui, 'Segoe UI', sans-serif; }
  .wrap { width: 100%; height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; padding: 6vw; box-sizing: border-box; }
  h1 { font-size: 9vw; margin: 0; line-height: 1.1; }
  p  { font-size: 4.5vw; opacity: .85; }
</style>
</head>
<body>
  <div class="wrap">
    <h1 id="title">Tu producto, sin friccion</h1>
    <p id="sub">Prueba gratis 14 dias</p>
  </div>
  <script>
    var DURATION = 2.0; // debe coincidir con el campo duration del item scene
    window.vcSeek = function (t) {
      var p = Math.min(Math.max(t / DURATION, 0), 1);
      var appear = Math.min(1, p * 3);            // entrada en el primer tercio
      document.getElementById('title').style.opacity = appear;
      document.getElementById('title').style.transform = 'translateY(' + (1 - appear) * 40 + 'px)';
      document.getElementById('sub').style.opacity = Math.min(1, Math.max(0, (p - 0.3) * 3));
    };
  </script>
</body>
</html>
```

## Buenas practicas

- Disenar con unidades relativas (vw, %) para que la escena valga en vertical y horizontal.
- Tamano de texto grande y contraste alto: se vera en un movil y a 5 segundos.
- El transito entre escenas y clips es un corte directo; si se necesita fundido, dibujarlo dentro de la propia escena cambiando el fondo con t.
- Duraciones cortas (1 a 3 segundos por escena). La escena se cuantiza a fotogramas: una escena de 2.0 s a 30 fps son 60 fotogramas exactos.
- Probar antes de renderizar abriendo el HTML en un navegador normal y llamando a vcSeek(0), vcSeek(DURATION/2) y vcSeek(DURATION) desde la consola: los tres estados deben verse correctos.
- Revisar el video final: la automatizacion garantiza determinismo, no calidad visual.
