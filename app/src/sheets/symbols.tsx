import {
  Anchor, Book, Braces, Briefcase, Bug, Building2, Camera, Cloud, Code, Cpu, Database, Flag, Flame, FlaskConical,
  Folder, Gamepad2, GitBranch, Globe, GraduationCap, Hammer, Heart, HeartPulse, House, Key, Leaf, Lock, Moon,
  Music, Package, Rocket, Server, Shield, Ship, ShoppingCart, Sparkles, Star, Stethoscope, Sun, Terminal, Wrench, Zap,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';

/** The 15 project swatches (mock `SW`). `Project.color` stores the key. */
export const SWATCHES: Record<string, string> = {
  red: '#FF5E57', coral: '#FF7F50', orange: '#FF9F40', yellow: '#F5C518', green: '#4CB963',
  teal: '#2EC4B6', sky: '#7CC6F2', blue: '#3B82F6', indigo: '#5B6CD9', purple: '#9B59D0',
  lavender: '#B47EE5', pink: '#E87ACB', gray: '#8E8E93', sage: '#9BB59F', tan: '#C2A88A',
};

/** Symbol grid (mock `ICONS`), rendered with Lucide (SF Symbols helper is WP7, optional). `Project.icon` stores the key. */
export const ICON_GROUPS: Record<string, string[]> = {
  Development: ['folder', 'terminal', 'code', 'braces', 'git-branch', 'bug', 'hammer', 'wrench', 'cpu', 'database', 'server', 'cloud', 'package', 'flask-conical', 'rocket', 'shield'],
  Objects: ['book', 'briefcase', 'house', 'building-2', 'shopping-cart', 'globe', 'camera', 'music', 'heart-pulse', 'stethoscope', 'graduation-cap', 'gamepad-2'],
  Symbols: ['star', 'heart', 'anchor', 'ship', 'flame', 'leaf', 'flag', 'sparkles', 'sun', 'moon', 'zap', 'lock', 'key'],
};

const ICONS: Record<string, LucideIcon> = {
  folder: Folder, terminal: Terminal, code: Code, braces: Braces, 'git-branch': GitBranch, bug: Bug, hammer: Hammer,
  wrench: Wrench, cpu: Cpu, database: Database, server: Server, cloud: Cloud, package: Package,
  'flask-conical': FlaskConical, rocket: Rocket, shield: Shield, book: Book, briefcase: Briefcase, house: House,
  'building-2': Building2, 'shopping-cart': ShoppingCart, globe: Globe, camera: Camera, music: Music,
  'heart-pulse': HeartPulse, stethoscope: Stethoscope, 'graduation-cap': GraduationCap, 'gamepad-2': Gamepad2,
  star: Star, heart: Heart, anchor: Anchor, ship: Ship, flame: Flame, leaf: Leaf, flag: Flag, sparkles: Sparkles,
  sun: Sun, moon: Moon, zap: Zap, lock: Lock, key: Key,
};

/** Lucide icon for a project icon key; unknown keys fall back to the folder. */
export function ProjectIcon({ name, size = 20, strokeWidth = 1.8 }: { name: string; size?: number; strokeWidth?: number }) {
  const Icon = ICONS[name] ?? Folder;
  return <Icon width={size} height={size} strokeWidth={strokeWidth} aria-hidden="true" />;
}

/** Swatch colour for a key (unknown keys: gray). */
export const swatchColor = (key: string): string => SWATCHES[key] ?? '#8E8E93';

/** Symbols matching a search: by name or group, like the mock. Empty groups are dropped. */
export function filterIcons(query: string): [string, string[]][] {
  const q = query.toLowerCase().trim();
  return Object.entries(ICON_GROUPS)
    .map(([group, names]): [string, string[]] => [group, names.filter((n) => !q || n.includes(q) || group.toLowerCase().includes(q))])
    .filter(([, names]) => names.length > 0);
}
