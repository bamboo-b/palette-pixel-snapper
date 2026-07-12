use image::{GenericImageView, ImageBuffer, Rgba, RgbaImage};
use rand::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, WeightedIndex};
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use std::cmp::Ordering;
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::env;
use std::error::Error;
use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Config {
    pub k_colors: usize,
    pub pixel_size_override: Option<f64>,
    k_seed: u64,
    /// Input image path only used for CLI use
    #[allow(dead_code)]
    input_path: String,
    /// Output image path only used for CLI use
    #[allow(dead_code)]
    output_path: String,
    max_kmeans_iterations: usize,
    peak_threshold_multiplier: f64,
    peak_distance_filter: usize,
    walker_search_window_ratio: f64,
    walker_min_search_window: f64,
    walker_strength_threshold: f64,
    min_cuts_per_axis: usize,
    fallback_target_segments: usize,
    max_step_ratio: f64,
    /// Explicit palette as RGB centroids. When set without an explicit color
    /// count, k-means is skipped and pixels snap straight to these colors.
    custom_palette: Option<Vec<[f32; 3]>>,
    /// CLI-only: human-readable label for the palette source (file path or "inline").
    #[cfg(not(target_arch = "wasm32"))]
    palette_source: Option<String>,
    /// Whether the user explicitly passed a color count. With a palette, this
    /// switches from direct snapping to "k-means then snap centroids".
    k_colors_explicit: bool,
    /// Floyd–Steinberg dither the output against the effective palette.
    /// Applied at output resolution, after the grid is resolved.
    dither: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            k_colors: 16,
            k_seed: 42,
            input_path: "samples/2/skeleton.png".to_string(),
            output_path: "samples/2/skeleton_fixed_clean2.png".to_string(),
            max_kmeans_iterations: 15,
            peak_threshold_multiplier: 0.2,
            peak_distance_filter: 4,
            walker_search_window_ratio: 0.35,
            walker_min_search_window: 2.0,
            walker_strength_threshold: 0.5,
            min_cuts_per_axis: 4,
            fallback_target_segments: 64,
            max_step_ratio: 1.8, // Lowered from 3.0 to catch more skew cases
            pixel_size_override: None,
            custom_palette: None,
            #[cfg(not(target_arch = "wasm32"))]
            palette_source: None,
            k_colors_explicit: false,
            dither: false,
        }
    }
}

#[derive(Debug)]
pub enum PixelSnapperError {
    ImageError(image::ImageError),
    InvalidInput(String),
    ProcessingError(String),
}

impl fmt::Display for PixelSnapperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PixelSnapperError::ImageError(e) => write!(f, "Image error: {}", e),
            PixelSnapperError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            PixelSnapperError::ProcessingError(msg) => write!(f, "Processing error: {}", msg),
        }
    }
}

impl Error for PixelSnapperError {}

impl From<image::ImageError> for PixelSnapperError {
    fn from(error: image::ImageError) -> Self {
        PixelSnapperError::ImageError(error)
    }
}

