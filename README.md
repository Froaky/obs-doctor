# OBS Doctor

Un ejecutable que revisa tu PC, tu internet y tu configuración de OBS, y te dice, en simple, qué cambiar para que el directo salga mejor. Pensado para streamers que recién empiezan y transmiten con el hardware que tienen.

**Solo lee.** No cambia tu configuración de OBS ni de Windows y no cierra programas. Te explica qué tocar, por qué y dónde, y los cambios los haces tú.

## Qué revisa

- **Tu último directo**: lee los registros que guarda OBS y te dice qué pasó la última vez que transmitiste. Cuadros perdidos por internet, cuadros que la PC no alcanzó a preparar o a dibujar, reconexiones, cortes y cierres inesperados.
- **Tu OBS**: plataforma, tamaño de imagen, cuadros por segundo, calidad de envío, quién prepara el video (el procesador o la placa) y sus opciones, intervalo de fotogramas clave, filtro de escalado, audio y grabación.
- **Tus escenas**: fuentes de navegador, cámaras, videos, capturas y filtros que le piden mucho a la PC, nombrados como tú los llamaste.
- **Tu internet**: si estás por cable o por wifi, la velocidad con el router y, en wifi, la señal y la banda.
- **Windows**: energía y batería, modo de juego, grabación en segundo plano, programación de la placa de video, controladores, espacio en disco, archivo de paginación y programas que arrancan con Windows.
- **Tu PC**: procesador, memoria y placa de video, con una configuración de partida pensada para ella.
- **Programas abiertos**: descargas, otros programas de captura y programas que reparten la red, ordenados por tipo. Nunca te pide cerrar Windows, controladores, antivirus ni anticheats.

Con eso arma una lista de cambios ordenada por importancia (importante, recomendado y para probar), con el paso a paso dentro de los menús de OBS y de Windows en español. Cuando dos cosas se explican entre sí, como cuadros perdidos en un directo que salió por wifi, te da un solo consejo con la causa en lugar de varios sueltos.

## Cómo usarlo

1. Descarga el `.exe` de la [última versión](https://github.com/Froaky/obs-doctor/releases/latest).
2. Ábrelo con doble clic. Si aparece "Windows protegió su PC", toca **Más información** y después **Ejecutar de todas formas**: sale porque el programa todavía no está firmado (ver [Code signing policy](#code-signing-policy)).
3. Se abre el reporte en tu navegador. Lee los consejos y aplica los que quieras. Puedes marcar cada uno como "Listo" a medida que avanzas.
4. Después de cambiar algo, haz un directo corto de prueba y vuelve a correr OBS Doctor para ver si mejoró.

El reporte se guarda en `%LOCALAPPDATA%\obs-doctor\reporte.html`. Es un solo archivo que funciona sin internet.

No se instala. Para quitarlo, borra el `.exe` y la carpeta `%LOCALAPPDATA%\obs-doctor`.

### Opciones

```
obs-doctor.exe [opciones]
```

| Opción | Qué hace |
|---|---|
| `--consola` | Muestra el reporte completo en la consola, sin abrir el navegador. |
| `--no-abrir` | Guarda el reporte pero no lo abre. |
| `--salida <archivo>` | Dónde guardar el reporte. |
| `--obs-dir <carpeta>` | Carpeta de configuración de OBS, para instalaciones portables. |
| `--sin-internet` | No usa internet para nada. Hoy OBS Doctor tampoco lo hace sin esta opción (ver Privacidad). |
| `-h`, `--ayuda` | Muestra la ayuda. |
| `-V`, `--version` | Muestra la versión. |

Si usas OBS en modo portable, indícale la carpeta de configuración:

```
obs-doctor.exe --obs-dir "D:\OBS\config\obs-studio"
```

## Privacidad

- Lee los archivos de configuración, escenas y registros de OBS de tu usuario. No los modifica.
- Tu clave de transmisión nunca se muestra ni se guarda: del archivo donde está solo se toma el nombre de la plataforma y si usas un servidor propio.
- De tus escenas no guarda ni muestra direcciones web ni rutas de archivos, solo los nombres que les pusiste a las fuentes.
- De los registros de OBS no guarda líneas: solo los números que necesita (cuadros perdidos, duración, reconexiones).
- De los programas abiertos usa la ubicación para reconocerlos, pero nunca la muestra ni la guarda.
- No muestra tu nombre de usuario de Windows.
- OBS Doctor revisa tu internet sin enviar ni descargar datos: le pregunta a Windows cómo está conectada tu PC (cable o wifi, velocidad con el router, señal y banda del wifi). No guarda ni muestra nombres de redes wifi, direcciones IP ni MAC. Por ahora no mide la velocidad de tu internet.
- El reporte queda solo en tu PC. OBS Doctor no envía nada a ningún lado.

### Por qué no mide la velocidad de tu internet

La idea era medir la subida con speed.cloudflare.com, pero sus términos de uso no permiten usarlo desde una herramienta de terceros como esta, así que no se implementó. Por eso OBS Doctor no puede decirte cuántos kbps aguanta tu subida. Mientras tanto, si tu último directo perdió cuadros por internet, el reporte te lo dice con los números del registro de OBS.

## Compilarlo

Necesitas [Rust](https://rustup.rs) estable (edición 2024).

```
cargo build --release
cargo test
```

El ejecutable queda en `target/release/obs-doctor.exe`. Compila también fuera de Windows (para la integración continua), pero las lecturas de Windows solo funcionan en Windows.

## Estado

Versión 0.2, solo para Windows. Las recomendaciones son un punto de partida basado en la base de conocimiento de OBS y en las guías públicas de las plataformas y los fabricantes: haz siempre un directo de prueba.

Ideas para más adelante:

- Medir la subida de tu internet con un servicio que lo permita.
- Diagnóstico en vivo mientras transmites.
- Linux y macOS.

## Code signing policy

La firma de los ejecutables con SignPath está en trámite: las versiones hasta la 0.2.1 no están firmadas. Cuando se apruebe, cada versión se firmará así:

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

- **Cómo se compila:** cada versión se compila en GitHub Actions a partir de este repositorio, con [`.github/workflows/release.yml`](.github/workflows/release.yml). No se firman ejecutables compilados a mano.
- **Aprobación:** cada pedido de firma se aprueba a mano, versión por versión.
- **Autores** (pueden cambiar el código sin otra revisión): [Froaky](https://github.com/Froaky).
- **Revisores** (revisan cada cambio de otras personas, como los pull requests): [Froaky](https://github.com/Froaky).
- **Aprobadores** (aprueban cada firma): [Froaky](https://github.com/Froaky).
- **Privacidad:** este programa no transfiere información a otros sistemas de la red, salvo que quien lo usa lo pida expresamente. El detalle está en [Privacidad](#privacidad).

In English: release binaries are built by GitHub Actions from this repository and every signing request is approved manually. Committers, reviewers and approvers: [Froaky](https://github.com/Froaky). This program will not transfer any information to other networked systems unless specifically requested by the user.

## Licencia

MIT.
