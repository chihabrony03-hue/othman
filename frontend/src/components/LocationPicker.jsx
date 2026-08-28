import { useState } from 'react';
import { useApp } from '../store';
import { Users } from '../api';

/** Lets the user set their location (browser geolocation or manual entry). */
export default function LocationPicker({ onSaved }) {
  const { toast } = useApp();
  const [busy, setBusy] = useState(false);
  const [place, setPlace] = useState('');
  const [country, setCountry] = useState('');
  const [lat, setLat] = useState('');
  const [lng, setLng] = useState('');

  const save = async (la, ln, name, ctry) => {
    setBusy(true);
    try {
      await Users.setLocation(Number(la), Number(ln), name || undefined, ctry || undefined);
      toast('تم حفظ موقعك بنجاح', 'ok');
      onSaved?.();
    } catch (e) {
      toast(e.message, 'error');
    } finally {
      setBusy(false);
    }
  };

  const detect = () => {
    if (!navigator.geolocation) {
      toast('المتصفح لا يدعم تحديد الموقع', 'error');
      return;
    }
    setBusy(true);
    navigator.geolocation.getCurrentPosition(
      async (pos) => {
        const la = pos.coords.latitude.toFixed(5);
        const ln = pos.coords.longitude.toFixed(5);
        const name = place || `${la}, ${ln}`;
        await save(la, ln, name, country);
        setLat(String(la));
        setLng(String(ln));
        setBusy(false);
      },
      () => {
        setBusy(false);
        toast('تعذر تحديد الموقع — أدخل الإحداثيات يدوياً', 'error');
      },
      { enableHighAccuracy: true, timeout: 10000 },
    );
  };

  return (
    <div className="grid" style={{ gap: 10 }}>
      <button type="button" className="btn btn-primary" onClick={detect} disabled={busy}>
        {busy ? 'جارٍ التحديد…' : '📍 تحديد موقعي الحالي'}
      </button>
      <input className="input" placeholder="اسم المكان (اختياري)" value={place} onChange={(e) => setPlace(e.target.value)} />
      <input className="input" placeholder="الدولة (اختياري)" value={country} onChange={(e) => setCountry(e.target.value)} />
      <div className="row">
        <input className="input" placeholder="خط العرض" value={lat} onChange={(e) => setLat(e.target.value)} />
        <input className="input" placeholder="خط الطول" value={lng} onChange={(e) => setLng(e.target.value)} />
      </div>
      <button
        type="button"
        className="btn"
        disabled={busy || !lat || !lng}
        onClick={() => save(lat, lng, place, country)}
      >
        حفظ الإحداثيات يدوياً
      </button>
    </div>
  );
}
