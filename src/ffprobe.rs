use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct Stream {
    pub index: u32,
    pub codec_type: String,
    pub codec_name: Option<String>,
    pub channels: Option<u32>,
    pub channel_layout: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub r_frame_rate: Option<String>,
    pub avg_frame_rate: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct Format {
    pub filename: Option<String>,
    pub format_name: Option<String>,
    pub duration: Option<String>,
    pub size: Option<String>,
    pub bit_rate: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProbeOutput {
    pub streams: Vec<Stream>,
    pub format: Option<Format>,
}

impl Stream {
    pub fn is_5_1_audio(&self) -> bool {
        if self.codec_type != "audio" {
            return false;
        }

        let num_channels_5_1 = self.channels.map_or(false, |c| c == 6);
        let layout_5_1 = self
            .channel_layout
            .as_deref()
            .map_or(false, |l| l.contains("5.1"));

        num_channels_5_1 || layout_5_1
    }

    pub fn audio_description(&self) -> String {
        let codec = self.codec_name.as_deref().unwrap_or("unknown");
        let layout = self.channel_layout.as_deref().unwrap_or("unknown layout");
        let ch = self.channels.map_or("?".to_string(), |c| c.to_string());
        format!("{} ({} ch, {})", codec.to_uppercase(), ch, layout)
    }
}

impl ProbeOutput {
    pub fn get_5_1_audio_streams(&self) -> Vec<&Stream> {
        self.streams
            .iter()
            .filter(|s| s.is_5_1_audio())
            .collect()
    }

    pub fn get_video_streams(&self) -> Vec<&Stream> {
        self.streams
            .iter()
            .filter(|s| s.codec_type == "video")
            .collect()
    }

    pub fn has_5_1_audio(&self) -> bool {
        !self.get_5_1_audio_streams().is_empty()
    }
}

pub fn probe_file_with_retry<P: AsRef<Path>>(
    path: P,
    ffprobe_bin: &str,
    max_retries: u32,
    initial_delay_ms: u64,
) -> Result<ProbeOutput> {
    let p = path.as_ref();
    let mut attempt = 0;
    let mut delay = initial_delay_ms;

    loop {
        attempt += 1;
        match probe_file(p, ffprobe_bin) {
            Ok(output) => return Ok(output),
            Err(e) => {
                if attempt >= max_retries {
                    return Err(e);
                }
                sleep(Duration::from_millis(delay));
                delay = (delay * 2).min(10_000); // cap max delay at 10 seconds
            }
        }
    }
}

pub fn probe_file<P: AsRef<Path>>(path: P, ffprobe_bin: &str) -> Result<ProbeOutput> {
    let p = path.as_ref();

    if !p.exists() {
        return Err(anyhow!("File does not exist: {}", p.display()));
    }

    if let Some(health_issue) = check_file_health(p) {
        return Err(anyhow!("{}", health_issue));
    }

    let output = Command::new(ffprobe_bin)
        .arg("-v")
        .arg("error")
        .arg("-show_streams")
        .arg("-show_format")
        .arg("-of")
        .arg("json")
        .arg(p)
        .output()
        .map_err(|e| anyhow!("Failed to execute ffprobe: {}", e))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let first_line = err.lines().next().unwrap_or("Invalid media format").trim();
        return Err(anyhow!("ffprobe unable to parse file: {}", first_line));
    }

    let probe: ProbeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow!("Failed to parse ffprobe JSON for {}: {}", p.display(), e))?;

    Ok(probe)
}

fn check_file_health(path: &Path) -> Option<String> {
    if let Ok(metadata) = path.metadata() {
        if metadata.len() == 0 {
            return Some("File is empty (0 bytes)".to_string());
        }
    }

    if let Ok(mut f) = File::open(path) {
        let mut header = [0u8; 16];
        if let Ok(n) = f.read(&mut header) {
            if n > 0 && header[..n].iter().all(|&b| b == 0) {
                return Some("Corrupted file header (starts with null 0x00 bytes - file is incomplete or damaged)".to_string());
            }
        }
    } else {
        return Some("File is locked or inaccessible (permission denied)".to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_5_1_audio_by_channels() {
        let stream = Stream {
            index: 1,
            codec_type: "audio".to_string(),
            codec_name: Some("ac3".to_string()),
            channels: Some(6),
            channel_layout: None,
            width: None,
            height: None,
            r_frame_rate: None,
            avg_frame_rate: None,
        };
        assert!(stream.is_5_1_audio());
    }

    #[test]
    fn test_is_5_1_audio_by_layout() {
        let stream = Stream {
            index: 1,
            codec_type: "audio".to_string(),
            codec_name: Some("aac".to_string()),
            channels: Some(6),
            channel_layout: Some("5.1(side)".to_string()),
            width: None,
            height: None,
            r_frame_rate: None,
            avg_frame_rate: None,
        };
        assert!(stream.is_5_1_audio());
    }

    #[test]
    fn test_is_not_5_1_audio_stereo() {
        let stream = Stream {
            index: 1,
            codec_type: "audio".to_string(),
            codec_name: Some("aac".to_string()),
            channels: Some(2),
            channel_layout: Some("stereo".to_string()),
            width: None,
            height: None,
            r_frame_rate: None,
            avg_frame_rate: None,
        };
        assert!(!stream.is_5_1_audio());
    }

    #[test]
    fn test_video_stream_ignored_for_audio_check() {
        let stream = Stream {
            index: 0,
            codec_type: "video".to_string(),
            codec_name: Some("h264".to_string()),
            channels: Some(6),
            channel_layout: Some("5.1".to_string()),
            width: Some(1920),
            height: Some(1080),
            r_frame_rate: None,
            avg_frame_rate: None,
        };
        assert!(!stream.is_5_1_audio());
    }
}
