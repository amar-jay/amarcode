import { Config } from "@remotion/cli/config";

Config.setCodec("h264");
Config.setPixelFormat("yuv420p");
Config.setCrf(16);
Config.setVideoImageFormat("jpeg");
Config.setJpegQuality(95);
Config.setOverwriteOutput(true);
if (process.env.CHROME_PATH)
  Config.setBrowserExecutable(process.env.CHROME_PATH);
