use crate::{
    cache,
    ffmpeg::{audio_hash, extract_frame, probe_duration},
    hash::{dhash, VideoData},
};
use rusqlite::Connection;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};
use walkdir::WalkDir;

const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts", "mpg", "mpeg",
];

pub fn collect(dir: &Path) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| VIDEO_EXTS.contains(&s.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .map(|e| e.into_path())
        .collect()
}

pub fn mtime(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
        .unwrap_or(0)
}

pub fn process(
    path: &Path,
    frames: u32,
    thumbnail: bool,
    use_audio: bool,
    db: &Mutex<Connection>,
    sem: &Arc<Mutex<usize>>,
    max_jobs: usize,
) -> Option<VideoData> {
    let mt = mtime(path);
    let size_bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    // Cache hit
    {
        let conn = db.lock().unwrap();
        if let Some((hashes, audio_hash, duration)) = cache::get(&conn, path, mt) {
            return Some(VideoData { path: path.to_owned(), hashes, audio_hash, duration, size_bytes });
        }
    }

    // Semaphore: limit concurrent ffmpeg processes
    loop {
        let mut count = sem.lock().unwrap();
        if *count < max_jobs {
            *count += 1;
            break;
        }
        drop(count);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let duration = probe_duration(path);
    let hashes: Vec<u64> = (0..frames)
        .filter_map(|i| {
            let t = duration * (i as f64 + 0.5) / frames as f64;
            extract_frame(path, t, thumbnail).map(|img| dhash(&img))
        })
        .collect();

    let audio = if use_audio { audio_hash(path) } else { 0 };

    *sem.lock().unwrap() -= 1;

    if hashes.is_empty() {
        return None;
    }

    {
        let conn = db.lock().unwrap();
        cache::put(&conn, path, mt, &hashes, audio, duration);
    }

    Some(VideoData { path: path.to_owned(), hashes, audio_hash: audio, duration, size_bytes })
}
