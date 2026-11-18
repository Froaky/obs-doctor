//! The internet connection: how it is connected and how fast and steady it is.
//!
//! What this version does: it asks Windows which network adapter carries the
//! traffic to the internet (a routing-table lookup, no packet is sent), whether
//! that adapter is a cable or wifi, how fast the link to the router is, and on
//! wifi the signal and the band. Nothing leaves the PC.
//!
//! What it does not do (yet): measure upload, download, latency and jitter.
//! The plan was to use speed.cloudflare.com, but its terms do not cover use
//! from a third-party tool: the site terms forbid automated bots on its
//! content (section 8, https://www.cloudflare.com/website-terms/) and its
//! robots.txt disallows the test endpoints `/__down` and `/__up` for every
//! client (https://speed.cloudflare.com/robots.txt). Until a service that
//! allows it is chosen, `measured` stays false and the rules that need the
//! numbers simply do not fire. They are already written against the CONTRACT
//! fields so that a future measurement only has to fill them in.

use crate::facts::Facts;
use crate::model::{Advice, Area, Level, Review, Row, Section, Tone};
use crate::obs::menu;
use crate::platforms;
use crate::report::text;

/// How the PC reaches the internet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Connection {
    #[default]
    Unknown,
    Cable,
    Wifi,
    /// Phone tethering, mobile broadband, anything else.
    Other,
}

/// The radio band a wifi connection uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Ghz2_4,
    Ghz5,
    Ghz6,
}

impl Band {
    pub fn label(self) -> &'static str {
        match self {
            Band::Ghz2_4 => "2,4 GHz",
            Band::Ghz5 => "5 GHz",
            Band::Ghz6 => "6 GHz",
        }
    }
}

/// What we know about the connection. `Default` means "not measured".
///
/// CONTRACT: other areas read the fields below. Keep their names, types and
/// meaning; add more fields freely.
#[derive(Debug, Clone, Default)]
pub struct NetFacts {
    /// True only when the speed test actually ran and produced numbers.
    pub measured: bool,
    pub connection: Connection,
    /// Sustained upload speed in megabits per second. The number that
    /// decides how much bitrate a stream can carry.
    pub upload_mbps: Option<f64>,
    pub download_mbps: Option<f64>,
    /// Typical round trip in milliseconds.
    pub latency_ms: Option<f64>,
    /// How much the round trip varies, in milliseconds.
    pub jitter_ms: Option<f64>,
    // ---- added in v0.2, not part of the contract ----
    /// Speed of the link between the PC and the router (not the internet
    /// plan), in megabits per second, as the adapter reports it.
    pub link_mbps: Option<f64>,
    /// Wifi signal quality as Windows reports it, 0 to 100.
    pub wifi_signal: Option<u32>,
    pub wifi_band: Option<Band>,
    /// The streamer asked not to use the internet (`--sin-internet`).
    pub skipped_by_user: bool,
}

/// Measures the connection. `progress` receives short status lines to show
/// while it works ("Midiendo la subida..."). Must never panic and must give
/// up politely when there is no internet.
pub fn read(progress: &mut dyn FnMut(&str)) -> NetFacts {
    progress("No envío ni descargo datos de prueba: solo miro cómo está conectada esta PC.");
    connection_facts()
}

/// What we can know without sending or downloading test data (the streamer
/// passed `--sin-internet`): connection type yes, speeds no.
pub fn skipped() -> NetFacts {
    NetFacts { skipped_by_user: true, ..connection_facts() }
}

