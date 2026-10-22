//! The network the computer is on, as the bar's network indicator and its panel show it
//! (DECISIONS D88). Pure: what Windows reports in, what to draw out.

/// How the computer reaches the network: Windows' taskbar shows Ethernet before Wi-Fi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// No connection (or every radio off).
    None,
    Ethernet,
    /// Connected to a Wi-Fi network; `signal` is its quality, 0 to 100.
    Wifi {
        ssid: String,
        signal: u8,
    },
}

/// Signal quality from which Windows draws three arcs, and two.
const THREE_ARCS: u8 = 67;
const TWO_ARCS: u8 = 34;

/// How many of the three arcs a Wi-Fi signal of `quality` (0 to 100) lights.
pub fn arcs(quality: u8) -> u8 {
    if quality >= THREE_ARCS {
        3
    } else if quality >= TWO_ARCS {
        2
    } else {
        1
    }
}

/// The design page's icon for a Wi-Fi signal of `quality`.
pub fn wifi_icon(quality: u8) -> &'static str {
    match arcs(quality) {
        3 => "wifi",
        2 => "wifi-2",
        _ => "wifi-1",
    }
}

/// The bar's icon for `link`.
pub fn icon(link: &Link) -> &'static str {
    match link {
        Link::None => "wifi-off",
        Link::Ethernet => "eth",
        Link::Wifi { signal, .. } => wifi_icon(*signal),
    }
}

/// A Wi-Fi network the panel lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Network {
    pub ssid: String,
    /// Signal quality, 0 to 100.
    pub signal: u8,
    /// Asks for a key to join.
    pub secured: bool,
    /// Joined with WPA3-Personal (SAE) rather than WPA2-Personal.
    pub wpa3: bool,
    /// Windows has a profile for it (joined before): no key is asked again.
    pub known: bool,
    pub connected: bool,
}

/// The networks as the panel lists them: the connected one first, then by signal,
/// strongest first; one row per name (Windows reports a network once per access point
/// and profile), hidden networks (no name) left out.
pub fn listed(mut networks: Vec<Network>) -> Vec<Network> {
    networks.retain(|n| !n.ssid.is_empty());
    networks.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
    let mut out: Vec<Network> = Vec::new();
    for n in networks {
        match out.iter_mut().find(|o| o.ssid == n.ssid) {
            Some(o) => {
                o.known |= n.known;
                o.secured |= n.secured;
            }
            None => out.push(n),
        }
    }
    out
}

/// A WLAN profile for joining `ssid` (Windows' profile schema): WPA2-Personal with
/// `key` when given (WPA3-Personal with `wpa3`), open otherwise. The name and key are
/// escaped for XML.
pub fn profile_xml(ssid: &str, key: Option<&str>, wpa3: bool) -> String {
    let name = xml_escape(ssid);
    let hex: String = ssid.bytes().map(|b| format!("{b:02X}")).collect();
    let security = match key {
        Some(k) => format!(
            "<authEncryption><authentication>{}</authentication><encryption>AES</encryption><useOneX>false</useOneX></authEncryption>\
             <sharedKey><keyType>passPhrase</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey>",
            if wpa3 { "WPA3SAE" } else { "WPA2PSK" },
            xml_escape(k)
        ),
        None => "<authEncryption><authentication>open</authentication><encryption>none</encryption><useOneX>false</useOneX></authEncryption>".to_string(),
    };
    format!(
        "<?xml version=\"1.0\"?>\
         <WLANProfile xmlns=\"http://www.microsoft.com/networking/WLAN/profile/v1\">\
         <name>{name}</name>\
         <SSIDConfig><SSID><hex>{hex}</hex><name>{name}</name></SSID></SSIDConfig>\
         <connectionType>ESS</connectionType><connectionMode>auto</connectionMode>\
         <MSM><security>{security}</security></MSM>\
         </WLANProfile>"
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Whether `key` can be a WPA2-Personal key: a passphrase of 8 to 63 characters, or 64
/// hexadecimal digits.
pub fn key_fits(key: &str) -> bool {
    let n = key.chars().count();
    (8..=63).contains(&n) || (n == 64 && key.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    //! Edge cases: the arcs' borders, no connection, Ethernet; the list's order, the same
    //! network from two access points, a hidden network; names and keys with XML's
    //! special characters; keys too short, too long, 64 hex digits.
    use super::*;

    #[test]
    fn a_signal_lights_its_arcs() {
        assert_eq!(arcs(100), 3);
        assert_eq!(arcs(67), 3);
        assert_eq!(arcs(66), 2);
        assert_eq!(arcs(34), 2);
        assert_eq!(arcs(33), 1);
        assert_eq!(arcs(0), 1);
        assert_eq!(wifi_icon(80), "wifi");
        assert_eq!(wifi_icon(50), "wifi-2");
        assert_eq!(wifi_icon(10), "wifi-1");
    }

    #[test]
    fn each_link_has_its_icon() {
        assert_eq!(icon(&Link::None), "wifi-off");
        assert_eq!(icon(&Link::Ethernet), "eth");
        let wifi = Link::Wifi {
            ssid: "milva".into(),
            signal: 40,
        };
        assert_eq!(icon(&wifi), "wifi-2");
    }

    fn net(ssid: &str, signal: u8, connected: bool) -> Network {
        Network {
            ssid: ssid.into(),
            signal,
            secured: true,
            wpa3: false,
            known: false,
            connected,
        }
    }

    #[test]
    fn the_connected_network_leads_and_the_rest_follow_by_signal() {
        let listed = listed(vec![
            net("weak", 20, false),
            net("", 99, false),
            net("home", 30, true),
            net("strong", 90, false),
            Network {
                known: true,
                secured: false,
                ..net("strong", 60, false)
            },
        ]);
        let names: Vec<&str> = listed.iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(names, ["home", "strong", "weak"]);
        // One row per name, with the strongest signal and what any of them knows.
        assert_eq!(listed[1].signal, 90);
        assert!(listed[1].known && listed[1].secured);
    }

    #[test]
    fn a_profile_names_the_network_and_escapes_it() {
        let p = profile_xml("Ev & <Ofis>", Some("şifre\"123"), false);
        assert!(p.contains("<name>Ev &amp; &lt;Ofis&gt;</name>"));
        assert!(p.contains("<hex>45762026203C4F6669733E</hex>"));
        assert!(p.contains("<keyMaterial>şifre&quot;123</keyMaterial>"));
        assert!(p.contains("WPA2PSK"));
        let open = profile_xml("Kafe", None, false);
        assert!(profile_xml("Yeni", Some("12345678"), true).contains("<authentication>WPA3SAE</authentication>"));
        assert!(open.contains("<authentication>open</authentication>") && !open.contains("sharedKey"));
    }

    #[test]
    fn a_key_must_fit_wpa2() {
        assert!(!key_fits("1234567"));
        assert!(key_fits("12345678"));
        assert!(key_fits(&"a".repeat(63)));
        assert!(!key_fits(&"a".repeat(65)));
        assert!(key_fits(&"0f".repeat(32)));
        assert!(!key_fits(&"zz".repeat(32)));
    }
}
