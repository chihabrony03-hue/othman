// MEEV realtime WebSocket client with auto-reconnect + room subscription.

import { getAccessToken } from './api';

export class Realtime {
  constructor() {
    this.listeners = new Map();
    this.rooms = new Set();
    this.ws = null;
    this.connected = false;
    this.tries = 0;
    this.closedByUser = false;
    this.pendingRooms = new Set();
  }

  on(type, cb) {
    if (!this.listeners.has(type)) this.listeners.set(type, new Set());
    this.listeners.get(type).add(cb);
    return () => this.off(type, cb);
  }

  off(type, cb) {
    this.listeners.get(type)?.delete(cb);
  }

  emit(type, data) {
    const set = this.listeners.get(type);
    if (set) for (const cb of set) {
      try { cb(data); } catch (e) { console.error(e); }
    }
  }

  connect() {
    const token = getAccessToken();
    if (!token) return;
    this.closedByUser = false;
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${proto}://${location.host}/ws?token=${encodeURIComponent(token)}`);
    this.ws = ws;

    ws.onopen = () => {
      this.connected = true;
      this.tries = 0;
      this.emit('connection', { online: true });
      const rooms = [...new Set([...this.rooms, ...this.pendingRooms])];
      this.pendingRooms.clear();
      if (rooms.length) this.send({ type: 'init', rooms });
    };

    ws.onmessage = (e) => {
      try {
        const msg = JSON.parse(e.data);
        if (msg.type) this.emit(msg.type, msg.data);
        if (msg.type === 'welcome') this.emit('ready', msg);
      } catch (_) { /* ignore malformed frames */ }
    };

    ws.onclose = () => {
      this.connected = false;
      this.emit('connection', { online: false });
      if (!this.closedByUser) {
        const delay = Math.min(15000, 1000 * 2 ** this.tries++);
        this.pendingRooms = new Set(this.rooms);
        setTimeout(() => this.connect(), delay);
      }
    };

    ws.onerror = () => ws.close();
  }

  send(obj) {
    if (this.ws && this.connected) {
      this.ws.send(JSON.stringify(obj));
    }
  }

  joinRoom(room) {
    this.rooms.add(room);
    this.send({ type: 'join', room });
  }

  leaveRoom(room) {
    this.rooms.delete(room);
    this.send({ type: 'leave', room });
  }

  joinRooms(rooms) {
    rooms.forEach((r) => this.rooms.add(r));
    this.send({ type: 'init', rooms: [...this.rooms] });
  }

  close() {
    this.closedByUser = true;
    this.ws?.close();
  }
}

export const realtime = new Realtime();

export function useRealtime() {
  return realtime;
}