fn connection_facts() -> NetFacts {
    let Some(link) = platform::active_link() else {
        return NetFacts::default();
    };
    let connection = connection_of(link.if_type, link.physical_medium);
    let wifi = (connection == Connection::Wifi).then(|| platform::wifi_details(&link)).flatten();
    NetFacts {
        connection,
        link_mbps: link_mbps(link.transmit_bps),
        wifi_signal: wifi.and_then(|w| w.signal),
        wifi_band: wifi.and_then(|w| w.band),
        ..NetFacts::default()
    }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

// Values from ipifcons.h and ntddndis.h (IANA ifType and NDIS_PHYSICAL_MEDIUM).
const IF_TYPE_ETHERNET: u32 = 6;
const IF_TYPE_PPP: u32 = 23;
const IF_TYPE_IEEE80211: u32 = 71;
const IF_TYPE_WWANPP: u32 = 243;
const IF_TYPE_WWANPP2: u32 = 244;
const MEDIUM_WIRELESS_LAN: i32 = 1;
const MEDIUM_WIRELESS_WAN: i32 = 8;
const MEDIUM_NATIVE_802_11: i32 = 9;
const MEDIUM_802_3: i32 = 14;

/// Classifies an adapter from its interface type and physical medium.
/// Only a real Ethernet port counts as cable: VPNs, virtual switches and USB
/// phone tethering also call themselves "Ethernet" but hide what is really
/// underneath, so they stay `Unknown` rather than risk a wrong "all good".
fn connection_of(if_type: u32, physical_medium: i32) -> Connection {
    match (if_type, physical_medium) {
        (IF_TYPE_IEEE80211, _) | (_, MEDIUM_NATIVE_802_11 | MEDIUM_WIRELESS_LAN) => Connection::Wifi,
        (IF_TYPE_WWANPP | IF_TYPE_WWANPP2 | IF_TYPE_PPP, _) | (_, MEDIUM_WIRELESS_WAN) => Connection::Other,
        (IF_TYPE_ETHERNET, MEDIUM_802_3) => Connection::Cable,
        _ => Connection::Unknown,
    }
}

/// Link speed in Mbps from the bits per second the adapter reports. Windows
/// uses 0 and `u64::MAX` for "unknown".
fn link_mbps(bps: u64) -> Option<f64> {
    (bps > 0 && bps != u64::MAX).then(|| bps as f64 / 1_000_000.0)
}

/// The band of a wifi channel from its centre frequency in kHz, as
/// `WLAN_BSS_ENTRY::ulChCenterFrequency` gives it.
fn band_from_khz(khz: u32) -> Option<Band> {
    match khz {
        2_400_000..=2_500_000 => Some(Band::Ghz2_4),
        4_900_000..=5_924_999 => Some(Band::Ghz5),
        5_925_000..=7_125_000 => Some(Band::Ghz6),
        _ => None,
    }
}

/// Windows maps signal quality linearly: 0 is -100 dBm and 100 is -50 dBm
/// (https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/ns-wlanapi-wlan_association_attributes).
fn signal_dbm(quality: u32) -> i32 {
    -100 + quality.min(100) as i32 / 2
}

/// How good a wifi signal is for a live stream. The cut-offs are the usual
/// rules of thumb for live video over wifi, not a standard: about -67 dBm or
/// better is reliable, below about -80 dBm connections start to drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Signal {
    Good,
    Fair,
    Weak,
}

pub(crate) fn signal_class(quality: u32) -> Signal {
    match signal_dbm(quality) {
        d if d >= -67 => Signal::Good,
        d if d >= -80 => Signal::Fair,
        _ => Signal::Weak,
    }
}

/// Below this the PC and the router are talking at old 10 Mbps Ethernet
/// speed, which almost always means a damaged cable or a bad port.
const SLOW_CABLE_MBPS: f64 = 100.0;

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

