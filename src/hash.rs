use image::GrayImage;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct VideoInfo {
    pub path: String,
    pub size_mb: f64,
    pub duration: f64,
}

#[derive(Serialize)]
pub struct Group {
    pub videos: Vec<VideoInfo>,
}

pub struct VideoData {
    pub path: PathBuf,
    pub hashes: Vec<u64>,
    pub audio_hash: u64,
    pub duration: f64,
    pub size_bytes: u64,
}

pub fn dhash(img: &GrayImage) -> u64 {
    let resized = image::imageops::resize(img, 9, 8, image::imageops::FilterType::Lanczos3);
    let mut hash = 0u64;
    for y in 0..8u32 {
        for x in 0..8u32 {
            let left = resized.get_pixel(x, y)[0];
            let right = resized.get_pixel(x + 1, y)[0];
            hash = (hash << 1) | (left > right) as u64;
        }
    }
    hash
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

pub fn video_distance(a: &VideoData, b: &VideoData, audio_weight: f64) -> f64 {
    let n = a.hashes.len().min(b.hashes.len());
    if n == 0 {
        return f64::MAX;
    }
    let visual: f64 = a
        .hashes
        .iter()
        .zip(b.hashes.iter())
        .map(|(&x, &y)| hamming(x, y) as f64)
        .sum::<f64>()
        / n as f64;

    let audio = if a.audio_hash != 0 && b.audio_hash != 0 {
        hamming(a.audio_hash, b.audio_hash) as f64
    } else {
        visual
    };

    visual * (1.0 - audio_weight) + audio * audio_weight
}

pub fn find(parent: &mut Vec<usize>, x: usize) -> usize {
    if parent[x] != x {
        parent[x] = find(parent, parent[x]);
    }
    parent[x]
}

pub fn union(parent: &mut Vec<usize>, a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        parent[rb] = ra;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GrayImage;

    #[test]
    fn hamming_identical() {
        assert_eq!(hamming(0xDEADBEEF, 0xDEADBEEF), 0);
    }

    #[test]
    fn hamming_all_differ() {
        assert_eq!(hamming(0u64, u64::MAX), 64);
    }

    #[test]
    fn dhash_same_image() {
        let img = GrayImage::from_pixel(33, 32, image::Luma([128u8]));
        assert_eq!(dhash(&img), dhash(&img));
    }

    #[test]
    fn union_find_groups() {
        let mut p = vec![0, 1, 2, 3];
        union(&mut p, 0, 1);
        union(&mut p, 2, 3);
        assert_eq!(find(&mut p, 0), find(&mut p, 1));
        assert_eq!(find(&mut p, 2), find(&mut p, 3));
        assert_ne!(find(&mut p, 0), find(&mut p, 2));
    }
}
