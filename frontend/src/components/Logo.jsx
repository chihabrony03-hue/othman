export default function Logo({ size = 40, text = true }) {
  return (
    <div className="auth-logo" style={{ marginBottom: 0 }}>
      <img src="/brand/meev1.png" alt="MEEV" style={{ width: size, height: size }} />
      {text && <h1 style={{ fontSize: size * 0.62, letterSpacing: 3 }}>MEEV</h1>}
    </div>
  );
}
