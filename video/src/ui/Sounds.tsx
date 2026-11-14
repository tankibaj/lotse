import { Audio, Sequence, staticFile } from "remotion";
import { s } from "../lib/anim";

type Ev = [number, string, number];

// UI sound design: soft, tactile, tied to the visible action. The film also works muted.
const warm: Ev[] = Array.from({ length: 8 }, (_, k): Ev => [1.75 + k * 0.4, "tick", 0.14 + k * 0.015]); // quiet clock under the problem beat
const focus: Ev[] = Array.from({ length: 10 }, (_, k): Ev => [5.0 + k * 0.38, "tick", 0.4]);
const EVENTS: Ev[] = [
  [0.05, "pop", 0.32], [0.18, "pop-hi", 0.3], [0.31, "pop", 0.32], [0.44, "pop-hi", 0.3], [0.57, "pop", 0.32],
  [0.6, "pop-hi", 0.25], [1.25, "thud", 0.3],
  [2.5, "pop-hi", 0.3], [5.0, "pop", 0.3],
  ...warm,
  ...focus,
  [8.85, "whoosh", 0.4], [9.6, "whoosh-soft", 0.32], [10.1, "thud", 0.45],
  [11.4, "thud", 0.22], [12.1, "click", 0.35],
  [13.0, "pop", 0.5], [13.5, "pop-hi", 0.4], [13.7, "swish", 0.22], [14.9, "tick", 0.3], [15.9, "swish", 0.2],
  [15.5, "ping", 0.5], [15.9, "pop", 0.4],
  [17.15, "key", 0.6], [17.25, "swish", 0.28], [18.1, "tick", 0.35], [18.6, "tick", 0.3], [19.5, "swish", 0.2],
  [20.4, "swish", 0.2], [21.3, "click", 0.6], [22.4, "chime", 0.5],
  [24.3, "click", 0.5], [24.35, "whoosh-soft", 0.32],
  [27.2, "pop-hi", 0.4], [28.1, "chime", 0.32],
];

export const Sounds = () => (
  <>
    {EVENTS.map(([t, name, vol], i) => (
      <Sequence key={i} from={s(t)} layout="none">
        <Audio src={staticFile(`sfx/${name}.wav`)} volume={vol} />
      </Sequence>
    ))}
  </>
);
