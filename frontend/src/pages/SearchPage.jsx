import { useEffect, useState } from 'react';
import Layout from '../components/Layout';
import UserCard from '../components/UserCard';
import { Users } from '../api';
import { useApp } from '../store';
import { debounce } from '../utils';

export default function SearchPage() {
  const { toast } = useApp();
  const [q, setQ] = useState('');
  const [results, setResults] = useState([]);
  const [total, setTotal] = useState(0);
  const [busy, setBusy] = useState(false);
  const [searched, setSearched] = useState(false);

  const run = debounce(async (query) => {
    if (!query.trim()) { setResults([]); setSearched(false); return; }
    setBusy(true);
    try {
      const data = await Users.search(query.trim());
      setResults(data.users || []);
      setTotal(data.total || 0);
      setSearched(true);
    } catch (e) {
      toast(e.message, 'error');
    } finally {
      setBusy(false);
    }
  }, 300);

  useEffect(() => { run(q); }, [q]); // eslint-disable-line

  return (
    <Layout>
      <div className="section-title"><h2>🔎 البحث عن أصدقاء</h2></div>
      <input
        className="input"
        style={{ maxWidth: 620 }}
        placeholder="ابحث بالاسم أو اسم المستخدم…"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        autoFocus
        dir="ltr"
      />
      {busy && <div className="spinner" />}
      {!busy && searched && results.length === 0 && (
        <div className="empty"><div className="big">🔍</div>لم نجد أحداً بهذا الاسم — جرّب كلمة أخرى</div>
      )}
      <div className="grid grid-3 mt-16">
        {results.map((u) => (
          <UserCard key={u.id} user={u} />
        ))}
      </div>
      {searched && results.length > 0 && <div className="muted mt-12">تم العثور على {total} نتيجة</div>}
    </Layout>
  );
}
