import { useState } from 'react';
import { Media } from '../api';
import { cx, initials } from '../utils';

/** Avatar with authenticated media loading (token cannot be put on <img src>). */
export default function Avatar({ user, size = 'md', online = false, className }) {
  const [src, setSrc] = useState(null);
  const [failed, setFailed] = useState(false);

  const avatarUrl = typeof user === 'string' ? user : user?.avatar_url;
  const name = typeof user === 'string' ? '' : user?.display_name || user?.username || '';

  if (avatarUrl && !failed) {
    const id = avatarUrl.split('/')[4];
    if (id && !src) {
      Media.fetchBlob(id, false)
        .then((blob) => setSrc(URL.createObjectURL(blob)))
        .catch(() => setFailed(true));
    }
    if (src) {
      return (
        <span className={cx('avatar-wrap', className)}>
          <img alt={name} src={src} className={cx('avatar', size !== 'md' && size)} style={{ objectFit: 'cover' }} />
          {online && <span className="online-dot" />}
        </span>
      );
    }
  }

  return (
    <span className={cx('avatar-wrap', className)}>
      <span className={cx('avatar', size !== 'md' && size)}>{initials(name)}</span>
      {online && <span className="online-dot" />}
    </span>
  );
}
