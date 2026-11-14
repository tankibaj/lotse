import { useCallback, useEffect, useState } from 'react';
import { syncMenuCheck } from '../settings/appMenu';

const read = (key: string): boolean => {
  try {
    return localStorage.getItem(key) === '1';
  } catch {
    return false; // storage unavailable
  }
};

/**
 * A boolean saved in localStorage under `key` (off unless saved as "1"). With `menuId`, the View menu item of that
 * id mirrors it as a checkmark. Returns the value and a toggle.
 */
export function usePersistedFlag(key: string, menuId?: string): [boolean, () => void] {
  const [on, setOn] = useState(() => read(key));
  const toggle = useCallback(() => setOn((v) => !v), []);
  useEffect(() => {
    try {
      localStorage.setItem(key, on ? '1' : '0');
    } catch {
      // storage unavailable: the choice lasts until quit
    }
  }, [key, on]);
  useEffect(() => {
    if (menuId) syncMenuCheck(menuId, on);
  }, [menuId, on]);
  return [on, toggle];
}
