import { beforeEach, describe, expect, it } from 'vitest';
import { useAuthStore } from './auth';

describe('auth store', () => {
  beforeEach(() => {
    useAuthStore.setState({ token: null, username: null, isAuthenticated: false });
  });

  it('starts unauthenticated', () => {
    expect(useAuthStore.getState().isAuthenticated).toBe(false);
    expect(useAuthStore.getState().token).toBeNull();
  });

  it('login stores token and username', () => {
    useAuthStore.getState().login('tok-123', 'erik');
    const s = useAuthStore.getState();
    expect(s.token).toBe('tok-123');
    expect(s.username).toBe('erik');
    expect(s.isAuthenticated).toBe(true);
  });

  it('logout clears the session', () => {
    useAuthStore.getState().login('tok', 'erik');
    useAuthStore.getState().logout();
    expect(useAuthStore.getState().isAuthenticated).toBe(false);
    expect(useAuthStore.getState().token).toBeNull();
    expect(useAuthStore.getState().username).toBeNull();
  });
});
