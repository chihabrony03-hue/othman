import { useEffect, useState } from 'react';
import { Media } from '../api';
import { formatBytes } from '../utils';

/** Authenticated attachment renderer (image/video/audio/file) with lazy blob fetch. */
export default function Attachment({ attachment, onClick }) {
  const [src, setSrc] = useState(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    let alive = true;
    setSrc(null);
    setError(false);
    if (!attachment) return;
    Media.fetchBlob(attachment.id, attachment.kind !== 'image' && !!attachment.thumb_url)
      .then((blob) => { if (alive) setSrc(URL.createObjectURL(blob)); })
      .catch(() => alive && setError(true));
    return () => { alive = false; };
  }, [attachment?.id]); // eslint-disable-line

  if (!attachment) return null;
  if (error) {
    return (
      <div className="attachment">
        <div className="file-box" onClick={onClick}>
          <span>⚠️</span>
          <div>
            <b>{attachment.original_name}</b>
            <div className="muted">{formatBytes(attachment.size)} — تعذر العرض</div>
          </div>
        </div>
      </div>
    );
  }
  if (!src) return <div className="attachment"><div className="spinner" style={{ margin: '4px auto' }} /></div>;

  if (attachment.kind === 'image') {
    return (
      <div className="attachment">
        <img src={src} alt={attachment.original_name} onClick={onClick} loading="lazy" />
      </div>
    );
  }
  if (attachment.kind === 'video') {
    return (
      <div className="attachment">
        <video src={src} controls preload="metadata" />
      </div>
    );
  }
  if (attachment.kind === 'audio') {
    return (
      <div className="attachment">
        <audio src={src} controls preload="none" />
      </div>
    );
  }
  return (
    <div className="attachment">
      <div className="file-box" onClick={onClick}>
        <span>📄</span>
        <div>
          <b>{attachment.original_name}</b>
          <div className="muted">{formatBytes(attachment.size)}</div>
        </div>
      </div>
    </div>
  );
}
