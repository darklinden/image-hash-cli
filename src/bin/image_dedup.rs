use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;
use walkdir::WalkDir;
use exif::{Reader as ExifReader, In};

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

    /// Delete duplicate images (keeps the first occurrence)
    #[arg(long)]
    delete: bool,
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

    println!("Scanning {} images in '{}'...\n", image_files.len(), args.i.display());

    // Build hash -> list of files map
    let mut hash_map: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for path in &image_files {
        match image::open(path) {
            Ok(img) => {
                let hash = imagehash::average_hash(&img).to_string();
                hash_map.entry(hash).or_default().push(path.clone());
            }
            Err(e) => {
                eprintln!("Warning: could not open '{}': {}", path.display(), e);
            }
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
