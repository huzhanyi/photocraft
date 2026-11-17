//! Commands for the built-in export presets and the pixel steps they play.
//!
//! Nothing here calls face detection or subject selection. See [`super::preset_library::omitted`].

use photocraft_color::{ColorMode, PixelFormat};
use photocraft_doc::{Affine, Document, Layer, LayerContent, LayerId, Size};
use photocraft_geom::Rect;
use photocraft_raster::Surface;
use serde_json::{Value, json};

pub use super::preset_library::{Omitted, Preset, actions, all, find, merged_params, mm_to_px, omitted};
use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}

fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| "no document open".into())
}

fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}

fn reject_unknown(cmd: &str, p: &Value, allowed: &[&str]) -> Result<()> {
    if p.is_null() {
        return Ok(());
    }
    let obj = p.as_object().ok_or_else(|| bad(cmd, "parameters must be an object"))?;
    for key in obj.keys() {
        if !allowed.iter().any(|name| name == key) {
            return Err(bad(cmd, format!("unknown parameter `{key}`")));
        }
    }
    Ok(())
}

fn required_px(cmd: &str, p: &Value, key: &str) -> Result<u32> {
    let Some(value) = p.get(key) else { return Err(bad(cmd, format!("`{key}` is required"))) };
    let Some(number) = value.as_u64() else { return Err(bad(cmd, format!("`{key}` must be an integer from 1 to 30000"))) };
    u32::try_from(number).ok().filter(|n| (1..=30_000).contains(n)).ok_or_else(|| bad(cmd, format!("`{key}` must be an integer from 1 to 30000")))
}

fn optional_bool(cmd: &str, p: &Value, key: &str, default: bool) -> Result<bool> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

fn optional_dpi(cmd: &str, p: &Value) -> Result<Option<f32>> {
    match p.get("resolution") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let dpi = value
                .as_f64()
                .filter(|dpi| dpi.is_finite() && (1.0..=2400.0).contains(dpi))
                .ok_or_else(|| bad(cmd, "resolution must be a number of pixels per inch from 1 to 2400"))?;
            Ok(Some(dpi as f32))
        }
    }
}

fn parse_color(cmd: &str, key: &str, text: &str) -> Result<[f32; 3]> {
    let named = match text.to_ascii_lowercase().as_str() {
        "white" => Some([1.0, 1.0, 1.0]),
        "black" => Some([0.0, 0.0, 0.0]),
        "blue" => Some([0x43 as f32 / 255.0, 0x8E as f32 / 255.0, 0xDB as f32 / 255.0]),
        "red" => Some([0xC0 as f32 / 255.0, 0.0, 0.0]),
        _ => None,
    };
    if let Some(color) = named {
        return Ok(color);
    }
    hex_rgb(text).ok_or_else(|| bad(cmd, format!("`{key}` must be white, black, blue, red, or #rrggbb")))
}

fn hex_rgb(text: &str) -> Option<[f32; 3]> {
    let hex = text.strip_prefix('#').unwrap_or(text);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |index: usize| {
        let digits = hex.get(index..index + 2)?;
        let value = u8::from_str_radix(digits, 16).ok()?;
        Some(f32::from(value) / 255.0)
    };
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn color_param(cmd: &str, p: &Value, key: &str, default: [f32; 3]) -> Result<[f32; 3]> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::String(text)) => parse_color(cmd, key, text),
        Some(_) => Err(bad(cmd, format!("`{key}` must be white, black, blue, red, or #rrggbb"))),
    }
}

fn hex_of(color: [f32; 3]) -> String {
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02X}{:02X}{:02X}", byte(color[0]), byte(color[1]), byte(color[2]))
}

fn check_output(cmd: &str, w: u32, h: u32) -> Result<()> {
    if w == 0 || h == 0 || w > 30_000 || h > 30_000 {
        return Err(bad(cmd, "width and height must be integers from 1 to 30000"));
    }
    crate::allocation::check_dense(Rect::from_xywh(0, 0, w, h), 16, cmd)
}

fn flattened(doc: &Document, cmd: &str) -> Result<Vec<[f32; 4]>> {
    let (w, h) = (doc.size.width, doc.size.height);
    let count = u64::from(w).saturating_mul(u64::from(h));
    if w == 0 || h == 0 || count > photocraft_compose::MAX_RENDER_PIXELS {
        return Err(bad(cmd, format!("the document ({w} x {h} px) is too large to flatten")));
    }
    crate::allocation::check_dense(Rect::from_xywh(0, 0, w, h), 16, cmd)?;
    let buf = photocraft_compose::flatten(doc);
    if buf.px.len() as u64 != count {
        return Err(bad(cmd, format!("the document ({w} x {h} px) is too large to flatten")));
    }
    Ok(buf.into_px())
}

fn at(px: &[[f32; 4]], w: u32, h: u32, x: i32, y: i32) -> [f32; 4] {
    if x < 0 || y < 0 {
        return [0.0; 4];
    }
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else { return [0.0; 4] };
    if x >= w || y >= h {
        return [0.0; 4];
    }
    let index = (y as usize).saturating_mul(w as usize).saturating_add(x as usize);
    px.get(index).copied().unwrap_or([0.0; 4])
}

fn float_i32(value: f64) -> Option<i32> {
    if value.is_finite() && value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX) { Some(value as i32) } else { None }
}

fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

fn premul(pixel: [f32; 4]) -> [f32; 4] {
    let alpha = pixel[3].clamp(0.0, 1.0);
    [pixel[0] * alpha, pixel[1] * alpha, pixel[2] * alpha, alpha]
}

fn unpremul(pixel: [f32; 4]) -> [f32; 4] {
    if pixel[3] <= 1.0e-5 { [0.0; 4] } else { [pixel[0] / pixel[3], pixel[1] / pixel[3], pixel[2] / pixel[3], pixel[3].clamp(0.0, 1.0)] }
}

/// Straight-alpha samples are premultiplied before the mix, and coordinates outside the bitmap
/// repeat the edge pixel. Inventing transparent black past the edge would turn an opaque border
/// pink or grey once it is matted.
fn bilinear(px: &[[f32; 4]], w: u32, h: u32, x: f64, y: f64) -> [f32; 4] {
    if !x.is_finite() || !y.is_finite() || w == 0 || h == 0 {
        return [0.0; 4];
    }
    let x = x.clamp(0.0, f64::from(w - 1));
    let y = y.clamp(0.0, f64::from(h - 1));
    let (Some(ix), Some(iy)) = (float_i32(x.floor()), float_i32(y.floor())) else { return [0.0; 4] };
    let tx = (x - x.floor()) as f32;
    let ty = (y - y.floor()) as f32;
    let top = lerp4(premul(at(px, w, h, ix, iy)), premul(at(px, w, h, ix.saturating_add(1), iy)), tx);
    let bottom = lerp4(premul(at(px, w, h, ix, iy.saturating_add(1))), premul(at(px, w, h, ix.saturating_add(1), iy.saturating_add(1))), tx);
    unpremul(lerp4(top, bottom, ty))
}

