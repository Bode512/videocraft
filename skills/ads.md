# Anuncios con VideoCraft

Usar para producir un anuncio desde material aportado por el usuario.
1. Obtener producto, audiencia, promesa demostrable, plataforma y llamada a la accion.
2. Elegir formato de salida: vertical 1080x1920 para reels/shorts/tiktok, horizontal 1920x1080 para web/TV.
3. Escribir un gancho de 0 a 2 segundos, demostracion del beneficio y cierre con una sola accion.
4. Ejecutar `videocraft probe ARCHIVO` para conocer duraciones, dimensiones y pistas de cada material.
5. Planificar la linea de tiempo: los items `scene` (HTML) aportan textos animados, graficos y llamadas a la accion; los items `clip` e `image` aportan material real del producto. Ejecutar `videocraft skills web` para el contrato de escenas y `videocraft example` para un proyecto completo.
6. Obtener el formato con `videocraft example`. Guardar solo result como proyecto JSON con rutas reales.
7. Leer `videocraft skills music`, elegir musica coherente con el mensaje y ajustar duraciones al ritmo.
8. Ejecutar check, render --dry-run, render y aplicar la skill quality.
No inventar testimonios ni resultados del producto. Los textos y graficos de las escenas los escribe la IA: revisar ortografia, legibilidad y claims antes de entregar. No prometer calidad 10/10 sin ver y escuchar el video final.
