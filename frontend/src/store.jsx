import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { Auth, clearTokens } from './api';
import { realtime } from './realtime';

const Ctx = createContext(null);

export function AppProvider({ children }) {
  const [user, setUser] = useState(null);
  const [ready, setReady] = useState(false);
  const [toasts, setToasts] = useState([]);
  const [connection, setConnection] = useState(false);
  const toastId = useRef(0);

  const toast = useCallback((message, type = 'info', timeout = 3500) => {
    const id = ++toastId.current;
    setToasts((prev) => [...prev, { id, message, type }]);
    setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), timeout);
  }, []);

  useEffect(() => {
    (async () => {
      try {
        const me = await Auth.me();
        setUser(me);
        realtime.connect();
        setReady(true);
      } catch (e) {
        clearTokens();
        setReady(true);
      }
    })();

    const onExpired = () => {
      setUser(null);
      realtime.close();
      toast('انتهت الجلسة، سجل الدخول مجدداً', 'error');
    };
    window.addEventListener('meev:auth-expired', onExpired);
    const unsub = realtime.on('connection', ({ online }) => setConnection(online));
    return () => {
      window.removeEventListener('meev:auth-expired', onExpired);
      unsub();
    };
  }, [toast]);

  const login = useCallback(async (payload) => {
    const data = await Auth.login(payload);
    setUser(data.user);
    realtime.connect();
    return data;
  }, []);

  const register = useCallback(async (payload) => {
    const data = await Auth.register(payload);
    setUser(data.user);
    realtime.connect();
    return data;
  }, []);

  const logout = useCallback(async () => {
    await Auth.logout().catch(() => {});
    clearTokens();
    realtime.close();
    setUser(null);
  }, []);

  const value = useMemo(
    () => ({ user, setUser, ready, toasts, toast, connection, login, register, logout }),
    [user, ready, toasts, toast, connection, login, register, logout],
  );

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useApp() {
  const v = useContext(Ctx);
  if (!v) throw new Error('AppProvider missing');
  return v;
}
