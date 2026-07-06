pub fn command_exists(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

pub struct DepStatus {
    pub spotdl: bool,
    pub scdl: bool,
    pub ffmpeg: bool,
}

impl DepStatus {
    pub fn check() -> Self {
        Self {
            spotdl: command_exists("spotdl"),
            scdl: command_exists("scdl"),
            ffmpeg: command_exists("ffmpeg"),
        }
    }
}
