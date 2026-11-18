//! The curated table of programs we recognise. Pure data: the rules that use
//! it live in `mod.rs`.
//!
//! Each entry is `(executable name, friendly name, category)`. The executable
//! name is lowercase and without `.exe`, exactly as `normalize` produces it.
//! Several executables may share one friendly name: that is how the helper
//! processes of one program (Steam and its web helpers, G HUB and its agent...)
//! end up grouped as a single line in the report.
//!
//! Sources for the conflict entries (fixes quoted in `NOTES`):
//! - https://obsproject.com/kb/known-conflicts
//! - https://obsproject.com/kb/stream-connection-troubleshooting

use crate::model::ProgramKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Browser,
    Chat,
    Launcher,
    CloudSync,
    /// Torrents and download managers.
    Download,
    /// Another program that records or captures the game (OBS KB: "Other
    /// Capture / Recording Software" conflict through their hooks).
    Recorder,
    /// On-screen displays that hook into games (OBS KB: "On-screen Displays /
    /// Overlays").
    Overlay,
    /// Named by the OBS KB under "Other Apps": close it, uninstall if it keeps
    /// causing trouble.
    Conflict,
    /// Network "optimisers" that may deprioritise OBS (OBS KB, stream
    /// connection troubleshooting).
    NetShaper,
    Vpn,
    Peripherals,
    /// Animated wallpapers and desktop widgets.
    Desktop,
    Heavy,
    Media,
    /// Tools streamers use while live: recognised and left alone.
    StreamTool,
    /// A popular game.
    Game,
    /// Windows, drivers, antivirus and anticheats: never listed, never
    /// suggested to close.
    Protected,
}

impl Category {
    /// How the report groups it.
    pub fn kind(self) -> Option<ProgramKind> {
        match self {
            Category::Download
            | Category::Recorder
            | Category::Overlay
            | Category::NetShaper
            | Category::CloudSync
            | Category::Desktop => Some(ProgramKind::Close),
            Category::Browser
            | Category::Chat
            | Category::Launcher
            | Category::Conflict
            | Category::Vpn
            | Category::Peripherals
            | Category::Heavy
            | Category::Media => Some(ProgramKind::Review),
            Category::StreamTool => Some(ProgramKind::Keep),
            Category::Game => Some(ProgramKind::Game),
            Category::Protected => None,
        }
    }

    /// Worth listing even when it is barely using anything right now: its
    /// harm (or its reassurance) does not depend on how busy it looks.
    pub fn always_report(self) -> bool {
        matches!(
            self,
            Category::Download
                | Category::CloudSync
                | Category::Recorder
                | Category::Overlay
                | Category::Conflict
                | Category::NetShaper
                | Category::Vpn
                | Category::StreamTool
                | Category::Game
        )
    }

    /// Plain-language advice shown next to the program.
    pub fn tip(self) -> &'static str {
        match self {
            Category::Browser => {
                "Cada pestaña abierta ocupa memoria. Deja solo las que uses en el directo, como el chat o las alertas, y cierra el resto."
            }
            Category::Chat => {
                "Si no lo vas a usar durante el directo, ciérralo: las llamadas y las notificaciones gastan memoria y procesador."
            }
            Category::Launcher => {
                "Si el juego que vas a transmitir no es de esta tienda, ciérrala: puede ponerse a descargar actualizaciones en pleno directo."
            }
            Category::CloudSync => {
                "Ponlo en pausa mientras transmites: cuando sincroniza archivos usa tu disco y la subida de tu internet."
            }
            Category::Download => {
                "Ciérralo antes del directo: puede usar la subida de tu internet en segundo plano y es una causa típica de cortes."
            }
            Category::Recorder => {
                "Graba o captura el juego igual que OBS. Dos programas capturando a la vez se pisan: deja solo OBS."
            }
            Category::Overlay => {
                "Dibuja encima del juego. OBS lo nombra entre los programas que chocan con la captura: ciérralo o apaga su indicador en pantalla mientras transmites."
            }
            Category::Conflict => {
                "OBS lo nombra entre los programas que pueden causarle problemas. Ciérralo antes del directo y, si los problemas siguen, desinstálalo."
            }
            Category::NetShaper => {
                "Reparte tu internet entre programas y puede darle menos prioridad a OBS. Eso causa cortes en el directo."
            }
            Category::Vpn => {
                "Si está conectada, el directo pasa por otro servidor y la conexión puede ser inestable. Desconéctala para transmitir."
            }
            Category::Peripherals => {
                "Los programas de luces y periféricos gastan procesador todo el tiempo. Configura tus luces y prueba cerrarlo."
            }
            Category::Desktop => {
                "Los fondos animados y los widgets del escritorio usan la tarjeta de video todo el tiempo. Ponlos en pausa mientras transmites."
            }
            Category::Heavy => {
                "Es un programa pesado. Si no lo necesitas para el directo, ciérralo y recuperas memoria y procesador."
            }
            Category::Media => "No suele ser un problema. Si no lo usas en el directo, puedes cerrarlo.",
            Category::StreamTool | Category::Game | Category::Protected => "",
        }
    }
}

