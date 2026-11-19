use crate::track::Track;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountMetadata {
    pub follow_count: Option<i64>,
    pub fans_count: Option<i64>,
    pub follow_artist_count: Option<i64>,
    pub praised: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountPlayData {
    pub play_count: Option<i64>,
    pub play_seconds: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VipBadge {
    None,
    Standard,
    Big,
    Welfare,
}

/// Display information only; playback rights are determined by the server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountVipInfo {
    pub badge: VipBadge,
    pub expires_unix_ms: Option<i64>,
    /// Service-provided local date/time; preserve its original zone-less representation.
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayRankingEntry {
    pub track: Track,
    pub play_count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountOverview {
    pub metadata: Option<AccountMetadata>,
    pub playback: Option<AccountPlayData>,
    pub vip: Option<AccountVipInfo>,
    pub weekly: Option<Vec<PlayRankingEntry>>,
    pub month_summary: Option<MonthReportSummary>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MonthPeriod {
    year: u16,
    month: u8,
}

impl MonthPeriod {
    pub fn new(year: u16, month: u8) -> Option<Self> {
        (year > 0 && (1..=12).contains(&month)).then_some(Self { year, month })
    }
    pub fn request_value(self) -> String {
        format!("{:04}{:02}", self.year, self.month)
    }
    pub fn year(self) -> u16 {
        self.year
    }
    pub fn month(self) -> u8 {
        self.month
    }
    pub fn from_date(value: &str) -> Option<Self> {
        let mut parts = value.split('-');
        Self::new(parts.next()?.parse().ok()?, parts.next()?.parse().ok()?)
    }

    pub fn offset(self, months: i32) -> Option<Self> {
        let index = (i32::from(self.year) * 12 + i32::from(self.month) - 1).checked_add(months)?;
        Self::new(
            index.div_euclid(12).try_into().ok()?,
            (index.rem_euclid(12) + 1) as u8,
        )
    }

    pub fn from_summary(description: &str, current: Self) -> Option<Self> {
        let mut numbers = description
            .split(|ch: char| !ch.is_ascii_digit())
            .filter(|part| !part.is_empty());
        let first = numbers.next()?;
        match first.len() {
            6 => Self::new(first[..4].parse().ok()?, first[4..].parse().ok()?),
            4 => Self::new(first.parse().ok()?, numbers.next()?.parse().ok()?),
            1 | 2 => {
                let month = first.parse().ok()?;
                let year = current.year.checked_sub(u16::from(month > current.month))?;
                Self::new(year, month)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonthReportSummary {
    pub month_desc: Option<String>,
    pub listen_total_minutes: Option<String>,
    pub listen_chain: Option<String>,
    pub cover_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonthAvailability {
    Available,
    Empty,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportTrack {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album_name: Option<String>,
    pub cover_url: Option<String>,
    pub play_count: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonthReport {
    pub period: MonthPeriod,
    pub listen_total_minutes: Option<String>,
    pub play_count: Option<i64>,
    pub song_count: Option<i64>,
    pub time_rank: Option<i64>,
    pub listen_chain: Option<String>,
    pub play_chain: Option<String>,
    pub listen_content: Option<String>,
    pub listen_top_track_title: Option<String>,
    pub play_content: Option<String>,
    pub artist_name: Option<String>,
    pub artist_cover_url: Option<String>,
    pub artist_music_name: Option<String>,
    pub artist_music_cover_url: Option<String>,
    pub artist_content: Option<String>,
    pub top_tracks: Vec<ReportTrack>,
    pub album_covers: Vec<String>,
}

#[cfg(test)]
#[path = "account_tests.rs"]
mod tests;
