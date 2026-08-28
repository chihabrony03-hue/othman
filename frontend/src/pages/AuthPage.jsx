import { useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { useApp } from '../store';

export default function AuthPage({ mode }) {
  const { login, register } = useApp();
  const navigate = useNavigate();
  const isLogin = mode === 'login';
  const [form, setForm] = useState({ username: '', email: '', display_name: '', password: '', confirm: '' });
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  const set = (k) => (e) => setForm((f) => ({ ...f, [k]: e.target.value }));

  const submit = async (e) => {
    e.preventDefault();
    setError('');
    if (!isLogin && form.password !== form.confirm) {
      setError('كلمتا المرور غير متطابقتين');
      return;
    }
    if (!isLogin && !/[A-Za-z]/.test(form.password) && form.password.length) {
      setError('كلمة المرور يجب أن تحتوي على حرف كبير وحرف صغير ورقم');
      return;
    }
    setBusy(true);
    try {
      if (isLogin) {
        await login({ identifier: form.username.trim(), password: form.password });
      } else {
        await register({
          username: form.username.trim(),
          email: form.email.trim(),
          password: form.password,
          display_name: form.display_name.trim() || undefined,
        });
      }
      navigate('/');
    } catch (err) {
      setError(err.message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="auth-wrap">
      <div className="auth-card">
        <div className="auth-logo">
          <img src="/brand/meev1.png" alt="MEEV" />
          <h1>MEEV</h1>
          <p>تواصل بذكاء — مراسلة آمنة وأصدقاء جدد</p>
        </div>
        <form className="form" onSubmit={submit}>
          <div className="field">
            <label>{isLogin ? 'اسم المستخدم أو البريد الإلكتروني' : 'اسم المستخدم'}</label>
            <input className="input" value={form.username} onChange={set('username')} required
                   autoComplete={isLogin ? 'username' : 'off'} minLength={3} maxLength={24} dir="ltr" />
          </div>
          {!isLogin && (
            <>
              <div className="field">
                <label>البريد الإلكتروني</label>
                <input className="input" type="email" value={form.email} onChange={set('email')} required dir="ltr" />
              </div>
              <div className="field">
                <label>الاسم الظاهر (اختياري)</label>
                <input className="input" value={form.display_name} onChange={set('display_name')} maxLength={40} />
              </div>
            </>
          )}
          <div className="field">
            <label>كلمة المرور</label>
            <input className="input" type="password" value={form.password} onChange={set('password')} required
                   autoComplete={isLogin ? 'current-password' : 'new-password'} dir="ltr" />
            {!isLogin && <small className="muted">10 أحرف على الأقل، بحرف كبير وحرف صغير ورقم.</small>}
          </div>
          {!isLogin && (
            <div className="field">
              <label>تأكيد كلمة المرور</label>
              <input className="input" type="password" value={form.confirm} onChange={set('confirm')} required dir="ltr" />
            </div>
          )}
          {error && <div className="alert alert-error">{error}</div>}
          <button className="btn btn-primary btn-block" disabled={busy}>
            {busy ? '…' : isLogin ? 'تسجيل الدخول' : 'إنشاء الحساب'}
          </button>
        </form>
        <div className="link-row">
          {isLogin ? (
            <>ليس لديك حساب؟ <Link to="/register">أنشئ حساباً الآن</Link></>
          ) : (
            <>لديك حساب بالفعل؟ <Link to="/login">سجل الدخول</Link></>
          )}
        </div>
      </div>
    </div>
  );
}
