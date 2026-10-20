import { registerRoot, Composition } from "remotion";
import { LaunchFilm } from "./LaunchFilm";
import "./style.css";

const Root = () => (
  <Composition
    id="AmarcodeLaunch"
    component={LaunchFilm}
    durationInFrames={1800}
    fps={60}
    width={1920}
    height={1080}
  />
);
registerRoot(Root);
