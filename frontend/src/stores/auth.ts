import { create } from 'zustand';
import { persist, createJSONStorage, StateStorage } from 'zustand/middleware';

interface AuthState {
  token: string | null;
  refreshToken: string | null;
  username: string | null;
  isAuthenticated: boolean;
  login: (token: string, username: string, refresh?: string) => void;
  logout: () => void;
}

const memory = new Map<string, string>();
const memoryStorage: StateStorage = {
  getItem: (name) => memory.get(name) ?? null,
  setItem: (name, value) => { memory.set(name, value); },
  removeItem: (name) => { memory.delete(name); },
};

function authStorage(): StateStorage {
  try {
    if (typeof localStorage !== 'undefined') return localStorage;
  } catch { /* node vitest */ }
  return memoryStorage;
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set, get) => ({
      token: null,
      refreshToken: null,
      username: null,
      isAuthenticated: false,
      login: (token, username, refresh = '') => set({
        token,
        refreshToken: refresh || null,
        username,
        isAuthenticated: true,
      }),
      logout: () => {
        const token = get().token;
        if (token) {
          fetch('/api/auth/logout', {
            method: 'POST',
            headers: {
              Authorization: `Bearer ${token}`,
              'Content-Type': 'application/json',
            },
          }).catch(() => {});
        }
        set({ token: null, refreshToken: null, username: null, isAuthenticated: false });
      },
    }),
    {
      name: 'superops-auth',
      storage: createJSONStorage(authStorage),
    },
  ),
);