fn over(src: [f32; 4], background: [f32; 3]) -> [f32; 4] {
    let alpha = src[3].clamp(0.0, 1.0);
    [src[0] * alpha + background[0] * (1.0 - alpha), src[1] * alpha + background[1] * (1.0 - alpha), src[2] * alpha + background[2] * (1.0 - alpha), 1.0]
}

fn pack(fmt: PixelFormat, px: &[[f32; 4]]) -> Result<Vec<f32>> {
    let channels = fmt.channels();
    let count = px.len().checked_mul(channels).ok_or_else(|| EngineError::Other("image is too large to store".into()))?;
    let mut data = vec![0.0; count];
    for (index, pixel) in px.iter().enumerate() {
        let start = index * channels;
        let end = start + channels;
        let wrote =
            photocraft_raster::from_rgba_into(&fmt, *pixel, data.get_mut(start..end).ok_or_else(|| EngineError::Other("image is too large to store".into()))?);
        if wrote != channels {
            return Err(EngineError::Other("could not store a pixel in the document format".into()));
        }
    }
    Ok(data)
}

fn commit_rgb(s: &mut Session, label: &str, px: &[[f32; 4]], w: u32, h: u32, opaque: bool, dpi: Option<f32>) -> Result<()> {
    let depth = s.active().ok_or(EngineError::NoDocument)?.doc.depth;
    let fmt = PixelFormat::new(ColorMode::Rgb, depth, !opaque);
    if px.len() != (w as usize).saturating_mul(h as usize) {
        return Err(EngineError::Other("internal pixel buffer has the wrong size".into()));
    }
    let samples = pack(fmt, px)?;
    s.edit(label, move |doc, active| install_rgb(doc, active, Installed { fmt, samples, w, h, opaque, dpi }))
}

struct Installed {
    fmt: PixelFormat,
    samples: Vec<f32>,
    w: u32,
    h: u32,
    opaque: bool,
    dpi: Option<f32>,
}

fn install_rgb(doc: &mut Document, active: &mut Option<LayerId>, image: Installed) -> Result<()> {
    let Installed { fmt, samples, w, h, opaque, dpi } = image;
    let rect = Rect::from_xywh(0, 0, w, h);
    let expected = (rect.width() as usize).saturating_mul(rect.height() as usize).saturating_mul(fmt.channels());
    if samples.len() != expected || rect.width() != w || rect.height() != h {
        return Err(EngineError::Other("internal pixel buffer has the wrong size".into()));
    }
    let mut surf = Surface::new(fmt);
    surf.write_region(rect, &samples);
    let mut layer = Layer::raster(if opaque { "Background" } else { "Layer 1" }, fmt);
    if opaque {
        layer.locks.transparency = true;
        layer.locks.position = true;
    }
    *crate::pixels_mut(&mut layer)? = surf;
    let id = layer.id;
    doc.mode = ColorMode::Rgb;
    doc.depth = fmt.sample;
    doc.size = Size::new(w, h);
    doc.layers = vec![layer];
    doc.selection = None;
    doc.channels.clear();
    doc.quick_mask = None;
    doc.quick_mask_unselected = false;
    doc.layer_comps.clear();
    doc.last_applied_comp = None;
    doc.last_document_state = None;
    doc.color_table = None;
    doc.duotone = None;
    doc.icc_profile = None;
    if let Some(dpi) = dpi {
        doc.resolution_dpi = dpi;
    }
    *active = Some(id);
    Ok(())
}

#[derive(Clone, Copy)]
enum FrameMode {
    Cover,
    Contain,
}

fn mode_name(mode: FrameMode) -> &'static str {
    match mode {
        FrameMode::Cover => "cover",
        FrameMode::Contain => "contain",
    }
}

fn render_frame(src: &[[f32; 4]], sw: u32, sh: u32, dw: u32, dh: u32, mode: FrameMode, background: [f32; 3]) -> Result<Vec<[f32; 4]>> {
    let mut out = Vec::new();
    let count = (dw as usize).checked_mul(dh as usize).ok_or_else(|| EngineError::Other("frame is too large".into()))?;
    out.try_reserve(count).map_err(|_| EngineError::Other("not enough memory for the frame".into()))?;
    match mode {
        FrameMode::Cover => push_cover(&mut out, src, sw, sh, dw, dh, background),
        FrameMode::Contain => push_contain(&mut out, src, sw, sh, dw, dh, background),
    }
    Ok(out)
}

fn push_cover(out: &mut Vec<[f32; 4]>, src: &[[f32; 4]], sw: u32, sh: u32, dw: u32, dh: u32, background: [f32; 3]) {
    let scale = (f64::from(dw) / f64::from(sw)).max(f64::from(dh) / f64::from(sh));
    let crop_w = f64::from(dw) / scale;
    let crop_h = f64::from(dh) / scale;
    let x0 = (f64::from(sw) - crop_w) / 2.0;
    let y0 = (f64::from(sh) - crop_h) / 2.0;
    for y in 0..dh {
        for x in 0..dw {
            let sx = x0 + (f64::from(x) + 0.5) * crop_w / f64::from(dw) - 0.5;
            let sy = y0 + (f64::from(y) + 0.5) * crop_h / f64::from(dh) - 0.5;
            out.push(over(bilinear(src, sw, sh, sx, sy), background));
        }
    }
}

fn push_contain(out: &mut Vec<[f32; 4]>, src: &[[f32; 4]], sw: u32, sh: u32, dw: u32, dh: u32, background: [f32; 3]) {
    let scale = (f64::from(dw) / f64::from(sw)).min(f64::from(dh) / f64::from(sh));
    let placed_w = f64::from(sw) * scale;
    let placed_h = f64::from(sh) * scale;
    let ox = (f64::from(dw) - placed_w) / 2.0;
    let oy = (f64::from(dh) - placed_h) / 2.0;
    for y in 0..dh {
        for x in 0..dw {
            let px = f64::from(x) + 0.5;
            let py = f64::from(y) + 0.5;
            if px < ox || py < oy || px >= ox + placed_w || py >= oy + placed_h {
                out.push([background[0], background[1], background[2], 1.0]);
            } else {
                let sx = (px - ox) / scale - 0.5;
                let sy = (py - oy) / scale - 0.5;
                out.push(over(bilinear(src, sw, sh, sx, sy), background));
            }
        }
    }
}

