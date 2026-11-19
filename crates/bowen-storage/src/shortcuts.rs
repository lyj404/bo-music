use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    PlayPause,
    Previous,
    Next,
    VolumeUp,
    VolumeDown,
    Mute,
    Favorite,
}

impl Action {
    pub const ALL: [Self; 7] = [
        Self::PlayPause,
        Self::Previous,
        Self::Next,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::Mute,
        Self::Favorite,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::PlayPause => "播放 / 暂停",
            Self::Previous => "上一首",
            Self::Next => "下一首",
            Self::VolumeUp => "增大音量",
            Self::VolumeDown => "减小音量",
            Self::Mute => "静音开关",
            Self::Favorite => "喜欢歌曲",
        }
    }

    fn legacy(self) -> (&'static str, u64) {
        match self {
            Self::PlayPause => ("TogglePlayPause", 0),
            Self::Previous => ("PreviousTrack", 1),
            Self::Next => ("NextTrack", 2),
            Self::VolumeUp => ("VolumeUp", 3),
            Self::VolumeDown => ("VolumeDown", 4),
            Self::Mute => ("ToggleMute", 6),
            Self::Favorite => ("ToggleFavorite", 5),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Gesture {
    key: String,
    modifiers: u8,
}

impl Gesture {
    pub fn new(key: &str, ctrl: bool, alt: bool, shift: bool) -> Option<Self> {
        let key = match key {
            " " => "space",
            "return" => "enter",
            "plus" | "=" => "+",
            "minus" => "-",
            key => key,
        };
        let gesture = Self {
            key: key.to_ascii_lowercase(),
            modifiers: u8::from(ctrl) | u8::from(alt) << 1 | u8::from(shift) << 2,
        };
        gesture.valid().then_some(gesture)
    }

    fn valid(&self) -> bool {
        let key = self.key.as_str();
        let function = key
            .strip_prefix('f')
            .and_then(|part| part.parse::<u8>().ok())
            .is_some_and(|n| (1..=12).contains(&n));
        let ordinary = matches!(
            key,
            "left"
                | "right"
                | "up"
                | "down"
                | "enter"
                | "tab"
                | "pageup"
                | "pagedown"
                | "home"
                | "end"
                | "insert"
                | "delete"
                | "+"
                | "-"
        ) || matches!(key.as_bytes(), [b'a'..=b'z' | b'0'..=b'9']);
        self.modifiers <= 7
            && (key != "f11" || self.modifiers != 0)
            && (key == "space" || function || (self.modifiers != 0 && ordinary))
    }

    pub fn label(&self) -> String {
        let mut label = String::new();
        for (mask, name) in [(1, "Ctrl+"), (2, "Alt+"), (4, "Shift+")] {
            if self.modifiers & mask != 0 {
                label.push_str(name);
            }
        }
        label.push_str(&self.key.to_ascii_uppercase());
        label
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Shortcuts([Option<Gesture>; 7]);

impl Default for Shortcuts {
    fn default() -> Self {
        Self(
            ["space", "left", "right", "up", "down", "m", "l"]
                .map(|key| Gesture::new(key, key != "space", false, false)),
        )
    }
}

impl Shortcuts {
    pub fn binding(&self, action: Action) -> Option<&Gesture> {
        self.0[action as usize].as_ref()
    }

    pub fn action_for(&self, gesture: &Gesture) -> Option<Action> {
        Action::ALL
            .into_iter()
            .find(|action| self.binding(*action) == Some(gesture))
    }

    pub fn rebind(&mut self, action: Action, gesture: Gesture) -> Result<(), Action> {
        if let Some(conflict) = self.action_for(&gesture).filter(|other| *other != action) {
            return Err(conflict);
        }
        self.0[action as usize] = Some(gesture);
        Ok(())
    }

    pub(crate) fn normalized(mut self) -> Self {
        for index in 0..self.0.len() {
            if self.0[index]
                .as_ref()
                .is_some_and(|key| !key.valid() || self.0[..index].contains(&Some(key.clone())))
            {
                self.0[index] = None;
            }
        }
        for (index, fallback) in Self::default().0.into_iter().enumerate() {
            if self.0[index].is_none() && !self.0.contains(&fallback) {
                self.0[index] = fallback;
            }
        }
        self
    }

    pub(crate) fn from_legacy(value: &Value) -> Option<Self> {
        let entries = field(value, "bindings")?.as_array()?;
        let mut shortcuts = Self([const { None }; 7]);
        for entry in entries {
            let Some(action) = field(entry, "action").and_then(|value| {
                Action::ALL.into_iter().find(|action| {
                    let (name, number) = action.legacy();
                    value.as_u64() == Some(number)
                        || value
                            .as_str()
                            .is_some_and(|text| text.eq_ignore_ascii_case(name))
                })
            }) else {
                continue;
            };
            let Some(key) = field(entry, "key").and_then(legacy_key) else {
                continue;
            };
            let Some(modifiers) = field(entry, "modifiers").map_or(Some(0), legacy_modifiers)
            else {
                continue;
            };
            shortcuts.0[action as usize] = Gesture::new(
                &key,
                modifiers & 1 != 0,
                modifiers & 2 != 0,
                modifiers & 4 != 0,
            );
        }
        Some(shortcuts.normalized())
    }
}

fn field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value
        .as_object()?
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value))
}

fn legacy_key(value: &Value) -> Option<String> {
    if let Some(name) = value.as_str() {
        let key = name.to_ascii_lowercase();
        return Some(match key.as_str() {
            "oemplus" | "add" => "+".into(),
            "oemminus" | "subtract" => "-".into(),
            _ if matches!(key.as_bytes(), [b'd', b'0'..=b'9']) => key[1..].into(),
            _ => key,
        });
    }
    let code = value.as_u64()?;
    Some(
        match code {
            0x09 => "tab",
            0x0d => "enter",
            0x20 => "space",
            0x21 => "pageup",
            0x22 => "pagedown",
            0x23 => "end",
            0x24 => "home",
            0x25 => "left",
            0x26 => "up",
            0x27 => "right",
            0x28 => "down",
            0x2d => "insert",
            0x2e => "delete",
            0x6b | 0xbb => "+",
            0x6d | 0xbd => "-",
            0x30..=0x39 | 0x41..=0x5a => return Some(char::from(code as u8).to_string()),
            0x70..=0x7b => return Some(format!("f{}", code - 0x6f)),
            _ => return None,
        }
        .into(),
    )
}

fn legacy_modifiers(value: &Value) -> Option<u64> {
    if let Some(bits) = value.as_u64() {
        return (bits <= 7).then_some(bits);
    }
    value.as_str()?.split(',').try_fold(0, |bits, name| {
        Some(
            bits | match name.trim().to_ascii_lowercase().as_str() {
                "none" => 0,
                "control" => 1,
                "alt" => 2,
                "shift" => 4,
                _ => return None,
            },
        )
    })
}
