#[derive(Clone, PartialEq)]
pub enum Source {
    Spotify,
    SoundCloud,
    Unknown,
}

impl Source {
    pub fn from_url(url: &str) -> Self {
        if url.contains("spotify.com") {
            Source::Spotify
        } else if url.contains("soundcloud.com") {
            Source::SoundCloud
        } else {
            Source::Unknown
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Source::Spotify => "Spotify",
            Source::SoundCloud => "SoundCloud",
            Source::Unknown => "Unknown",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Flac,
    Mp3,
    Opus,
    M4a,
    Ogg,
    Wav,
}

impl AudioFormat {
    pub const ALL: [AudioFormat; 6] = [
        AudioFormat::Flac,
        AudioFormat::Mp3,
        AudioFormat::Opus,
        AudioFormat::M4a,
        AudioFormat::Ogg,
        AudioFormat::Wav,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            AudioFormat::Flac => "FLAC",
            AudioFormat::Mp3 => "MP3",
            AudioFormat::Opus => "Opus",
            AudioFormat::M4a => "M4A",
            AudioFormat::Ogg => "OGG",
            AudioFormat::Wav => "WAV",
        }
    }

    pub fn spotdl_arg(&self) -> &'static str {
        match self {
            AudioFormat::Flac => "flac",
            AudioFormat::Mp3 => "mp3",
            AudioFormat::Opus => "opus",
            AudioFormat::M4a => "m4a",
            AudioFormat::Ogg => "ogg",
            AudioFormat::Wav => "wav",
        }
    }
}

impl Default for AudioFormat {
    fn default() -> Self {
        AudioFormat::Flac
    }
}

#[derive(Clone, PartialEq)]
pub enum Status {
    Pending,
    Running,
    Done,
    Failed,
}

#[derive(Clone)]
pub struct QueueItem {
    pub url: String,
    pub playlist_name: String,
    pub source: Source,
    pub status: Status,
    pub format: AudioFormat,
    pub error: Option<String>,
}