fn render_scale(src: &[[f32; 4]], sw: u32, sh: u32, dw: u32, dh: u32) -> Result<Vec<[f32; 4]>> {
    let mut out = Vec::new();
    let count = (dw as usize).checked_mul(dh as usize).ok_or_else(|| EngineError::Other("scaled image is too large".into()))?;
    out.try_reserve(count).map_err(|_| EngineError::Other("not enough memory to scale the image".into()))?;
    for y in 0..dh {
        for x in 0..dw {
            let sx = (f64::from(x) + 0.5) * f64::from(sw) / f64::from(dw) - 0.5;
            let sy = (f64::from(y) + 0.5) * f64::from(sh) / f64::from(dh) - 0.5;
            out.push(bilinear(src, sw, sh, sx, sy));
        }
    }
    Ok(out)
}

fn alpha_bounds(px: &[[f32; 4]], w: u32, _h: u32) -> Option<(i32, i32, i32, i32)> {
    if w == 0 {
        return None;
    }
    let mut bounds: Option<(i32, i32, i32, i32)> = None;
    let threshold = 0.5 / 255.0;
    for (index, pixel) in px.iter().enumerate() {
        if pixel[3] < threshold {
            continue;
        }
        let x = (index % w as usize) as i32;
        let y = (index / w as usize) as i32;
        bounds = Some(match bounds {
            None => (x, y, x + 1, y + 1),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
        });
    }
    bounds
}

fn square_side(width: u32, height: u32, padding: f64) -> Result<u32> {
    let denom = 1.0 - 2.0 * padding;
    if denom <= 0.0 {
        return Err(bad("image.squarePad", "padding must be a fraction from 0 to 0.45"));
    }
    let side = f64::from(width.max(height)) / denom;
    let side = side.round();
    if !(1.0..=30_000.0).contains(&side) {
        return Err(bad("image.squarePad", "the square does not fit in 30000 px"));
    }
    Ok(side as u32)
}

fn render_square(src: &[[f32; 4]], sw: u32, sh: u32, padding: f64, background: [f32; 3]) -> Result<(Vec<[f32; 4]>, u32)> {
    let (x0, y0, x1, y1) = alpha_bounds(src, sw, sh).ok_or_else(|| bad("image.squarePad", "nothing to place; the picture is empty"))?;
    let content_w = x1.abs_diff(x0);
    let content_h = y1.abs_diff(y0);
    let side = square_side(content_w, content_h, padding)?;
    check_output("image.squarePad", side, side)?;
    let ox = (i64::from(side) - i64::from(content_w)) / 2;
    let oy = (i64::from(side) - i64::from(content_h)) / 2;
    let mut out = Vec::new();
    let count = (side as usize).checked_mul(side as usize).ok_or_else(|| EngineError::Other("square is too large".into()))?;
    out.try_reserve(count).map_err(|_| EngineError::Other("not enough memory for the square".into()))?;
    for y in 0..side {
        for x in 0..side {
            let sx = i64::from(x0) + i64::from(x) - ox;
            let sy = i64::from(y0) + i64::from(y) - oy;
            let sample = at_i64(src, sw, sh, sx, sy);
            out.push(over(sample, background));
        }
    }
    Ok((out, side))
}

fn at_i64(px: &[[f32; 4]], w: u32, h: u32, x: i64, y: i64) -> [f32; 4] {
    let (Some(x), Some(y)) = (i32::try_from(x).ok(), i32::try_from(y).ok()) else { return [0.0; 4] };
    at(px, w, h, x, y)
}

fn fit_frame(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "image.fitFrame";
    reject_unknown(CMD, p, &["width", "height", "mode", "background", "resolution"])?;
    let width = required_px(CMD, p, "width")?;
    let height = required_px(CMD, p, "height")?;
    let mode = match p.get("mode").and_then(Value::as_str).unwrap_or("cover") {
        "cover" => FrameMode::Cover,
        "contain" => FrameMode::Contain,
        other => return Err(bad(CMD, format!("mode must be cover or contain, not {other}"))),
    };
    if p.get("mode").is_some_and(|value| !value.is_string()) {
        return Err(bad(CMD, "mode must be cover or contain"));
    }
    let background = color_param(CMD, p, "background", [1.0, 1.0, 1.0])?;
    let dpi = optional_dpi(CMD, p)?;
    check_output(CMD, width, height)?;
    let (src, sw, sh) = source(s, CMD)?;
    let out = render_frame(&src, sw, sh, width, height, mode, background)?;
    commit_rgb(s, "Fit Frame", &out, width, height, true, dpi)?;
    let mut result = json!({"width": width, "height": height, "mode": mode_name(mode), "background": hex_of(background), "changed": true});
    if let Some(dpi) = dpi {
        result["resolution"] = json!(dpi);
    }
    Ok(result)
}

fn source(s: &Session, cmd: &str) -> Result<(Vec<[f32; 4]>, u32, u32)> {
    let doc = &s.active().ok_or(EngineError::NoDocument)?.doc;
    let (w, h) = (doc.size.width, doc.size.height);
    Ok((flattened(doc, cmd)?, w, h))
}

fn scaled_pair(sw: u32, sh: u32, pixels: u32, dont_enlarge: bool) -> Result<(u32, u32)> {
    let long = sw.max(sh).max(1);
    let mut scale = f64::from(pixels) / f64::from(long);
    if dont_enlarge && scale > 1.0 {
        scale = 1.0;
    }
    let dim = |value: u32| {
        let scaled = (f64::from(value) * scale).round();
        if (1.0..=30_000.0).contains(&scaled) { Some(scaled as u32) } else { None }
    };
    let width = dim(sw).ok_or_else(|| bad("image.fitLongEdge", "the scaled width is outside 1 to 30000"))?;
    let height = dim(sh).ok_or_else(|| bad("image.fitLongEdge", "the scaled height is outside 1 to 30000"))?;
    Ok((width, height))
}

fn fit_long_edge(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "image.fitLongEdge";
    reject_unknown(CMD, p, &["pixels", "dontEnlarge"])?;
    let pixels = required_px(CMD, p, "pixels")?;
    let dont_enlarge = optional_bool(CMD, p, "dontEnlarge", false)?;
    let (sw, sh) = {
        let doc = &s.active().ok_or(EngineError::NoDocument)?.doc;
        (doc.size.width, doc.size.height)
    };
    let (width, height) = scaled_pair(sw, sh, pixels, dont_enlarge)?;
    if (width, height) == (sw, sh) {
        return Ok(json!({"width": width, "height": height, "pixels": pixels, "changed": false}));
    }
    check_output(CMD, width, height)?;
    let (src, sw, sh) = source(s, CMD)?;
    let out = render_scale(&src, sw, sh, width, height)?;
    commit_rgb(s, "Fit Long Edge", &out, width, height, false, None)?;
    Ok(json!({"width": width, "height": height, "pixels": pixels, "changed": true}))
}

