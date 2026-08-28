import { Link } from 'react-router-dom';
import Avatar from './Avatar';
import { cx } from '../utils';

export default function UserCard({ user, online, reasons, right, className }) {
  return (
    <div className={cx('user-card', className)}>
      <Link to={`/u/${user.username}`}>
        <Avatar user={user} online={online} />
      </Link>
      <div className="info">
        <b>
          <Link to={`/u/${user.username}`}>{user.display_name || user.username}</Link>
        </b>
        <small>@{user.username}{user.location_name ? ` • ${user.location_name}` : ''}</small>
        {reasons && reasons.length > 0 && (
          <div className="reasons">
            {reasons.slice(0, 2).map((r) => (
              <span key={r} className="chip chip-gold">{r}</span>
            ))}
          </div>
        )}
        {!reasons && user.bio && <small>{user.bio}</small>}
      </div>
      {right && <div>{right}</div>}
    </div>
  );
}
