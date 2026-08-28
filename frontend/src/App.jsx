import { lazy, Suspense } from 'react';
import { Navigate, Route, Routes, useLocation } from 'react-router-dom';
import { useApp } from './store';
import Toasts from './components/Toast';

// Route-level lazy loading => each page is a small separate JS chunk.
const AuthPage = lazy(() => import('./pages/AuthPage'));
const HomePage = lazy(() => import('./pages/HomePage'));
const ExplorePage = lazy(() => import('./pages/ExplorePage'));
const SearchPage = lazy(() => import('./pages/SearchPage'));
const ProfilePage = lazy(() => import('./pages/ProfilePage'));
const SettingsPage = lazy(() => import('./pages/SettingsPage'));
const NotFoundPage = lazy(() => import('./pages/NotFoundPage'));

function FullLoader() {
  return (
    <div className="container" style={{ display: 'grid', placeItems: 'center', minHeight: '70vh' }}>
      <div>
        <div className="spinner" />
        <div className="muted" style={{ textAlign: 'center' }}>جارٍ التحميل…</div>
      </div>
    </div>
  );
}

function Guard({ children }) {
  const { user, ready } = useApp();
  const loc = useLocation();
  if (!ready) return <FullLoader />;
  if (!user) return <Navigate to="/login" state={{ from: loc }} replace />;
  return children;
}

export default function App() {
  const { ready, user } = useApp();
  return (
    <Suspense fallback={<FullLoader />}>
      <Routes>
        <Route
          path="/login"
          element={ready && user ? <Navigate to="/" replace /> : <AuthPage mode="login" />}
        />
        <Route
          path="/register"
          element={ready && user ? <Navigate to="/" replace /> : <AuthPage mode="register" />}
        />
        <Route path="/" element={<Guard><HomePage /></Guard>} />
        <Route path="/explore" element={<Guard><ExplorePage /></Guard>} />
        <Route path="/search" element={<Guard><SearchPage /></Guard>} />
        <Route path="/u/:username" element={<Guard><ProfilePage /></Guard>} />
        <Route path="/settings" element={<Guard><SettingsPage /></Guard>} />
        <Route path="*" element={<NotFoundPage />} />
      </Routes>
      <Toasts />
    </Suspense>
  );
}
