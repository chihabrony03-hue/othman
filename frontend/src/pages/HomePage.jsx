import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import Layout from '../components/Layout';
import Avatar from '../components/Avatar';
import Attachment from '../components/Attachment';
import Modal from '../components/Modal';
import { Chat, Media, Users } from '../api';
import { realtime } from '../realtime';
import { useApp } from '../store';
import { conversationTitle, conversationAvatar, debounce, formatDate, timeAgo } from '../utils';

export default function HomePage() {
  const { user, toast } = useApp();
  const [convs, setConvs] = useState([]);
  const [active, setActive] = useState(null);
  const [loading, setLoading] = useState(true);
  const [showNew, setShowNew] = useState(false);
  const [searchQ, setSearchQ] = useState('');
  const [searchRes, setSearchRes] = useState([]);
  const [searchBusy, setSearchBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const data = await Chat.list();
      setConvs(data.conversations || []);
      setLoading(false);
    } catch (e) {
      toast(e.message, 'error');
      setLoading(false);
    }
  }, [toast]);

  useEffect(() => { load(); }, [load]);

  const applyMessage = useCallback((msg) => {
    setConvs((prev) => {
      const exists = prev.some((c) => c.id === msg.conversation_id);
      const next = exists
        ? prev.map((c) => (c.id === msg.conversation_id ? { ...c, last_message: msg, updated_at: msg.sent_at } : c))
        : prev;
      return next.sort((a, b) => new Date(b.updated_at || 0) - new Date(a.updated_at || 0));
    });
  }, []);

  useEffect(() => {
    const offMsg = realtime.on('message', applyMessage);
    return () => offMsg();
  }, [applyMessage]);

  // Join rooms of loaded conversations.
  useEffect(() => {
    if (convs.length) realtime.joinRooms(convs.map((c) => c.id));
  }, [convs.length]);

  const search = debounce(async (q) => {
    if (!q.trim()) { setSearchRes([]); return; }
    setSearchBusy(true);
    try {
      const data = await Users.search(q.trim());
      setSearchRes(data.users || []);
    } catch (e) {
      toast(e.message, 'error');
    } finally {
      setSearchBusy(false);
    }
  }, 300);

  const startDm = async (target) => {
    try {
      const { conversation_id } = await Chat.createDm(target.id);
      setActive(conversation_id);
      setShowNew(false);
      await load();
      realtime.joinRoom(conversation_id);
    } catch (e) {
      toast(e.message, 'error');
    }
  };

  const activeConv = useMemo(() => convs.find((c) => c.id === active) || null, [convs, active]);

  return (
    <Layout>
      <div className="messenger">
        <aside className="sidebar">
          <div className="head">
            <h2>المحادثات</h2>
            <button className="btn btn-primary btn-sm" onClick={() => setShowNew(true)}>+ جديد</button>
          </div>
          <div className="conv-list">
            {loading && <div className="spinner" />}
            {!loading && convs.length === 0 && (
              <div className="empty">
                <div className="big">💬</div>
                لا توجد محادثات بعد
                <div className="mt-8"><button className="btn" onClick={() => setShowNew(true)}>ابدأ أول محادثة</button></div>
              </div>
            )}
            {convs.map((c) => {
              const last = c.last_message;
              const title = conversationTitle(c, user.id);
              const av = conversationAvatar(c, user.id);
              return (
                <div key={c.id} className={`conv-item ${active === c.id ? 'active' : ''}`} onClick={() => { setActive(c.id); realtime.joinRoom(c.id); }}>
                  <Avatar user={av ? { avatar_url: av, display_name: title } : { display_name: title }} size="sm" />
                  <div className="meta">
                    <b>{title}</b>
                    <span>{last ? (last.sender.id === user.id ? 'أنت: ' : '') + (last.content || '📎 مرفق') : c.is_group ? `${c.members?.length || 0} أعضاء` : 'ابدأ المحادثة…'}</span>
                  </div>
                  <div className="time">{last ? timeAgo(last.sent_at) : ''}</div>
                </div>
              );
            })}
          </div>
        </aside>

        <section className="chat-pane">
          {activeConv ? (
            <ChatPane conv={activeConv} me={user} onNewMessage={applyMessage} onConversationUpdate={load} />
          ) : (
            <div className="empty" style={{ flex: 1, display: 'grid', placeItems: 'center' }}>
              <div>
                <div className="big">✉️</div>
                اختر محادثة أو ابدأ واحدة جديدة
              </div>
            </div>
          )}
        </section>
      </div>

      <Modal open={showNew} onClose={() => setShowNew(false)} title="محادثة جديدة">
        <input
          className="input"
          placeholder="ابحث عن مستخدم…"
          value={searchQ}
          onChange={(e) => { setSearchQ(e.target.value); search(e.target.value); }}
          autoFocus
        />
        <div className="mt-12" style={{ display: 'flex', flexDirection: 'column', gap: 8, maxHeight: 320, overflowY: 'auto' }}>
          {searchBusy && <div className="spinner" style={{ margin: '8px auto' }} />}
          {searchRes.map((u) => (
            <div key={u.id} className="row" style={{ padding: 8, borderRadius: 10, cursor: 'pointer', background: 'var(--bg-3)' }} onClick={() => startDm(u)}>
              <Avatar user={u} size="sm" />
              <div style={{ flex: 1, minWidth: 0 }}>
                <b>{u.display_name}</b>
                <div className="muted">@{u.username}</div>
              </div>
              <button className="btn btn-sm btn-primary">مراسلة</button>
            </div>
          ))}
          {!searchBusy && searchQ && searchRes.length === 0 && <div className="muted" style={{ textAlign: 'center' }}>لا نتائج</div>}
        </div>
      </Modal>
    </Layout>
  );
}

