// MEEV API client — small typed fetch wrapper with automatic token refresh.

const BASE = '/api';

let accessToken = localStorage.getItem('meev.access') || null;
let refreshToken = localStorage.getItem('meev.refresh') || null;
let refreshPromise = null;

export function setTokens(access, refresh) {
  accessToken = access;
  refreshToken = refresh;
  if (access) localStorage.setItem('meev.access', access);
  else localStorage.removeItem('meev.access');
  if (refresh) localStorage.setItem('meev.refresh', refresh);
  else localStorage.removeItem('meev.refresh');
}

export function clearTokens() {
  setTokens(null, null);
}

export function getAccessToken() {
  return accessToken;
}

async function tryRefresh() {
  if (!refreshToken) throw new Error('no refresh token');
  if (!refreshPromise) {
    refreshPromise = (async () => {
      try {
        const res = await fetch(`${BASE}/auth/refresh`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ refresh_token: refreshToken }),
        });
        if (!res.ok) throw new Error('refresh failed');
        const data = await res.json();
        setTokens(data.access_token, data.refresh_token);
        return data;
      } finally {
        refreshPromise = null;
      }
    })();
  }
  return refreshPromise;
}

export class ApiError extends Error {
  constructor(message, status, data) {
    super(message);
    this.status = status;
    this.data = data;
  }
}

export async function api(path, { method = 'GET', body, formData, auth = true, retry = true } = {}) {
  const headers = {};
  if (auth && accessToken) headers.Authorization = `Bearer ${accessToken}`;
  let payload;
  if (formData) {
    payload = formData;
  } else if (body !== undefined) {
    headers['Content-Type'] = 'application/json';
    payload = JSON.stringify(body);
  }

  const res = await fetch(`${BASE}${path}`, { method, headers, body: payload });
  if (res.status === 401 && auth && retry && refreshToken) {
    try {
      await tryRefresh();
      return api(path, { method, body, formData, auth, retry: false });
    } catch (e) {
      clearTokens();
      window.dispatchEvent(new CustomEvent('meev:auth-expired'));
      throw new ApiError('انتهت الجلسة', 401, null);
    }
  }

  const ct = res.headers.get('content-type') || '';
  const data = ct.includes('application/json') ? await res.json() : await res.arrayBuffer();
  if (!res.ok) {
    const message = data && data.error ? data.error : 'حدث خطأ غير متوقع';
    throw new ApiError(message, res.status, data);
  }
  return data;
}

export const Auth = {
  register: (payload) => api('/auth/register', { method: 'POST', body: payload, auth: false }),
  login: (payload) => api('/auth/login', { method: 'POST', body: payload, auth: false }),
  me: () => api('/auth/me'),
  logout: () => api('/auth/logout', { method: 'POST', body: { refresh_token: refreshToken } }).catch(() => {}),
};

export const Users = {
  search: (q, offset = 0) => api(`/users/search?q=${encodeURIComponent(q)}&offset=${offset}`),
  get: (username) => api(`/users/${encodeURIComponent(username)}`),
  update: (payload) => api('/users/me', { method: 'PATCH', body: payload }),
  setInterests: (interests) => api('/users/me/interests', { method: 'PUT', body: { interests } }),
  setLocation: (lat, lng, name, country) =>
    api('/users/me/location', { method: 'PATCH', body: { lat, lng, name, country } }),
  changePassword: (current_password, new_password) =>
    api('/users/me/password', { method: 'PATCH', body: { current_password, new_password } }),
  uploadAvatar: (file) => {
    const fd = new FormData();
    fd.append('file', file);
    return api('/users/me/avatar', { method: 'POST', formData: fd });
  },
  uploadBanner: (file) => {
    const fd = new FormData();
    fd.append('file', file);
    return api('/users/me/banner', { method: 'POST', formData: fd });
  },
  follow: (username) => api(`/users/${encodeURIComponent(username)}/follow`, { method: 'POST' }),
  unfollow: (username) => api(`/users/${encodeURIComponent(username)}/unfollow`, { method: 'DELETE' }),
  followers: (username) => api(`/users/me/followers?username=${encodeURIComponent(username || '')}`),
  following: (username) => api(`/users/me/following?username=${encodeURIComponent(username || '')}`),
};

export const Suggestions = {
  list: (limit = 24) => api(`/suggestions?limit=${limit}`),
  interests: (q = '') => api(`/interests?q=${encodeURIComponent(q)}`),
};

export const Chat = {
  list: () => api('/conversations'),
  createDm: (user_id) => api('/conversations', { method: 'POST', body: { user_id } }),
  createGroup: (member_ids, name) => api('/conversations/group', { method: 'POST', body: { member_ids, name } }),
  messages: (id, before, limit = 50) => {
    const q = new URLSearchParams();
    if (before) q.set('before', before);
    q.set('limit', String(limit));
    return api(`/conversations/${id}/messages?${q.toString()}`);
  },
  send: (id, content, attachment_id) =>
    api(`/conversations/${id}/messages`, { method: 'POST', body: { content, attachment_id } }),
  read: (id) => api(`/conversations/${id}/read`, { method: 'POST' }),
  typing: (id) => api(`/conversations/${id}/typing`, { method: 'POST' }).catch(() => {}),
};

export const Media = {
  upload: (file) => {
    const fd = new FormData();
    fd.append('file', file);
    return api('/media', { method: 'POST', formData: fd });
  },
  url: (id, thumb) => `/api/media/${id}/${thumb ? 'thumb' : 'file'}`,
  async fetchBlob(id, thumb = false) {
    const res = await fetch(`${BASE}/media/${id}/${thumb ? 'thumb' : 'file'}?token=${encodeURIComponent(accessToken || '')}`, {
      headers: { Authorization: `Bearer ${accessToken}` },
    });
    if (!res.ok) throw new Error('media failed');
    return res.blob();
  },
};
