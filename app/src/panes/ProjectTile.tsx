import { Folder } from 'lucide-react';
import type { Project } from '../lib/client';

// Swatch palette from spec/mock.html (`SW`).
const SWATCH: Record<string, string> = {
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

/** The swatch colour of a project colour key (gray for an unknown key). */
export const swatchColor = (key: string): string => SWATCH[key] ?? SWATCH.gray ?? '#8E8E93';

/** The project's colour as an outlined folder (mock `folder()`), decorative. TODO(WP3): other project icons once the sidebar's icon map is shared; every icon renders as a folder here. */
export function ProjectTile({ project, size = 13 }: { project: Pick<Project, 'color'>; size?: number }) {
  return (
    <span
      className="inline-grid shrink-0 place-items-center"
      style={{ width: size, height: size, color: SWATCH[project.color] ?? SWATCH.gray }}
      aria-hidden="true"
    >
      <Folder size={size} strokeWidth={1.8} />
    </span>
  );
}