pub fn sections(facts: &Facts) -> Vec<Section> {
    let net = &facts.net;
    let mut section = Section::new("net", "Tu internet");

    let (kind, tone) = match net.connection {
        Connection::Cable => ("Cable de red", Tone::Good),
        Connection::Wifi => ("Wifi", Tone::Warn),
        Connection::Other => ("Otra (módem del celular u otra)", Tone::Neutral),
        Connection::Unknown => ("No pude saberlo", Tone::Neutral),
    };
    let mut row = Row::new("Conexión", kind).tone(tone);
    if net.connection == Connection::Unknown {
        row = row.note("Puede ser una VPN, un adaptador virtual o el celular conectado por USB.");
    }
    section.push(row);

    if let Some(mbps) = net.link_mbps {
        section.push(
            Row::new("Enlace con el router", text::mbps(mbps))
                .note("Es la velocidad entre tu PC y el router, no la de tu plan de internet."),
        );
    }
    if let Some(quality) = net.wifi_signal {
        let (word, tone) = match signal_class(quality) {
            Signal::Good => ("buena", Tone::Good),
            Signal::Fair => ("regular", Tone::Warn),
            Signal::Weak => ("débil", Tone::Bad),
        };
        section
            .push(Row::new("Señal del wifi", format!("{quality} % ({word})")).tone(tone).meter(quality as f32 / 100.0));
    }
    if let Some(band) = net.wifi_band {
        let tone = if band == Band::Ghz2_4 { Tone::Warn } else { Tone::Good };
        section.push(Row::new("Banda del wifi", band.label()).tone(tone));
    }

    if net.measured {
        if let Some(up) = net.upload_mbps {
            let mut row = Row::new("Subida", text::mbps(up));
            if let Some(max) = max_bitrate_for_upload(Some(up)) {
                row = row.note(format!("Con tu subida puedes enviar hasta {max} kbps."));
            }
            section.push(row);
        }
        if let Some(down) = net.download_mbps {
            section.push(Row::new("Bajada", text::mbps(down)));
        }
        if let Some(ms) = net.latency_ms {
            section.push(Row::new("Tiempo de respuesta", format!("{} ms", ms.round())));
        }
        if let Some(ms) = net.jitter_ms {
            section.push(Row::new("Variación", format!("{} ms", ms.round())));
        }
    } else {
        let example = max_bitrate_for_upload(Some(9.0)).unwrap_or(6000);
        section = section.intro(format!(
            "No medí la velocidad de tu internet. Para saber cuánto puedes enviar, mira la subida en un medidor de velocidad y usa como mucho dos tercios: con 9 Mbps de subida puedes enviar hasta {example} kbps."
        ));
    }
    vec![section]
}

pub fn advise(facts: &Facts, review: &mut Review) {
    let net = &facts.net;
    match net.connection {
        Connection::Wifi => advise_wifi(net, review),
        Connection::Cable => advise_cable(net, review),
        Connection::Other => {}
        Connection::Unknown => review.note("No pude saber si te conectas a internet por cable o por wifi."),
    }

    if !net.measured {
        if net.skipped_by_user {
            review.note("No medí la velocidad de tu internet porque usaste --sin-internet.");
        } else {
            review.note("Esta versión todavía no mide la velocidad de tu internet.");
        }
        return;
    }
    check_upload(net, review);
}

