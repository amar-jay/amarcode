import { registerRoot, Composition } from "remotion";
import { PromoFilm } from "./PromoFilm";
import { FPS, HEIGHT, TOTAL_FRAMES, WIDTH } from "./promo-state";
import "./style.css";

const Root = () => (
  <Composition
    id="PromoLanding"
    component={PromoFilm}
    durationInFrames={TOTAL_FRAMES}
    fps={FPS}
    width={WIDTH}
    height={HEIGHT}
  />
);
registerRoot(Root);
