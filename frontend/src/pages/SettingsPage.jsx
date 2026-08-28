import { useRef, useState } from 'react';
import Layout from '../components/Layout';
import Avatar from '../components/Avatar';
import InterestPicker from '../components/InterestPicker';
import LocationPicker from '../components/LocationPicker';
import { Users } from '../api';
import { useApp } from '../store';

export default function SettingsPage() {
  const { user, setUser, toast } = useApp();
  const [displayName, setDisplayName] = useState(user?.display_name || '');
  const [bio, setBio] = useState(user?.bio || '');
  const [isPrivate, setIsPrivate] = useState(!!user?.is_private);
  const [interests, setInterests] = useState(user?.interests || []);
  const [busy, setBusy] = useState(false);
  const avatarRef = useRef(null);
  const bannerRef = useRef(null);

  const saveProfile = async () => {
    setBusy(true);
    try {
      const updated = await Users.update({ display_name: displayName, bio, is_private: isPrivate });
      setUser({ ...user, ...updated });
      toast('تم حفظ الملف الشخصي', 'ok');
    } catch (e) {
      toast(e.message, 'error');
    } finally {
      setBusy(false);
    }
  };

  const saveInterests = async () => {
    setBusy(true);
    try {
      const updated = await Users.setInterests(interests);
      setUser({ ...user, ...updated });
      toast('تم حفظ اهتماماتك — ستتحسن اقتراحات الأصدقاء', 'ok');
    } catch (e) {
      toast(e.message, 'error');
    } finally {
      setBusy(false);
    }
  };

  const upload = async (file, kind) => {
    if (!file) return;
    setBusy(true);
    try {
      const res = kind === 'avatar' ? await Users.uploadAvatar(file) : await Users.uploadBanner(file);
      const updated = await Users.update({ display_name: displayName || undefined, bio: bio || undefined });
      setUser({ ...user, ...updated, avatar_url: res.avatar_url, banner_url: res.banner_url });
      toast('تم رفع الصورة وضغطها تلقائياً (WebP)', 'ok');
    } catch (e) {
      toast(e.message, 'error');
    } finally {
      setBusy(false);
    }
  };

  const changePassword = async (e) => {
    e.preventDefault();
    const fd = new FormData(e.target);
    const current = fd.get('current');
    const next = fd.get('next');
    const confirm = fd.get('confirm');
    if (next !== confirm) { toast('كلمتا المرور غير متطابقتين', 'error'); return; }
    setBusy(true);
    try {
      await Users.changePassword(current, next);
      toast('تم تغيير كلمة المرور — سجّل الدخول مجدداً بعد إعادة التحميل', 'ok');
    } catch (err) {
      toast(err.message, 'error');
    } finally {
      setBusy(false);
    }
  };

  return (
    <Layout>
      <div className="section-title"><h2>⚙️ تخصيص حسابك</h2></div>
      <div className="settings-grid">
        <div className="card">
          <h3 style={{ marginTop: 0, color: 'var(--gold-2)' }}>الملف الشخصي</h3>
          <div className="row" style={{ marginBottom: 14 }}>
            <Avatar user={user} size="lg" />
            <div>
              <input ref={avatarRef} type="file" accept="image/*" hidden
                     onChange={(e) => upload(e.target.files?.[0], 'avatar')} />
              <button className="btn btn-sm" onClick={() => avatarRef.current?.click()} disabled={busy}>تغيير الصورة الشخصية</button>
            </div>
          </div>
          <div className="field">
            <label>الاسم الظاهر</label>
            <input className="input" value={displayName} onChange={(e) => setDisplayName(e.target.value)} maxLength={40} />
          </div>
          <div className="field mt-8">
            <label>نبذة عنك</label>
            <textarea className="input" value={bio} onChange={(e) => setBio(e.target.value)} maxLength={300} />
          </div>
          <label className="row mt-12" style={{ cursor: 'pointer' }}>
            <input type="checkbox" checked={isPrivate} onChange={(e) => setIsPrivate(e.target.checked)} />
            حساب خاص (تتطلب المتابعة موافقتك)
          </label>
          <button className="btn btn-primary mt-12" onClick={saveProfile} disabled={busy}>حفظ الملف</button>
        </div>

        <div className="card">
          <h3 style={{ marginTop: 0, color: 'var(--gold-2)' }}>🧠 اهتماماتك</h3>
          <p className="muted">اهتماماتك تُستخدم في خوارزمية اقتراح الأصدقاء (نسبة توافق تصل إلى 45% من النتيجة).</p>
          <InterestPicker value={interests} onChange={setInterests} />
          <button className="btn btn-primary mt-12" onClick={saveInterests} disabled={busy}>حفظ الاهتمامات</button>
        </div>

        <div className="card">
          <h3 style={{ marginTop: 0, color: 'var(--gold-2)' }}>📍 موقع تواجدك</h3>
          <p className="muted">الموقع يضيف نسبة توافق جغرافي تصل إلى 30% في اقتراحات الأصدقاء.</p>
          <LocationPicker onSaved={() => {}} />
        </div>

        <div className="card">
          <h3 style={{ marginTop: 0, color: 'var(--gold-2)' }}>🔐 كلمة المرور</h3>
          <form className="form" onSubmit={changePassword}>
            <input className="input" type="password" name="current" placeholder="كلمة المرور الحالية" required dir="ltr" />
            <input className="input" type="password" name="next" placeholder="كلمة المرور الجديدة (10+، كبيرة/صغيرة/رقم)" required dir="ltr" />
            <input className="input" type="password" name="confirm" placeholder="تأكيد الجديدة" required dir="ltr" />
            <button className="btn btn-primary" disabled={busy}>تغيير كلمة المرور</button>
          </form>
        </div>
      </div>
    </Layout>
  );
}