/// Program-specific fixes from the OBS knowledge base. They replace the
/// category tip for that program.
pub const NOTES: &[(&str, &str)] = &[
    // https://obsproject.com/kb/known-conflicts ("D3DGear")
    (
        "d3dgear",
        "OBS avisa que deja ganchos en todos los juegos aunque lo cierres. Desinstálalo desde Configuración > Aplicaciones > Aplicaciones instaladas.",
    ),
    // https://obsproject.com/kb/stream-connection-troubleshooting ("Killer NIC")
    (
        "killernetworkservice",
        "OBS recomienda desinstalar el paquete de programas de Killer y dejar solo el controlador de la placa de red. Puede darle menos prioridad a OBS y causar cortes.",
    ),
    // https://obsproject.com/kb/stream-connection-troubleshooting ("Lenovo Vantage")
    (
        "lenovovantageservice",
        "Si usas su opción Network Boost, apágala: puede darle menos prioridad a OBS y causar cortes en el directo.",
    ),
    // https://obsproject.com/kb/known-conflicts ("On-screen Displays / Overlays")
    (
        "msiafterburner",
        "Su indicador en pantalla choca con la captura de OBS. Ciérralo o apaga el indicador en pantalla mientras transmites.",
    ),
    (
        "rtss",
        "Su indicador en pantalla choca con la captura de OBS. Ciérralo o apaga el indicador en pantalla mientras transmites.",
    ),
    // Same page: "TeamSpeak / Mumble / Discord overlays".
    ("discord", VOICE_OVERLAY),
    ("discordptb", VOICE_OVERLAY),
    ("discordcanary", VOICE_OVERLAY),
    ("ts3client_win64", VOICE_OVERLAY),
    ("ts3client_win32", VOICE_OVERLAY),
    ("teamspeak", VOICE_OVERLAY),
    ("mumble", VOICE_OVERLAY),
];

/// Voice chats whose in-game overlay the OBS KB lists as a conflict.
const VOICE_OVERLAY: &str = "Si no lo vas a usar durante el directo, ciérralo. Si lo usas, apaga su superposición dentro del juego: OBS la nombra entre las que chocan con la captura.";

/// The friendly name of the D3DGear entry, used by its dedicated advice.
pub const D3DGEAR: &str = "D3DGear";

use Category::*;

