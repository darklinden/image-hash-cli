use clap::{Parser, ValueEnum};
use std::collections::HashMap;
use std::path::PathBuf;
use walkdir::WalkDir;
use exif::{Reader as ExifReader, In};
use sha2::{Sha256, Digest};
use std::io::Read;

fn sha256_of(path: &std::path::Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Some(hex::encode(hasher.finalize()))
}

/// CLI duplicate image finder / remover
#[derive(Parser)]
#[command(
    name = "image-dedup",
    about = "Find (and optionally delete) duplicate images in a folder"
)]
struct Args {
    /// Folder to scan for duplicate images
    #[arg(short, long)]
    i: PathBuf,

    /// Delete duplicate images (keeps the best quality file)
    #[arg(long)]
    delete: bool,

    /// Dedup method: 'hash' uses perceptual image hash (default), 'sha256' uses exact file checksum
    #[arg(long, value_enum, default_value_t = DedupMethod::Hash)]
    dedup: DedupMethod,
}

#[derive(ValueEnum, Clone, Debug)]
enum DedupMethod {
    /// Perceptual average hash (catches visually identical images)
    Hash,
    /// Exact SHA-256 file checksum (only catches byte-for-byte duplicates)
    Sha256,
}

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif"];

fn is_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Returns (width * height, exif_tag_count) for a file.
/// Falls back to (0, 0) on any error.
fn image_score(path: &std::path::Path) -> (u64, usize) {
    let resolution = image::image_dimensions(path)
        .map(|(w, h)| w as u64 * h as u64)
        .unwrap_or(0);

    let exif_count = std::fs::File::open(path)
        .ok()
        .and_then(|f| {
            ExifReader::new()
                .read_from_container(&mut std::io::BufReader::new(f))
                .ok()
        })
        .map(|exif| {
            exif.fields()
                .filter(|f| f.ifd_num == In::PRIMARY)
                .count()
        })
        .unwrap_or(0);

    (resolution, exif_count)
}

fn main() {
    let args = Args::parse();

    if !args.i.is_dir() {
        eprintln!("Error: '{}' is not a directory", args.i.display());
        std::process::exit(1);
    }

    // Collect all image files
    let image_files: Vec<PathBuf> = WalkDir::new(&args.i)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && is_image(e.path()))
        .map(|e| e.path().to_path_buf())
        .collect();

    println!(
        "Scanning {} images in '{}' using {:?} dedup...\n",
        image_files.len(),
        args.i.display(),
        args.dedup
    );

    // Build hash -> list of files map
    let mut hash_map: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for path in &image_files {
        let key = match args.dedup {
            DedupMethod::Hash => {
                match image::open(path) {
                    Ok(img) => Some(imagehash::average_hash(&img).to_string()),
                    Err(e) => {
                        eprintln!("Warning: could not open '{}': {}", path.display(), e);
                        None
                    }
                }
            }
            DedupMethod::Sha256 => {
                match sha256_of(path) {
                    Some(h) => Some(h),
                    None => {
                        eprintln!("Warning: could not hash '{}'", path.display());
                        None
                    }
                }
            }
        };
        if let Some(k) = key {
            hash_map.entry(k).or_default().push(path.clone());
        }
    }

    // Collect groups that have duplicates
    let mut dup_groups: Vec<(String, Vec<PathBuf>)> = hash_map
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(hash, mut files)| {
            // Sort files: highest resolution first, then highest exif tag count
            files.sort_by(|a, b| {
                let (ra, ea) = image_score(a);
                let (rb, eb) = image_score(b);
                rb.cmp(&ra).then(eb.cmp(&ea))
            });
            (hash, files)
        })
        .collect();
    dup_groups.sort_by(|a, b| a.0.cmp(&b.0));

    if dup_groups.is_empty() {
        println!("No duplicate images found.");
        return;
    }

    println!("Found {} duplicate group(s):\n", dup_groups.len());

    let mut total_deleted = 0usize;

    for (hash, files) in &dup_groups {
        println!("Hash: {}", hash);
        let (keep, duplicates) = files.split_first().unwrap();
        let (kr, ke) = image_score(keep);
        println!(
            "  [keep]    {} ({}px, {} exif tags)",
            keep.display(),
            kr,
            ke
        );
        for dup in duplicates {
            let (dr, de) = image_score(dup);
            if args.delete {
                match std::fs::remove_file(dup) {
                    Ok(_) => {
                        println!(
                            "  [deleted] {} ({}px, {} exif tags)",
                            dup.display(),
                            dr,
                            de
                        );
                        total_deleted += 1;
                    }
                    Err(e) => {
                        eprintln!("  [error]   could not delete '{}': {}", dup.display(), e);
                    }
                }
            } else {
                println!(
                    "  [dup]     {} ({}px, {} exif tags)",
                    dup.display(),
                    dr,
                    de
                );
            }
        }
        println!();
    }

    if args.delete {
        println!("Deleted {} duplicate file(s).", total_deleted);
    } else {
        println!("Run with --delete to remove duplicates.");
    }
}
