export const cx = (...args) => args.filter(Boolean).join(' ');

export function debounce(fn, ms = 300) {
  let timer;
  return (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), ms);
  };
}

export function formatTime(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  return d.toLocaleTimeString('ar', { hour: '2-digit', minute: '2-digit' });
}

export function formatDate(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  const today = new Date();
  const sameDay = d.toDateString() === today.toDateString();
  if (sameDay) return d.toLocaleTimeString('ar', { hour: '2-digit', minute: '2-digit' });
  return d.toLocaleDateString('ar', { day: 'numeric', month: 'short' });
}

export function formatBytes(bytes) {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let i = 0;
  let v = bytes;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(v >= 10 || i === 0 ? 0 : 1)} ${units[i]}`;
}

export function initials(name = '') {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (!parts.length) return '؟';
  if (parts.length === 1) return parts[0].slice(0, 2);
  return parts[0][0] + parts[1][0];
}

export function timeAgo(iso) {
  if (!iso) return '';
  const s = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
  if (s < 60) return 'الآن';
  const m = Math.floor(s / 60);
  if (m < 60) return `منذ ${m} د`;
  const h = Math.floor(m / 60);
  if (h < 24) return `منذ ${h} س`;
  const d = Math.floor(h / 24);
  if (d < 30) return `منذ ${d} يوم`;
  return new Date(iso).toLocaleDateString('ar');
}

export function conversationTitle(conv, meId) {
  if (conv.is_group) return conv.name || 'مجموعة MEEV';
  const other = (conv.members || []).find((m) => m.id !== meId);
  return other ? other.display_name : conv.name || 'محادثة';
}

export function conversationAvatar(conv, meId) {
  if (conv.is_group) return conv.avatar_url || null;
  const other = (conv.members || []).find((m) => m.id !== meId);
  return other?.avatar_url || null;
}
