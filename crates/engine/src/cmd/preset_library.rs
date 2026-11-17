//! Built-in export presets, stored as Actions (`name`, `set`, `steps`).
//!
//! The steps call ordinary engine commands: fit, pad, matte, watermark. Face-centered ID crop
//! and background removal are listed under [`omitted`] and are not actions: both depend on a
//! model (the face CNN, and subject selection), and this library does not call them.

use serde_json::{Value, json};

use crate::actions_cmds::Action;
use crate::{EngineError, Result};

/// One ready-to-use preset. `name` is English and `name_zh` is Chinese; the Actions panel shows both.
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub name_zh: &'static str,
    pub set: &'static str,
    pub set_zh: &'static str,
    pub summary: &'static str,
    pub summary_zh: &'static str,
    /// Filename extension the CLI writes, when the caller does not pass one. `None` keeps the source type.
    pub format: Option<&'static str>,
    /// JPEG and WebP quality, 1–100, when the caller does not pass one.
    pub quality: Option<u8>,
    /// Batch naming template. `None` keeps the source file name and changes only the extension.
    pub naming: Option<&'static str>,
    recipe: Recipe,
}

/// A preset that is named so it can be looked up, and refused, instead of pretending to run.
#[derive(Clone, Copy, Debug)]
pub struct Omitted {
    pub id: &'static str,
    pub name: &'static str,
    pub name_zh: &'static str,
    pub reason: &'static str,
    pub reason_zh: &'static str,
}

#[derive(Clone, Copy, Debug)]
enum Recipe {
    /// Cover or contain into an exact pixel frame.
    Frame {
        width: u32,
        height: u32,
        mode: &'static str,
        background: &'static str,
        dpi: Option<u32>,
    },
    /// Trim transparent margins, then center on a square of `background`.
    Square {
        padding: f64,
        background: &'static str,
    },
    LongEdge {
        pixels: u32,
    },
    Watermark,
    Matte {
        color: &'static str,
    },
    Pass,
}

impl Preset {
    pub fn display_name(self) -> String {
        format!("{} · {}", self.name, self.name_zh)
    }

    pub fn set_name(self) -> String {
        format!("{} · {}", self.set, self.set_zh)
    }

    pub fn command(self) -> &'static str {
        match self.recipe {
            Recipe::Frame { .. } => "image.fitFrame",
            Recipe::Square { .. } => "image.squarePad",
            Recipe::LongEdge { .. } => "image.fitLongEdge",
            Recipe::Watermark => "layer.watermark",
            Recipe::Matte { .. } => "image.matte",
            Recipe::Pass => "presets.pass",
        }
    }

    pub fn params(self) -> Value {
        match self.recipe {
            Recipe::Frame { width, height, mode, background, dpi } => {
                let mut params = json!({"width": width, "height": height, "mode": mode, "background": background});
                if let Some(dpi) = dpi {
                    params["resolution"] = json!(dpi);
                }
                params
            }
            Recipe::Square { padding, background } => json!({"padding": padding, "background": background}),
            Recipe::LongEdge { pixels } => json!({"pixels": pixels, "dontEnlarge": false}),
            Recipe::Watermark => json!({"text": "PhotoCraft", "opacity": 40, "position": "bottomRight", "margin": 24, "size": 36, "color": "#FFFFFF"}),
            Recipe::Matte { color } => json!({"color": color}),
            Recipe::Pass => json!({}),
        }
    }

    /// The Action the panel, Batch and `actions.play` store. Export format and quality are not
    /// steps: Batch and `photocraft-cli` apply them when they write the file.
    pub fn action(self) -> Action {
        let mut action = Action::new(self.display_name(), vec![(self.command().to_string(), self.params())]);
        action.set = Some(self.set_name());
        action
    }

    pub fn to_json(self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "nameZh": self.name_zh,
            "set": self.set,
            "setZh": self.set_zh,
            "summary": self.summary,
            "summaryZh": self.summary_zh,
            "command": self.command(),
            "params": self.params(),
            "export": {"format": self.format, "quality": self.quality, "naming": self.naming},
        })
    }

    fn matches(self, query: &str) -> bool {
        self.id.eq_ignore_ascii_case(query) || self.name.eq_ignore_ascii_case(query) || self.name_zh == query || self.display_name().eq_ignore_ascii_case(query)
    }
}

impl Omitted {
    pub fn to_json(self) -> Value {
        json!({"id": self.id, "name": self.name, "nameZh": self.name_zh, "reason": self.reason, "reasonZh": self.reason_zh})
    }

    fn matches(self, query: &str) -> bool {
        self.id.eq_ignore_ascii_case(query) || self.name.eq_ignore_ascii_case(query) || self.name_zh == query
    }
}

