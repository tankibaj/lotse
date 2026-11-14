import { Composition } from "remotion";
import { LotseFilm } from "./LotseFilm";
import { FPS, HEIGHT, WIDTH, TOTAL_FRAMES } from "./lib/anim";

export const Root = () => (
  <Composition
    id="LotseFilm"
    component={LotseFilm}
    durationInFrames={TOTAL_FRAMES}
    fps={FPS}
    width={WIDTH}
    height={HEIGHT}
    defaultProps={{ showRepo: true }}
  />
);
