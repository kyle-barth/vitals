//! Saved preferences, kept in a small text file under `%APPDATA%\Vitals`.

use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    /// The restored window position in physical pixels: left, top, right, bottom.
    pub placement: Option<(i32, i32, i32, i32)>,
    pub maximized: bool,
    pub always_on_top: bool,
    /// The user has opted in to reading core temperature, so ask again at launch.
    pub core_temp: bool,
}

impl Settings {
    fn path() -> Option<PathBuf> {
        Some(
            PathBuf::from(std::env::var_os("APPDATA")?)
                .join("Vitals")
                .join("settings.ini"),
        )
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::path().ok_or(io::ErrorKind::NotFound)?;
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)?;
        }
        // Write then rename, so a crash mid-write cannot leave a damaged file.
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, self.serialize())?;
        fs::rename(temporary, path)
    }

    /// Unknown or malformed lines are ignored, so a hand-edited or older file
    /// can never stop the app from starting.
    fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for (key, value) in text.lines().filter_map(|line| line.split_once('=')) {
            let (key, value) = (key.trim(), value.trim());
            let flag = value == "1";
            match key {
                "placement" => {
                    let parts: Vec<i32> = value
                        .split(',')
                        .filter_map(|p| p.trim().parse().ok())
                        .collect();
                    if let [left, top, right, bottom] = parts[..] {
                        settings.placement = Some((left, top, right, bottom));
                    }
                }
                "maximized" => settings.maximized = flag,
                "always_on_top" => settings.always_on_top = flag,
                "core_temp" => settings.core_temp = flag,
                _ => {}
            }
        }
        settings
    }

    fn serialize(&self) -> String {
        let mut text = String::new();
        if let Some((left, top, right, bottom)) = self.placement {
            text += &format!("placement={left},{top},{right},{bottom}\n");
        }
        text += &format!("maximized={}\n", self.maximized as u8);
        text += &format!("always_on_top={}\n", self.always_on_top as u8);
        text += &format!("core_temp={}\n", self.core_temp as u8);
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let settings = Settings {
            placement: Some((-100, 20, 240, 490)),
            maximized: true,
            always_on_top: true,
            core_temp: true,
        };
        assert_eq!(Settings::parse(&settings.serialize()), settings);
    }

    #[test]
    fn ignores_damaged_input() {
        let parsed = Settings::parse("placement=1,2\nnonsense\nunknown=1\nalways_on_top=1\n");
        assert_eq!(parsed.placement, None);
        assert!(parsed.always_on_top);
    }

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Settings::parse(""), Settings::default());
    }
}