fn advise_wifi(net: &NetFacts, review: &mut Review) {
    let signal = net.wifi_signal.map(signal_class);
    let on_2_4 = net.wifi_band == Some(Band::Ghz2_4);
    let weak = matches!(signal, Some(Signal::Fair | Signal::Weak));

    // Wifi is never ideal for a stream, but how much it hurts depends on the
    // signal and the band. Without a measured upload this stays a suggestion
    // to try: it never reaches Important on its own.
    let level = if weak || on_2_4 { Level::Recommended } else { Level::Tip };
    let why = match (signal, on_2_4) {
        (Some(Signal::Weak), _) => {
            "La señal del wifi es débil. Con una señal así, el directo se corta o pierde cuadros cuando alguien más usa la red o te alejas del router."
        }
        (Some(Signal::Fair), _) => {
            "La señal del wifi es regular. El wifi tiene bajones de un segundo que no notas al navegar, pero en directo se ven como cortes o cuadros perdidos."
        }
        (_, true) => {
            "Estás en la banda de 2,4 GHz, la más lenta y la que más comparten los vecinos. En directo eso se nota como cortes o cuadros perdidos."
        }
        _ => {
            "Tu wifi está bien, pero el wifi tiene bajones de un segundo que no notas al navegar y que en directo se ven como cortes. Con cable no pasa."
        }
    };

    let mut steps = vec![
        "Conecta la PC al router con un cable de red. Si el router está lejos, un adaptador de red por el enchufe eléctrico suele funcionar mejor que el wifi.".to_string(),
    ];
    if on_2_4 {
        steps.push(
            "Si por ahora no puedes usar cable, conéctate a la red de 5 GHz de tu router. Suele tener el mismo nombre con \"5G\" al final.".to_string(),
        );
    }
    if weak {
        steps.push(
            "Si sigues en wifi, acerca la PC al router o quita lo que haya entre ellos (paredes, muebles de metal)."
                .to_string(),
        );
    }
    steps.push("Vuelve a correr OBS Doctor para ver el cambio.".to_string());

    review.add(Advice {
        id: "net.wifi.use-cable",
        level,
        area: Area::Internet,
        title: "Conecta la PC al router con un cable".to_string(),
        why: why.to_string(),
        steps,
    });
}

fn advise_cable(net: &NetFacts, review: &mut Review) {
    match net.link_mbps {
        Some(mbps) if mbps < SLOW_CABLE_MBPS => review.add(Advice {
            id: "net.cable.slow-link",
            level: Level::Recommended,
            area: Area::Internet,
            title: "Cambia el cable de red o el puerto del router".to_string(),
            why: format!(
                "Tu PC y el router se conectan a solo {}. Eso suele ser un cable dañado o mal enchufado, y limita lo que puedes enviar.",
                text::mbps(mbps)
            ),
            steps: vec![
                "Desenchufa el cable de los dos lados y vuelve a enchufarlo hasta que haga clic.".to_string(),
                "Si sigue igual, prueba otro puerto del router y después otro cable.".to_string(),
                "Vuelve a correr OBS Doctor para ver el cambio.".to_string(),
            ],
        }),
        _ => review.ok("Estás conectado por cable, que es lo más estable para transmitir."),
    }
}

/// Compares the measured upload with what the smallest sensible streams need
/// (the same bitrate table OBS rules use). How it compares with the bitrate
/// actually set in OBS belongs to `obs.*`.
fn check_upload(net: &NetFacts, review: &mut Review) {
    let Some(max) = max_bitrate_for_upload(net.upload_mbps) else {
        return;
    };
    let for_720p = platforms::bitrate_for(720, 30.0);
    let for_480p = platforms::bitrate_for(480, 30.0);
    if max >= for_720p {
        review.ok(format!("Tu subida alcanza para enviar hasta {max} kbps."));
        return;
    }
    let (level, height) = if max < for_480p { (Level::Important, 480) } else { (Level::Recommended, 720) };
    review.add(Advice {
        id: "net.upload.too-low",
        level,
        area: Area::Internet,
        title: format!("Baja la calidad de envío a {max} kbps o menos"),
        why: format!(
            "La subida de tu internet es de {}. No alcanza para una imagen de {height}p a 30 cuadros: el directo se va a cortar.",
            text::mbps(net.upload_mbps.unwrap_or_default())
        ),
        steps: vec![
            menu::open(menu::OUTPUT),
            format!("En \"{}\" escribe {max} o menos.", menu::BITRATE),
            "Si puedes, conecta la PC por cable y cierra en otros equipos lo que esté subiendo archivos o videos.".to_string(),
        ],
    });
}

/// Upload speed (Mbps) a stream of this bitrate needs, leaving headroom:
/// the stream should use about two thirds of the upload at most.
pub fn upload_needed_mbps(bitrate_kbps: u32) -> u32 {
    ((bitrate_kbps as f64 * 1.5) / 1000.0).ceil() as u32
}