/// (executable name without `.exe`, friendly name, category)
pub const PROGRAMS: &[(&str, &str, Category)] = &[
    // Browsers.
    ("chrome", "Google Chrome", Browser),
    ("msedge", "Microsoft Edge", Browser),
    ("firefox", "Firefox", Browser),
    ("opera", "Opera", Browser),
    ("brave", "Brave", Browser),
    ("vivaldi", "Vivaldi", Browser),
    ("waterfox", "Waterfox", Browser),
    ("librewolf", "LibreWolf", Browser),
    ("floorp", "Floorp", Browser),
    ("zen", "Zen Browser", Browser),
    ("arc", "Arc", Browser),
    ("thorium", "Thorium", Browser),
    ("chromium", "Chromium", Browser),
    // Chat and calls.
    ("discord", "Discord", Chat),
    ("discordptb", "Discord", Chat),
    ("discordcanary", "Discord", Chat),
    ("slack", "Slack", Chat),
    ("ms-teams", "Microsoft Teams", Chat),
    ("teams", "Microsoft Teams", Chat),
    ("whatsapp", "WhatsApp", Chat),
    ("whatsapp.root", "WhatsApp", Chat),
    ("telegram", "Telegram", Chat),
    ("zoom", "Zoom", Chat),
    ("skype", "Skype", Chat),
    ("ts3client_win64", "TeamSpeak", Chat),
    ("ts3client_win32", "TeamSpeak", Chat),
    ("teamspeak", "TeamSpeak", Chat),
    ("mumble", "Mumble", Chat),
    ("signal", "Signal", Chat),
    ("element", "Element", Chat),
    ("guilded", "Guilded", Chat),
    ("viber", "Viber", Chat),
    ("messenger", "Messenger", Chat),
    // Game stores and launchers.
    ("steam", "Steam", Launcher),
    ("steamwebhelper", "Steam", Launcher),
    ("steamservice", "Steam", Launcher),
    ("epicgameslauncher", "Epic Games Launcher", Launcher),
    ("epicwebhelper", "Epic Games Launcher", Launcher),
    ("unrealcefsubprocess", "Epic Games Launcher", Launcher),
    ("battle.net", "Battle.net", Launcher),
    ("eadesktop", "EA app", Launcher),
    ("eabackgroundservice", "EA app", Launcher),
    ("origin", "Origin", Launcher),
    ("galaxyclient", "GOG Galaxy", Launcher),
    ("galaxyclient helper", "GOG Galaxy", Launcher),
    ("upc", "Ubisoft Connect", Launcher),
    ("ubisoftconnect", "Ubisoft Connect", Launcher),
    ("uplaywebcore", "Ubisoft Connect", Launcher),
    ("ubisoftgamelauncher", "Ubisoft Connect", Launcher),
    ("riotclientservices", "Riot Client", Launcher),
    ("riotclientux", "Riot Client", Launcher),
    ("riotclientuxrender", "Riot Client", Launcher),
    ("leagueclient", "Cliente de League of Legends", Launcher),
    ("leagueclientux", "Cliente de League of Legends", Launcher),
    ("leagueclientuxrender", "Cliente de League of Legends", Launcher),
    ("xboxpcapp", "Xbox", Launcher),
    ("rockstarservice", "Rockstar Games Launcher", Launcher),
    ("socialclubhelper", "Rockstar Games Launcher", Launcher),
    ("amazon games ui", "Amazon Games", Launcher),
    ("itch", "itch.io", Launcher),
    ("playnite.desktopapp", "Playnite", Launcher),
    ("playnite.fullscreenapp", "Playnite", Launcher),
    ("heroic", "Heroic Games Launcher", Launcher),
    ("bsglauncher", "Battlestate Games Launcher", Launcher),
    ("minecraftlauncher", "Minecraft Launcher", Launcher),
    // Cloud sync.
    ("onedrive", "OneDrive", CloudSync),
    ("dropbox", "Dropbox", CloudSync),
    ("googledrivefs", "Google Drive", CloudSync),
    ("googledrivesync", "Google Drive", CloudSync),
    ("megasync", "MEGA", CloudSync),
    ("icloudservices", "iCloud", CloudSync),
    ("iclouddrive", "iCloud", CloudSync),
    ("pcloud", "pCloud", CloudSync),
    ("syncthing", "Syncthing", CloudSync),
    ("nextcloud", "Nextcloud", CloudSync),
    ("owncloud", "ownCloud", CloudSync),
    // Torrents and download managers.
    ("qbittorrent", "qBittorrent", Download),
    ("utorrent", "uTorrent", Download),
    ("utorrentie", "uTorrent", Download),
    ("bittorrent", "BitTorrent", Download),
    ("bittorrentie", "BitTorrent", Download),
    ("transmission-qt", "Transmission", Download),
    ("deluge", "Deluge", Download),
    ("deluged", "Deluge", Download),
    ("tixati", "Tixati", Download),
    ("azureus", "Vuze", Download),
    ("biglybt", "BiglyBT", Download),
    ("frostwire", "FrostWire", Download),
    ("picotorrent", "PicoTorrent", Download),
    ("idman", "Internet Download Manager", Download),
    ("fdm", "Free Download Manager", Download),
    ("jdownloader2", "JDownloader", Download),
    ("motrix", "Motrix", Download),
    ("emule", "eMule", Download),
    ("soulseekqt", "Soulseek", Download),
    ("aria2c", "aria2", Download),
    // Other capture and recording software.
    ("action", "Mirillis Action!", Recorder),
    ("dxtory", "Dxtory", Recorder),
    ("bandicam", "Bandicam", Recorder),
    ("bdcam", "Bandicam", Recorder),
    ("d3dgear", D3DGEAR, Recorder),
    ("fraps", "Fraps", Recorder),
    ("medal", "Medal", Recorder),
    ("medalencoder", "Medal", Recorder),
    ("streamlabs obs", "Streamlabs", Recorder),
    ("streamlabs desktop", "Streamlabs", Recorder),
    ("xsplit.core", "XSplit", Recorder),
    ("twitch studio", "Twitch Studio", Recorder),
    ("prismlivestudio", "PRISM Live Studio", Recorder),
    ("vmix64", "vMix", Recorder),
    ("vmix", "vMix", Recorder),
    ("wirecast", "Wirecast", Recorder),
    ("camtasiastudio", "Camtasia", Recorder),
    ("camrecorder", "Camtasia", Recorder),
    ("ocam", "oCam", Recorder),
    ("evcapture", "EV Capture", Recorder),
    ("meld studio", "Meld Studio", Recorder),
    // On-screen displays and overlays.
    ("msiafterburner", "MSI Afterburner", Overlay),
    ("rtss", "RivaTuner Statistics Server", Overlay),
    ("rtsshooksloader", "RivaTuner Statistics Server", Overlay),
    ("rtsshooksloader64", "RivaTuner Statistics Server", Overlay),
    ("precisionx_x64", "EVGA Precision", Overlay),
    ("gputweakii", "ASUS GPU Tweak", Overlay),
    ("gputweak", "ASUS GPU Tweak", Overlay),
    ("overwolf", "Overwolf", Overlay),
    ("overwolfbrowser", "Overwolf", Overlay),
    ("overwolfhelper", "Overwolf", Overlay),
    ("overwolfhelper64", "Overwolf", Overlay),
    ("athenascope", "Athenascope", Overlay),
    ("lowkey", "Lowkey", Overlay),
    // Other apps the OBS KB lists as conflicting.
    ("sendori", "Sendori", Conflict),
    ("sendorisvc", "Sendori", Conflict),
    ("astrill", "Astrill VPN", Conflict),
    ("nahimic3", "Nahimic", Conflict),
    ("nahimicsvc64", "Nahimic", Conflict),
    ("nahimicsvc32", "Nahimic", Conflict),
    ("sonicradar3", "ASUS Sonic Radar", Conflict),
    ("personify", "Personify", Conflict),
    ("dynamiclinkmanager", "Adobe Dynamic Link", Conflict),
    // Network "optimisers" that may deprioritise OBS.
    ("killernetworkservice", "Killer Network", NetShaper),
    ("lenovovantageservice", "Lenovo Vantage", NetShaper),
    ("cfosspeed", "cFosSpeed", NetShaper),
    ("nlclientapp", "NetLimiter", NetShaper),
    ("nlsvc", "NetLimiter", NetShaper),
    ("smartbytenetworkservice", "SmartByte", NetShaper),
    ("gamefirst", "ASUS GameFirst", NetShaper),
    // VPNs.
    ("nordvpn", "NordVPN", Vpn),
    ("nordvpn-service", "NordVPN", Vpn),
    ("expressvpn", "ExpressVPN", Vpn),
    ("protonvpn", "Proton VPN", Vpn),
    ("protonvpn.client", "Proton VPN", Vpn),
    ("surfshark", "Surfshark", Vpn),
    ("windscribe", "Windscribe", Vpn),
    ("mullvad vpn", "Mullvad VPN", Vpn),
    ("pia-client", "Private Internet Access", Vpn),
    ("tunnelbear", "TunnelBear", Vpn),
    ("cloudflare warp", "Cloudflare WARP", Vpn),
    ("openvpn-gui", "OpenVPN", Vpn),
    ("wireguard", "WireGuard", Vpn),
    // Lights and peripherals.
    ("icue", "Corsair iCUE", Peripherals),
    ("lghub", "Logitech G HUB", Peripherals),
    ("lghub_agent", "Logitech G HUB", Peripherals),
    ("lghub_updater", "Logitech G HUB", Peripherals),
    ("razer synapse 3", "Razer Synapse", Peripherals),
    ("razer synapse service", "Razer Synapse", Peripherals),
    ("razerappengine", "Razer Synapse", Peripherals),
    ("razer central", "Razer Synapse", Peripherals),
    ("armourycrate", "ASUS Armoury Crate", Peripherals),
    ("armourycrate.service", "ASUS Armoury Crate", Peripherals),
    ("lightingservice", "ASUS Aura", Peripherals),
    ("nzxt cam", "NZXT CAM", Peripherals),
    ("signalrgb", "SignalRGB", Peripherals),
    ("openrgb", "OpenRGB", Peripherals),
    ("steelseriesgg", "SteelSeries GG", Peripherals),
    ("steelseriesengine", "SteelSeries GG", Peripherals),
    ("ngenuity", "HyperX NGENUITY", Peripherals),
    ("wootility", "Wootility", Peripherals),
    // Animated wallpapers and widgets.
    ("wallpaper32", "Wallpaper Engine", Desktop),
    ("wallpaper64", "Wallpaper Engine", Desktop),
    ("lively", "Lively Wallpaper", Desktop),
    ("rainmeter", "Rainmeter", Desktop),
    // Heavy work programs, virtual machines and phone emulators.
    ("photoshop", "Photoshop", Heavy),
    ("illustrator", "Illustrator", Heavy),
    ("lightroom", "Lightroom", Heavy),
    ("adobe premiere pro", "Premiere Pro", Heavy),
    ("afterfx", "After Effects", Heavy),
    ("audition", "Audition", Heavy),
    ("creative cloud", "Adobe Creative Cloud", Heavy),
    ("adobe desktop service", "Adobe Creative Cloud", Heavy),
    ("ccxprocess", "Adobe Creative Cloud", Heavy),
    ("resolve", "DaVinci Resolve", Heavy),
    ("blender", "Blender", Heavy),
    ("handbrake", "HandBrake", Heavy),
    ("code", "Visual Studio Code", Heavy),
    ("devenv", "Visual Studio", Heavy),
    ("idea64", "IntelliJ IDEA", Heavy),
    ("pycharm64", "PyCharm", Heavy),
    ("rider64", "Rider", Heavy),
    ("studio64", "Android Studio", Heavy),
    ("unity", "Unity", Heavy),
    ("unrealeditor", "Unreal Editor", Heavy),
    ("ue4editor", "Unreal Editor", Heavy),
    ("docker desktop", "Docker Desktop", Heavy),
    ("com.docker.backend", "Docker Desktop", Heavy),
    ("vmmem", "Máquina virtual (WSL o Docker)", Heavy),
    ("vmmemwsl", "Máquina virtual (WSL o Docker)", Heavy),
    ("vmware", "VMware", Heavy),
    ("vmware-vmx", "VMware", Heavy),
    ("virtualbox", "VirtualBox", Heavy),
    ("virtualboxvm", "VirtualBox", Heavy),
    ("ollama", "Ollama", Heavy),
    ("ollama app", "Ollama", Heavy),
    ("lm studio", "LM Studio", Heavy),
    ("hd-player", "BlueStacks", Heavy),
    ("dnplayer", "LDPlayer", Heavy),
    ("memu", "MEmu", Heavy),
    // Music and video players.
    ("spotify", "Spotify", Media),
    ("applemusic", "Apple Music", Media),
    ("itunes", "iTunes", Media),
    ("amazon music", "Amazon Music", Media),
    ("deezer", "Deezer", Media),
    ("tidal", "TIDAL", Media),
    ("foobar2000", "foobar2000", Media),
    ("aimp", "AIMP", Media),
    ("musicbee", "MusicBee", Media),
    ("winamp", "Winamp", Media),
    ("vlc", "VLC", Media),
    ("potplayermini64", "PotPlayer", Media),
    // Tools streamers use while live.
    ("streamer.bot", "Streamer.bot", StreamTool),
    ("streamdeck", "Stream Deck", StreamTool),
    ("wavelink", "Elgato Wave Link", StreamTool),
    ("camera hub", "Elgato Camera Hub", StreamTool),
    ("touchportal", "Touch Portal", StreamTool),
    ("loupedeck", "Loupedeck", StreamTool),
    ("voicemoddesktop", "Voicemod", StreamTool),
    ("voicemeeter", "Voicemeeter", StreamTool),
    ("voicemeeterpro", "Voicemeeter", StreamTool),
    ("voicemeeter8", "Voicemeeter", StreamTool),
    ("voicemeeter8x64", "Voicemeeter", StreamTool),
    ("vtube studio", "VTube Studio", StreamTool),
    ("veadotube mini", "veadotube", StreamTool),
    ("vseeface", "VSeeFace", StreamTool),
    ("warudo", "Warudo", StreamTool),
    ("mixitup", "Mix It Up", StreamTool),
    ("firebot v5", "Firebot", StreamTool),
    ("streamlabs chatbot", "Streamlabs Chatbot", StreamTool),
    ("chatterino", "Chatterino", StreamTool),
    ("nvidia broadcast", "NVIDIA Broadcast", StreamTool),
    ("nvidia broadcast ui", "NVIDIA Broadcast", StreamTool),
    ("krisp", "Krisp", StreamTool),
    // Popular games.
    ("cs2", "Counter-Strike 2", Game),
    ("valorant-win64-shipping", "VALORANT", Game),
    ("league of legends", "League of Legends", Game),
    ("fortniteclient-win64-shipping", "Fortnite", Game),
    ("dota2", "Dota 2", Game),
    ("r5apex", "Apex Legends", Game),
    ("r5apex_dx12", "Apex Legends", Game),
    ("overwatch", "Overwatch 2", Game),
    ("gta5", "Grand Theft Auto V", Game),
    ("gta5_enhanced", "Grand Theft Auto V", Game),
    ("rdr2", "Red Dead Redemption 2", Game),
    ("minecraft.windows", "Minecraft", Game),
    ("robloxplayerbeta", "Roblox", Game),
    ("rocketleague", "Rocket League", Game),
    ("eldenring", "Elden Ring", Game),
    ("cod", "Call of Duty", Game),
    ("tslgame", "PUBG: Battlegrounds", Game),
    ("rainbowsix", "Rainbow Six Siege", Game),
    ("destiny2", "Destiny 2", Game),
    ("escapefromtarkov", "Escape from Tarkov", Game),
    ("wow", "World of Warcraft", Game),
    ("wowclassic", "World of Warcraft", Game),
    ("hearthstone", "Hearthstone", Game),
    ("diablo iv", "Diablo IV", Game),
    ("deadbydaylight-win64-shipping", "Dead by Daylight", Game),
    ("deadbydaylight-egs-shipping", "Dead by Daylight", Game),
    ("among us", "Among Us", Game),
    ("terraria", "Terraria", Game),
    ("stardew valley", "Stardew Valley", Game),
    ("palworld-win64-shipping", "Palworld", Game),
    ("helldivers2", "Helldivers 2", Game),
    ("bg3", "Baldur's Gate 3", Game),
    ("bg3_dx11", "Baldur's Gate 3", Game),
    ("cyberpunk2077", "Cyberpunk 2077", Game),
    ("witcher3", "The Witcher 3", Game),
    ("eurotrucks2", "Euro Truck Simulator 2", Game),
    ("marvel-win64-shipping", "Marvel Rivals", Game),
    ("warframe.x64", "Warframe", Game),
    ("pathofexile_x64", "Path of Exile", Game),
    ("geometrydash", "Geometry Dash", Game),
    ("osu!", "osu!", Game),
    ("phasmophobia", "Phasmophobia", Game),
    ("valheim", "Valheim", Game),
    ("rustclient", "Rust", Game),
    ("ffxiv_dx11", "Final Fantasy XIV", Game),
    ("genshinimpact", "Genshin Impact", Game),
    ("zenlesszonezero", "Zenless Zone Zero", Game),
    ("starrail", "Honkai: Star Rail", Game),
    ("brawlhalla", "Brawlhalla", Game),
    ("streetfighter6", "Street Fighter 6", Game),
    ("ts4_x64", "Los Sims 4", Game),
    ("forzahorizon5", "Forza Horizon 5", Game),
    ("forzahorizon4", "Forza Horizon 4", Game),
    ("hollow knight", "Hollow Knight", Game),
    ("balatro", "Balatro", Game),
    ("lethal company", "Lethal Company", Game),
    // Windows itself.
    ("system", "Windows", Protected),
    ("idle", "Windows", Protected),
    ("system idle process", "Windows", Protected),
    ("registry", "Windows", Protected),
    ("memory compression", "Windows", Protected),
    ("memcompression", "Windows", Protected),
    ("secure system", "Windows", Protected),
    ("smss", "Windows", Protected),
    ("csrss", "Windows", Protected),
    ("wininit", "Windows", Protected),
    ("winlogon", "Windows", Protected),
    ("services", "Windows", Protected),
    ("lsass", "Windows", Protected),
    ("lsaiso", "Windows", Protected),
    ("svchost", "Windows", Protected),
    ("dwm", "Windows", Protected),
    ("explorer", "Windows", Protected),
    ("fontdrvhost", "Windows", Protected),
    ("sihost", "Windows", Protected),
    ("taskhostw", "Windows", Protected),
    ("ctfmon", "Windows", Protected),
    ("conhost", "Windows", Protected),
    ("runtimebroker", "Windows", Protected),
    ("searchhost", "Windows", Protected),
    ("searchapp", "Windows", Protected),
    ("searchindexer", "Windows", Protected),
    ("startmenuexperiencehost", "Windows", Protected),
    ("shellexperiencehost", "Windows", Protected),
    ("shellhost", "Windows", Protected),
    ("textinputhost", "Windows", Protected),
    ("applicationframehost", "Windows", Protected),
    ("backgroundtaskhost", "Windows", Protected),
    ("lockapp", "Windows", Protected),
    ("smartscreen", "Windows", Protected),
    ("phoneexperiencehost", "Windows", Protected),
    ("widgets", "Windows", Protected),
    ("widgetservice", "Windows", Protected),
    ("systemsettings", "Windows", Protected),
    ("taskmgr", "Windows", Protected),
    ("sgrmbroker", "Windows", Protected),
    ("wudfhost", "Windows", Protected),
    ("dashost", "Windows", Protected),
    ("unsecapp", "Windows", Protected),
    ("audiodg", "Windows", Protected),
    ("spoolsv", "Windows", Protected),
    ("wmiprvse", "Windows", Protected),
    ("dllhost", "Windows", Protected),
    ("msedgewebview2", "Windows", Protected),
    ("gamebar", "Windows", Protected),
    ("gamebarftserver", "Windows", Protected),
    ("gamingservices", "Windows", Protected),
    ("gamingservicesnet", "Windows", Protected),
    // Our own console windows.
    ("obs-doctor", "OBS Doctor", Protected),
    ("windowsterminal", "Windows", Protected),
    ("openconsole", "Windows", Protected),
    ("cmd", "Windows", Protected),
    ("powershell", "Windows", Protected),
    ("pwsh", "Windows", Protected),
    // Graphics and audio drivers and their panels.
    ("nvcontainer", "NVIDIA", Protected),
    ("nvdisplay.container", "NVIDIA", Protected),
    ("nvidia overlay", "NVIDIA", Protected),
    ("nvidia app", "NVIDIA", Protected),
    ("nvsphelper64", "NVIDIA", Protected),
    ("radeonsoftware", "AMD", Protected),
    ("amdrsserv", "AMD", Protected),
    ("atieclxx", "AMD", Protected),
    ("igfxem", "Intel", Protected),
    ("rtkauduservice64", "Realtek", Protected),
    // Crash reporters that ship inside games: harmless, never the game itself.
    ("unitycrashhandler64", "Windows", Protected),
    ("unitycrashhandler32", "Windows", Protected),
    ("crashreportclient", "Windows", Protected),
    // Antivirus.
    ("msmpeng", "Antivirus", Protected),
    ("nissrv", "Antivirus", Protected),
    ("mpdefendercoreservice", "Antivirus", Protected),
    ("securityhealthservice", "Antivirus", Protected),
    ("securityhealthsystray", "Antivirus", Protected),
    ("avastui", "Antivirus", Protected),
    ("avastsvc", "Antivirus", Protected),
    ("avgui", "Antivirus", Protected),
    ("avgsvc", "Antivirus", Protected),
    ("bdagent", "Antivirus", Protected),
    ("vsserv", "Antivirus", Protected),
    ("bdservicehost", "Antivirus", Protected),
    ("ekrn", "Antivirus", Protected),
    ("egui", "Antivirus", Protected),
    ("mcshield", "Antivirus", Protected),
    ("mfemms", "Antivirus", Protected),
    ("avp", "Antivirus", Protected),
    ("avpui", "Antivirus", Protected),
    ("mbam", "Antivirus", Protected),
    ("mbamservice", "Antivirus", Protected),
    ("nortonsecurity", "Antivirus", Protected),
    ("wrsa", "Antivirus", Protected),
    // Anticheats: closing them kicks the streamer out of the game.
    ("easyanticheat", "Anticheat", Protected),
    ("easyanticheat_eos", "Anticheat", Protected),
    ("beservice", "Anticheat", Protected),
    ("beservice_x64", "Anticheat", Protected),
    ("vgc", "Anticheat", Protected),
    ("vgtray", "Anticheat", Protected),
    ("faceit", "Anticheat", Protected),
    ("faceitservice", "Anticheat", Protected),
];