#[cfg(target_arch = "wasm32")]
impl From<PixelSnapperError> for wasm_bindgen::JsValue {
    fn from(err: PixelSnapperError) -> wasm_bindgen::JsValue {
        wasm_bindgen::JsValue::from_str(&err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, PixelSnapperError>;

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct ProcessedImage {
    output_bytes: Vec<u8>,
    pixel_size: f64,
    pixel_size_override: bool,
    output_width: u32,
    output_height: u32,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct BatchConfig {
    pub input_dir: PathBuf,
    pub output_dir: PathBuf,
    pub k_colors: usize,
    pub pixel_size_override: Option<f64>,
    pub custom_palette: Option<Vec<[f32; 3]>>,
    pub k_colors_explicit: bool,
    pub dither: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl From<&Config> for BatchConfig {
    fn from(config: &Config) -> Self {
        Self {
            input_dir: PathBuf::from(&config.input_path),
            output_dir: PathBuf::from(&config.output_path),
            k_colors: config.k_colors,
            pixel_size_override: config.pixel_size_override,
            custom_palette: config.custom_palette.clone(),
            k_colors_explicit: config.k_colors_explicit,
            dither: config.dither,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<&BatchConfig> for Config {
    fn from(config: &BatchConfig) -> Self {
        Self {
            k_colors: config.k_colors,
            pixel_size_override: config.pixel_size_override,
            custom_palette: config.custom_palette.clone(),
            palette_source: config
                .custom_palette
                .as_ref()
                .map(|_| "batch palette".to_string()),
            k_colors_explicit: config.k_colors_explicit,
            dither: config.dither,
            ..Default::default()
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub enum BatchEvent {
    BatchStarted {
        input_dir: PathBuf,
        total: usize,
    },
    Started {
        input: PathBuf,
        index: usize,
        total: usize,
    },
    Finished {
        input: PathBuf,
        output: PathBuf,
        index: usize,
        total: usize,
    },
    Failed {
        input: PathBuf,
        output: PathBuf,
        error: String,
        index: usize,
        total: usize,
    },
    BatchFinished {
        input_dir: PathBuf,
        total: usize,
    },
}

/// CLI entry point
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
fn main() -> Result<()> {
    let config = parse_args().unwrap_or_default();
    process(&config)
}

#[cfg(target_arch = "wasm32")]
fn process_image_bytes_common(input_bytes: &[u8], config: Option<Config>) -> Result<Vec<u8>> {
    process_image_common(input_bytes, config).map(|processed| processed.output_bytes)
}

fn process_image_common(input_bytes: &[u8], config: Option<Config>) -> Result<ProcessedImage> {
    let config = config.unwrap_or_default();

    let img = image::load_from_memory(input_bytes)?;
    let (width, height) = img.dimensions();

    validate_image_dimensions(width, height)?;

    if let Some(px) = config.pixel_size_override {
        if !px.is_finite() || px < 1.0 || px > (width.min(height) as f64 / 2.0) {
            return Err(PixelSnapperError::InvalidInput(format!(
                "pixel_size_override {:.1} is out of valid range [1, {}]",
                px,
                width.min(height) / 2
            )));
        }
    }

    let rgba_img = img.to_rgba8();

    let (quantized_img, palette_used) = quantize_image(&rgba_img, &config)?;
    let (profile_x, profile_y) = compute_profiles(&quantized_img)?;

    // Estimate step sizes
    let step_x_opt = estimate_step_size(&profile_x, &config);
    let step_y_opt = estimate_step_size(&profile_y, &config);

    // Resolve step sizes. Some instabilities so use sibling axis if one fails, or fallback if both fail
    let (step_x, step_y) = resolve_step_sizes(step_x_opt, step_y_opt, width, height, &config);

    let raw_col_cuts = walk(&profile_x, step_x, width as usize, &config)?;
    let raw_row_cuts = walk(&profile_y, step_y, height as usize, &config)?;

    // Two-pass stabilization: first pass with raw cuts, then cross-validate
    let (col_cuts, row_cuts) = stabilize_both_axes(
        &profile_x,
        &profile_y,
        raw_col_cuts,
        raw_row_cuts,
        width as usize,
        height as usize,
        &config,
    );

    // Dithering is applied at output resolution, after the grid is resolved:
    // dithering the full-size image first would be voted away by `resample`'s
    // per-cell majority and would pollute the edge profiles used for grid
    // detection. Cells are averaged from the original image so the dither has
    // real gradients to diffuse.
    let output_img = match &palette_used {
        Some(p) if config.dither => {
            dither_to_palette(&resample_average(&rgba_img, &col_cuts, &row_cuts)?, p)
        }
        _ => resample(&quantized_img, &col_cuts, &row_cuts)?,
    };

    // Returns bytes for both implementations
    let mut output_bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut output_bytes);
    output_img
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| PixelSnapperError::ImageError(e))?;

    Ok(ProcessedImage {
        output_bytes,
        pixel_size: step_x,
        pixel_size_override: config.pixel_size_override.is_some(),
        output_width: (col_cuts.len() - 1) as u32,
        output_height: (row_cuts.len() - 1) as u32,
    })
}

/// WASM entry point
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn process_image(
    input_bytes: &[u8],
    k_colors: Option<u32>,
    pixel_size_override: Option<f64>,
    palette_rgb: Option<Box<[u8]>>,
    seed: Option<u32>,
    dither: Option<bool>,
) -> std::result::Result<Vec<u8>, wasm_bindgen::JsValue> {
    let mut config = Config::default();
    config.dither = dither.unwrap_or(false);
    if let Some(s) = seed {
        // Re-seeds k-means init, so the discovered k colors (and thus which
        // palette colors get used in the limit-colors path) vary per seed.
        config.k_seed = s as u64;
    }
    if let Some(k) = k_colors {
        if k == 0 {
            return Err(wasm_bindgen::JsValue::from_str(
                "k_colors must be greater than 0",
            ));
        }
        config.k_colors = k as usize;
        // Mirror the CLI: an explicit count switches palette handling to
        // "reduce to N colors via k-means, then snap centroids to the palette".
        config.k_colors_explicit = true;
    }

    config.pixel_size_override = pixel_size_override;

    if let Some(flat) = palette_rgb {
        if flat.is_empty() || flat.len() % 3 != 0 {
            return Err(wasm_bindgen::JsValue::from_str(
                "palette_rgb must be a non-empty multiple of 3 (flat RGB)",
            ));
        }
        if flat.len() / 3 > 256 {
            return Err(wasm_bindgen::JsValue::from_str(
                "Palette too large (max 256 colors)",
            ));
        }
        let palette: Vec<[f32; 3]> = flat
            .chunks_exact(3)
            .map(|c| [c[0] as f32, c[1] as f32, c[2] as f32])
            .collect();
        config.custom_palette = Some(palette);
    }

    process_image_bytes_common(input_bytes, Some(config))
        .map_err(|e| wasm_bindgen::JsValue::from(e))
}

/// Extract a palette from an image (unique opaque colors, reduced via k-means
/// when there are more than `max_colors`). Returns flat `[r,g,b, ...]` bytes.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn extract_palette(
    input_bytes: &[u8],
    max_colors: Option<u32>,
) -> std::result::Result<Vec<u8>, wasm_bindgen::JsValue> {
    let max = max_colors.unwrap_or(64).clamp(1, 256) as usize;
    let (palette, _unique) =
        palette_from_image_bytes(input_bytes, max).map_err(wasm_bindgen::JsValue::from)?;
    Ok(palette
        .iter()
        .flat_map(|c| c.iter().map(|v| v.round().clamp(0.0, 255.0) as u8))
        .collect())
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
fn parse_args() -> Option<Config> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        return None;
    }

    let mut config = Config {
        input_path: args[1].clone(),
        output_path: args[2].clone(),
        ..Default::default()
    };

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--pixel-size" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("Warning: --pixel-size requires a value");
                    break;
                };

                match val.parse::<f64>() {
                    Ok(px) if px.is_finite() && px > 0.0 => config.pixel_size_override = Some(px),
                    _ => eprintln!("Warning: invalid --pixel-size '{}', ignoring", val),
                }
                i += 2;
            }
            "--seed" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("Warning: --seed requires a value");
                    break;
                };

                match val.parse::<u64>() {
                    Ok(s) => config.k_seed = s,
                    _ => eprintln!("Warning: invalid --seed '{}', ignoring", val),
                }
                i += 2;
            }
            "--dither" => {
                config.dither = true;
                i += 1;
            }
            "--palette" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("Error: --palette requires a value");
                    std::process::exit(1);
                };
                match resolve_palette(val) {
                    Ok((palette, source)) => {
                        config.custom_palette = Some(palette);
                        config.palette_source = Some(source);
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                i += 2;
            }
            arg if arg.starts_with("--") => {
                eprintln!("Warning: unknown argument '{}', ignoring", arg);
                i += 1;
            }
            k_arg => {
                match k_arg.parse::<usize>() {
                    Ok(k) if k > 0 => {
                        config.k_colors = k;
                        config.k_colors_explicit = true;
                    }
                    _ => eprintln!(
                        "Warning: invalid k_colors '{}', falling back to default ({})",
                        k_arg, config.k_colors
                    ),
                }
                i += 1;
            }
        }
    }

    Some(config)
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
fn process(config: &Config) -> Result<()> {
    let input_path = Path::new(&config.input_path);
    if input_path.is_dir() {
        process_batch(config)
    } else {
        process_single(config)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
fn process_single(config: &Config) -> Result<()> {
    let input_path = Path::new(&config.input_path);
    let output_path = Path::new(&config.output_path);
    let processed = process_file(input_path, output_path, config)?;
    println!("Processing: {}", config.input_path);
    if let Some(msg) = palette_summary(config) {
        println!("{}", msg);
    }
    print_processed_image(
        processed.pixel_size,
        processed.pixel_size_override,
        processed.output_width,
        processed.output_height,
    );
    println!("Saved to: {}", config.output_path);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
fn process_batch(config: &Config) -> Result<()> {
    process_batch_with_reporter(&BatchConfig::from(config), |event| match event {
        BatchEvent::BatchStarted { input_dir, total } => {
            println!(
                "Batch processing {} image{} from: {}",
                total,
                if total == 1 { "" } else { "s" },
                input_dir.display()
            );
            if let Some(msg) = palette_summary(config) {
                println!("{}", msg);
            }
        }
        BatchEvent::Started {
            input,
            index,
            total,
        } => {
            println!("Processing {}/{}: {}", index + 1, total, input.display());
        }
        BatchEvent::Finished {
            input,
            output,
            index,
            total,
        } => {
            println!(
                "Done {}/{}: {} -> {}",
                index + 1,
                total,
                input.display(),
                output.display()
            );
        }
        BatchEvent::Failed {
            input,
            output,
            error,
            index,
            total,
        } => {
            eprintln!(
                "Failed {}/{}: {} -> {} ({})",
                index + 1,
                total,
                input.display(),
                output.display(),
                error
            );
        }
        BatchEvent::BatchFinished { input_dir, total } => {
            println!(
                "Processed {} image{} in: {}",
                total,
                if total == 1 { "" } else { "s" },
                input_dir.display()
            );
        }
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn process_batch_with_reporter<F>(config: &BatchConfig, reporter: F) -> Result<()>
where
    F: Fn(BatchEvent) + Send + Sync,
{
    let input_dir = &config.input_dir;
    let output_dir = &config.output_dir;

    // Do not silently replace inputs; maybe that's ok though
    if input_dir == output_dir {
        return Err(PixelSnapperError::InvalidInput(
            "Batch output directory must be different from the input directory".to_string(),
        ));
    }

    if output_dir.exists() && !output_dir.is_dir() {
        return Err(PixelSnapperError::InvalidInput(format!(
            "Batch output path must be a directory: {}",
            output_dir.display()
        )));
    }

    std::fs::create_dir_all(output_dir).map_err(|e| {
        PixelSnapperError::ProcessingError(format!(
            "Failed to create output directory '{}': {}",
            output_dir.display(),
            e
        ))
    })?;

    let mut inputs = collect_batch_inputs(input_dir)?;
    inputs.sort();

    if inputs.is_empty() {
        return Err(PixelSnapperError::InvalidInput(format!(
            "No supported images found in '{}'",
            input_dir.display()
        )));
    }

    let items: Vec<(PathBuf, PathBuf)> = inputs
        .iter()
        .map(|input| Ok((input.clone(), get_output_path(output_dir, input)?)))
        .collect::<Result<_>>()?;

    reporter(BatchEvent::BatchStarted {
        input_dir: input_dir.clone(),
        total: items.len(),
    });

    let results: Vec<(PathBuf, Result<()>)> = items
        .par_iter()
        .enumerate()
        .map(|(index, (input, output))| {
            reporter(BatchEvent::Started {
                input: input.clone(),
                index,
                total: items.len(),
            });
            let item_config = Config::from(config);
            let result = process_file(input, output, &item_config).map(|_| ());
            match &result {
                Ok(()) => reporter(BatchEvent::Finished {
                    input: input.clone(),
                    output: output.clone(),
                    index,
                    total: items.len(),
                }),
                Err(err) => reporter(BatchEvent::Failed {
                    input: input.clone(),
                    output: output.clone(),
                    error: err.to_string(),
                    index,
                    total: items.len(),
                }),
            }
            (input.clone(), result)
        })
        .collect();

    let mut failures = Vec::new();
    for (input, result) in results {
        match result {
            Ok(()) => {}
            Err(err) => failures.push(format!("{} ({})", input.display(), err)),
        }
    }

    if failures.is_empty() {
        reporter(BatchEvent::BatchFinished {
            input_dir: input_dir.clone(),
            total: items.len(),
        });
        Ok(())
    } else {
        Err(PixelSnapperError::ProcessingError(format!(
            "Batch completed with {} failure{}: {}",
            failures.len(),
            if failures.len() == 1 { "" } else { "s" },
            failures.join("; ")
        )))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn process_file(input_path: &Path, output_path: &Path, config: &Config) -> Result<ProcessedImage> {
    let img_bytes = std::fs::read(input_path).map_err(|e| {
        PixelSnapperError::ProcessingError(format!(
            "Failed to read input file '{}': {}",
            input_path.display(),
            e
        ))
    })?;

    let processed = process_image_common(&img_bytes, Some(config.clone()))?;

    std::fs::write(output_path, &processed.output_bytes).map_err(|e| {
        PixelSnapperError::ProcessingError(format!(
            "Failed to write output file '{}': {}",
            output_path.display(),
            e
        ))
    })?;

    Ok(processed)
}

#[cfg(not(target_arch = "wasm32"))]
fn print_processed_image(
    pixel_size: f64,
    pixel_size_override: bool,
    output_width: u32,
    output_height: u32,
) {
    println!(
        "Pixel size: {:.1}px ({})",
        pixel_size,
        if pixel_size_override {
            "override"
        } else {
            "auto-detected"
        }
    );
    println!("Output size: {}x{}", output_width, output_height);
}

#[cfg(not(target_arch = "wasm32"))]
fn collect_batch_inputs(input_dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = std::fs::read_dir(input_dir).map_err(|e| {
        PixelSnapperError::ProcessingError(format!(
            "Failed to read input directory '{}': {}",
            input_dir.display(),
            e
        ))
    })?;

    let mut inputs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| {
            PixelSnapperError::ProcessingError(format!(
                "Failed to read an entry from '{}': {}",
                input_dir.display(),
                e
            ))
        })?;
        let path = entry.path();
        if path.is_file() && is_supported_image_path(&path) {
            inputs.push(path);
        }
    }

    Ok(inputs)
}

#[cfg(not(target_arch = "wasm32"))]
fn is_supported_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg"))
        .unwrap_or(false)
}

#[cfg(not(target_arch = "wasm32"))]
fn get_output_path(output_dir: &Path, input_path: &Path) -> Result<PathBuf> {
    let stem = input_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| {
            PixelSnapperError::InvalidInput(format!(
                "Input path has no file stem: {}",
                input_path.display()
            ))
        })?;

    Ok(output_dir.join(format!("{}.png", stem)))
}

fn validate_image_dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(PixelSnapperError::InvalidInput(
            "Image dimensions cannot be zero".to_string(),
        ));
    }
    if width > 10000 || height > 10000 {
        return Err(PixelSnapperError::InvalidInput(
            "Image dimensions too large (max 10000x10000)".to_string(),
        ));
    }
    Ok(())
}

fn dist_sq(p: &[f32; 3], c: &[f32; 3]) -> f32 {
    let dr = p[0] - c[0];
    let dg = p[1] - c[1];
    let db = p[2] - c[2];
    dr * dr + dg * dg + db * db
}

/// sRGB channel (0-255) -> linear light (0-1).
fn srgb_channel_to_linear(c: f32) -> f32 {
    let c = c / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB [r,g,b] in 0-255 -> OKLab [L,a,b] (Björn Ottosson's constants).
/// Palette matching uses OKLab because Euclidean distance in raw sRGB badly
/// misjudges perceived similarity for dark and saturated colors.
fn srgb_to_oklab(rgb: &[f32; 3]) -> [f32; 3] {
    let r = srgb_channel_to_linear(rgb[0]);
    let g = srgb_channel_to_linear(rgb[1]);
    let b = srgb_channel_to_linear(rgb[2]);
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

/// Index of the nearest color in a pre-converted OKLab palette.
fn nearest_in_oklab(lab: &[f32; 3], palette_lab: &[[f32; 3]]) -> usize {
    let mut best = 0;
    let mut min_dist = f32::MAX;
    for (i, p) in palette_lab.iter().enumerate() {
        let d = dist_sq(lab, p);
        if d < min_dist {
            min_dist = d;
            best = i;
        }
    }
    best
}

/// Map every opaque pixel to its nearest color in `centroids`. Transparent
/// pixels (alpha == 0) are passed through unchanged. Shared by the k-means path
/// and the custom-palette path.
fn map_to_palette(img: &RgbaImage, centroids: &[[f32; 3]]) -> RgbaImage {
    if centroids.is_empty() {
        return img.clone();
    }
    let palette_lab: Vec<[f32; 3]> = centroids.iter().map(srgb_to_oklab).collect();
    // Pixel-art inputs have few unique colors, so memoizing pixel -> palette
    // index keeps the OKLab conversion off the per-pixel hot path.
    let mut memo: HashMap<[u8; 3], usize> = HashMap::new();
    let mut new_img = RgbaImage::new(img.width(), img.height());
    for (x, y, pixel) in img.enumerate_pixels() {
        if pixel[3] == 0 {
            new_img.put_pixel(x, y, *pixel);
            continue;
        }
        let key = [pixel[0], pixel[1], pixel[2]];
        let idx = *memo.entry(key).or_insert_with(|| {
            let lab = srgb_to_oklab(&[key[0] as f32, key[1] as f32, key[2] as f32]);
            nearest_in_oklab(&lab, &palette_lab)
        });
        let c = &centroids[idx];
        new_img.put_pixel(
            x,
            y,
            Rgba([
                c[0].round() as u8,
                c[1].round() as u8,
                c[2].round() as u8,
                pixel[3],
            ]),
        );
    }
    new_img
}

/// Parse a single hex color. The leading `#` is optional and both 3-digit
/// (`#abc` -> `#aabbcc`) and 6-digit forms are accepted.
#[cfg(not(target_arch = "wasm32"))]
fn parse_hex_color(s: &str) -> Result<[f32; 3]> {
    let h = s.trim().trim_start_matches('#');
    if !h.is_ascii() {
        return Err(PixelSnapperError::InvalidInput(format!(
            "Invalid hex color '{}': non-ascii characters",
            s
        )));
    }
    let rgb = match h.len() {
        6 => [
            u8::from_str_radix(&h[0..2], 16),
            u8::from_str_radix(&h[2..4], 16),
            u8::from_str_radix(&h[4..6], 16),
        ],
        3 => {
            let dup = |c: &str| u8::from_str_radix(&format!("{0}{0}", c), 16);
            [dup(&h[0..1]), dup(&h[1..2]), dup(&h[2..3])]
        }
        _ => {
            return Err(PixelSnapperError::InvalidInput(format!(
                "Invalid hex color '{}': expected 3 or 6 hex digits",
                s
            )))
        }
    };
    match rgb {
        [Ok(r), Ok(g), Ok(b)] => Ok([r as f32, g as f32, b as f32]),
        _ => Err(PixelSnapperError::InvalidInput(format!(
            "Invalid hex color '{}': non-hex characters",
            s
        ))),
    }
}

/// Parse a comma-separated list of hex colors (e.g. `"#1a1a2e,16213e"`).
#[cfg(not(target_arch = "wasm32"))]
fn parse_palette_string(s: &str) -> Result<Vec<[f32; 3]>> {
    s.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(parse_hex_color)
        .collect()
}

/// Parse a GIMP `.gpl` palette: "GIMP Palette" header, `#` comments, optional
/// `Name:`/`Columns:` lines, then `R G B [name]` rows.
#[cfg(not(target_arch = "wasm32"))]
fn parse_gpl(content: &str) -> Result<Vec<[f32; 3]>> {
    let mut lines = content.lines().map(str::trim);
    if !matches!(lines.next(), Some(first) if first.starts_with("GIMP Palette")) {
        return Err(PixelSnapperError::InvalidInput(
            "Not a GIMP palette: missing 'GIMP Palette' header".to_string(),
        ));
    }

    let mut palette = Vec::new();
    for line in lines {
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with("Name:")
            || line.starts_with("Columns:")
        {
            continue;
        }
        let mut parts = line.split_whitespace();
        let rgb: Option<[u8; 3]> = (|| {
            let r = parts.next()?.parse().ok()?;
            let g = parts.next()?.parse().ok()?;
            let b = parts.next()?.parse().ok()?;
            Some([r, g, b])
        })();
        match rgb {
            Some([r, g, b]) => palette.push([r as f32, g as f32, b as f32]),
            None => {
                return Err(PixelSnapperError::InvalidInput(format!(
                    "Invalid .gpl color line: '{}'",
                    line
                )))
            }
        }
    }
    Ok(palette)
}

/// Parse a JASC `.pal` palette: "JASC-PAL" magic, version, color count, then
/// `R G B` rows.
#[cfg(not(target_arch = "wasm32"))]
fn parse_jasc_pal(content: &str) -> Result<Vec<[f32; 3]>> {
    let mut lines = content.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some("JASC-PAL") {
        return Err(PixelSnapperError::InvalidInput(
            "Not a JASC palette: missing 'JASC-PAL' magic".to_string(),
        ));
    }
    let _version = lines.next();
    let _count = lines.next();

    let mut palette = Vec::new();
    for line in lines {
        let mut parts = line.split_whitespace();
        let rgb: Option<[u8; 3]> = (|| {
            let r = parts.next()?.parse().ok()?;
            let g = parts.next()?.parse().ok()?;
            let b = parts.next()?.parse().ok()?;
            Some([r, g, b])
        })();
        match rgb {
            Some([r, g, b]) => palette.push([r as f32, g as f32, b as f32]),
            None => {
                return Err(PixelSnapperError::InvalidInput(format!(
                    "Invalid .pal color line: '{}'",
                    line
                )))
            }
        }
    }
    Ok(palette)
}

/// Extract a palette from an image's opaque pixels. Returns the palette and
/// the number of unique colors found; when that exceeds `max_colors` the
/// palette is reduced deterministically via k-means (seed 42) instead of
/// erroring, so photos also work as palette sources.
fn palette_from_image_bytes(bytes: &[u8], max_colors: usize) -> Result<(Vec<[f32; 3]>, usize)> {
    let img = image::load_from_memory(bytes)?.to_rgba8();

    let mut seen = std::collections::HashSet::new();
    let mut unique: Vec<[f32; 3]> = Vec::new();
    let mut opaque: Vec<[f32; 3]> = Vec::new();
    for p in img.pixels() {
        if p[3] == 0 {
            continue;
        }
        let rgb = [p[0] as f32, p[1] as f32, p[2] as f32];
        opaque.push(rgb);
        if seen.insert([p[0], p[1], p[2]]) {
            unique.push(rgb);
        }
    }

    if unique.is_empty() {
        return Err(PixelSnapperError::InvalidInput(
            "Palette image has no opaque pixels".to_string(),
        ));
    }

    let unique_count = unique.len();
    if unique_count <= max_colors {
        return Ok((unique, unique_count));
    }
    let reduced = kmeans_centroids(&opaque, max_colors, 42, 15)?;
    Ok((reduced, unique_count))
}

/// Parse a palette file by extension: GIMP `.gpl`, JASC `.pal`, or Lospec
/// `.hex` style (one hex color per line; blank lines and `;` comment lines are
/// ignored) for everything else.
#[cfg(not(target_arch = "wasm32"))]
fn parse_palette_file(path: &Path) -> Result<Vec<[f32; 3]>> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        PixelSnapperError::ProcessingError(format!(
            "Failed to read palette file '{}': {}",
            path.display(),
            e
        ))
    })?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("gpl") => parse_gpl(&content),
        Some("pal") => parse_jasc_pal(&content),
        _ => content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with(';'))
            .map(parse_hex_color)
            .collect(),
    }
}

/// Resolve a `--palette` argument into RGB centroids plus a human-readable
/// source label. Image files (`.png`/`.jpg`/`.jpeg`) have their palette
/// extracted; palette files (`.hex`/`.gpl`/`.pal`, or any existing file) are
/// parsed by extension; anything else is a comma-separated list of hex colors.
#[cfg(not(target_arch = "wasm32"))]
fn resolve_palette(value: &str) -> Result<(Vec<[f32; 3]>, String)> {
    const MAX_PALETTE: usize = 256;
    /// Palette-image sources with more unique colors than this get reduced
    /// via k-means instead of erroring out.
    const IMAGE_PALETTE_COLORS: usize = 64;

    let path = Path::new(value);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    if matches!(ext.as_deref(), Some("png" | "jpg" | "jpeg")) {
        let bytes = std::fs::read(path).map_err(|e| {
            PixelSnapperError::ProcessingError(format!(
                "Failed to read palette image '{}': {}",
                path.display(),
                e
            ))
        })?;
        let (palette, unique_count) = palette_from_image_bytes(&bytes, IMAGE_PALETTE_COLORS)?;
        let source = if unique_count > palette.len() {
            format!(
                "{} ({} unique colors reduced to {} via k-means)",
                value,
                unique_count,
                palette.len()
            )
        } else {
            value.to_string()
        };
        return Ok((palette, source));
    }

    let is_palette_file =
        path.is_file() || matches!(ext.as_deref(), Some("hex" | "gpl" | "pal"));

    let palette = if is_palette_file {
        parse_palette_file(path)?
    } else {
        parse_palette_string(value)?
    };

    if palette.is_empty() {
        return Err(PixelSnapperError::InvalidInput(
            "Palette is empty (no valid colors)".to_string(),
        ));
    }
    if palette.len() > MAX_PALETTE {
        return Err(PixelSnapperError::InvalidInput(format!(
            "Palette too large: {} colors (max {})",
            palette.len(),
            MAX_PALETTE
        )));
    }

    let source = if is_palette_file {
        value.to_string()
    } else {
        "inline".to_string()
    };
    Ok((palette, source))
}

/// Snap each k-means centroid to its nearest color in `palette`. Used when a
/// palette is combined with an explicit color count.
fn snap_centroids_to_palette(centroids: &[[f32; 3]], palette: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let palette_lab: Vec<[f32; 3]> = palette.iter().map(srgb_to_oklab).collect();
    centroids
        .iter()
        .map(|c| palette[nearest_in_oklab(&srgb_to_oklab(c), &palette_lab)])
        .collect()
}

/// One-line summary of the active palette for CLI output, or `None` when no
/// palette is set.
#[cfg(not(target_arch = "wasm32"))]
fn palette_summary(config: &Config) -> Option<String> {
    let palette = config.custom_palette.as_ref()?;
    let source = config.palette_source.as_deref().unwrap_or("inline");
    Some(if config.k_colors_explicit {
        format!(
            "Palette: {} colors (from {}), limited to {} via k-means",
            palette.len(),
            source,
            config.k_colors
        )
    } else {
        format!(
            "Palette: {} colors (from {}) — k_colors ignored",
            palette.len(),
            source
        )
    })
}

/// Quantize the image and also return the effective palette actually used
/// (explicit palette, snapped centroids, or discovered centroids) so the
/// optional dither pass can target the same colors. `None` only when the
/// image had no opaque pixels.
fn quantize_image(img: &RgbaImage, config: &Config) -> Result<(RgbaImage, Option<Vec<[f32; 3]>>)> {
    if let Some(palette) = &config.custom_palette {
        if palette.is_empty() {
            return Err(PixelSnapperError::InvalidInput(
                "Palette is empty".to_string(),
            ));
        }
        // Palette only: snap every pixel straight to the palette. With an
        // explicit color count we fall through to k-means below and snap the
        // resulting centroids to the palette instead.
        if !config.k_colors_explicit {
            return Ok((map_to_palette(img, palette), Some(palette.clone())));
        }
    }

    if config.k_colors == 0 {
        return Err(PixelSnapperError::InvalidInput(
            "Number of colors must be greater than 0".to_string(),
        ));
    }

    let opaque_pixels: Vec<[f32; 3]> = img
        .pixels()
        .filter_map(|p| {
            if p[3] == 0 {
                None
            } else {
                Some([p[0] as f32, p[1] as f32, p[2] as f32])
            }
        })
        .collect();
    let n_pixels = opaque_pixels.len();
    if n_pixels == 0 {
        return Ok((img.clone(), None));
    }

    let centroids = kmeans_centroids(
        &opaque_pixels,
        config.k_colors,
        config.k_seed,
        config.max_kmeans_iterations,
    )?;

    // When a palette is combined with an explicit color count, snap the k-means
    // centroids to their nearest palette color so the output keeps the palette's
    // hues while staying within `k_colors` swatches.
    let centroids = match &config.custom_palette {
        Some(palette) => snap_centroids_to_palette(&centroids, palette),
        None => centroids,
    };

    Ok((map_to_palette(img, &centroids), Some(centroids)))
}

/// Deterministic k-means++ (ChaCha8-seeded) over RGB pixels. Extracted from
/// `quantize_image` so palette-from-image extraction can reuse it; the RNG
/// sampling order must stay bit-identical to keep seeded outputs stable.
fn kmeans_centroids(
    opaque_pixels: &[[f32; 3]],
    k: usize,
    seed: u64,
    max_iterations: usize,
) -> Result<Vec<[f32; 3]>> {
    let n_pixels = opaque_pixels.len();
    if n_pixels == 0 || k == 0 {
        return Err(PixelSnapperError::InvalidInput(
            "k-means requires at least one pixel and one cluster".to_string(),
        ));
    }

    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let k = k.min(n_pixels);

    fn sample_index(rng: &mut ChaCha8Rng, upper: usize) -> usize {
        debug_assert!(upper > 0);
        let upper = upper as u64;
        rng.gen_range(0..upper) as usize
    }

    let mut centroids: Vec<[f32; 3]> = Vec::with_capacity(k);
    let first_idx = sample_index(&mut rng, n_pixels);
    centroids.push(opaque_pixels[first_idx]);
    let mut distances = vec![f32::MAX; n_pixels];

    // Maybe try a faster algorithm for this? like https://crates.io/crates/kmeans_colors
    for _ in 1..k {
        let last_c = centroids.last().unwrap();
        let mut sum_sq_dist = 0.0;

        for (i, p) in opaque_pixels.iter().enumerate() {
            let d_sq = dist_sq(p, last_c);
            if d_sq < distances[i] {
                distances[i] = d_sq;
            }
            sum_sq_dist += distances[i];
        }

        if sum_sq_dist <= 0.0 {
            let idx = sample_index(&mut rng, n_pixels);
            centroids.push(opaque_pixels[idx]);
        } else {
            let dist = WeightedIndex::new(&distances).map_err(|e| {
                PixelSnapperError::ProcessingError(format!("Failed to sample new centroid: {}", e))
            })?;
            let idx = dist.sample(&mut rng);
            centroids.push(opaque_pixels[idx]);
        }
    }

    let mut prev_centroids = centroids.clone();
    for iteration in 0..max_iterations {
        let mut sums = vec![[0.0f32; 3]; k];
        let mut counts = vec![0usize; k];

        for p in opaque_pixels {
            let mut min_dist = f32::MAX;
            let mut best_k = 0;

            for (i, c) in centroids.iter().enumerate() {
                let d = dist_sq(p, c);
                if d < min_dist {
                    min_dist = d;
                    best_k = i;
                }
            }
            sums[best_k][0] += p[0];
            sums[best_k][1] += p[1];
            sums[best_k][2] += p[2];
            counts[best_k] += 1;
        }

        for i in 0..k {
            if counts[i] > 0 {
                let fcount = counts[i] as f32;
                centroids[i] = [
                    sums[i][0] / fcount,
                    sums[i][1] / fcount,
                    sums[i][2] / fcount,
                ];
            }
        }

        if iteration > 0 {
            let mut max_movement = 0.0f32;
            for (new_c, old_c) in centroids.iter().zip(prev_centroids.iter()) {
                let movement = dist_sq(new_c, old_c);
                if movement > max_movement {
                    max_movement = movement;
                }
            }

            if max_movement < 0.01 {
                break;
            }
        }

        prev_centroids.copy_from_slice(&centroids);
    }

    Ok(centroids)
}

fn compute_profiles(img: &RgbaImage) -> Result<(Vec<f64>, Vec<f64>)> {
    let (w, h) = img.dimensions();

    if w < 3 || h < 3 {
        return Err(PixelSnapperError::InvalidInput(
            "Image too small (minimum 3x3)".to_string(),
        ));
    }

    let mut col_proj = vec![0.0; w as usize];
    let mut row_proj = vec![0.0; h as usize];

    let gray = |x, y| {
        let p = img.get_pixel(x, y);
        if p[3] == 0 {
            0.0
        } else {
            0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64
        }
    };

    // kernels: [-1, 0, 1]
    for y in 0..h {
        for x in 1..w - 1 {
            let left = gray(x - 1, y);
            let right = gray(x + 1, y);
            let grad = (right - left).abs();
            col_proj[x as usize] += grad;
        }
    }
    for x in 0..w {
        for y in 1..h - 1 {
            let top = gray(x, y - 1);
            let bottom = gray(x, y + 1);
            let grad = (bottom - top).abs();
            row_proj[y as usize] += grad;
        }
    }

    Ok((col_proj, row_proj))
}

fn estimate_step_size(profile: &[f64], config: &Config) -> Option<f64> {
    if profile.is_empty() {
        return None;
    }

    let max_val = profile.iter().cloned().fold(0.0 / 0.0, f64::max);
    if max_val == 0.0 {
        return None; // Decide later
    }
    let threshold = max_val * config.peak_threshold_multiplier;

    let mut peaks = Vec::new();
    for i in 1..profile.len() - 1 {
        if profile[i] > threshold && profile[i] > profile[i - 1] && profile[i] > profile[i + 1] {
            peaks.push(i);
        }
    }

    if peaks.len() < 2 {
        return None;
    }

    let mut clean_peaks = vec![peaks[0]];
    for &p in peaks.iter().skip(1) {
        if p - clean_peaks.last().unwrap() > (config.peak_distance_filter - 1) {
            clean_peaks.push(p);
        }
    }

    if clean_peaks.len() < 2 {
        return None;
    }

    // Compute diffs
    let mut diffs: Vec<f64> = clean_peaks
        .windows(2)
        .map(|w| (w[1] - w[0]) as f64)
        .collect();

    // Median
    diffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    Some(diffs[diffs.len() / 2])
}

fn resolve_step_sizes(
    step_x_opt: Option<f64>,
    step_y_opt: Option<f64>,
    width: u32,
    height: u32,
    config: &Config,
) -> (f64, f64) {
    if let Some(px) = config.pixel_size_override {
        return (px, px);
    }

    match (step_x_opt, step_y_opt) {
        (Some(sx), Some(sy)) => {
            let ratio = if sx > sy { sx / sy } else { sy / sx };
            if ratio > config.max_step_ratio {
                let smaller = sx.min(sy);
                (smaller, smaller)
            } else {
                let avg = (sx + sy) / 2.0;
                (avg, avg)
            }
        }

        (Some(sx), None) => (sx, sx),

        (None, Some(sy)) => (sy, sy),

        (None, None) => {
            let fallback_step =
                ((width.min(height) as f64) / config.fallback_target_segments as f64).max(1.0);
            (fallback_step, fallback_step)
        }
    }
}

fn stabilize_both_axes(
    profile_x: &[f64],
    profile_y: &[f64],
    raw_col_cuts: Vec<usize>,
    raw_row_cuts: Vec<usize>,
    width: usize,
    height: usize,
    config: &Config,
) -> (Vec<usize>, Vec<usize>) {
    let col_cuts_pass1 = stabilize_cuts(
        profile_x,
        raw_col_cuts.clone(),
        width,
        &raw_row_cuts,
        height,
        config,
    );
    let row_cuts_pass1 = stabilize_cuts(
        profile_y,
        raw_row_cuts.clone(),
        height,
        &raw_col_cuts,
        width,
        config,
    );

    // Check if the results are coherent
    let col_cells = col_cuts_pass1.len().saturating_sub(1).max(1);
    let row_cells = row_cuts_pass1.len().saturating_sub(1).max(1);
    let col_step = width as f64 / col_cells as f64;
    let row_step = height as f64 / row_cells as f64;

    let step_ratio = if col_step > row_step {
        col_step / row_step
    } else {
        row_step / col_step
    };

    if step_ratio > config.max_step_ratio {
        let target_step = col_step.min(row_step);

        let final_col_cuts = if col_step > target_step * 1.2 {
            snap_uniform_cuts(
                profile_x,
                width,
                target_step,
                config,
                config.min_cuts_per_axis,
            )
        } else {
            col_cuts_pass1
        };

        let final_row_cuts = if row_step > target_step * 1.2 {
            snap_uniform_cuts(
                profile_y,
                height,
                target_step,
                config,
                config.min_cuts_per_axis,
            )
        } else {
            row_cuts_pass1
        };

        (final_col_cuts, final_row_cuts)
    } else {
        (col_cuts_pass1, row_cuts_pass1)
    }
}

// Tried uniform grid instead of an elastic-ish walker, but the result was a bit worse.
// Keeping the walker for now. But some distortions might happen...
fn walk(profile: &[f64], step_size: f64, limit: usize, config: &Config) -> Result<Vec<usize>> {
    if profile.is_empty() {
        return Err(PixelSnapperError::ProcessingError(
            "Cannot walk on empty profile".to_string(),
        ));
    }

    let mut cuts = vec![0];
    let mut current_pos = 0.0;
    let search_window =
        (step_size * config.walker_search_window_ratio).max(config.walker_min_search_window);
    let mean_val: f64 = profile.iter().sum::<f64>() / profile.len() as f64;

    while current_pos < limit as f64 {
        let target = current_pos + step_size;
        if target >= limit as f64 {
            cuts.push(limit);
            break;
        }

        let start_search = ((target - search_window) as usize).max((current_pos + 1.0) as usize);
        let end_search = ((target + search_window) as usize).min(limit);

        if end_search <= start_search {
            current_pos = target;
            continue;
        }

        let mut max_val = -1.0;
        let mut max_idx = start_search;
        for i in start_search..end_search {
            if profile[i] > max_val {
                max_val = profile[i];
                max_idx = i;
            }
        }

        if max_val > mean_val * config.walker_strength_threshold {
            cuts.push(max_idx);
            current_pos = max_idx as f64;
        } else {
            cuts.push(target as usize);
            current_pos = target;
        }
    }
    Ok(cuts)
}

fn stabilize_cuts(
    profile: &[f64],
    cuts: Vec<usize>,
    limit: usize,
    sibling_cuts: &[usize],
    sibling_limit: usize,
    config: &Config,
) -> Vec<usize> {
    if limit == 0 {
        return vec![0];
    }

    let cuts = sanitize_cuts(cuts, limit);
    let min_required = config.min_cuts_per_axis.max(2).min(limit.saturating_add(1));
    let axis_cells = cuts.len().saturating_sub(1);
    let sibling_cells = sibling_cuts.len().saturating_sub(1);
    let sibling_has_grid =
        sibling_limit > 0 && sibling_cells >= min_required.saturating_sub(1) && sibling_cells > 0;
    let steps_skewed = sibling_has_grid && axis_cells > 0 && {
        let axis_step = limit as f64 / axis_cells as f64;
        let sibling_step = sibling_limit as f64 / sibling_cells as f64;
        let step_ratio = axis_step / sibling_step;
        step_ratio > config.max_step_ratio || step_ratio < 1.0 / config.max_step_ratio
    };
    let has_enough = cuts.len() >= min_required;

    if has_enough && !steps_skewed {
        return cuts;
    }

    let mut target_step = if sibling_has_grid {
        sibling_limit as f64 / sibling_cells as f64
    } else if config.fallback_target_segments > 1 {
        limit as f64 / config.fallback_target_segments as f64
    } else if axis_cells > 0 {
        limit as f64 / axis_cells as f64
    } else {
        limit as f64
    };
    if !target_step.is_finite() || target_step <= 0.0 {
        target_step = 1.0;
    }

    snap_uniform_cuts(profile, limit, target_step, config, min_required)
}

fn sanitize_cuts(mut cuts: Vec<usize>, limit: usize) -> Vec<usize> {
    if limit == 0 {
        return vec![0];
    }

    let mut has_zero = false;
    let mut has_limit = false;

    for value in cuts.iter_mut() {
        if *value == 0 {
            has_zero = true;
        }
        if *value >= limit {
            *value = limit;
        }
        if *value == limit {
            has_limit = true;
        }
    }

    if !has_zero {
        cuts.push(0);
    }
    if !has_limit {
        cuts.push(limit);
    }

    cuts.sort_unstable();
    cuts.dedup();
    cuts
}

fn snap_uniform_cuts(
    profile: &[f64],
    limit: usize,
    target_step: f64,
    config: &Config,
    min_required: usize,
) -> Vec<usize> {
    if limit == 0 {
        return vec![0];
    }
    if limit == 1 {
        return vec![0, 1];
    }

    // Get desired cells
    let mut desired_cells = if target_step.is_finite() && target_step > 0.0 {
        (limit as f64 / target_step).round() as usize
    } else {
        0
    };
    desired_cells = desired_cells
        .max(min_required.saturating_sub(1))
        .max(1)
        .min(limit);

    let cell_width = limit as f64 / desired_cells as f64;
    let search_window =
        (cell_width * config.walker_search_window_ratio).max(config.walker_min_search_window);
    let mean_val = if profile.is_empty() {
        0.0
    } else {
        profile.iter().sum::<f64>() / profile.len() as f64
    };

    let mut cuts = Vec::with_capacity(desired_cells + 1);
    cuts.push(0);
    for idx in 1..desired_cells {
        let target = cell_width * idx as f64;
        let prev = *cuts.last().unwrap();
        if prev + 1 >= limit {
            break;
        }
        let mut start = ((target - search_window).floor() as isize)
            .max(prev as isize + 1)
            .max(0);
        let mut end = ((target + search_window).ceil() as isize).min(limit as isize - 1);
        if end < start {
            start = prev as isize + 1;
            end = start;
        }
        let start = start as usize;
        let end = end as usize;
        let mut best_idx = start.min(profile.len().saturating_sub(1));
        let mut best_val = -1.0;
        for i in start..=end.min(profile.len().saturating_sub(1)) {
            let v = profile.get(i).copied().unwrap_or(0.0);
            if v > best_val {
                best_val = v;
                best_idx = i;
            }
        }
        let strength_threshold = mean_val * config.walker_strength_threshold;
        if best_val < strength_threshold {
            let mut fallback_idx = target.round() as isize;
            if fallback_idx <= prev as isize {
                fallback_idx = prev as isize + 1;
            }
            if fallback_idx >= limit as isize {
                fallback_idx = (limit as isize - 1).max(prev as isize + 1);
            }
            best_idx = fallback_idx as usize;
        }
        cuts.push(best_idx);
    }
    if *cuts.last().unwrap() != limit {
        cuts.push(limit);
    }
    cuts = sanitize_cuts(cuts, limit);
    cuts
}

fn resample(img: &RgbaImage, cols: &[usize], rows: &[usize]) -> Result<RgbaImage> {
    if cols.len() < 2 || rows.len() < 2 {
        return Err(PixelSnapperError::ProcessingError(
            "Insufficient grid cuts for resampling".to_string(),
        ));
    }

    let out_w = (cols.len().max(1) - 1) as u32;
    let out_h = (rows.len().max(1) - 1) as u32;
    let mut final_img: RgbaImage = ImageBuffer::new(out_w, out_h);

    for (y_i, w_y) in rows.windows(2).enumerate() {
        for (x_i, w_x) in cols.windows(2).enumerate() {
            let ys = w_y[0];
            let ye = w_y[1];
            let xs = w_x[0];
            let xe = w_x[1];

            if xe <= xs || ye <= ys {
                continue;
            }

            let mut counts: HashMap<[u8; 4], usize> = HashMap::new();

            for y in ys..ye {
                for x in xs..xe {
                    if x < img.width() as usize && y < img.height() as usize {
                        let p = img.get_pixel(x as u32, y as u32).0;
                        *counts.entry(p).or_insert(0) += 1;
                    }
                }
            }

            let mut best_pixel = [0, 0, 0, 0];

            let mut candidates: Vec<([u8; 4], usize)> = counts.into_iter().collect();
            candidates.sort_by(|a, b| {
                let count_cmp = b.1.cmp(&a.1);
                if count_cmp == Ordering::Equal {
                    a.0.cmp(&b.0)
                } else {
                    count_cmp
                }
            });

            if let Some(winner) = candidates.first() {
                best_pixel = winner.0;
            }

            final_img.put_pixel(x_i as u32, y_i as u32, Rgba(best_pixel));
        }
    }
    Ok(final_img)
}

/// Like `resample`, but averages each cell's opaque pixels instead of taking
/// the majority color. Cells with no opaque pixels stay fully transparent.
/// Used as the dither input so gradients survive into the output grid.
fn resample_average(img: &RgbaImage, cols: &[usize], rows: &[usize]) -> Result<RgbaImage> {
    if cols.len() < 2 || rows.len() < 2 {
        return Err(PixelSnapperError::ProcessingError(
            "Insufficient grid cuts for resampling".to_string(),
        ));
    }

    let out_w = (cols.len().max(1) - 1) as u32;
    let out_h = (rows.len().max(1) - 1) as u32;
    let mut final_img: RgbaImage = ImageBuffer::new(out_w, out_h);

    for (y_i, w_y) in rows.windows(2).enumerate() {
        for (x_i, w_x) in cols.windows(2).enumerate() {
            let (ys, ye, xs, xe) = (w_y[0], w_y[1], w_x[0], w_x[1]);
            if xe <= xs || ye <= ys {
                continue;
            }

            let mut sum = [0.0f64; 4];
            let mut count = 0usize;
            for y in ys..ye {
                for x in xs..xe {
                    if x < img.width() as usize && y < img.height() as usize {
                        let p = img.get_pixel(x as u32, y as u32).0;
                        if p[3] == 0 {
                            continue;
                        }
                        for ch in 0..4 {
                            sum[ch] += p[ch] as f64;
                        }
                        count += 1;
                    }
                }
            }

            let pixel = if count > 0 {
                let avg = |ch: usize| (sum[ch] / count as f64).round() as u8;
                [avg(0), avg(1), avg(2), avg(3)]
            } else {
                [0, 0, 0, 0]
            };
            final_img.put_pixel(x_i as u32, y_i as u32, Rgba(pixel));
        }
    }
    Ok(final_img)
}

/// Floyd–Steinberg dither against `palette`: error accumulated in f32 sRGB,
/// nearest color chosen in OKLab. Transparent pixels neither receive nor emit
/// diffused error.
fn dither_to_palette(img: &RgbaImage, palette: &[[f32; 3]]) -> RgbaImage {
    if palette.is_empty() {
        return img.clone();
    }
    let palette_lab: Vec<[f32; 3]> = palette.iter().map(srgb_to_oklab).collect();
    let (w, h) = img.dimensions();
    let mut work: Vec<[f32; 3]> = img
        .pixels()
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
        .collect();
    let mut out = RgbaImage::new(w, h);

    for y in 0..h {
        for x in 0..w {
            let src = img.get_pixel(x, y);
            if src[3] == 0 {
                out.put_pixel(x, y, *src);
                continue;
            }
            let idx = (y * w + x) as usize;
            let old = [
                work[idx][0].clamp(0.0, 255.0),
                work[idx][1].clamp(0.0, 255.0),
                work[idx][2].clamp(0.0, 255.0),
            ];
            let chosen = palette[nearest_in_oklab(&srgb_to_oklab(&old), &palette_lab)];
            out.put_pixel(
                x,
                y,
                Rgba([
                    chosen[0].round() as u8,
                    chosen[1].round() as u8,
                    chosen[2].round() as u8,
                    src[3],
                ]),
            );

            let err = [old[0] - chosen[0], old[1] - chosen[1], old[2] - chosen[2]];
            let mut diffuse = |dx: i64, dy: i64, weight: f32| {
                let nx = x as i64 + dx;
                let ny = y as i64 + dy;
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    return;
                }
                if img.get_pixel(nx as u32, ny as u32)[3] == 0 {
                    return;
                }
                let n_idx = (ny as u64 * w as u64 + nx as u64) as usize;
                for ch in 0..3 {
                    work[n_idx][ch] += err[ch] * weight;
                }
            };
            diffuse(1, 0, 7.0 / 16.0);
            diffuse(-1, 1, 3.0 / 16.0);
            diffuse(0, 1, 5.0 / 16.0);
            diffuse(1, 1, 1.0 / 16.0);
        }
    }
    out
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_color_six_digits() {
        assert_eq!(parse_hex_color("#1a1a2e").unwrap(), [26.0, 26.0, 46.0]);
        assert_eq!(parse_hex_color("1a1a2e").unwrap(), [26.0, 26.0, 46.0]);
    }

    #[test]
    fn parse_hex_color_three_digits_expands() {
        assert_eq!(parse_hex_color("#abc").unwrap(), [170.0, 187.0, 204.0]);
    }

    #[test]
    fn parse_hex_color_rejects_bad_input() {
        assert!(parse_hex_color("zzz").is_err());
        assert!(parse_hex_color("12345").is_err());
        assert!(parse_hex_color("#gg0000").is_err());
    }

    #[test]
    fn parse_palette_string_parses_multiple_and_skips_empty() {
        let p = parse_palette_string("#fff,000,#ff0000").unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(p[0], [255.0, 255.0, 255.0]);
        assert_eq!(p[2], [255.0, 0.0, 0.0]);

        let p2 = parse_palette_string("#fff,,").unwrap();
        assert_eq!(p2.len(), 1);

        let empty = parse_palette_string("").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn resolve_palette_inline_ok() {
        let (palette, source) = resolve_palette("#fff,#000").unwrap();
        assert_eq!(palette.len(), 2);
        assert_eq!(source, "inline");
    }

    #[test]
    fn resolve_palette_rejects_empty_and_oversized() {
        assert!(resolve_palette("").is_err());
        assert!(resolve_palette(",,").is_err());

        let big: String = std::iter::repeat("#010101")
            .take(257)
            .collect::<Vec<_>>()
            .join(",");
        assert!(resolve_palette(&big).is_err());
    }

    #[test]
    fn map_to_palette_snaps_and_preserves_transparency() {
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(0, 0, Rgba([250, 250, 250, 255])); // near white
        img.put_pixel(1, 0, Rgba([5, 5, 5, 255])); // near black
        img.put_pixel(0, 1, Rgba([10, 10, 10, 0])); // transparent
        img.put_pixel(1, 1, Rgba([200, 10, 10, 255])); // perceptually closer to white of {white,black}

        let palette = [[255.0, 255.0, 255.0], [0.0, 0.0, 0.0]];
        let out = map_to_palette(&img, &palette);

        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(out.get_pixel(1, 0).0, [0, 0, 0, 255]);
        // transparent pixel passes through unchanged
        assert_eq!(out.get_pixel(0, 1).0, [10, 10, 10, 0]);
    }

    #[test]
    fn oklab_known_values() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;

        let white = srgb_to_oklab(&[255.0, 255.0, 255.0]);
        assert!(close(white[0], 1.0) && close(white[1], 0.0) && close(white[2], 0.0));

        let black = srgb_to_oklab(&[0.0, 0.0, 0.0]);
        assert!(close(black[0], 0.0) && close(black[1], 0.0) && close(black[2], 0.0));

        let red = srgb_to_oklab(&[255.0, 0.0, 0.0]);
        assert!(close(red[0], 0.628) && close(red[1], 0.225) && close(red[2], 0.126));
    }

    #[test]
    fn map_to_palette_uses_perceptual_distance() {
        // Under raw sRGB Euclidean distance [200,10,10] snaps to black; in
        // OKLab it is perceptually closer to white. Regression-guards the
        // perceptual matching switch.
        let mut img = RgbaImage::new(1, 1);
        img.put_pixel(0, 0, Rgba([200, 10, 10, 255]));
        let palette = [[255.0, 255.0, 255.0], [0.0, 0.0, 0.0]];
        let out = map_to_palette(&img, &palette);
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn parse_gpl_basic() {
        let content = "GIMP Palette\nName: Test\nColumns: 4\n# a comment\n255 0 0 Red\n0 255 0\n  0 0 255  Blue Color\n";
        let p = parse_gpl(content).unwrap();
        assert_eq!(
            p,
            vec![[255.0, 0.0, 0.0], [0.0, 255.0, 0.0], [0.0, 0.0, 255.0]]
        );
    }

    #[test]
    fn parse_gpl_rejects_missing_header() {
        assert!(parse_gpl("255 0 0\n").is_err());
        assert!(parse_gpl("").is_err());
    }

    #[test]
    fn parse_jasc_pal_basic() {
        let content = "JASC-PAL\n0100\n2\n255 255 255\n0 0 0\n";
        let p = parse_jasc_pal(content).unwrap();
        assert_eq!(p, vec![[255.0, 255.0, 255.0], [0.0, 0.0, 0.0]]);
    }

    #[test]
    fn parse_jasc_pal_rejects_bad_magic() {
        assert!(parse_jasc_pal("RIFF-PAL\n0100\n1\n0 0 0\n").is_err());
        assert!(parse_jasc_pal("").is_err());
    }

    fn png_bytes(img: &RgbaImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
        bytes
    }

    #[test]
    fn palette_from_image_unique_colors() {
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([0, 255, 0, 255]));
        img.put_pixel(0, 1, Rgba([0, 0, 255, 255]));
        img.put_pixel(1, 1, Rgba([255, 255, 255, 255]));

        let (palette, unique) = palette_from_image_bytes(&png_bytes(&img), 64).unwrap();
        assert_eq!(unique, 4);
        assert_eq!(palette.len(), 4);
        assert!(palette.contains(&[255.0, 0.0, 0.0]));
    }

    #[test]
    fn palette_from_image_reduces_when_over_limit() {
        // 16x16 image with 256 unique colors, reduced to 8.
        let mut img = RgbaImage::new(16, 16);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgba([(x * 16 + y) as u8, x as u8 * 10, y as u8 * 10, 255]);
        }
        let (palette, unique) = palette_from_image_bytes(&png_bytes(&img), 8).unwrap();
        assert_eq!(unique, 256);
        assert_eq!(palette.len(), 8);
    }

