//! The privacy indicators (DECISIONS D88): which programs use the microphone, the camera
//! or the location now, as Windows' capability access store records them. Pure.

/// What a program uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Microphone,
    Camera,
    Location,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Microphone, Kind::Camera, Kind::Location];

    /// The store's key for it (`ConsentStore\<key>`).
    pub fn store_key(self) -> &'static str {
        match self {
            Kind::Microphone => "microphone",
            Kind::Camera => "webcam",
            Kind::Location => "location",
        }
    }

    /// The design page's icon.
    pub fn icon(self) -> &'static str {
        match self {
            Kind::Microphone => "mic",
            Kind::Camera => "cam",
            Kind::Location => "loc",
        }
    }

    /// The translation of "<program> is using your …".
    pub fn message_key(self) -> &'static str {
        match self {
            Kind::Microphone => "priv-mic",
            Kind::Camera => "priv-cam",
            Kind::Location => "priv-loc",
        }
    }
}

/// A program using something now.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Use {
    pub kind: Kind,
    pub program: String,
}

/// Whether a record's times say "in use": started, and not stopped since.
pub fn in_use(started: u64, stopped: u64) -> bool {
    started != 0 && stopped == 0
}

/// A program's name from its record's key: a desktop program's path with `#` for `\`
/// (`C:#Program Files#Zoom#bin#Zoom.exe` → `Zoom`), or an app's package family name
/// (`MSTeams_8wekyb3d8bbwe` → `MSTeams`).
pub fn program_name(key: &str) -> String {
    if key.contains('#') {
        let file = key.rsplit('#').next().unwrap_or(key);
        return file.rsplit_once('.').map_or(file, |(stem, _)| stem).to_string();
    }
    key.split_once('_').map_or(key, |(name, _)| name).to_string()
}

/// The kinds in use, in [`Kind::ALL`]'s order, each once.
pub fn kinds(uses: &[Use]) -> Vec<Kind> {
    Kind::ALL
        .into_iter()
        .filter(|k| uses.iter().any(|u| u.kind == *k))
        .collect()
}

#[cfg(test)]
mod tests {
    //! Edge cases: never used, in use, used and stopped; a path with dots in folders, a
    //! file without an extension, a package name, a bare name; kinds repeated and out of
    //! order.
    use super::*;

    #[test]
    fn in_use_is_started_and_not_stopped() {
        assert!(!in_use(0, 0));
        assert!(in_use(133_000, 0));
        assert!(!in_use(133_000, 133_100));
    }

    #[test]
    fn a_program_is_named_from_its_record() {
        assert_eq!(program_name("C:#Program Files#Zoom#bin#Zoom.exe"), "Zoom");
        assert_eq!(
            program_name("C:#Users#a.b#AppData#Discord#app-1.0#Discord.exe"),
            "Discord"
        );
        assert_eq!(program_name("C:#tools#recorder"), "recorder");
        assert_eq!(program_name("MSTeams_8wekyb3d8bbwe"), "MSTeams");
        assert_eq!(program_name("Camera"), "Camera");
    }

    #[test]
    fn each_kind_in_use_once_in_order() {
        let u = |kind, p: &str| Use {
            kind,
            program: p.into(),
        };
        let uses = [
            u(Kind::Location, "Maps"),
            u(Kind::Microphone, "Zoom"),
            u(Kind::Microphone, "Discord"),
        ];
        assert_eq!(kinds(&uses), vec![Kind::Microphone, Kind::Location]);
        assert!(kinds(&[]).is_empty());
    }
}
