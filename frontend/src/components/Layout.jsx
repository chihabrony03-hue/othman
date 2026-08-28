import { NavLink, useNavigate } from 'react-router-dom';
import { useApp } from '../store';
import Avatar from './Avatar';

const links = [
  { to: '/', label: 'المحادثات', icon: '💬', end: true },
  { to: '/explore', label: 'اكتشف', icon: '✨' },
  { to: '/search', label: 'البحث', icon: '🔎' },
];

export default function Layout({ children }) {
  const { user, logout, connection } = useApp();
  const navigate = useNavigate();

  const onLogout = async () => {
    await logout();
    navigate('/login');
  };

  return (
    <div className="app-shell">
      <header className="topbar">
        <NavLink to="/" className="brand">
          <img src="/brand/meev1.png" alt="MEEV" />
          <b>MEEV</b>
        </NavLink>
        <nav className="nav-links">
          {links.map((l) => (
            <NavLink key={l.to} to={l.to} end={l.end} className={({ isActive }) => (isActive ? 'active' : '')}>
              <span>{l.icon}</span>
              <span className="label">{l.label}</span>
            </NavLink>
          ))}
        </nav>
        <div className="actions">
          <span className="chip" title={connection ? 'متصل' : 'جارٍ الاتصال'}>
            <span style={{ width: 8, height: 8, borderRadius: '50%', background: connection ? 'var(--ok)' : 'var(--text-3)' }} />
            {connection ? 'متصل' : '…'}
          </span>
          <NavLink to="/settings" className="icon-btn" title="الإعدادات">⚙️</NavLink>
          <NavLink to={`/u/${user?.username}`} title="ملفي">
            <Avatar user={user} size="sm" online={connection} />
          </NavLink>
          <button className="btn btn-ghost btn-sm" onClick={onLogout}>خروج</button>
        </div>
      </header>
      <main className="container">{children}</main>
    </div>
  );
}