fn square_pad(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "image.squarePad";
    reject_unknown(CMD, p, &["padding", "background"])?;
    let padding = match p.get("padding") {
        None | Some(Value::Null) => 0.08,
        Some(value) => value
            .as_f64()
            .filter(|padding| padding.is_finite() && (0.0..=0.45).contains(padding))
            .ok_or_else(|| bad(CMD, "padding must be a fraction from 0 to 0.45"))?,
    };
    let background = color_param(CMD, p, "background", [1.0, 1.0, 1.0])?;
    let (src, sw, sh) = source(s, CMD)?;
    let (out, side) = render_square(&src, sw, sh, padding, background)?;
    commit_rgb(s, "Square White Pad", &out, side, side, true, None)?;
    Ok(json!({"width": side, "height": side, "padding": padding, "background": hex_of(background), "changed": true}))
}

fn matte(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "image.matte";
    reject_unknown(CMD, p, &["color"])?;
    let background = color_param(CMD, p, "color", [1.0, 1.0, 1.0])?;
    let (src, w, h) = source(s, CMD)?;
    check_output(CMD, w, h)?;
    let mut out = Vec::new();
    out.try_reserve(src.len()).map_err(|_| EngineError::Other("not enough memory to flatten transparency".into()))?;
    out.extend(src.iter().copied().map(|pixel| over(pixel, background)));
    commit_rgb(s, "Matte", &out, w, h, true, None)?;
    Ok(json!({"width": w, "height": h, "color": hex_of(background), "changed": true}))
}

struct Mark {
    text: Option<String>,
    opacity: f32,
    anchor: (f64, f64),
    margin: i32,
    size: f64,
    color: String,
    scale: f64,
    logo: Option<(Vec<[f32; 4]>, u32, u32)>,
}

fn anchor_of(name: &str) -> Option<(f64, f64)> {
    Some(match name {
        "topLeft" => (0.0, 0.0),
        "top" => (0.5, 0.0),
        "topRight" => (1.0, 0.0),
        "left" => (0.0, 0.5),
        "center" => (0.5, 0.5),
        "right" => (1.0, 0.5),
        "bottomLeft" => (0.0, 1.0),
        "bottom" => (0.5, 1.0),
        "bottomRight" => (1.0, 1.0),
        _ => return None,
    })
}

fn watermark_fields(p: &Value) -> Result<Mark> {
    const CMD: &str = "layer.watermark";
    reject_unknown(CMD, p, &["text", "logo", "opacity", "position", "margin", "size", "color", "scale"])?;
    let text = match p.get("text") {
        None => Some("PhotoCraft".to_string()),
        Some(Value::Null) => None,
        Some(Value::String(text)) => {
            if text.chars().count() > 200 || text.contains('\0') {
                return Err(bad(CMD, "text must be at most 200 characters"));
            }
            let trimmed = text.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        }
        Some(_) => return Err(bad(CMD, "text must be a string")),
    };
    let logo = match p.get("logo") {
        None | Some(Value::Null) => None,
        Some(Value::String(path)) => {
            if path.is_empty() || path.contains('\0') || path.len() > 4096 {
                return Err(bad(CMD, "logo path is empty or too long"));
            }
            Some(path.clone())
        }
        Some(_) => return Err(bad(CMD, "logo must be a file path")),
    };
    if text.is_none() && logo.is_none() {
        return Err(bad(CMD, "text or logo is required"));
    }
    let opacity = match p.get("opacity") {
        None | Some(Value::Null) => 40.0,
        Some(value) => value
            .as_f64()
            .filter(|value| value.is_finite() && (0.0..=100.0).contains(value))
            .ok_or_else(|| bad(CMD, "opacity must be a number from 0 to 100"))?,
    };
    let position = match p.get("position") {
        None | Some(Value::Null) => "bottomRight",
        Some(Value::String(position)) => position.as_str(),
        Some(_) => return Err(bad(CMD, "position must be an anchor such as bottomRight")),
    };
    let anchor =
        anchor_of(position).ok_or_else(|| bad(CMD, "position must be topLeft, top, topRight, left, center, right, bottomLeft, bottom, or bottomRight"))?;
    let margin = match p.get("margin") {
        None | Some(Value::Null) => 24.0,
        Some(value) => value
            .as_f64()
            .filter(|value| value.is_finite() && (0.0..=10_000.0).contains(value))
            .ok_or_else(|| bad(CMD, "margin must be a number of pixels from 0 to 10000"))?,
    };
    let size = match p.get("size") {
        None | Some(Value::Null) => 36.0,
        Some(value) => value
            .as_f64()
            .filter(|value| value.is_finite() && (1.0..=1296.0).contains(value))
            .ok_or_else(|| bad(CMD, "size must be a type size in points from 1 to 1296"))?,
    };
    let color = match p.get("color") {
        None | Some(Value::Null) => "#FFFFFF".to_string(),
        Some(Value::String(color)) => {
            parse_color(CMD, "color", color)?;
            if color.starts_with('#') { color.clone() } else { hex_of(parse_color(CMD, "color", color)?) }
        }
        Some(_) => return Err(bad(CMD, "color must be white, black, blue, red, or #rrggbb")),
    };
    let scale = match p.get("scale") {
        None | Some(Value::Null) => 18.0,
        Some(value) => value
            .as_f64()
            .filter(|value| value.is_finite() && (1.0..=100.0).contains(value))
            .ok_or_else(|| bad(CMD, "scale must be a percent of the long edge from 1 to 100"))?,
    };
    let logo = logo.map(|path| load_logo(&path, CMD)).transpose()?;
    Ok(Mark { text, opacity: (opacity / 100.0) as f32, anchor, margin: margin.round() as i32, size, color, scale, logo })
}

fn load_logo(path: &str, cmd: &str) -> Result<(Vec<[f32; 4]>, u32, u32)> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        Err(bad(cmd, "logo files aren't available in the browser version of PhotoCraft; use text, or the desktop app"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    load_logo_file(path, cmd)
}

#[cfg(not(target_arch = "wasm32"))]
fn load_logo_file(path: &str, cmd: &str) -> Result<(Vec<[f32; 4]>, u32, u32)> {
    let meta = std::fs::metadata(path).map_err(|error| bad(cmd, format!("could not open logo: {error}")))?;
    if !meta.is_file() {
        return Err(bad(cmd, "logo must be a file"));
    }
    if meta.len() == 0 || meta.len() > 32 * 1024 * 1024 {
        return Err(bad(cmd, "logo file must be between 1 byte and 32 MB"));
    }
    let bytes = std::fs::read(path).map_err(|error| bad(cmd, format!("could not read logo: {error}")))?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(bad(cmd, "logo file must be at most 32 MB"));
    }
    let imported = photocraft_io::import(path, &bytes).map_err(|error| bad(cmd, format!("could not open logo: {error}")))?;
    let (w, h) = (imported.document.size.width, imported.document.size.height);
    if w == 0 || h == 0 || w > 8_000 || h > 8_000 || u64::from(w).saturating_mul(u64::from(h)) > 16_000_000 {
        return Err(bad(cmd, "logo must be at most 8000 px on a side and 16 megapixels"));
    }
    let pixels = flattened(&imported.document, cmd)?;
    Ok((pixels, w, h))
}