function ChatPane({ conv, me, onNewMessage, onConversationUpdate }) {
  const { toast } = useApp();
  const [messages, setMessages] = useState([]);
  const [loading, setLoading] = useState(true);
  const [text, setText] = useState('');
  const [sending, setSending] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [typers, setTypers] = useState(new Set());
  const listRef = useRef(null);
  const endRef = useRef(null);
  const fileRef = useRef(null);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    setMessages([]);
    Chat.messages(conv.id)
      .then((data) => { if (alive) { setMessages(data.messages || []); setLoading(false); } })
      .catch(() => alive && setLoading(false));
    Chat.read(conv.id).catch(() => {});
    realtime.joinRoom(conv.id);
    return () => { alive = false; };
  }, [conv.id]);

  useEffect(() => {
    const offMsg = realtime.on('message', (msg) => {
      if (msg.conversation_id !== conv.id) return;
      setMessages((prev) => (prev.some((m) => m.id === msg.id) ? prev : [...prev, msg]));
      onNewMessage(msg);
      Chat.read(conv.id).catch(() => {});
    });
    const offTyping = realtime.on('typing', (ev) => {
      if (ev.conversation_id !== conv.id) return;
      setTypers((prev) => {
        const next = new Set(prev);
        if (ev.user_id !== me.id) next.add(ev.user_id);
        return next;
      });
      setTimeout(() => {
        setTypers((prev) => {
          const next = new Set(prev);
          next.delete(ev.user_id);
          return next;
        });
      }, 3000);
    });
    return () => { offMsg(); offTyping(); };
  }, [conv.id, me.id, onNewMessage]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages.length, typers.size]);

  const sendTyping = useMemo(() => debounce(() => Chat.typing(conv.id), 800), [conv.id]);

  const send = async () => {
    const content = text.trim();
    if (!content || sending) return;
    setSending(true);
    setText('');
    try {
      const msg = await Chat.send(conv.id, content, null);
      setMessages((prev) => (prev.some((m) => m.id === msg.id) ? prev : [...prev, msg]));
      onNewMessage(msg);
    } catch (e) {
      setText(content);
      // keep it simple: notify
      toast(e.message, 'error');
    } finally {
      setSending(false);
    }
  };

  const pickFile = () => fileRef.current?.click();

  const onFile = async (e) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    setUploading(true);
    try {
      const att = await Media.upload(file);
      const msg = await Chat.send(conv.id, '', att.id);
      setMessages((prev) => (prev.some((m) => m.id === msg.id) ? prev : [...prev, msg]));
      onNewMessage(msg);
    } catch (err) {
      toast(err.message, 'error');
    } finally {
      setUploading(false);
    }
  };

  const loadOlder = async () => {
    if (!messages.length) return;
    const oldest = messages[0];
    const data = await Chat.messages(conv.id, oldest.sent_at, 50).catch(() => null);
    if (data?.messages?.length) {
      setMessages((prev) => [...data.messages, ...prev]);
    }
  };

  const title = conversationTitle(conv, me.id);
  const typerNames = [...typers].map((uid) => (conv.members || []).find((m) => m.id === uid)?.display_name || 'شخص');

  return (
    <>
      <div className="chat-head">
        <Avatar user={{ avatar_url: conversationAvatar(conv, me.id), display_name: title }} />
        <div className="info">
          <b>{title}</b>
          <small>{typers.length ? `${typerNames.join('، ')} ${typers.size > 1 ? 'يكتبون' : 'يكتب'} الآن…` : conv.is_group ? `${conv.members?.length || 0} عضو` : 'محادثة خاصة'}</small>
        </div>
      </div>
      <div className="messages" ref={listRef}>
        {loading && <div className="spinner" />}
        {!loading && messages.length > 0 && (
          <button className="btn btn-sm btn-ghost" style={{ alignSelf: 'center' }} onClick={loadOlder}>تحميل الأقدم</button>
        )}
        {!loading && messages.map((m, i) => {
          const prev = messages[i - 1];
          const newDay = !prev || new Date(prev.sent_at).toDateString() !== new Date(m.sent_at).toDateString();
          return (
            <div key={m.id}>
              {newDay && <div className="day-divider"><span>{formatDate(m.sent_at)}</span></div>}
              <div className={`msg-row ${m.sender.id === me.id ? 'mine' : ''}`}>
                {m.sender.id !== me.id && <Avatar user={m.sender} size="sm" />}
                <div className="bubble">
                  {conv.is_group && m.sender.id !== me.id && <div className="sender">{m.sender.display_name}</div>}
                  {m.attachment && <Attachment attachment={m.attachment} />}
                  {m.content && <div className="body">{m.content}</div>}
                  <div className="time">{new Date(m.sent_at).toLocaleTimeString('ar', { hour: '2-digit', minute: '2-digit' })}</div>
                </div>
              </div>
            </div>
          );
        })}
        {!loading && messages.length === 0 && (
          <div className="empty"><div className="big">💬</div>لا توجد رسائل بعد — ابدأ الحديث!</div>
        )}
        {messages.length > 0 && <div ref={endRef} />}
      </div>
      <div className="typing-line">{typers.length ? `${typerNames.join('، ')} يكتب الآن…` : ''}</div>
      <div className="chat-input">
        <button className="icon-btn" onClick={pickFile} title="إرفاق ملف" disabled={uploading}>
          {uploading ? '…' : '📎'}
        </button>
        <input ref={fileRef} type="file" hidden onChange={onFile} />
        <textarea
          className="input"
          rows={1}
          placeholder="اكتب رسالتك…"
          value={text}
          onChange={(e) => { setText(e.target.value); sendTyping(); }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); }
          }}
          style={{ resize: 'none' }}
        />
        <button className="btn btn-primary" onClick={send} disabled={sending || !text.trim()}>إرسال</button>
      </div>
    </>
  );
}
