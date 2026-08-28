import { useEffect, useRef, useState } from 'react';
import { Suggestions } from '../api';
import { debounce } from '../utils';

export default function InterestPicker({ value, onChange, max = 20 }) {
  const [query, setQuery] = useState('');
  const [options, setOptions] = useState([]);
  const [open, setOpen] = useState(false);
  const boxRef = useRef(null);

  const load = debounce(async (q) => {
    try {
      const res = await Suggestions.interests(q);
      setOptions(res.interests || []);
    } catch (_) { /* ignore */ }
  }, 250);

  useEffect(() => { load(query); }, [query]); // eslint-disable-line
  useEffect(() => {
    const onDoc = (e) => {
      if (boxRef.current && !boxRef.current.contains(e.target)) setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    return () => document.removeEventListener('mousedown', onDoc);
  }, []);

  const add = (name) => {
    const n = name.trim().toLowerCase();
    if (!n || value.includes(n) || value.length >= max) return;
    onChange([...value, n]);
    setQuery('');
    setOpen(false);
  };

  const remove = (name) => onChange(value.filter((v) => v !== name));

  return (
    <div className="interest-picker" ref={boxRef}>
      <div className="chips" style={{ marginBottom: 8 }}>
        {value.map((v) => (
          <span key={v} className="chip chip-gold">
            {v}
            <button type="button" onClick={() => remove(v)} aria-label={`حذف ${v}`}>×</button>
          </span>
        ))}
        {value.length === 0 && <span className="muted">لم تضف اهتمامات بعد — أضفها لتخصيص اقتراحات الأصدقاء</span>}
      </div>
      <input
        className="input"
        placeholder="أضف اهتماماً…"
        value={query}
        onChange={(e) => { setQuery(e.target.value); setOpen(true); }}
        onFocus={() => setOpen(true)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') { e.preventDefault(); add(query); }
          if (e.key === 'Backspace' && !query && value.length) remove(value[value.length - 1]);
        }}
      />
      {open && options.length > 0 && (
        <div className="card mt-8" style={{ padding: 8, maxHeight: 180, overflowY: 'auto' }}>
          {options.filter((o) => !value.includes(o.name)).map((o) => (
            <div key={o.name} className="row" style={{ padding: '6px 8px', borderRadius: 8, cursor: 'pointer' }}
                 onMouseDown={() => add(o.name)}>
              <span>{o.name}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