fn place_origin(canvas_w: u32, canvas_h: u32, item_w: u32, item_h: u32, anchor: (f64, f64), margin: i32) -> (i32, i32) {
    let (ax, ay) = anchor;
    let slack_x = i64::from(canvas_w) - i64::from(item_w);
    let slack_y = i64::from(canvas_h) - i64::from(item_h);
    let mut x = (slack_x as f64 * ax).round() as i64;
    let mut y = (slack_y as f64 * ay).round() as i64;
    if ax == 0.0 {
        x += i64::from(margin);
    }
    if ax == 1.0 {
        x -= i64::from(margin);
    }
    if ay == 0.0 {
        y += i64::from(margin);
    }
    if ay == 1.0 {
        y -= i64::from(margin);
    }
    let clamp = |value: i64| value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    (clamp(x), clamp(y))
}

fn scaled_logo(pixels: Vec<[f32; 4]>, sw: u32, sh: u32, canvas_w: u32, canvas_h: u32, scale: f64) -> Result<(Vec<[f32; 4]>, u32, u32)> {
    let long = f64::from(sw.max(sh).max(1));
    let target = (f64::from(canvas_w.max(canvas_h).max(1)) * scale / 100.0).round().clamp(1.0, 4_000.0);
    let factor = target / long;
    let dw = (f64::from(sw) * factor).round().clamp(1.0, 4_000.0) as u32;
    let dh = (f64::from(sh) * factor).round().clamp(1.0, 4_000.0) as u32;
    Ok((render_scale(&pixels, sw, sh, dw, dh)?, dw, dh))
}

fn watermark(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "layer.watermark";
    let mark = watermark_fields(p)?;
    let (canvas_w, canvas_h) = {
        let doc = &s.active().ok_or(EngineError::NoDocument)?.doc;
        (doc.size.width, doc.size.height)
    };
    let had_logo = mark.logo.is_some();
    let had_text = mark.text.is_some();
    let logo = mark.logo.map(|(pixels, w, h)| scaled_logo(pixels, w, h, canvas_w, canvas_h, mark.scale)).transpose()?;
    let typed = match &mark.text {
        Some(text) => Some(crate::type_cmds::new_text_layer(s, &json!({"text": text, "size": mark.size, "color": mark.color, "name": "Watermark"}), CMD)?),
        None => None,
    };
    let opacity = mark.opacity;
    let anchor = mark.anchor;
    let margin = mark.margin;
    let ids = s.edit("Watermark", move |doc, active| {
        let mut typed = typed;
        let mut ids = Vec::new();
        if let Some((pixels, dw, dh)) = logo {
            ids.push(place_logo(doc, active, &pixels, (dw, dh), Corner { anchor, margin, opacity })?);
        }
        if let Some(text) = typed.as_mut() {
            ids.push(place_text(doc, active, text, anchor, margin, opacity)?);
        }
        Ok(ids)
    })?;
    Ok(json!({"layers": ids, "text": had_text, "logo": had_logo}))
}

struct Corner {
    anchor: (f64, f64),
    margin: i32,
    opacity: f32,
}

fn place_logo(doc: &mut Document, active: &mut Option<LayerId>, pixels: &[[f32; 4]], size: (u32, u32), corner: Corner) -> Result<u64> {
    let (dw, dh) = size;
    let Corner { anchor, margin, opacity } = corner;
    let (x, y) = place_origin(doc.size.width, doc.size.height, dw, dh, anchor, margin);
    let rect = Rect::from_xywh(x, y, dw, dh);
    if rect.width() != dw || rect.height() != dh {
        return Err(bad("layer.watermark", "the logo does not fit on the canvas"));
    }
    let fmt = doc.pixel_format();
    let samples = pack(fmt, pixels)?;
    let expected = (rect.width() as usize).saturating_mul(rect.height() as usize).saturating_mul(fmt.channels());
    if samples.len() != expected {
        return Err(EngineError::Other("internal logo buffer has the wrong size".into()));
    }
    let mut surf = Surface::new(fmt);
    surf.write_region(rect, &samples);
    let mut layer = Layer::raster("Logo", fmt);
    layer.opacity = opacity;
    *crate::pixels_mut(&mut layer)? = surf;
    // Above every existing layer, including ones stacked above the active layer.
    let id = crate::layer_place::insert_new(doc, None, layer, crate::layer_place::Place::Above);
    *active = Some(id);
    Ok(id.0)
}

fn place_text(
    doc: &mut Document,
    active: &mut Option<LayerId>,
    text: &mut photocraft_doc::TextLayer,
    anchor: (f64, f64),
    margin: i32,
    opacity: f32,
) -> Result<u64> {
    crate::type_cmds::refresh(doc, text);
    let bounds = text.cache.as_ref().map(|cache| cache.content_bounds()).filter(|rect| rect.width() > 0 && rect.height() > 0);
    let (item_w, item_h, origin_x, origin_y) = if let Some(bounds) = bounds {
        (bounds.width(), bounds.height(), bounds.x0, bounds.y0)
    } else {
        let px = (f64::from(text.size_pt) * f64::from(doc.resolution_dpi) / 72.0).max(1.0);
        let width = (text.text.chars().count() as f64 * px * 0.5).clamp(1.0, 100_000.0) as u32;
        (width, px.clamp(1.0, 10_000.0) as u32, 0, 0)
    };
    let (x, y) = place_origin(doc.size.width, doc.size.height, item_w, item_h, anchor, margin);
    text.transform = Affine::translate(f64::from(x) - f64::from(origin_x), f64::from(y) - f64::from(origin_y));
    let owned = text.clone();
    let mut layer = Layer::new("Watermark", LayerContent::Text(owned));
    layer.opacity = opacity;
    // After the logo, so the text is the topmost layer.
    let id = crate::layer_place::insert_new(doc, None, layer, crate::layer_place::Place::Above);
    *active = Some(id);
    Ok(id.0)
}