const fn frame(width: u32, height: u32, mode: &'static str, background: &'static str, dpi: Option<u32>) -> Recipe {
    Recipe::Frame { width, height, mode, background, dpi }
}

macro_rules! social {
    ($id:literal, $name:literal, $name_zh:literal, $width:literal, $height:literal, $quality:literal, $summary:literal, $summary_zh:literal $(,)?) => {
        Preset {
            id: $id,
            name: $name,
            name_zh: $name_zh,
            set: "Social",
            set_zh: "社交平台",
            summary: $summary,
            summary_zh: $summary_zh,
            format: Some("jpg"),
            quality: Some($quality),
            naming: None,
            recipe: frame($width, $height, "cover", "#FFFFFF", None),
        }
    };
}

macro_rules! id_frame {
    ($id:literal, $name:literal, $name_zh:literal, $width:literal, $height:literal, $background:literal, $summary:literal, $summary_zh:literal $(,)?) => {
        Preset {
            id: $id,
            name: $name,
            name_zh: $name_zh,
            set: "ID frame",
            set_zh: "证件尺寸",
            summary: $summary,
            summary_zh: $summary_zh,
            format: Some("jpg"),
            quality: Some(92),
            naming: None,
            recipe: frame($width, $height, "contain", $background, Some(300)),
        }
    };
}

/// Millimetres to pixels at `dpi`, rounded. 1 inch is 25 × 35 mm and 2 inch is 35 × 49 mm.
pub fn mm_to_px(mm: f64, dpi: f64) -> u32 {
    (mm / 25.4 * dpi).round() as u32
}

/// Every preset that can be loaded and played, in panel order.
pub fn all() -> &'static [Preset] {
    &PRESETS
}

/// Presets that are named only so a request for them fails with a reason.
pub fn omitted() -> &'static [Omitted] {
    &OMITTED
}

/// The Actions library, in the on-disk `{"actions":[...]}` shape.
pub fn actions() -> Vec<Action> {
    PRESETS.iter().copied().map(Preset::action).collect()
}

/// `query` is an id, an English name, a Chinese name, or the combined panel name.
pub fn find(query: &str) -> Result<&'static Preset> {
    let query = query.trim();
    if query.is_empty() {
        return Err(miss("preset name is empty"));
    }
    if let Some(omitted) = OMITTED.iter().copied().find(|item| item.matches(query)) {
        return Err(miss(format!("`{}` ({}) is not available: {} {}", omitted.id, omitted.name_zh, omitted.reason, omitted.reason_zh)));
    }
    let mut hits = PRESETS.iter().filter(|preset| preset.matches(query));
    match (hits.next(), hits.next()) {
        (Some(preset), None) => Ok(preset),
        (None, _) => Err(miss(format!("no preset `{query}`"))),
        (Some(_), Some(_)) => Err(miss(format!("`{query}` matches more than one preset; use its id"))),
    }
}

/// Default params with `over` copied on top. `over` replaces whole keys; it is not merged deeper.
pub fn merged_params(preset: &Preset, over: Option<&Value>) -> Result<Value> {
    let mut params = preset.params();
    let Some(over) = over.filter(|value| !value.is_null()) else { return Ok(params) };
    let extra = over.as_object().ok_or_else(|| miss("preset params must be an object"))?;
    let dest = params.as_object_mut().ok_or_else(|| EngineError::Other("internal preset params are not an object".into()))?;
    for (key, value) in extra {
        dest.insert(key.clone(), value.clone());
    }
    Ok(params)
}

fn miss(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: "presets".into(), msg: msg.into() }
}

const OMITTED: [Omitted; 2] = [
    Omitted {
        id: "id.face",
        name: "Face-centered ID photo",
        name_zh: "证件照人脸居中",
        reason: "Face-centered crop needs the face detector, which is a model. It is not part of this library; the ID frame presets keep the whole picture and fill the margin.",
        reason_zh: "人脸居中裁切依赖人脸检测模型，本库不提供。证件尺寸预设保留整张图片，只把四周填成底色。",
    },
    Omitted {
        id: "product.cutout",
        name: "Remove background",
        name_zh: "去除背景",
        reason: "Removing the background needs subject selection. It is not part of this library; Square white pad only centers and pads.",
        reason_zh: "去除背景依赖主体识别，本库不提供。方形白底只做居中和补边。",
    },
];

