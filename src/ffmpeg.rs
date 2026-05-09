use image::{GrayImage, ImageReader};
use std::{io::Cursor, path::Path, process::Command};

pub struct VideoQuality {
    pub width: u32,
    pub height: u32,
    pub video_codec: String,
    pub video_bitrate: u64,
    pub audio_bitrate: u64,
}

pub fn probe_quality(path: &Path) -> Option<VideoQuality> {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=width,height,codec_name,bit_rate",
            "-of", "default=noprint_wrappers=1:nokey=0",
            path.to_str()?,
        ])
        .output()
        .ok()?;

    let vstats = String::from_utf8_lossy(&out.stdout);
    let get = |key: &str| -> String {
        vstats.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.splitn(2, '=').nth(1))
            .unwrap_or("N/A")
            .trim()
            .to_string()
    };

    let width: u32 = get("width").parse().unwrap_or(0);
    let height: u32 = get("height").parse().unwrap_or(0);
    let video_codec = get("codec_name");
    let video_bitrate: u64 = get("bit_rate").parse().unwrap_or(0);

    // Audio bitrate via separate stream query
    let aout = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "a:0",
            "-show_entries", "stream=bit_rate",
            "-of", "default=noprint_wrappers=1:nokey=1",
            path.to_str()?,
        ])
        .output()
        .ok()?;

    let audio_bitrate: u64 = String::from_utf8_lossy(&aout.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    Some(VideoQuality { width, height, video_codec, video_bitrate, audio_bitrate })
}

pub fn probe_duration(path: &Path) -> f64 {
    Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path.to_str().unwrap_or(""),
        ])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0.0)
}

pub fn extract_frame(path: &Path, t: f64, thumbnail: bool) -> Option<GrayImage> {
    let vf = if thumbnail { "thumbnail,scale=33:32" } else { "scale=33:32" };
    let out = Command::new("ffmpeg")
        .args([
            "-ss",
            &format!("{t:.3}"),
            "-i",
            path.to_str()?,
            "-frames:v",
            "1",
            "-vf",
            vf,
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "pipe:1",
        ])
        .output()
        .ok()?;

    if out.stdout.is_empty() {
        return None;
    }

    ImageReader::new(Cursor::new(&out.stdout))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()
        .map(|i| i.into_luma8())
}

pub fn audio_hash(path: &Path) -> u64 {
    let out = Command::new("ffmpeg")
        .args([
            "-i",
            path.to_str().unwrap_or(""),
            "-vn",
            "-af",
            "aresample=8000,astats=metadata=1:reset=1",
            "-f",
            "null",
            "-",
        ])
        .output();

    let Ok(out) = out else { return 0 };
    let stderr = String::from_utf8_lossy(&out.stderr);

    let rms_values: Vec<f64> = stderr
        .lines()
        .filter(|l| l.contains("RMS level dB"))
        .filter_map(|l| l.split_whitespace().last()?.parse().ok())
        .collect();

    if rms_values.is_empty() {
        return 0;
    }

    let mut sorted = rms_values.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted[sorted.len() / 2];

    rms_values
        .iter()
        .take(64)
        .enumerate()
        .fold(0u64, |h, (i, &v)| h | (((v > median) as u64) << i))
}
