import { Link } from 'react-router-dom';

export default function NotFoundPage() {
  return (
    <div className="auth-wrap">
      <div className="auth-card" style={{ textAlign: 'center' }}>
        <div style={{ fontSize: 56 }}>🛸</div>
        <h1>404</h1>
        <p className="muted">لم نجد هذه الصفحة في نظام MEEV.</p>
        <Link className="btn btn-primary" to="/">العودة للرئيسية</Link>
      </div>
    </div>
  );
}
