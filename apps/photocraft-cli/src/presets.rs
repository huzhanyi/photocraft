//! Built-in export presets for `presets`, `run --preset` and `batch --preset`.

use std::io::Write;

use photocraft_engine::preset_library_cmds::{Preset, all, find, omitted};
use photocraft_io::ExportOptions;
use serde_json::{Value, json};

use crate::{Result, args::PresetsArgs};

pub(crate) fn list(args: &PresetsArgs, out: &mut dyn Write) -> Result {
    if let Some(id) = &args.id {
        let preset = find(id)?;
        if args.json {
            writeln_json(out, &preset.to_json())?;
        } else {
            write_one(out, *preset)?;
        }
        return Ok(());
    }
    if args.json {
        let body = json!({
            "presets": all().iter().copied().map(Preset::to_json).collect::<Vec<_>>(),
            "omitted": omitted().iter().copied().map(|item| item.to_json()).collect::<Vec<_>>(),
        });
        writeln_json(out, &body)?;
        return Ok(());
    }
    let _ = writeln!(out, "Built-in presets. A preset export does not replace a file that is already there; pass --overwrite to replace it.");
    let _ = writeln!(out);
    for preset in all() {
        write_one(out, *preset)?;
        let _ = writeln!(out);
    }
    let _ = writeln!(out, "Not included:");
    for item in omitted() {
        let _ = writeln!(out, "{}\t{} · {}", item.id, item.name, item.name_zh);
        let _ = writeln!(out, "  {}", item.reason);
        let _ = writeln!(out, "  {}", item.reason_zh);
    }
    Ok(())
}

fn write_one(out: &mut dyn Write, preset: Preset) -> Result {
    let format = preset.format.unwrap_or("same");
    let quality = preset.quality.map(|q| q.to_string()).unwrap_or_else(|| "unchanged".to_string());
    let naming = preset.naming.unwrap_or("(keep the file name)");
    let _ = writeln!(out, "{}\t{} · {}", preset.id, preset.name, preset.name_zh);
    let _ = writeln!(out, "  {}", preset.summary);
    let _ = writeln!(out, "  {}", preset.summary_zh);
    let _ = writeln!(out, "  export {format}, quality {quality}, naming {naming}");
    let _ = writeln!(out, "  photocraft-cli batch --preset {} --in DIR --out DIR", preset.id);
    Ok(())
}

fn writeln_json(out: &mut dyn Write, value: &Value) -> Result {
    writeln!(out, "{value}")?;
    Ok(())
}

pub(crate) fn apply_quality(opts: &mut ExportOptions, quality: Option<u8>) {
    if let Some(quality) = quality {
        opts.encode.jpeg_quality = quality;
        opts.encode.webp_quality = quality;
        opts.encode.webp_lossless = false;
    }
}

pub(crate) fn describe(preset: &Preset, format: &str, quality: Option<u8>, naming: Option<&str>, overwrite: bool) -> String {
    let quality = quality.map(|q| q.to_string()).unwrap_or_else(|| "encoder default".to_string());
    let naming = naming.unwrap_or("(keep the file name)");
    let replace = if overwrite { "replace existing files" } else { "leave existing files in place" };
    format!("preset {} · {} · {}; export {}, quality {}, naming {}; {replace}", preset.id, preset.name, preset.name_zh, format, quality, naming)
}

#[cfg(test)]
mod tests {
    use crate::test_support::invoke;
    use serde_json::Value;

