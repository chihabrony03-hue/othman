import { useCallback, useEffect, useState } from 'react';
import { useParams } from 'react-router-dom';
import Layout from '../components/Layout';
import Avatar from '../components/Avatar';
import { Chat, Users } from '../api';
import { useApp } from '../store';
import { timeAgo } from '../utils';

export default function ProfilePage() {
  const { username } = useParams();
  const { user: me, toast } = useApp();
  const [profile, setProfile] = useState(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [tab, setTab] = useState('followers');
  const [list, setList] = useState([]);
  const [listLoading, setListLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await Users.get(username);
      setProfile(data);
    } catch (e) {
      toast(e.message, 'error');
    } finally {
      setLoading(false);
    }
  }, [username, toast]);

  useEffect(() => { load(); }, [load]);

  useEffect(() => {
    if (!profile) return;
    const isMe = me.username === profile.username;
    if (isMe) return; // no lists for self here (Settings shows more)
    setListLoading(true);
    const fn = tab === 'followers' ? Users.followers : Users.following;
    fn(username)
      .then((d) => setList(d.users || []))
      .catch(() => setList([]))
      .finally(() => setListLoading(false));
  }, [tab, profile, username, me.username]);

  if (loading) return <Layout><div className="spinner" /></Layout>;
  if (!profile) return <Layout><div className="empty">المستخدم غير موجود</div></Layout>;

  const isMe = me.username === profile.username;
  const follow = async () => {
    setBusy(true);
    try {
      if (profile.is_following || profile.pending_follow) {
        await Users.unfollow(username);
        toast('تم إلغاء المتابعة', 'ok');
      } else {
        await Users.follow(username);
        toast(profile.is_private ? 'تم إرسال طلب المتابعة' : 'بدأت المتابعة', 'ok');
      }
      await load();
    } catch (e) {
      toast(e.message, 'error');
    } finally {
      setBusy(false);
    }
  };

  const message = async () => {
    try {
      const { conversation_id } = await Chat.createDm(profile.id);
      window.location.href = '/?conv=' + conversation_id;
    } catch (e) {
      toast(e.message, 'error');
    }
  };

  return (
    <Layout>
      <div className="profile-head">
        <div className="banner" style={profile.banner_url ? undefined : undefined} />
        <div className="profile-body">
          <Avatar user={profile} size="xl" online={profile.online} />
          <div className="profile-main">
            <h1>{profile.display_name} {profile.is_private ? '🔒' : ''}</h1>
            <p className="username">@{profile.username} • {profile.online ? '🟢 متصل الآن' : `آخر ظهور ${timeAgo(profile.last_seen)}`}</p>
            {profile.bio && <p>{profile.bio}</p>}
            {profile.location_name && <p className="muted">📍 {profile.location_name}</p>}
            <div className="stats">
              <div><b>{profile.followers_count}</b> <span>متابِع</span></div>
              <div><b>{profile.following_count}</b> <span>يتابع</span></div>
            </div>
            {profile.interests?.length > 0 && (
              <div className="reasons mt-12">
                {profile.interests.map((i) => <span key={i} className="chip">{i}</span>)}
              </div>
            )}
            <div className="row mt-16">
              {isMe ? (
                <a className="btn btn-primary" href="/settings">تعديل ملفي</a>
              ) : (
                <>
                  {!profile.blocked && (
                    <>
                      <button className="btn btn-primary" onClick={follow} disabled={busy}>
                        {profile.is_following ? 'إلغاء المتابعة' : profile.pending_follow ? 'طلب معلّق' : profile.is_followed_by ? 'متابعة بالمقابل' : 'متابعة'}
                      </button>
                      <button className="btn" onClick={message}>مراسلة</button>
                    </>
                  )}
                </>
              )}
            </div>
          </div>
        </div>
      </div>

      {!isMe && (
        <div className="mt-16">
          <div className="row" style={{ marginBottom: 12 }}>
            <button className={`btn btn-sm ${tab === 'followers' ? 'btn-primary' : ''}`} onClick={() => setTab('followers')}>المتابِعون ({profile.followers_count})</button>
            <button className={`btn btn-sm ${tab === 'following' ? 'btn-primary' : ''}`} onClick={() => setTab('following')}>يتابع ({profile.following_count})</button>
          </div>
          {listLoading && <div className="spinner" />}
          <div className="grid grid-3">
            {list.map((u) => (
              <div className="user-card" key={u.id}>
                <Avatar user={u} size="sm" />
                <div className="info">
                  <b><a href={`/u/${u.username}`}>{u.display_name}</a></b>
                  <small>@{u.username}</small>
                </div>
              </div>
            ))}
          </div>
          {!listLoading && list.length === 0 && <div className="empty">لا يوجد أفراد في هذه القائمة</div>}
        </div>
      )}
    </Layout>
  );
}