fn query<'a>(p: &'a Value, cmd: &str) -> Result<&'a str> {
    match (p.get("id"), p.get("name")) {
        (Some(Value::String(id)), None) | (None, Some(Value::String(id))) if !id.trim().is_empty() => Ok(id),
        (Some(Value::String(id)), None) | (None, Some(Value::String(id))) if id.trim().is_empty() => Err(bad(cmd, "preset name is empty")),
        (Some(Value::String(_)), Some(_)) => Err(bad(cmd, "pass id or name, not both")),
        (Some(_), _) => Err(bad(cmd, "id must be a string")),
        (_, Some(_)) => Err(bad(cmd, "name must be a string")),
        (None, None) => Err(bad(cmd, "pass id or name")),
    }
}

fn list(_s: &mut Session, p: &Value) -> Result<Value> {
    reject_unknown("presets.list", p, &[])?;
    Ok(json!({
        "presets": all().iter().copied().map(Preset::to_json).collect::<Vec<_>>(),
        "omitted": omitted().iter().copied().map(|item| item.to_json()).collect::<Vec<_>>(),
    }))
}

fn get(_s: &mut Session, p: &Value) -> Result<Value> {
    reject_unknown("presets.get", p, &["id", "name"])?;
    let preset = find(query(p, "presets.get")?)?;
    let mut value = preset.to_json();
    value["action"] = serde_json::to_value(preset.action()).map_err(|error| EngineError::Other(error.to_string()))?;
    Ok(value)
}

fn load(s: &mut Session, p: &Value) -> Result<Value> {
    reject_unknown("presets.load", p, &[])?;
    crate::actions_cmds::not_playing(s, "presets.load")?;
    if s.actions.recording.is_some() {
        return Err(bad("presets.load", "stop recording before loading presets"));
    }
    let mut loaded = Vec::new();
    let mut skipped = Vec::new();
    for preset in all() {
        let name = preset.display_name();
        if s.actions.list.iter().any(|action| action.name == name) {
            skipped.push(name);
            continue;
        }
        s.actions.list.push(preset.action());
        loaded.push(name);
    }
    if !loaded.is_empty() {
        s.actions.touch();
    }
    Ok(json!({"loaded": loaded, "skipped": skipped}))
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    reject_unknown("presets.apply", p, &["id", "name", "params"])?;
    let preset = find(query(p, "presets.apply")?)?;
    if s.active().is_none() {
        return Err(EngineError::NoDocument);
    }
    let params = merged_params(preset, p.get("params"))?;
    let result = s.execute(preset.command(), params)?;
    Ok(json!({
        "id": preset.id,
        "name": preset.name,
        "nameZh": preset.name_zh,
        "result": result,
        "export": {"format": preset.format, "quality": preset.quality, "naming": preset.naming},
    }))
}