    #[test]
    fn kmeans_centroids_deterministic_for_seed() {
        let pixels: Vec<[f32; 3]> = (0..200)
            .map(|i| [(i % 256) as f32, ((i * 7) % 256) as f32, ((i * 13) % 256) as f32])
            .collect();
        let a = kmeans_centroids(&pixels, 5, 42, 15).unwrap();
        let b = kmeans_centroids(&pixels, 5, 42, 15).unwrap();
        let c = kmeans_centroids(&pixels, 5, 43, 15).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn dither_gradient_uses_both_palette_colors() {
        // Horizontal 0..255 gray gradient dithered to {black, white} must use
        // both colors and roughly preserve the mean luminance.
        let w = 64u32;
        let mut img = RgbaImage::new(w, 8);
        for (x, _y, p) in img.enumerate_pixels_mut() {
            let v = (x * 255 / (w - 1)) as u8;
            *p = Rgba([v, v, v, 255]);
        }
        let input_mean: f64 =
            img.pixels().map(|p| p[0] as f64).sum::<f64>() / (img.width() * img.height()) as f64;

        let palette = [[0.0, 0.0, 0.0], [255.0, 255.0, 255.0]];
        let out = dither_to_palette(&img, &palette);

        let mut blacks = 0;
        let mut whites = 0;
        let mut out_sum = 0.0f64;
        for p in out.pixels() {
            match p[0] {
                0 => blacks += 1,
                255 => whites += 1,
                other => panic!("unexpected color {} in dithered output", other),
            }
            out_sum += p[0] as f64;
        }
        assert!(blacks > 0 && whites > 0);
        let out_mean = out_sum / (out.width() * out.height()) as f64;
        assert!(
            (out_mean - input_mean).abs() < 16.0,
            "mean luminance drifted: in {:.1} out {:.1}",
            input_mean,
            out_mean
        );
    }

    #[test]
    fn resample_average_handles_transparency() {
        // 4x2 image, 2x1 cells: left cell has 2 opaque + 2 transparent pixels
        // (average of opaque only), right cell is fully transparent.
        let mut img = RgbaImage::new(4, 2);
        img.put_pixel(0, 0, Rgba([100, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([200, 0, 0, 255]));
        img.put_pixel(0, 1, Rgba([9, 9, 9, 0]));
        img.put_pixel(1, 1, Rgba([9, 9, 9, 0]));
        // pixels (2..4, 0..2) stay default transparent black

        let out = resample_average(&img, &[0, 2, 4], &[0, 2]).unwrap();
        assert_eq!(out.dimensions(), (2, 1));
        assert_eq!(out.get_pixel(0, 0).0, [150, 0, 0, 255]);
        assert_eq!(out.get_pixel(1, 0).0[3], 0);
    }

    #[test]
    fn snap_centroids_to_palette_picks_nearest() {
        let palette = [[0.0, 0.0, 0.0], [255.0, 255.0, 255.0]];
        let centroids = [
            [10.0, 10.0, 10.0],
            [240.0, 240.0, 240.0],
            [130.0, 130.0, 130.0],
        ];
        let snapped = snap_centroids_to_palette(&centroids, &palette);
        assert_eq!(snapped[0], [0.0, 0.0, 0.0]);
        assert_eq!(snapped[1], [255.0, 255.0, 255.0]);
        // 130-gray is closer to white in OKLab too (L ≈ 0.60)
        assert_eq!(snapped[2], [255.0, 255.0, 255.0]);
    }
}
