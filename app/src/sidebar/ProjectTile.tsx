import {
  Anchor,
  Book,
  Braces,
  Briefcase,
  Bug,
  Building2,
  Camera,
  Cloud,
  Code,
  Cpu,
  Database,
  Flag,
  Flame,
  FlaskConical,
  Folder,
  Gamepad2,
  GitBranch,
  Globe,
  GraduationCap,
  Hammer,
  Heart,
  HeartPulse,
  House,
  Key,
  Leaf,
  Lock,
  Moon,
  Music,
  Package,
  Rocket,
  Server,
  Shield,
  Ship,
  ShoppingCart,
  Sparkles,
  Star,
  Stethoscope,
  Sun,
  Terminal,
  Wrench,
  Zap,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { ProjectIcon } from '../sheets/symbols';

/** Swatch keys from the mock palette (`SW` in spec/mock.html). Project colours never use status hues by default. */
export const SWATCHES: Record<string, string> = {
  red: '#FF5E57',
  coral: '#FF7F50',
  orange: '#FF9F40',
  yellow: '#F5C518',
  green: '#4CB963',
  teal: '#2EC4B6',
  sky: '#7CC6F2',
  blue: '#3B82F6',
  indigo: '#5B6CD9',
  purple: '#9B59D0',
  lavender: '#B47EE5',
  pink: '#E87ACB',
  gray: '#8E8E93',
  sage: '#9BB59F',
  tan: '#C2A88A',
};

/** Lucide stand-ins for the mock's symbol picker (SF Symbols arrive with the native helper). */
export const PROJECT_ICONS: Record<string, LucideIcon> = {
  folder: Folder,
  terminal: Terminal,
  code: Code,
  braces: Braces,
  'git-branch': GitBranch,
  bug: Bug,
  hammer: Hammer,
  wrench: Wrench,
  cpu: Cpu,
  database: Database,
  server: Server,
  cloud: Cloud,
  package: Package,
  'flask-conical': FlaskConical,
  rocket: Rocket,
  shield: Shield,
  book: Book,
  briefcase: Briefcase,
  house: House,
  'building-2': Building2,
  'shopping-cart': ShoppingCart,
  globe: Globe,
  camera: Camera,
  music: Music,
  'heart-pulse': HeartPulse,
  stethoscope: Stethoscope,
  'graduation-cap': GraduationCap,
  'gamepad-2': Gamepad2,
  star: Star,
  heart: Heart,
  anchor: Anchor,
  ship: Ship,
  flame: Flame,
  leaf: Leaf,
  flag: Flag,
  sparkles: Sparkles,
  sun: Sun,
  moon: Moon,
  zap: Zap,
  lock: Lock,
  key: Key,
};

/** The project's symbol in its colour, outlined with no fill (16 px in the sidebar): SF Symbol in the app, Lucide in the browser. */
export function ProjectTile({ color, icon, size = 16 }: { color: string; icon: string; size?: number }) {
  return (
    <span
      className="inline-grid flex-none place-items-center"
      style={{ width: size, height: size, color: SWATCHES[color] ?? SWATCHES.gray }}
      aria-hidden="true"
    >
      <ProjectIcon name={icon} size={size} strokeWidth={1.8} outline />
    </span>
  );
}