const PRESETS: [Preset; 21] = [
    Preset {
        id: "product.square-white",
        name: "Square white pad, JPEG 92",
        name_zh: "方形白底",
        set: "Product",
        set_zh: "电商",
        summary: "Trims transparent margins, centers the picture on a square of pure white (#FFFFFF) with an 8% margin, and exports JPEG at quality 92. An opaque background is kept; the background is not removed.",
        summary_zh: "裁掉透明边，把图片居中放到纯白（#FFFFFF）方形画布上并留 8% 边，导出 JPEG 质量 92。不透明的背景会保留，不会去除背景。",
        format: Some("jpg"),
        quality: Some(92),
        naming: None,
        recipe: Recipe::Square { padding: 0.08, background: "#FFFFFF" },
    },
    id_frame!(
        "id.1inch.white",
        "1-inch frame, white, JPEG 92",
        "一寸白底留边",
        295,
        413,
        "#FFFFFF",
        "Fits the whole picture inside 25 × 35 mm (295 × 413 px) at 300 dpi and fills the margin with white. It does not look for a face.",
        "把整张图片放进 300 dpi 的 25×35 mm（295×413 像素）画幅，空白处填白。不检测人脸。",
    ),
    id_frame!(
        "id.1inch.blue",
        "1-inch frame, blue, JPEG 92",
        "一寸蓝底留边",
        295,
        413,
        "#438EDB",
        "Fits the whole picture inside 25 × 35 mm (295 × 413 px) at 300 dpi and fills the margin with blue #438EDB. It does not look for a face.",
        "把整张图片放进 300 dpi 的 25×35 mm（295×413 像素）画幅，空白处填蓝 #438EDB。不检测人脸。",
    ),
    id_frame!(
        "id.1inch.red",
        "1-inch frame, red, JPEG 92",
        "一寸红底留边",
        295,
        413,
        "#C00000",
        "Fits the whole picture inside 25 × 35 mm (295 × 413 px) at 300 dpi and fills the margin with red #C00000. It does not look for a face.",
        "把整张图片放进 300 dpi 的 25×35 mm（295×413 像素）画幅，空白处填红 #C00000。不检测人脸。",
    ),
    id_frame!(
        "id.2inch.white",
        "2-inch frame, white, JPEG 92",
        "二寸白底留边",
        413,
        579,
        "#FFFFFF",
        "Fits the whole picture inside 35 × 49 mm (413 × 579 px) at 300 dpi and fills the margin with white. It does not look for a face.",
        "把整张图片放进 300 dpi 的 35×49 mm（413×579 像素）画幅，空白处填白。不检测人脸。",
    ),
    id_frame!(
        "id.2inch.blue",
        "2-inch frame, blue, JPEG 92",
        "二寸蓝底留边",
        413,
        579,
        "#438EDB",
        "Fits the whole picture inside 35 × 49 mm (413 × 579 px) at 300 dpi and fills the margin with blue #438EDB. It does not look for a face.",
        "把整张图片放进 300 dpi 的 35×49 mm（413×579 像素）画幅，空白处填蓝 #438EDB。不检测人脸。",
    ),
    id_frame!(
        "id.2inch.red",
        "2-inch frame, red, JPEG 92",
        "二寸红底留边",
        413,
        579,
        "#C00000",
        "Fits the whole picture inside 35 × 49 mm (413 × 579 px) at 300 dpi and fills the margin with red #C00000. It does not look for a face.",
        "把整张图片放进 300 dpi 的 35×49 mm（413×579 像素）画幅，空白处填红 #C00000。不检测人脸。",
    ),
    social!(
        "social.wechat-moments",
        "WeChat Moments, JPEG 80",
        "微信朋友圈",
        1080,
        1080,
        80,
        "Center-crops to 1080 × 1080 and exports JPEG at quality 80. Transparency is filled with white.",
        "居中裁成 1080×1080，导出 JPEG 质量 80。透明处填白。",
    ),
    social!(
        "social.xiaohongshu",
        "Xiaohongshu 3:4, JPEG 90",
        "小红书",
        1080,
        1440,
        90,
        "Center-crops to 1080 × 1440 (3:4) and exports JPEG at quality 90. Transparency is filled with white.",
        "居中裁成 1080×1440（3:4），导出 JPEG 质量 90。透明处填白。",
    ),
    social!(
        "social.douyin",
        "Douyin 9:16, JPEG 82",
        "抖音",
        1080,
        1920,
        82,
        "Center-crops to 1080 × 1920 (9:16) and exports JPEG at quality 82. Transparency is filled with white.",
        "居中裁成 1080×1920（9:16），导出 JPEG 质量 82。透明处填白。",
    ),
    social!(
        "social.weibo",
        "Weibo, JPEG 80",
        "微博",
        1080,
        1080,
        80,
        "Center-crops to 1080 × 1080 and exports JPEG at quality 80. Transparency is filled with white.",
        "居中裁成 1080×1080，导出 JPEG 质量 80。透明处填白。",
    ),
    social!(
        "social.instagram-square",
        "Instagram square, JPEG 85",
        "Instagram 方形",
        1080,
        1080,
        85,
        "Center-crops to 1080 × 1080 and exports JPEG at quality 85. Transparency is filled with white.",
        "居中裁成 1080×1080，导出 JPEG 质量 85。透明处填白。",
    ),
    social!(
        "social.instagram-portrait",
        "Instagram portrait, JPEG 85",
        "Instagram 竖图",
        1080,
        1350,
        85,
        "Center-crops to 1080 × 1350 (4:5) and exports JPEG at quality 85. Transparency is filled with white.",
        "居中裁成 1080×1350（4:5），导出 JPEG 质量 85。透明处填白。",
    ),
    social!(
        "social.x",
        "X post, JPEG 75",
        "X 帖子",
        1600,
        900,
        75,
        "Center-crops to 1600 × 900 (16:9) and exports JPEG at quality 75. Transparency is filled with white.",
        "居中裁成 1600×900（16:9），导出 JPEG 质量 75。透明处填白。",
    ),
    Preset {
        id: "batch.long-edge",
        name: "Long edge 1600 px",
        name_zh: "长边 1600 像素",
        set: "Batch",
        set_zh: "批处理",
        summary: "Scales so the longer side is 1600 px and keeps the file type. Pass pixels to choose another length. Does not enlarge when dontEnlarge is true.",
        summary_zh: "把较长的一边缩放到 1600 像素，并保留原格式。用 pixels 指定别的长度。dontEnlarge 为 true 时不放大。",
        format: None,
        quality: None,
        naming: None,
        recipe: Recipe::LongEdge { pixels: 1600 },
    },
    Preset {
        id: "batch.watermark",
        name: "Text watermark",
        name_zh: "文字水印",
        set: "Batch",
        set_zh: "批处理",
        summary: "Puts PhotoCraft at 40% white in the bottom-right corner. Replace text, or pass logo with an image path and text set to an empty string. The file type stays the same.",
        summary_zh: "在右下角以 40% 白色写上 PhotoCraft。可改 text，或把 text 设为空并传 logo 图片路径。文件格式不变。",
        format: None,
        quality: None,
        naming: None,
        recipe: Recipe::Watermark,
    },
    Preset {
        id: "batch.rename",
        name: "Rename 001_name",
        name_zh: "序号重命名",
        set: "Batch",
        set_zh: "批处理",
        summary: "Leaves the pixels alone and names each file 001_name.ext, using {serial:3}_{name}.{ext}. The file type stays the same unless a format is passed.",
        summary_zh: "不改像素，按 {serial:3}_{name}.{ext} 命名为 001_文件名.扩展名。不另传格式时保留原格式。",
        format: None,
        quality: None,
        naming: Some("{serial:3}_{name}.{ext}"),
        recipe: Recipe::Pass,
    },
    Preset {
        id: "batch.to-jpg",
        name: "Convert to JPEG 85",
        name_zh: "转为 JPEG",
        set: "Batch",
        set_zh: "批处理",
        summary: "Flattens transparency onto white and exports JPEG at quality 85.",
        summary_zh: "把透明处合成到白色上，导出 JPEG 质量 85。",
        format: Some("jpg"),
        quality: Some(85),
        naming: None,
        recipe: Recipe::Matte { color: "#FFFFFF" },
    },
    Preset {
        id: "batch.to-png",
        name: "Convert to PNG",
        name_zh: "转为 PNG",
        set: "Batch",
        set_zh: "批处理",
        summary: "Exports PNG and keeps transparency. Pixels are unchanged.",
        summary_zh: "导出 PNG 并保留透明。像素不变。",
        format: Some("png"),
        quality: None,
        naming: None,
        recipe: Recipe::Pass,
    },
    Preset {
        id: "batch.to-webp",
        name: "Convert to WebP 80",
        name_zh: "转为 WebP",
        set: "Batch",
        set_zh: "批处理",
        summary: "Flattens transparency onto white and exports lossy WebP at quality 80.",
        summary_zh: "把透明处合成到白色上，导出有损 WebP 质量 80。",
        format: Some("webp"),
        quality: Some(80),
        naming: None,
        recipe: Recipe::Matte { color: "#FFFFFF" },
    },
    Preset {
        id: "batch.to-tiff",
        name: "Convert to TIFF",
        name_zh: "转为 TIFF",
        set: "Batch",
        set_zh: "批处理",
        summary: "Exports TIFF and keeps transparency. Pixels are unchanged.",
        summary_zh: "导出 TIFF 并保留透明。像素不变。",
        format: Some("tiff"),
        quality: None,
        naming: None,
        recipe: Recipe::Pass,
    },
];
