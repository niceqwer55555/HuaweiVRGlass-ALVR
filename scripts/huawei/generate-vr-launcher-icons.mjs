import { createRequire } from "node:module";
import { mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const sharpModule = process.env.HUAWEI_ICON_SHARP_MODULE || "sharp";
const sharp = require(sharpModule);

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDirectory, "..", "..");
const assetDirectory = path.join(
  repoRoot,
  "alvr",
  "client_openxr",
  "android_huawei",
  "assets",
);
const previewDirectory = path.resolve(repoRoot, "..", "work");

const backgroundSource = path.join(repoRoot, "resources", "BG-Gray.svg");
const foregroundSource = path.join(repoRoot, "resources", "ALVR-White.svg");
const backgroundOutput = path.join(assetDirectory, "vr_icon_background.png");
const foregroundOutput = path.join(assetDirectory, "vr_icon_foreground.png");
const previewOutput = path.join(previewDirectory, "huawei-vr-launcher-icon-preview.png");

await mkdir(assetDirectory, { recursive: true });
await mkdir(previewDirectory, { recursive: true });

await sharp(backgroundSource, { density: 144 })
  .resize(576, 324, { fit: "fill" })
  .png({ compressionLevel: 9, adaptiveFiltering: true })
  .toFile(backgroundOutput);

const wordmark = await sharp(foregroundSource, { density: 288 })
  .resize(320, 160, { fit: "contain" })
  .png()
  .toBuffer();

await sharp({
  create: {
    width: 576,
    height: 324,
    channels: 4,
    background: { r: 0, g: 0, b: 0, alpha: 0 },
  },
})
  .composite([{ input: wordmark, left: 128, top: 82 }])
  .png({ compressionLevel: 9, adaptiveFiltering: true })
  .toFile(foregroundOutput);

await sharp(backgroundOutput)
  .composite([{ input: foregroundOutput, left: 0, top: 0 }])
  .png({ compressionLevel: 9, adaptiveFiltering: true })
  .toFile(previewOutput);

for (const output of [backgroundOutput, foregroundOutput, previewOutput]) {
  const metadata = await sharp(output).metadata();
  if (metadata.width !== 576 || metadata.height !== 324) {
    throw new Error(`Unexpected icon size for ${output}: ${metadata.width}x${metadata.height}`);
  }
  process.stdout.write(`${output}\n`);
}