    #[test]
    fn presets_lists_both_languages_and_refuses_model_presets() {
        let listed = invoke(&["presets"]);
        assert_eq!(listed.code, 0, "{}", listed.stderr);
        assert!(listed.stdout.contains("product.square-white"), "{}", listed.stdout);
        assert!(listed.stdout.contains("方形白底"), "{}", listed.stdout);
        assert!(listed.stdout.contains("小红书"), "{}", listed.stdout);
        assert!(listed.stdout.contains("证件照人脸居中") || listed.stdout.contains("id.face"), "{}", listed.stdout);
        assert!(listed.stdout.contains("does not replace"), "{}", listed.stdout);
        let refused = invoke(&["presets", "--id", "id.face"]);
        assert_eq!(refused.code, 1, "{}", refused.stdout);
        assert!(refused.stderr.contains("face"), "{}", refused.stderr);
        let json_list = invoke(&["presets", "--json"]);
        assert_eq!(json_list.code, 0, "{}", json_list.stderr);
        let value: Value = serde_json::from_str(json_list.stdout.trim()).unwrap();
        assert!(value["presets"].as_array().unwrap().iter().any(|row| row["id"] == "batch.rename"));
        assert!(value["omitted"].as_array().unwrap().iter().any(|row| row["id"] == "product.cutout"));
    }

