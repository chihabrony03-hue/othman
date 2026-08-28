import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import Layout from '../components/Layout';
import UserCard from '../components/UserCard';
import { Chat, Suggestions, Users } from '../api';
import { useApp } from '../store';

export default function ExplorePage() {
  const { user, toast } = useApp();
  const [items, setItems] = useState([]);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState(null);

  const load = async () => {
    setLoading(true);
    try {
      const data = await Suggestions.list(30);
      setItems(data.suggestions || []);
    } catch (e) {
      toast(e.message, 'error');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { load(); }, []); // eslint-disable-line

  const follow = async (u) => {
    setBusyId(u.user_id);
    try {
      await Users.follow(u.username);
      toast(`بدأت متابعة ${u.display_name}`, 'ok');
      setItems((prev) => prev.filter((x) => x.user_id !== u.user_id));
    } catch (e) {
      toast(e.message, 'error');
    } finally {
      setBusyId(null);
    }
  };

  const message = async (u) => {
    try {
      const { conversation_id } = await Chat.createDm(u.user_id);
      toast('تم فتح المحادثة', 'ok');
      window.location.href = '/?conv=' + conversation_id;
    } catch (e) {
      toast(e.message, 'error');
    }
  };

  return (
    <Layout>
      <div className="section-title">
        <h2>✨ اقتراحات أصدقاء ذكية</h2>
        <button className="btn btn-sm" onClick={load}>تحديث</button>
      </div>
      <p className="muted" style={{ marginTop: -8 }}>
        تُحسب الاقتراحات من اهتماماتك، موقعك الجغرافي، الأصدقاء المشتركين ونشاطك — {user?.display_name || 'أهلاً'}.
      </p>
      {loading && <div className="spinner" />}
      {!loading && items.length === 0 && (
        <div className="empty">
          <div className="big">🎉</div>
          لا توجد اقتراحات جديدة حالياً — عدّل اهتماماتك من الإعدادات لتظهر نتائج أفضل.
          <div className="mt-12"><Link className="btn" to="/settings">تخصيص حسابي</Link></div>
        </div>
      )}
      <div className="grid grid-3">
        {items.map((s) => (
          <UserCard
            key={s.user_id}
            user={{ ...s, id: s.user_id, username: s.username, display_name: s.display_name, avatar_url: s.avatar_url, bio: s.bio }}
            reasons={s.reasons}
            right={
              <div className="row" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 6 }}>
                <span className="chip chip-gold" style={{ justifyContent: 'center' }}>توافق {s.score}%</span>
                <button className="btn btn-sm btn-primary" disabled={busyId === s.user_id} onClick={() => follow(s)}>متابعة</button>
                <button className="btn btn-sm" disabled={busyId === s.user_id} onClick={() => message(s)}>مراسلة</button>
              </div>
            }
          />
        ))}
      </div>
    </Layout>
  );
}
