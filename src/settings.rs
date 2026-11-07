//! Saved preferences, kept in a small text file under `%APPDATA%\Vitals`.

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::apps::{Sort, SortKey};
use crate::layout::Page;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    /// The restored window position in physical pixels: left, top, right, bottom.
    pub placement: Option<(i32, i32, i32, i32)>,
    pub maximized: bool,
    pub always_on_top: bool,
    /// The user has opted in to reading core temperature, so ask again at launch.
    pub core_temp: bool,
    /// Show only the numbers, rather than the full detail on each tile.
    pub simple: bool,
    pub page: Page,
    pub sort: Sort,
    /// The size the user last chose by hand, in device-independent pixels.
    pub your_size: Option<(i32, i32)>,
}

/// The file can be edited by hand, so a size is only trusted if it could be real.
fn usable_size(width: i32, height: i32) -> Option<(i32, i32)> {
    const SIZES: std::ops::Range<i32> = 1..100_000;
    (SIZES.contains(&width) && SIZES.contains(&height)).then_some((width, height))
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
                "simple" => settings.simple = flag,
                "page" if value == "apps" => settings.page = Page::Apps,
                "page" if value == "specs" => settings.page = Page::Specs,
                "sort" => {
                    if let Some((key, direction)) = value.split_once(',') {
                        if let Some(key) = SortKey::from_id(key) {
                            settings.sort = Sort {
                                key,
                                descending: direction == "down",
                            };
                        }
                    }
                }
                "your_size" => {
                    if let Some((width, height)) = value.split_once(',') {
                        if let (Ok(width), Ok(height)) = (width.parse(), height.parse()) {
                            settings.your_size = usable_size(width, height);
                        }
                    }
                }
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
        text += &format!("simple={}\n", self.simple as u8);
        match self.page {
            Page::Apps => text += "page=apps\n",
            Page::Specs => text += "page=specs\n",
            Page::Vitals => {}
        }
        let direction = if self.sort.descending { "down" } else { "up" };
        text += &format!("sort={},{direction}\n", self.sort.key.id());
        if let Some((width, height)) = self.your_size {
            text += &format!("your_size={width},{height}\n");
        }
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
            simple: true,
            page: Page::Apps,
            sort: Sort {
                key: SortKey::VideoMemory,
                descending: false,
            },
            your_size: Some((254, 661)),
        };
        assert_eq!(Settings::parse(&settings.serialize()), settings);
    }

    #[test]
    fn the_specs_page_is_remembered() {
        let settings = Settings {
            page: Page::Specs,
            ..Default::default()
        };
        assert_eq!(Settings::parse(&settings.serialize()).page, Page::Specs);
    }

    #[test]
    fn an_unknown_sort_keeps_the_default() {
        assert_eq!(Settings::parse("sort=nonsense,up\n").sort, Sort::default());
    }

    #[test]
    fn a_damaged_size_is_ignored() {
        assert_eq!(Settings::parse("your_size=254\n").your_size, None);
        assert_eq!(Settings::parse("your_size=a,b\n").your_size, None);
        assert_eq!(Settings::parse("your_size=-5,0\n").your_size, None);
        assert_eq!(Settings::parse("your_size=9999999,10\n").your_size, None);
    }

    #[test]
    fn older_files_keep_full_detail_on_the_vitals_page() {
        let parsed = Settings::parse("always_on_top=1\ncore_temp=1\n");
        assert!(!parsed.simple);
        assert_eq!(parsed.page, Page::Vitals);
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