    #[test]
    fn run_preset_writes_a_square_png_and_will_not_replace_it() {
        let dir = std::env::temp_dir().join(format!("photocraft-cli-preset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("square.png");
        let out_s = out.to_string_lossy().to_string();
        let first = invoke(&[
            "run",
            "--new",
            r##"{"width":40,"height":20,"background":"#ff0000"}"##,
            "--preset",
            "product.square-white",
            "--preset-params",
            r#"{"padding":0}"#,
            "--format",
            "png",
            "--out",
            &out_s,
        ]);
        assert_eq!(first.code, 0, "stderr {}\nstdout {}", first.stderr, first.stdout);
        assert!(first.stderr.contains("product.square-white"), "{}", first.stderr);
        assert!(first.stderr.contains("leave existing files"), "{}", first.stderr);
        let image = photocraft_codecs::decode(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(image.dimensions(), (40, 40));
        let second =
            invoke(&["run", "--new", r##"{"width":40,"height":20,"background":"#ff0000"}"##, "--preset", "方形白底", "--format", "png", "--out", &out_s]);
        assert_eq!(second.code, 1, "{}", second.stdout);
        assert!(second.stderr.contains("already exists"), "{}", second.stderr);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch_preset_renames_and_scales_a_real_png() {
        let dir = std::env::temp_dir().join(format!("photocraft-cli-preset-batch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let input = dir.join("in");
        let renamed = dir.join("renamed");
        let scaled = dir.join("scaled");
        std::fs::create_dir_all(&input).unwrap();
        let source = input.join("photo.png");
        let source_s = source.to_string_lossy().to_string();
        let made = invoke(&["run", "--new", r##"{"width":40,"height":20,"background":"#00ff00"}"##, "--out", &source_s]);
        assert_eq!(made.code, 0, "{}", made.stderr);
        let rename = invoke(&["batch", "--preset", "batch.rename", "--in", input.to_str().unwrap(), "--out", renamed.to_str().unwrap(), "--format", "png"]);
        assert_eq!(rename.code, 0, "stderr {}\nstdout {}", rename.stderr, rename.stdout);
        let named = renamed.join("001_photo.png");
        assert!(named.is_file(), "{}", rename.stdout);
        let again = invoke(&["batch", "--preset", "batch.rename", "--in", input.to_str().unwrap(), "--out", renamed.to_str().unwrap(), "--format", "png"]);
        assert_eq!(again.code, 1, "{}", again.stdout);
        assert!(again.stderr.contains("already exists"), "{}", again.stderr);
        let scale = invoke(&[
            "batch",
            "--preset",
            "batch.long-edge",
            "--preset-params",
            r#"{"pixels":10}"#,
            "--in",
            input.to_str().unwrap(),
            "--out",
            scaled.to_str().unwrap(),
            "--format",
            "png",
        ]);
        assert_eq!(scale.code, 0, "stderr {}\nstdout {}", scale.stderr, scale.stdout);
        let image = photocraft_codecs::decode(&std::fs::read(scaled.join("photo.png")).unwrap()).unwrap();
        assert_eq!(image.dimensions(), (10, 5));
        let bytes_before = std::fs::read(&source).unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), bytes_before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_png(path: &std::path::Path, w: u32, h: u32, rgba: [u8; 4]) {
        let data = rgba.to_vec().repeat((w * h) as usize);
        let image = photocraft_codecs::Image::from_u8(w, h, photocraft_codecs::ChannelLayout::Rgba, data).unwrap();
        std::fs::write(path, photocraft_codecs::encode(&image, photocraft_codecs::Format::Png, &Default::default()).unwrap()).unwrap();
    }

    #[test]
    fn preset_exports_keep_dimensions_across_png_jpeg_and_webp_and_refuse_bad_paths() {
        let dir = std::env::temp_dir().join(format!("photocraft-cli-preset-formats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let input = dir.join("in");
        std::fs::create_dir_all(input.join("a")).unwrap();
        std::fs::create_dir_all(input.join("b")).unwrap();
        write_png(&input.join("a").join("photo.png"), 12, 8, [255, 0, 0, 255]);
        write_png(&input.join("b").join("photo.png"), 12, 8, [0, 0, 255, 255]);
        write_png(&input.join("...png"), 4, 4, [0, 255, 0, 255]);
        let source_bytes = std::fs::read(input.join("a").join("photo.png")).unwrap();
        for (format, name) in [("png", "out.png"), ("jpg", "out.jpg"), ("webp", "out.webp")] {
            let out = dir.join(name);
            let run = invoke(&[
                "run",
                input.join("a").join("photo.png").to_str().unwrap(),
                "--preset",
                "social.wechat-moments",
                "--preset-params",
                r#"{"width":6,"height":4}"#,
                "--format",
                format,
                "--out",
                out.to_str().unwrap(),
            ]);
            assert_eq!(run.code, 0, "{format} stderr {} stdout {}", run.stderr, run.stdout);
            let decoded = photocraft_codecs::decode(&std::fs::read(&out).unwrap()).unwrap();
            assert_eq!(decoded.dimensions(), (6, 4), "{format}");
        }
        assert_eq!(std::fs::read(input.join("a").join("photo.png")).unwrap(), source_bytes);
        let collided = dir.join("collided");
        let collision = invoke(&["batch", "--preset", "batch.to-png", "--in", input.to_str().unwrap(), "--out", collided.to_str().unwrap(), "--recursive"]);
        assert_ne!(collision.code, 0, "{}", collision.stdout);
        assert!(collision.stderr.contains("already exists") || collision.stderr.contains("outside"), "{}\n{}", collision.stderr, collision.stdout);
        let escaped = dir.join("escaped");
        std::fs::create_dir_all(&escaped).unwrap();
        let parent_before = std::fs::read_dir(&dir).unwrap().count();
        let escape = invoke(&["batch", "--preset", "batch.to-png", "--naming", "{name}", "--in", input.to_str().unwrap(), "--out", escaped.to_str().unwrap()]);
        assert_ne!(escape.code, 0, "{}", escape.stdout);
        assert!(escape.stderr.contains("`.`") || escape.stderr.contains("`..`") || escape.stderr.contains("naming"), "{}", escape.stderr);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), parent_before);
        let bad = invoke(&[
            "run",
            input.join("a").join("photo.png").to_str().unwrap(),
            "--preset",
            "batch.long-edge",
            "--preset-params",
            "[]",
            "--out",
            dir.join("bad.png").to_str().unwrap(),
        ]);
        assert_ne!(bad.code, 0, "{}", bad.stdout);
        assert!(!dir.join("bad.png").exists());
        let same = input.join("a");
        let in_place = invoke(&["batch", "--preset", "batch.to-png", "--in", same.to_str().unwrap(), "--out", same.to_str().unwrap(), "--in-place"]);
        assert_ne!(in_place.code, 0, "{}", in_place.stdout);
        assert!(in_place.stderr.contains("already exists"), "{}", in_place.stderr);
        assert_eq!(std::fs::read(input.join("a").join("photo.png")).unwrap(), source_bytes);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