/// Looks up a normalised executable name.
pub fn find(exe: &str) -> Option<(&'static str, Category)> {
    PROGRAMS.iter().find(|(e, _, _)| *e == exe).map(|(_, name, cat)| (*name, *cat))
}

/// The OBS-specific fix for this executable, when the knowledge base has one.
pub fn note(exe: &str) -> Option<&'static str> {
    NOTES.iter().find(|(e, _)| *e == exe).map(|(_, note)| *note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn has_more_than_two_hundred_programs() {
        assert!(PROGRAMS.len() > 200, "only {}", PROGRAMS.len());
    }

    #[test]
    fn no_executable_is_listed_twice() {
        let mut seen = HashSet::new();
        for (exe, _, _) in PROGRAMS {
            assert!(seen.insert(*exe), "duplicate: {exe}");
        }
    }

    #[test]
    fn executable_names_are_already_normalised() {
        for (exe, name, _) in PROGRAMS {
            assert_eq!(*exe, exe.to_lowercase(), "{exe}");
            assert!(!exe.ends_with(".exe") && !exe.contains(['\\', '/']), "{exe}");
            assert!(!exe.is_empty() && !name.is_empty());
        }
    }

    #[test]
    fn one_program_never_lands_in_two_categories() {
        // Helpers share a friendly name; that name must mean one thing.
        let mut by_name: HashMap<&str, Category> = HashMap::new();
        for (exe, name, cat) in PROGRAMS {
            let first = *by_name.entry(name).or_insert(*cat);
            assert_eq!(first, *cat, "{name} ({exe}) is in two categories");
        }
    }

    #[test]
    fn protected_programs_are_never_shown() {
        for (exe, _, cat) in PROGRAMS {
            if *cat == Protected {
                assert_eq!(cat.kind(), None, "{exe}");
            } else {
                assert!(cat.kind().is_some(), "{exe}");
            }
        }
        for exe in ["svchost", "msmpeng", "easyanticheat", "vgc", "nvcontainer", "audiodg", "beservice"] {
            assert_eq!(find(exe).map(|(_, c)| c), Some(Protected), "{exe}");
        }
    }

    #[test]
    fn known_obs_conflicts_are_covered() {
        // https://obsproject.com/kb/known-conflicts
        for (exe, cat) in [
            ("msiafterburner", Overlay),
            ("rtss", Overlay),
            ("precisionx_x64", Overlay),
            ("overwolf", Overlay),
            ("medal", Recorder),
            ("action", Recorder),
            ("dxtory", Recorder),
            ("fraps", Recorder),
            ("d3dgear", Recorder),
            ("nahimicsvc64", Conflict),
            ("sendori", Conflict),
            ("killernetworkservice", NetShaper),
            ("lenovovantageservice", NetShaper),
        ] {
            assert_eq!(find(exe).map(|(_, c)| c), Some(cat), "{exe}");
        }
    }

    #[test]
    fn every_note_belongs_to_a_listed_program() {
        for (exe, note) in NOTES {
            let (_, cat) = find(exe).unwrap_or_else(|| panic!("{exe} has a note but is not listed"));
            assert_ne!(cat, Protected);
            assert!(!note.is_empty());
        }
    }

    #[test]
    fn every_shown_category_has_a_tip() {
        for (_, _, cat) in PROGRAMS {
            let shown_with_advice = !matches!(cat, StreamTool | Game | Protected);
            assert_eq!(!cat.tip().is_empty(), shown_with_advice, "{cat:?}");
        }
    }
}
