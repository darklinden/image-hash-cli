use clap::Parser;

/// CLI image hash generator
#[derive(Parser)]
#[command(name = "image-hash", about = "Generate perceptual hashes for images")]
struct Args {
    /// Path to the image file
    #[arg(short, long)]
    i: String,
}

fn main() {
    let args = Args::parse();
    let img_filename = &args.i;

    let img = image::open(img_filename).unwrap_or_else(|e| {
        eprintln!("Error: failed to open '{}': {}", img_filename, e);
        std::process::exit(1);
    });

    let hash = imagehash::average_hash(&img);
    println!("{} hash: {}", img_filename, hash);
}
