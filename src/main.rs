mod cache;
mod ffmpeg;
mod hash;
mod scan;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use hash::{find, union, video_distance, Group, VideoData, VideoInfo};
use indicatif::{ParallelProgressIterator, ProgressStyle};
use rayon::prelude::*;
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};

#[derive(ValueEnum, Clone)]
enum Mode {
    /// Fast: 3 frames, no audio, simple seek
    Fast,
    /// Precise: 8 frames, thumbnail filter, audio fingerprint
    Precise,
}

#[derive(Parser)]
#[command(name = "vidsim", about = "Find duplicate/similar videos using perceptual hashing")]
struct Cli {
    /// First directory to search
    dir_a: PathBuf,

    /// Optional second directory (cross-compare with dir_a)
    dir_b: Option<PathBuf>,

    /// Preset: fast (quick scan) or precise (better accuracy, slower first run)
    #[arg(short, long, value_enum, default_value = "fast")]
    mode: Mode,

    /// Similarity threshold (0–64, lower = stricter)
    #[arg(short, long, default_value_t = 10)]
    threshold: u32,

    /// Max concurrent ffmpeg processes
    #[arg(short = 'j', long, default_value_t = 4)]
    jobs: usize,

    /// Minimum video duration in seconds
    #[arg(long, default_value_t = 5.0)]
    min_duration: f64,

    /// Minimum file size in MB
    #[arg(long, default_value_t = 1.0)]
    min_size_mb: f64,

    /// Output results as JSON
    #[arg(long)]
    json: bool,

    /// Move duplicates to this directory (keeps largest in group)
    #[arg(long)]
    move_to: Option<PathBuf>,

    /// Delete duplicates (keeps largest in group, asks confirmation)
    #[arg(long)]
    delete: bool,

    /// Interactive review: open each pair in mpv, then choose which to delete
    #[arg(long)]
    review: bool,

    /// Cache file path
    #[arg(long, default_value = "~/.vidsim_cache.db")]
    cache: String,
}

fn confirm(prompt: &str) -> bool {
    print!("{prompt} [y/N] ");
    std::io::stdout().flush().ok();
    let mut s = String::new();
    std::io::stdin().read_line(&mut s).ok();
    matches!(s.trim().to_lowercase().as_str(), "y" | "yes")
}

fn handle_duplicates(group: &[&VideoData], move_to: Option<&Path>, delete: bool) -> Result<()> {
    let mut sorted: Vec<&VideoData> = group.to_vec();
    sorted.sort_by_key(|v| std::cmp::Reverse(v.size_bytes));
    let keep = sorted[0];
    let dupes = &sorted[1..];

    println!("  ✓ keep: {}", keep.path.display());
    for d in dupes {
        println!("  ✗ dupe: {}", d.path.display());
    }

    if let Some(dest_dir) = move_to {
        if confirm(&format!("  Move {} file(s)?", dupes.len())) {
            fs::create_dir_all(dest_dir)?;
            for d in dupes {
                let dest = dest_dir.join(d.path.file_name().unwrap());
                fs::rename(&d.path, &dest)
                    .with_context(|| format!("moving {}", d.path.display()))?;
                println!("  → moved to {}", dest.display());
            }
        }
    } else if delete {
        if confirm(&format!("  Delete {} file(s)?", dupes.len())) {
            for d in dupes {
                fs::remove_file(&d.path)
                    .with_context(|| format!("deleting {}", d.path.display()))?;
                println!("  → deleted");
            }
        }
    }

    Ok(())
}

