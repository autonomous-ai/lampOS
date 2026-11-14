use super::*;
use std::time::Instant;

/// A camera-like JPEG: a gradient under a fine texture, so it is not all
/// flat colour and compresses about as a desk does.
pub(crate) fn camera_jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut seed = 0x2545_f491_u32;
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = seed % 24;
            rgb.push(((x * 200 / width) + noise) as u8);
            rgb.push(((y * 200 / height) + noise) as u8);
            rgb.push((120 + noise) as u8);
        }
    }
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 85)
        .encode(
            &rgb,
            width as u16,
            height as u16,
            jpeg_encoder::ColorType::Rgb,
        )
        .unwrap();
    out
}

/// A JPEG's size and its pixels.
pub(crate) fn decoded(jpeg: &[u8]) -> ((usize, usize), Vec<u8>) {
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(jpeg, options);
    let rgb = decoder.decode().unwrap();
    (decoder.dimensions().unwrap(), rgb)
}

fn mean(rgb: &[u8], channel: usize) -> f64 {
    let values: Vec<f64> = rgb
        .iter()
        .skip(channel)
        .step_by(3)
        .map(|&v| f64::from(v))
        .collect();
    values.iter().sum::<f64>() / values.len() as f64
}

#[test]
fn the_cameras_1280_by_960_is_sent_as_320_by_240() {
    let camera = camera_jpeg(1280, 960);
    let sent = for_sending(&camera, 1280, 960).unwrap();
    let (size, rgb) = decoded(&sent);
    assert_eq!(size, (320, 240));
    let (_, original) = decoded(&camera);
    for channel in 0..3 {
        let (a, b) = (mean(&original, channel), mean(&rgb, channel));
        assert!((a - b).abs() < 2.0, "channel {channel}: {a} vs {b}");
    }
    assert!(
        sent.len() < camera.len() / 4,
        "{} of {}",
        sent.len(),
        camera.len()
    );
}

#[test]
fn v1s_640_by_480_is_scaled_too() {
    let sent = for_sending(&camera_jpeg(640, 480), 640, 480).unwrap();
    assert_eq!(decoded(&sent).0, (320, 240));
}

#[test]
fn exactly_320_by_240_goes_as_it_came() {
    let camera = camera_jpeg(320, 240);
    assert_eq!(*for_sending(&camera, 320, 240).unwrap(), camera);
}

#[test]
fn smaller_than_320_by_240_is_refused() {
    for (width, height) in [(160, 120), (319, 240), (320, 239)] {
        let camera = camera_jpeg(width, height);
        assert_eq!(for_sending(&camera, width, height), Err(Refused::TooSmall));
    }
}

#[test]
fn another_shape_keeps_its_proportion() {
    assert_eq!(sent_size(1280, 960), SENT);
    assert_eq!(sent_size(640, 480), SENT);
    assert_eq!(sent_size(1280, 720), (427, 240));
    assert_eq!(sent_size(960, 1280), (320, 427));
    let sent = for_sending(&camera_jpeg(1280, 720), 1280, 720).unwrap();
    assert_eq!(decoded(&sent).0, (427, 240));
}

#[test]
fn a_frame_not_its_metas_size_or_not_a_jpeg_is_refused() {
    let camera = camera_jpeg(640, 480);
    assert_eq!(for_sending(&camera, 1280, 960), Err(Refused::NotAJpeg));
    assert_eq!(for_sending(b"not a jpeg", 640, 480), Err(Refused::NotAJpeg));
    assert_eq!(for_sending(&camera, 8192, 480), Err(Refused::TooLarge));
}

#[test]
fn area_scaling_takes_each_boxs_mean() {
    // 4×2 to 2×1: each sent pixel is the mean of a 2×2 box.
    let rgb: Vec<u8> = [0, 10, 100, 110, 20, 30, 120, 130]
        .iter()
        .flat_map(|&v| [v, v, v])
        .collect();
    assert_eq!(
        area_scaled(&rgb, (4, 2), (2, 1)),
        [15, 15, 15, 115, 115, 115]
    );
}

/// The time one frame takes, decode, scale and encode: run with
/// `cargo test --release -p lamp-seen -- --ignored --nocapture scale_time`.
#[test]
#[ignore = "a measurement, run by hand in release"]
fn scale_time() {
    for (width, height) in [(1280, 960), (640, 480)] {
        let camera = camera_jpeg(width, height);
        for _ in 0..5 {
            for_sending(&camera, width, height).unwrap();
        }
        let runs = 50;
        let mut times: Vec<f64> = (0..runs)
            .map(|_| {
                let start = Instant::now();
                let sent = for_sending(&camera, width, height).unwrap();
                let took = start.elapsed().as_secs_f64() * 1000.0;
                assert!(!sent.is_empty());
                took
            })
            .collect();
        times.sort_by(f64::total_cmp);
        eprintln!(
            "scale_time {width}x{height} ({} B in): p50 {:.2} ms, max {:.2} ms, {} B out",
            camera.len(),
            times[runs / 2],
            times[runs - 1],
            for_sending(&camera, width, height).unwrap().len()
        );
    }
}