/// The highest video bitrate (kbps) this upload can carry with the same
/// headroom, or `None` when the upload was not measured.
pub fn max_bitrate_for_upload(upload_mbps: Option<f64>) -> Option<u32> {
    upload_mbps.filter(|u| *u > 0.0).map(|u| (u * 1000.0 / 1.5).floor() as u32)
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

/// The adapter that carries internet traffic, as Windows describes it.
struct ActiveLink {
    if_type: u32,
    physical_medium: i32,
    transmit_bps: u64,
    #[cfg_attr(not(windows), allow(dead_code))]
    guid: [u8; 16],
}

#[derive(Debug, Clone, Copy, Default)]
struct WifiDetails {
    signal: Option<u32>,
    band: Option<Band>,
}

#[cfg(not(windows))]
mod platform {
    use super::{ActiveLink, WifiDetails};

    pub fn active_link() -> Option<ActiveLink> {
        None
    }

    pub fn wifi_details(_link: &ActiveLink) -> Option<WifiDetails> {
        None
    }
}

#[cfg(windows)]
mod platform {
    use super::{ActiveLink, WifiDetails, band_from_khz};
    use std::mem::size_of;
    use std::ptr::{addr_of, null_mut};

    use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
    use windows::Win32::NetworkManagement::IpHelper::{GetBestInterfaceEx, GetIfEntry2, MIB_IF_ROW2};
    use windows::Win32::NetworkManagement::Ndis::MediaConnectStateConnected;
    use windows::Win32::NetworkManagement::WiFi::{
        WLAN_API_VERSION_2_0, WLAN_BSS_ENTRY, WLAN_BSS_LIST, WLAN_CONNECTION_ATTRIBUTES, WlanCloseHandle,
        WlanFreeMemory, WlanGetNetworkBssList, WlanOpenHandle, WlanQueryInterface, dot11_BSS_type_any,
        wlan_intf_opcode_current_connection,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, IN_ADDR, IN_ADDR_0, SOCKADDR, SOCKADDR_IN};
    use windows::core::GUID;

    /// Any public address works: Windows only looks it up in the routing
    /// table to pick the adapter it would use. Nothing is sent to it.
    const PUBLIC_ADDRESS: [u8; 4] = [1, 1, 1, 1];

    pub fn active_link() -> Option<ActiveLink> {
        let destination = SOCKADDR_IN {
            sin_family: AF_INET,
            sin_addr: IN_ADDR { S_un: IN_ADDR_0 { S_addr: u32::from_ne_bytes(PUBLIC_ADDRESS) } },
            ..Default::default()
        };
        let mut index = 0u32;
        // SAFETY: `destination` is a valid SOCKADDR_IN that outlives the call
        // and starts with the SOCKADDR header the API reads; `index` is a
        // valid out pointer. The return code is checked.
        let status = unsafe { GetBestInterfaceEx(addr_of!(destination).cast::<SOCKADDR>(), &mut index) };
        if status != ERROR_SUCCESS.0 {
            return None;
        }

        let mut row = MIB_IF_ROW2 { InterfaceIndex: index, ..Default::default() };
        // SAFETY: `row` is a zeroed MIB_IF_ROW2 with InterfaceIndex set, as
        // GetIfEntry2 requires; it only writes into it. Result is checked.
        let status = unsafe { GetIfEntry2(&mut row) };
        if status != ERROR_SUCCESS || row.MediaConnectState != MediaConnectStateConnected {
            return None;
        }
        Some(ActiveLink {
            if_type: row.Type,
            physical_medium: row.PhysicalMediumType.0,
            transmit_bps: row.TransmitLinkSpeed,
            guid: row.InterfaceGuid.to_u128().to_be_bytes(),
        })
    }

    /// Owns a WLAN client handle and closes it when dropped.
    struct Client(HANDLE);

    impl Client {
        fn open() -> Option<Self> {
            let mut negotiated = 0u32;
            let mut handle = HANDLE::default();
            // SAFETY: both out pointers are valid locals. On success the
            // handle is owned by `Client` and closed in Drop.
            let status = unsafe { WlanOpenHandle(WLAN_API_VERSION_2_0, None, &mut negotiated, &mut handle) };
            (status == ERROR_SUCCESS.0).then_some(Client(handle))
        }
    }

    impl Drop for Client {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful WlanOpenHandle and is
            // closed exactly once, here.
            unsafe {
                WlanCloseHandle(self.0, None);
            }
        }
    }

    /// Memory returned by the WLAN API, freed with WlanFreeMemory on drop.
    struct WlanMemory(*mut core::ffi::c_void);

    impl Drop for WlanMemory {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the pointer was allocated by the WLAN API and is
                // freed exactly once, here.
                unsafe { WlanFreeMemory(self.0) };
            }
        }
    }

    /// Signal and band of the wifi network this adapter is connected to.
    /// Windows 11 may refuse both without location permission; then the
    /// report simply leaves them out.
    pub fn wifi_details(link: &ActiveLink) -> Option<WifiDetails> {
        let guid = GUID::from_u128(u128::from_be_bytes(link.guid));
        let client = Client::open()?;

        let mut size = 0u32;
        let mut data: *mut core::ffi::c_void = null_mut();
        // SAFETY: valid handle, GUID and out pointers. The returned buffer is
        // owned by `memory` and freed on drop.
        let status = unsafe {
            WlanQueryInterface(client.0, &guid, wlan_intf_opcode_current_connection, None, &mut size, &mut data, None)
        };
        let memory = WlanMemory(data);
        if status != ERROR_SUCCESS.0 || memory.0.is_null() || (size as usize) < size_of::<WLAN_CONNECTION_ATTRIBUTES>()
        {
            return None;
        }
        // SAFETY: the API succeeded and the buffer is at least as large as a
        // WLAN_CONNECTION_ATTRIBUTES, which is what this opcode returns.
        let association = unsafe { (*memory.0.cast::<WLAN_CONNECTION_ATTRIBUTES>()).wlanAssociationAttributes };

        // The access point's hardware address is only used here, in memory,
        // to find its channel. It is never stored or shown.
        let band = channel_khz(&client, &guid, &association.dot11Bssid).and_then(band_from_khz);
        Some(WifiDetails { signal: Some(association.wlanSignalQuality.min(100)), band })
    }

    fn channel_khz(client: &Client, guid: &GUID, bssid: &[u8; 6]) -> Option<u32> {
        let mut list: *mut WLAN_BSS_LIST = null_mut();
        // SAFETY: valid handle, GUID and out pointer. With no SSID the
        // security flag is ignored. The list is freed on drop.
        let status = unsafe { WlanGetNetworkBssList(client.0, guid, None, dot11_BSS_type_any, false, None, &mut list) };
        let memory = WlanMemory(list.cast());
        if status != ERROR_SUCCESS.0 || list.is_null() {
            return None;
        }
        // SAFETY: the API succeeded, so `list` points to a WLAN_BSS_LIST
        // header. The entry count is only trusted after checking it against
        // the total size the API reports.
        let entries = unsafe {
            let header = &*list;
            let first = addr_of!((*list).wlanBssEntries).cast::<WLAN_BSS_ENTRY>();
            let offset = (first as usize).saturating_sub(list as usize);
            let count = header.dwNumberOfItems as usize;
            let needed = count.checked_mul(size_of::<WLAN_BSS_ENTRY>())?.checked_add(offset)?;
            if needed > header.dwTotalSize as usize {
                return None;
            }
            std::slice::from_raw_parts(first, count)
        };
        let khz = entries.iter().find(|e| &e.dot11Bssid == bssid).map(|e| e.ulChCenterFrequency);
        drop(memory);
        khz
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(net: NetFacts) -> Facts {
        Facts { net, ..Facts::default() }
    }

    fn reviewed(net: NetFacts) -> Review {
        let mut review = Review::default();
        advise(&facts(net), &mut review);
        review
    }

    fn wifi(signal: Option<u32>, band: Option<Band>) -> NetFacts {
        NetFacts { connection: Connection::Wifi, wifi_signal: signal, wifi_band: band, ..NetFacts::default() }
    }

    fn cable(link: f64) -> NetFacts {
        NetFacts { connection: Connection::Cable, link_mbps: Some(link), ..NetFacts::default() }
    }

    fn measured(upload: f64) -> NetFacts {
        NetFacts { measured: true, upload_mbps: Some(upload), ..cable(1000.0) }
    }

    #[test]
    fn upload_needed_leaves_headroom() {
        assert_eq!(upload_needed_mbps(3000), 5);
        assert_eq!(upload_needed_mbps(6000), 9);
    }

    #[test]
    fn bitrate_cap_follows_the_upload() {
        assert_eq!(max_bitrate_for_upload(Some(9.0)), Some(6000));
        assert_eq!(max_bitrate_for_upload(Some(3.0)), Some(2000));
        assert_eq!(max_bitrate_for_upload(Some(0.0)), None);
        assert_eq!(max_bitrate_for_upload(None), None);
    }

    #[test]
    fn adapters_are_classified_conservatively() {
        assert_eq!(connection_of(IF_TYPE_ETHERNET, MEDIUM_802_3), Connection::Cable);
        assert_eq!(connection_of(IF_TYPE_IEEE80211, MEDIUM_NATIVE_802_11), Connection::Wifi);
        assert_eq!(connection_of(IF_TYPE_IEEE80211, 0), Connection::Wifi);
        assert_eq!(connection_of(IF_TYPE_WWANPP, MEDIUM_WIRELESS_WAN), Connection::Other);
        assert_eq!(connection_of(IF_TYPE_PPP, 0), Connection::Other);
        // A VPN or virtual switch claims Ethernet with no physical medium.
        assert_eq!(connection_of(IF_TYPE_ETHERNET, 0), Connection::Unknown);
        assert_eq!(connection_of(131, 0), Connection::Unknown);
    }

    #[test]
    fn link_speed_ignores_unknown_values() {
        assert_eq!(link_mbps(1_000_000_000), Some(1000.0));
        assert_eq!(link_mbps(0), None);
        assert_eq!(link_mbps(u64::MAX), None);
    }

    #[test]
    fn bands_follow_the_centre_frequency() {
        assert_eq!(band_from_khz(2_437_000), Some(Band::Ghz2_4));
        assert_eq!(band_from_khz(5_180_000), Some(Band::Ghz5));
        assert_eq!(band_from_khz(5_955_000), Some(Band::Ghz6));
        assert_eq!(band_from_khz(0), None);
        assert_eq!(band_from_khz(60_000_000), None);
    }

    #[test]
    fn signal_quality_maps_to_dbm_and_classes() {
        assert_eq!(signal_dbm(0), -100);
        assert_eq!(signal_dbm(100), -50);
        assert_eq!(signal_dbm(250), -50);
        assert_eq!(signal_class(100), Signal::Good);
        assert_eq!(signal_class(66), Signal::Good);
        assert_eq!(signal_class(60), Signal::Fair);
        assert_eq!(signal_class(40), Signal::Fair);
        assert_eq!(signal_class(30), Signal::Weak);
    }

    #[test]
    fn good_wifi_on_5ghz_is_only_a_tip() {
        let review = reviewed(wifi(Some(90), Some(Band::Ghz5)));
        assert_eq!(review.get("net.wifi.use-cable").map(|a| a.level), Some(Level::Tip));
    }

    #[test]
    fn weak_signal_or_2_4ghz_is_recommended() {
        for net in [wifi(Some(30), Some(Band::Ghz5)), wifi(Some(55), None), wifi(Some(95), Some(Band::Ghz2_4))] {
            let review = reviewed(net);
            assert_eq!(
                review.get("net.wifi.use-cable").map(|a| a.level),
                Some(Level::Recommended),
                "{:?}",
                review.ids()
            );
        }
    }

    #[test]
    fn wifi_never_becomes_important_without_a_measurement() {
        let review = reviewed(wifi(Some(5), Some(Band::Ghz2_4)));
        assert_eq!(review.count(Level::Important), 0);
    }

    #[test]
    fn wifi_with_unknown_signal_still_suggests_cable() {
        let review = reviewed(wifi(None, None));
        assert_eq!(review.get("net.wifi.use-cable").map(|a| a.level), Some(Level::Tip));
    }

    #[test]
    fn healthy_cable_is_reported_as_fine() {
        let review = reviewed(cable(1000.0));
        assert!(review.advice.is_empty(), "{:?}", review.ids());
        assert_eq!(review.good.len(), 1);
    }

    #[test]
    fn a_10_mbps_cable_link_is_flagged() {
        let review = reviewed(cable(10.0));
        assert_eq!(review.get("net.cable.slow-link").map(|a| a.level), Some(Level::Recommended));
        assert!(!reviewed(cable(100.0)).has("net.cable.slow-link"));
    }

    #[test]
    fn unknown_connection_leaves_a_note_and_no_advice() {
        let review = reviewed(NetFacts::default());
        assert!(review.advice.is_empty());
        assert_eq!(review.notes.len(), 2);
    }

    #[test]
    fn skipped_and_unmeasured_leave_one_speed_note() {
        let skipped = reviewed(NetFacts { skipped_by_user: true, ..cable(1000.0) });
        assert_eq!(skipped.notes.len(), 1);
        let unmeasured = reviewed(cable(1000.0));
        assert_eq!(unmeasured.notes.len(), 1);
    }

    #[test]
    fn upload_rules_follow_the_bitrate_table() {
        // 9 Mbps carries 6000 kbps: fine.
        let fine = reviewed(measured(9.0));
        assert!(!fine.has("net.upload.too-low"));
        assert_eq!(fine.good.len(), 2);
        // 3 Mbps carries 2000 kbps: below 720p30, above 480p.
        let low = reviewed(measured(3.0));
        assert_eq!(low.get("net.upload.too-low").map(|a| a.level), Some(Level::Recommended));
        // 1 Mbps carries 666 kbps: not even 480p.
        let tiny = reviewed(measured(1.0));
        assert_eq!(tiny.get("net.upload.too-low").map(|a| a.level), Some(Level::Important));
    }

    #[test]
    fn unmeasured_upload_never_triggers_upload_rules() {
        let review = reviewed(NetFacts { upload_mbps: Some(0.5), ..cable(1000.0) });
        assert!(!review.has("net.upload.too-low"));
    }

    #[test]
    fn section_shows_what_was_found() {
        let section = sections(&facts(wifi(Some(80), Some(Band::Ghz5)))).remove(0);
        assert_eq!(section.id, "net");
        let values: Vec<&str> = section.rows.iter().map(|r| r.value.as_str()).collect();
        assert!(values.contains(&"5 GHz"), "{values:?}");
        assert!(values.contains(&"80 % (buena)"), "{values:?}");
        assert!(section.intro.is_some());
    }

    #[test]
    fn measured_section_states_the_bitrate_the_upload_allows() {
        let section = sections(&facts(measured(9.0))).remove(0);
        assert!(section.intro.is_none());
        let upload = section.rows.iter().find(|r| r.label == "Subida").expect("upload row");
        assert!(upload.note.as_deref().is_some_and(|n| n.contains("6000")));
    }
}