fn review_duplicates(group: &[&VideoData]) -> Result<()> {
    let mut sorted: Vec<&VideoData> = group.to_vec();
    sorted.sort_by_key(|v| std::cmp::Reverse(v.size_bytes));

    let best = sorted[0];
    for other in &sorted[1..] {
        let q1 = ffmpeg::probe_quality(&best.path);
        let q2 = ffmpeg::probe_quality(&other.path);

        let fmt_q = |v: &VideoData, q: &Option<ffmpeg::VideoQuality>| {
            if let Some(q) = q {
                format!(
                    "{:.1} MB  {:.0}s  {}x{}  {}  v:{:.0}kbps  a:{:.0}kbps  {}",
                    v.size_bytes as f64 / 1_048_576.0,
                    v.duration,
                    q.width, q.height,
                    q.video_codec,
                    q.video_bitrate as f64 / 1000.0,
                    q.audio_bitrate as f64 / 1000.0,
                    v.path.display()
                )
            } else {
                format!(
                    "{:.1} MB  {:.0}s  {}",
                    v.size_bytes as f64 / 1_048_576.0,
                    v.duration,
                    v.path.display()
                )
            }
        };

        println!("\n  [1] {}", fmt_q(best, &q1));
        println!("  [2] {}", fmt_q(other, &q2));

        // Suggestion: score = video_bitrate * 0.7 + audio_bitrate * 0.3
        let score = |q: &Option<ffmpeg::VideoQuality>, v: &VideoData| -> u64 {
            q.as_ref().map(|q| {
                (q.video_bitrate as f64 * 0.7 + q.audio_bitrate as f64 * 0.3) as u64
            }).unwrap_or(v.size_bytes)
        };
        let s1 = score(&q1, best);
        let s2 = score(&q2, other);
        if s1 != s2 {
            let worse = if s1 > s2 { 2 } else { 1 };
            println!("  ★ sugestão: delete [{}] (menor qualidade)", worse);
        } else {
            println!("  ★ sugestão: qualidade similar, delete o que preferir");
        }

        println!("\n  Opening [1] in mpv... (close to continue)");
        Command::new("mpv").arg(&best.path).status().ok();

        println!("  Opening [2] in mpv... (close to continue)");
        Command::new("mpv").arg(&other.path).status().ok();

        loop {
            print!("\n  Delete which? [1/2/s(skip)/q(quit)] ");
            std::io::stdout().flush().ok();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();
            match input.trim() {
                "1" => {
                    fs::remove_file(&best.path)
                        .with_context(|| format!("deleting {}", best.path.display()))?;
                    println!("  → deleted [1]");
                    break;
                }
                "2" => {
                    fs::remove_file(&other.path)
                        .with_context(|| format!("deleting {}", other.path.display()))?;
                    println!("  → deleted [2]");
                    break;
                }
                "s" => { println!("  skipped"); break; }
                "q" => return Ok(()),
                _ => println!("  Invalid option. Use 1, 2, s or q."),
            }
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut videos = scan::collect(&cli.dir_a);
    if let Some(ref dir_b) = cli.dir_b {
        videos.extend(scan::collect(dir_b));
        videos.dedup();
    }

    let min_bytes = (cli.min_size_mb * 1024.0 * 1024.0) as u64;
    videos.retain(|p| fs::metadata(p).map(|m| m.len() >= min_bytes).unwrap_or(false));

    if videos.is_empty() {
        eprintln!("No videos found.");
        return Ok(());
    }

    eprintln!("Found {} videos, extracting hashes...", videos.len());

    let db = Mutex::new(cache::open(&cli.cache)?);
    let sem = Arc::new(Mutex::new(0usize));
    let jobs = cli.jobs;

    let (frames, thumbnail, use_audio, audio_weight) = match cli.mode {
        Mode::Fast    => (3u32, false, false, 0.0f64),
        Mode::Precise => (8u32, true,  true,  0.3f64),
    };

    let style = ProgressStyle::with_template("{bar:40} {pos}/{len} [{elapsed_precise}]").unwrap();

    let data: Vec<VideoData> = videos
        .par_iter()
        .progress_with_style(style)
        .filter_map(|p| scan::process(p, frames, thumbnail, use_audio, &db, &sem, jobs))
        .filter(|v| v.duration >= cli.min_duration)
        .collect();

    eprintln!("Comparing {} videos...", data.len());

    let n = data.len();
    let mut parent: Vec<usize> = (0..n).collect();

    let pairs: Vec<(usize, usize)> = (0..n)
        .into_par_iter()
        .flat_map(|i| {
            (i + 1..n)
                .filter_map(|j| {
                    let dist = video_distance(&data[i], &data[j], audio_weight);
                    if dist <= cli.threshold as f64 { Some((i, j)) } else { None }
                })
                .collect::<Vec<_>>()
        })
        .collect();

    for (i, j) in pairs {
        union(&mut parent, i, j);
    }
    for i in 0..n {
        find(&mut parent, i);
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        groups.entry(parent[i]).or_default().push(i);
    }

    let mut dup_groups: Vec<Vec<usize>> = groups.into_values().filter(|g| g.len() > 1).collect();
    dup_groups.sort_by_key(|g| std::cmp::Reverse(g.len()));

    if dup_groups.is_empty() {
        eprintln!("No similar videos found (threshold={}).", cli.threshold);
        return Ok(());
    }

    if cli.json {
        let out: Vec<Group> = dup_groups
            .iter()
            .map(|g| Group {
                videos: g
                    .iter()
                    .map(|&i| VideoInfo {
                        path: data[i].path.display().to_string(),
                        size_mb: data[i].size_bytes as f64 / 1_048_576.0,
                        duration: data[i].duration,
                    })
                    .collect(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    for (gi, group) in dup_groups.iter().enumerate() {
        println!("\n── Group {} ({} videos) ──", gi + 1, group.len());
        let members: Vec<&VideoData> = group.iter().map(|&i| &data[i]).collect();
        for v in &members {
            println!(
                "  {:.1} MB  {:.0}s  {}",
                v.size_bytes as f64 / 1_048_576.0,
                v.duration,
                v.path.display()
            );
        }
        if cli.move_to.is_some() || cli.delete {
            handle_duplicates(&members, cli.move_to.as_deref(), cli.delete)?;
        }
        if cli.review {
            review_duplicates(&members)?;
        }
    }

    Ok(())
}