fn pass(s: &mut Session, p: &Value) -> Result<Value> {
    reject_unknown("presets.pass", p, &[])?;
    if s.active().is_none() {
        return Err(EngineError::NoDocument);
    }
    Ok(json!({"changed": false}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "presets.list",
            label: "List Built-in Presets",
            menu: &[],
            shortcut: None,
            params: r##"{} → {presets:[{id,name,nameZh,set,setZh,summary,summaryZh,command,params,export:{format,quality,naming}}], omitted:[{id,name,nameZh,reason,reasonZh}]} (omitted entries are not actions; face-centered crop and background removal are omitted)"##,
            enabled: always,
            run: list,
            journal: false,
        },
        CommandSpec {
            id: "presets.get",
            label: "Get Built-in Preset",
            menu: &[],
            shortcut: None,
            params: r##"{"id":str|"name":str} → the presets.list row plus action:{name,set,steps} in the Actions file shape"##,
            enabled: always,
            run: get,
            journal: false,
        },
        CommandSpec {
            id: "presets.load",
            label: "Load Built-in Presets",
            menu: &[],
            shortcut: None,
            params: r##"{} → {loaded:[name…], skipped:[name…]} (adds missing presets to the Actions panel; an existing name is left as it is)"##,
            enabled: always,
            run: load,
            journal: false,
        },
        CommandSpec {
            id: "presets.apply",
            label: "Apply Built-in Preset",
            menu: &[],
            shortcut: None,
            params: r##"{"id":str|"name":str, "params":object?} → {id,name,nameZh,result,export:{format,quality,naming}} (params replace the preset's step values; export is what batch should write, and is not itself a save)"##,
            enabled: always,
            run: apply,
            journal: false,
        },
        CommandSpec {
            id: "presets.pass",
            label: "Preset Pass",
            menu: &[],
            shortcut: None,
            params: r##"{} → {changed:false} (a preset step that does not change pixels, used by rename and format-only presets)"##,
            enabled: has_doc,
            run: pass,
            journal: false,
        },
        CommandSpec {
            id: "image.fitFrame",
            label: "Fit Frame",
            menu: &[],
            shortcut: None,
            params: r##"{"width":1..30000,"height":1..30000,"mode":"cover|contain"="cover","background":"white|black|blue|red|#rrggbb"="#FFFFFF","resolution":1..2400?} → {width,height,mode,background,resolution?,changed} (cover center-crops to fill the frame; contain fits the whole picture and fills the margin; transparency is flattened onto background; one untagged RGB layer)"##,
            enabled: has_doc,
            run: fit_frame,
            journal: true,
        },
        CommandSpec {
            id: "image.squarePad",
            label: "Square Pad",
            menu: &[],
            shortcut: None,
            params: r##"{"padding":0..0.45=0.08,"background":"white|black|blue|red|#rrggbb"="#FFFFFF"} → {width,height,padding,background,changed} (transparent margins are trimmed, then the picture is centered on a square; padding is the empty fraction of the square on each side; an opaque picture keeps its background)"##,
            enabled: has_doc,
            run: square_pad,
            journal: true,
        },
        CommandSpec {
            id: "image.fitLongEdge",
            label: "Fit Long Edge",
            menu: &[],
            shortcut: None,
            params: r##"{"pixels":1..30000,"dontEnlarge":bool=false} → {width,height,pixels,changed} (scales so the longer side is pixels; keeps transparency; one RGB layer; unchanged size adds no history step)"##,
            enabled: has_doc,
            run: fit_long_edge,
            journal: true,
        },
        CommandSpec {
            id: "image.matte",
            label: "Matte",
            menu: &[],
            shortcut: None,
            params: r##"{"color":"white|black|blue|red|#rrggbb"="#FFFFFF"} → {width,height,color,changed} (flattens the composite onto the color; one opaque RGB layer)"##,
            enabled: has_doc,
            run: matte,
            journal: true,
        },
        CommandSpec {
            id: "layer.watermark",
            label: "Watermark",
            menu: &[],
            shortcut: None,
            params: r##"{"text":str="PhotoCraft","logo":path?,"opacity":0..100=40,"position":"topLeft|top|topRight|left|center|right|bottomLeft|bottom|bottomRight"="bottomRight","margin":0..10000=24,"size":1..1296=36,"color":"#rrggbb"="white","scale":1..100=18} → {layers,text,logo} (text, a logo image, or both; scale is the logo's long edge as a percent of the canvas long edge; a logo file is not available in the browser)"##,
            enabled: has_doc,
            run: watermark,
            journal: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_doc::LayerContent;
    use photocraft_geom::Rect;
    use serde_json::json;

    use crate::{EngineError, Session};

    fn new_doc(w: u32, h: u32, depth: u32, background: &str) -> Session {
        let mut session = Session::new();
        session.execute("file.new", json!({"width": w, "height": h, "depth": depth, "background": background})).unwrap();
        session
    }

    fn paint(session: &mut Session, x: i32, y: i32, w: u32, h: u32, color: [f32; 4]) {
        session
            .edit("paint", |doc, _| {
                let surf = doc.layers[0].surface_mut().ok_or_else(|| EngineError::Other("no pixels".into()))?;
                let px = photocraft_raster::from_rgba(&surf.format(), color);
                let channels = surf.channels();
                if px.len() != channels {
                    return Err(EngineError::Other("pixel size".into()));
                }
                let mut data = Vec::new();
                let count = (w as usize).saturating_mul(h as usize);
                data.try_reserve(count.saturating_mul(channels)).map_err(|_| EngineError::Other("memory".into()))?;
                for _ in 0..count {
                    data.extend_from_slice(&px);
                }
                surf.write_region(Rect::from_xywh(x, y, w, h), &data);
                Ok(())
            })
            .unwrap();
    }

    fn rgba(session: &Session, x: i32, y: i32) -> [f32; 4] {
        let surf = session.active().unwrap().doc.layers[0].surface().unwrap();
        let mut px = [[0.0; 4]; 1];
        surf.read_rgba_into(Rect::new(x, y, x + 1, y + 1), &mut px);
        px[0]
    }

    fn near(pixel: [f32; 4], expected: [f32; 4]) {
        for (got, want) in pixel.iter().zip(expected) {
            assert!((got - want).abs() < 0.02, "{pixel:?} vs {expected:?}");
        }
    }

    #[test]
    fn library_uses_no_model_and_names_both_languages() {
        let ids: std::collections::HashSet<&str> = crate::command_specs().iter().map(|spec| spec.id).collect();
        assert!(ids.contains("image.fitFrame") && ids.contains("presets.load"));
        let mut names = std::collections::HashSet::new();
        for preset in all() {
            assert!(names.insert(preset.display_name()), "duplicate {}", preset.display_name());
            assert!(ids.contains(preset.command()), "{}", preset.command());
            assert!(!preset.command().contains("detectFaces"));
            assert_ne!(preset.command(), "layer.removeBackground");
            let text = preset.params().to_string();
            assert!(!text.contains("detectFaces") && !text.contains("removeBackground"), "{text}");
            assert!(!preset.name_zh.is_ascii(), "{}", preset.id);
        }
        assert!(omitted().iter().any(|item| item.id == "id.face" && item.reason.contains("face")));
        assert!(omitted().iter().any(|item| item.id == "product.cutout" && item.reason.contains("subject")));
        assert_eq!(mm_to_px(25.0, 300.0), 295);
        assert_eq!(mm_to_px(35.0, 300.0), 413);
        assert_eq!(mm_to_px(49.0, 300.0), 579);
        let inch = find("id.1inch.white").unwrap();
        assert_eq!(inch.params()["width"], 295);
        assert_eq!(inch.params()["height"], 413);
        assert_eq!(find("二寸红底留边").unwrap().id, "id.2inch.red");
    }

    #[test]
    fn actions_round_trip_and_omitted_ids_are_refused() {
        let actions = actions();
        let value = serde_json::to_value(&actions).unwrap();
        let back: Vec<crate::actions_cmds::Action> = serde_json::from_value(value).unwrap();
        assert_eq!(back, actions);
        let mut session = Session::new();
        let missing = session.execute("presets.apply", json!({"id": "id.face"})).unwrap_err().to_string();
        assert!(missing.contains("face"), "{missing}");
        let cutout = session.execute("presets.apply", json!({"name": "去除背景"})).unwrap_err().to_string();
        assert!(cutout.contains("subject"), "{cutout}");
        let listed = session.execute("presets.list", json!({})).unwrap();
        assert!(listed["presets"].as_array().unwrap().iter().any(|row| row["id"] == "social.xiaohongshu"));
        assert!(listed["omitted"].as_array().unwrap().iter().any(|row| row["id"] == "id.face"));
        assert!(session.execute("presets.list", json!({"extra": true})).is_err());
    }

    #[test]
    fn square_pad_centers_at_every_depth_and_undoes() {
        for depth in [8, 16, 32] {
            let mut session = new_doc(40, 20, depth, "#ff0000");
            let before = session.active().unwrap().history.past_len();
            let result = session.execute("image.squarePad", json!({"padding": 0.0, "background": "#FFFFFF"})).unwrap();
            assert_eq!((result["width"].as_u64(), result["height"].as_u64()), (Some(40), Some(40)), "depth {depth}");
            near(rgba(&session, 0, 0), [1.0, 1.0, 1.0, 1.0]);
            near(rgba(&session, 0, 15), [1.0, 0.0, 0.0, 1.0]);
            assert_eq!(session.active().unwrap().history.past_len(), before + 1);
            session.execute("edit.undo", json!({})).unwrap();
            assert_eq!((session.active().unwrap().doc.size.width, session.active().unwrap().doc.size.height), (40, 20));
        }
        let mut session = new_doc(8, 8, 8, "transparent");
        paint(&mut session, 1, 2, 2, 2, [0.0, 0.0, 1.0, 1.0]);
        let result = session.execute("image.squarePad", json!({"padding": 0.0})).unwrap();
        assert_eq!(result["width"], 2);
        near(rgba(&session, 0, 0), [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn frame_long_edge_and_matte_resize_without_a_model() {
        for depth in [8, 16, 32] {
            let mut session = new_doc(40, 20, depth, "#ff0000");
            let fitted = session.execute("image.fitFrame", json!({"width": 10, "height": 10, "mode": "cover"})).unwrap();
            assert_eq!((fitted["width"].as_u64(), fitted["height"].as_u64()), (Some(10), Some(10)));
            near(rgba(&session, 5, 5), [1.0, 0.0, 0.0, 1.0]);
            let mut session = new_doc(40, 20, depth, "#ff0000");
            let contained =
                session.execute("image.fitFrame", json!({"width": 20, "height": 28, "mode": "contain", "background": "blue", "resolution": 300})).unwrap();
            assert_eq!(contained["resolution"].as_f64(), Some(300.0));
            assert!((session.active().unwrap().doc.resolution_dpi - 300.0).abs() < 0.01);
            near(rgba(&session, 0, 0), [0x43 as f32 / 255.0, 0x8E as f32 / 255.0, 0xDB as f32 / 255.0, 1.0]);
            near(rgba(&session, 10, 14), [1.0, 0.0, 0.0, 1.0]);
            let mut session = new_doc(40, 20, depth, "#ff0000");
            let scaled = session.execute("image.fitLongEdge", json!({"pixels": 10})).unwrap();
            assert_eq!((scaled["width"].as_u64(), scaled["height"].as_u64()), (Some(10), Some(5)));
            near(rgba(&session, 5, 2), [1.0, 0.0, 0.0, 1.0]);
            let mut session = new_doc(8, 4, depth, "#ff0000");
            session.execute("image.fitFrame", json!({"width": 16, "height": 8, "mode": "cover"})).unwrap();
            near(rgba(&session, 0, 0), [1.0, 0.0, 0.0, 1.0]);
            near(rgba(&session, 15, 7), [1.0, 0.0, 0.0, 1.0]);
            let steps = session.active().unwrap().history.past_len();
            let same = session.execute("image.fitLongEdge", json!({"pixels": 100, "dontEnlarge": true})).unwrap();
            assert_eq!(same["changed"], false);
            assert_eq!(session.active().unwrap().history.past_len(), steps);
        }
        let mut session = new_doc(8, 8, 8, "transparent");
        let before = session.active().unwrap().doc.size;
        assert!(session.execute("image.squarePad", json!({})).is_err());
        assert_eq!((session.active().unwrap().doc.size.width, session.active().unwrap().doc.size.height), (before.width, before.height));
        paint(&mut session, 3, 3, 2, 2, [1.0, 0.0, 0.0, 1.0]);
        session.execute("image.matte", json!({"color": "#FFFFFF"})).unwrap();
        near(rgba(&session, 0, 0), [1.0, 1.0, 1.0, 1.0]);
        near(rgba(&session, 3, 3), [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn bad_params_fail_and_presets_apply_and_play() {
        let mut session = Session::new();
        assert!(session.execute("image.fitFrame", json!({"width": 2, "height": 2})).is_err());
        let mut session = new_doc(16, 8, 8, "white");
        for (id, params) in [
            ("image.fitFrame", json!({"width": 0, "height": 10})),
            ("image.fitFrame", json!({"width": 10, "height": 10, "mode": "stretch"})),
            ("image.fitFrame", json!("nope")),
            ("image.squarePad", json!({"padding": 0.9})),
            ("image.fitLongEdge", json!({"pixels": -5})),
            ("image.matte", json!({"color": "purple"})),
            ("layer.watermark", json!({"text": "", "logo": ""})),
            ("presets.apply", json!({"id": "missing"})),
            ("presets.apply", json!({})),
        ] {
            assert!(session.execute(id, params.clone()).is_err(), "{id} {params}");
        }
        let applied = session.execute("presets.apply", json!({"id": "product.square-white", "params": {"padding": 0}})).unwrap();
        assert_eq!(applied["export"]["quality"], 92);
        assert_eq!(applied["result"]["width"], 16);
        session.execute("presets.load", json!({})).unwrap();
        let again = session.execute("presets.load", json!({})).unwrap();
        assert!(again["loaded"].as_array().unwrap().is_empty());
        assert!(!again["skipped"].as_array().unwrap().is_empty());
        session.execute("file.new", json!({"width": 30, "height": 10, "background": "#00ff00"})).unwrap();
        let played = session.execute("actions.play", json!({"action": "Long edge 1600 px · 长边 1600 像素"})).unwrap();
        assert!(played["failed"].is_null(), "{played}");
        assert_eq!(session.active().unwrap().doc.size.width, 1600);
    }

    #[test]
    fn watermark_text_is_one_history_step() {
        for depth in [8, 16] {
            let mut session = new_doc(80, 40, depth, "white");
            let before = session.active().unwrap().history.past_len();
            let result = session.execute("layer.watermark", json!({"text": "Hi", "opacity": 40, "position": "bottomRight"})).unwrap();
            assert_eq!(result["text"], true);
            assert_eq!(session.active().unwrap().history.past_len(), before + 1, "depth {depth}");
            let layer = session.active().unwrap().doc.layers.iter().find(|layer| layer.name == "Watermark").unwrap();
            assert!((layer.opacity - 0.4).abs() < 1e-5);
            match &layer.content {
                LayerContent::Text(text) => assert_eq!(text.text, "Hi"),
                other => panic!("expected text, got {other:?}"),
            }
            session.execute("edit.undo", json!({})).unwrap();
            assert!(session.active().unwrap().doc.layers.iter().all(|layer| layer.name != "Watermark"));
        }
        let mut session = new_doc(40, 20, 8, "white");
        session.execute("layer.new.layer", json!({"name": "Above"})).unwrap();
        session.execute("layer.watermark", json!({"text": "Top"})).unwrap();
        let names: Vec<_> = session.active().unwrap().doc.layers.iter().map(|layer| layer.name.as_str()).collect();
        assert_eq!(names.last().copied(), Some("Watermark"));
        let above = names.iter().position(|name| *name == "Above").unwrap();
        let mark = names.iter().position(|name| *name == "Watermark").unwrap();
        assert!(above < mark, "{names:?}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn watermark_logo_uses_a_real_png() {
        let dir = std::env::temp_dir().join(format!("photocraft-preset-logo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("logo.png");
        let data = [255u8, 0, 0, 255].repeat(8 * 8);
        let image = photocraft_codecs::Image::from_u8(8, 8, photocraft_codecs::ChannelLayout::Rgba, data).unwrap();
        let bytes = photocraft_codecs::encode(&image, photocraft_codecs::Format::Png, &Default::default()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        let mut session = new_doc(40, 20, 8, "white");
        let missing = session.execute("layer.watermark", json!({"text": "", "logo": dir.join("missing.png").to_string_lossy().to_string()})).unwrap_err();
        assert!(missing.to_string().contains("logo"), "{missing}");
        let result = session
            .execute(
                "layer.watermark",
                json!({"text": "", "logo": path.to_string_lossy().to_string(), "scale": 50, "opacity": 100, "position": "center", "margin": 0}),
            )
            .unwrap();
        assert_eq!(result["logo"], true);
        let layer = session.active().unwrap().doc.layers.iter().find(|layer| layer.name == "Logo").unwrap();
        let bounds = layer.surface().unwrap().content_bounds();
        assert!(bounds.width() > 0 && bounds.height() > 0);
        let mut px = [[0.0; 4]; 1];
        let x = bounds.x0 + bounds.width() as i32 / 2;
        let y = bounds.y0 + bounds.height() as i32 / 2;
        layer.surface().unwrap().read_rgba_into(Rect::new(x, y, x + 1, y + 1), &mut px);
        near(px[0], [1.0, 0.0, 0.0, 1.0]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
