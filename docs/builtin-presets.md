# Built-in export presets

PhotoCraft ships a library of export presets. Each one is an ordinary Action: a name, a set, and steps of engine commands (`[["command", {params}], …]`), the same shape as the Actions panel's `actions.json`. The panel, `photocraft-cli batch` / `run`, and MCP all play that shape.

English and Chinese names are both part of the action name (`English · 中文`). `presets.list` and `photocraft-cli presets` also return them separately, with the export format, JPEG/WebP quality (1–100), and filename template.

Face-centered ID crop and background removal are **not** in the library. Both depend on a model (the face detector, and subject selection). Asking for `id.face` or `product.cutout` returns an error that says so. The ID presets below keep the whole picture and fill the margin; they do not look for a face. Square white pad does not remove a background.

## From the Actions panel

Window ▸ Actions ▸ the panel menu ▸ **Load Built-in Presets**, or the same button on an empty panel. That runs `presets.load`. Names already in the panel are left as they are. Select a preset and press Play, then save the document yourself: the action changes pixels, and the export format and quality are applied by the CLI when it writes a file.

## From the CLI

```sh
photocraft-cli presets
photocraft-cli presets --id social.xiaohongshu
photocraft-cli presets --json

# One picture. An existing --out file is left in place unless --overwrite.
photocraft-cli run photo.png --preset "方形白底" --out square.jpg

# A folder. --format, --quality and --naming override the preset.
photocraft-cli batch --preset social.wechat-moments --in photos/ --out moments/
photocraft-cli batch --preset batch.long-edge --preset-params '{"pixels":800}' --in photos/ --out web/
photocraft-cli batch --preset batch.watermark --preset-params '{"text":"© Shop"}' --in photos/ --out marked/
photocraft-cli batch --preset batch.rename --in photos/ --out named/
```

`--out` must be a different folder from `--in` unless `--in-place` is also passed. A preset still will not replace a file that is already there unless `--overwrite` is passed. `--in-place --overwrite` is what replaces sources.

The preset line on stderr is the export that will be written, for example: format `jpg`, quality `80`, naming `(keep the file name)`, and whether existing files are left in place.

## From MCP

`photocraft-cli mcp --headless` exposes the same commands through `command_run`:

- `presets.list` → every preset and the two omitted ids
- `presets.get` with `{"id":"batch.to-jpg"}` → the action steps plus `export`
- `presets.apply` with `{"id":"image id or name","params":{…}}` → plays the step and returns `export` (it does not write a file)
- `presets.load`, then `actions.play` with `{"action":"WeChat Moments, JPEG 80 · 微信朋友圈"}`

Write the file with `doc_export` / `doc_save` using the `export` object (`format`, `quality`). Quality is 1–100.

## Presets

| id | Names | What it does | Export |
|---|---|---|---|
| `product.square-white` | Square white pad, JPEG 92 · 方形白底 | Trims transparent margins, centers on a square of `#FFFFFF` with an 8% margin. Opaque backgrounds stay. | JPEG 92 |
| `id.1inch.white` | 1-inch frame, white, JPEG 92 · 一寸白底留边 | Whole picture inside 25 × 35 mm (295 × 413 px) at 300 dpi, margin `#FFFFFF` | JPEG 92 |
| `id.1inch.blue` | 1-inch frame, blue, JPEG 92 · 一寸蓝底留边 | Same frame, margin `#438EDB` | JPEG 92 |
| `id.1inch.red` | 1-inch frame, red, JPEG 92 · 一寸红底留边 | Same frame, margin `#C00000` | JPEG 92 |
| `id.2inch.white` | 2-inch frame, white, JPEG 92 · 二寸白底留边 | Whole picture inside 35 × 49 mm (413 × 579 px) at 300 dpi, margin `#FFFFFF` | JPEG 92 |
| `id.2inch.blue` | 2-inch frame, blue, JPEG 92 · 二寸蓝底留边 | Same frame, margin `#438EDB` | JPEG 92 |
| `id.2inch.red` | 2-inch frame, red, JPEG 92 · 二寸红底留边 | Same frame, margin `#C00000` | JPEG 92 |
| `social.wechat-moments` | WeChat Moments, JPEG 80 · 微信朋友圈 | Center-crop 1080 × 1080, transparency filled white | JPEG 80 |
| `social.xiaohongshu` | Xiaohongshu 3:4, JPEG 90 · 小红书 | Center-crop 1080 × 1440 | JPEG 90 |
| `social.douyin` | Douyin 9:16, JPEG 82 · 抖音 | Center-crop 1080 × 1920 | JPEG 82 |
| `social.weibo` | Weibo, JPEG 80 · 微博 | Center-crop 1080 × 1080 | JPEG 80 |
| `social.instagram-square` | Instagram square, JPEG 85 · Instagram 方形 | Center-crop 1080 × 1080 | JPEG 85 |
| `social.instagram-portrait` | Instagram portrait, JPEG 85 · Instagram 竖图 | Center-crop 1080 × 1350 (4:5) | JPEG 85 |
| `social.x` | X post, JPEG 75 · X 帖子 | Center-crop 1600 × 900 | JPEG 75 |
| `batch.long-edge` | Long edge 1600 px · 长边 1600 像素 | Longer side becomes 1600 px (`pixels` overrides; `dontEnlarge` refuses to enlarge) | same type |
| `batch.watermark` | Text watermark · 文字水印 | `PhotoCraft` at 40% white, bottom-right. `text` replaces it. A logo is `logo` plus `"text":""`; `scale` is a percent of the long edge | same type |
| `batch.rename` | Rename 001_name · 序号重命名 | Pixels unchanged. Names `{serial:3}_{name}.{ext}` | same type |
| `batch.to-jpg` | Convert to JPEG 85 · 转为 JPEG | Transparency flattened onto white | JPEG 85 |
| `batch.to-png` | Convert to PNG · 转为 PNG | Pixels unchanged, transparency kept | PNG |
| `batch.to-webp` | Convert to WebP 80 · 转为 WebP | Transparency flattened onto white | WebP 80 |
| `batch.to-tiff` | Convert to TIFF · 转为 TIFF | Pixels unchanged, transparency kept | TIFF |

Filename tokens are the Batch tokens: `{name}`, `{serial}` / `{serial:3}`, `{date}`, `{ext}`, and the case variants. The template is the whole file name. A result of `.` or `..`, or any name with a slash, is refused, so the file cannot leave the output folder.

## Desktop bridge

`photocraft-cli mcp` without `--headless` forwards commands to the running desktop. A desktop that is still v0.23.1 does not contain `presets.*`, `image.fitFrame`, `image.squarePad`, `image.fitLongEdge`, `image.matte`, or `layer.watermark`. Those commands exist only in a build that includes this library. Headless `photocraft-cli batch` and `photocraft-cli run` use the CLI's own engine, so they do not need the open desktop. Bridging to an older desktop does not make the new commands appear there.

## Commands

`image.fitFrame`, `image.squarePad`, `image.fitLongEdge`, `image.matte`, and `layer.watermark` are the steps. They flatten to one RGB layer (watermark adds a layer instead). `presets.pass` is the step that does not change pixels, used by rename and by PNG/TIFF conversion.

`layer.watermark` reads a logo from a path on the desktop. The browser build has no logo files and says so; text watermarks still work.
