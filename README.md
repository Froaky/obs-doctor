# OBS Doctor

Un ejecutable que revisa tu PC y tu configuración de OBS y te dice, en simple, qué cambiar para que el directo salga mejor. Pensado para streamers que recién empiezan y transmiten con el hardware que tienen.

**Solo lee.** No cambia tu configuración, no cierra programas y no envía nada a ningún lado. Te explica qué tocar y por qué, y los cambios los haces tú.

## Qué revisa

- **Tu PC**: procesador, memoria, placa de video y sistema.
- **Tu OBS**: plataforma, tamaño de imagen, fps, calidad de envío y quién prepara el video (procesador o placa).
- **Programas abiertos**: los que le quitan memoria, procesador o internet al directo, con el motivo de cada uno.

Con eso arma una lista de cambios ordenada por importancia, con el paso a paso dentro de los menús de OBS, y una configuración de partida para tu PC.

## Cómo usarlo

1. Descarga `obs-doctor.exe`.
2. Ábrelo con doble clic.
3. Lee el reporte y aplica los cambios que quieras.

Si usas OBS en modo portable, indícale la carpeta de configuración:

```
obs-doctor.exe --obs-dir "D:\OBS\config\obs-studio"
```

## Privacidad

- Lee los archivos de configuración de OBS de tu usuario. No los modifica.
- Tu clave de transmisión nunca se muestra ni se guarda: del archivo donde está solo se toma el nombre de la plataforma.
- No se conecta a internet.

## Compilarlo

Necesitas [Rust](https://rustup.rs).

```
cargo build --release
cargo test
```

El ejecutable queda en `target/release/obs-doctor.exe`.

## Estado

Versión inicial, solo para Windows. Las recomendaciones son un punto de partida basado en las guías públicas de las plataformas: haz siempre un directo de prueba.

Ideas para más adelante:

- Medir la velocidad de subida.
- Diagnóstico en vivo mientras transmites (cuadros perdidos y su causa).
- Revisar ajustes de Windows (modo de juego, plan de energía).
- Linux y macOS.

## Licencia

MIT.
