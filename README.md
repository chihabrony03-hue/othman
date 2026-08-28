# MEEV — تواصل بذكاء 💬

منصة مراسلة نصية آمنة مع نظام أصدقاء ذكي، مبنية بـ **Rust 100%** في الباك اند و **React + Vite** في الواجهة.

> الشعارات (meev.png / meev1.png) مأخوذة من مستودع GitHub العام `chihabrony03-hue/othman`.

---

## ✨ الميزات

| الميزة | التفاصيل |
|---|---|
| مراسلة فورية | REST API + **WebSocket** للمرسل، وإرسال الرسائل عبر **بروتوكول MQTT** (QoS 1) كحافلة رسائل |
| حساب المستخدمين | تسجيل دخول + إنشاء حساب، كلمات مرور **Argon2id**، نوافذ وصول **JWT HS512** + رموز تحديث دوّارة تُخزَّن مشفَّرة (SHA-256) في قاعدة البيانات |
| قاعدة البيانات | **PostgreSQL** مع معاملات، فهارس، قيود CHECK — وكل الاستعلامات **مُعامَلة بالمُعامِلات** (`$1,$2...`) |
| منع حقن SQL | كل مدخلات المستخدم تُتحقق وتُطبَّع قبل قاعدة البيانات، ولا يُبنى SQL ولا حتى LIKE إلا بعوامل مهرَّبة |
| متابعات | نظام مدى توكّل متبوعين/يتابع، حسابات خاصة (طلبات موافقة)، خيارات متابعة/إلغاء |
| ملفات الشخصية | صورة شخصية/غلاف، نبذة، **اهتمامات**، **موقع تواجد** — كلها تُستخدم في التخصيص |
| اقتراح الأصدقاء | خوارزمية متعددة العوامل: تشابه الاهتمامات (45%) + القرب الجغرافي Haversine (30%) + الأصدقاء المشتركون (20%) + النشاط (5%) |
| البحث | بحث فوري بالاسم/اسم المستخدم عبر REST |
| رفع الوسائط | **ffmpeg** يضغط الصور إلى **WebP** والڤيديو إلى MP4 (H.264) والصور المصغرة WebP |
| الحماية من DoS | **Rate limit 120 طلب/دقيقة** لكل IP/مستخدم مع `Retry-After` |
| التجميع | بناء Vite يقسّم الواجهة إلى **عشرات الملفات الصغيرة** للتحميل السريع |

---

## 🏗️ البنية

```
backend/          Rust (Axum + sqlx + rumqttc + argon2 + jsonwebtoken)
  src/
    config.rs     قراءة .env والتحقق من كل قيمها
    auth.rs       Argon2id + JWT (access) + refresh tokens قابلة للإلغاء
    rate_limit.rs نافذة منزلقة 120 req/min
    mqtt.rs       جسر MQTT: نشر/اشتراك + إزالة تكرار
    hub.rs        موزّع أحداث WebSocket
    media.rs      ضغط ffmpeg (WebP / MP4 / m4a) + أسماء ملفات آمنة
    suggest.rs    خوارزمية اقتراح الأصدقاء
    routes/       REST: auth, users, chat, media, suggestions, ws
  migrations/     مخطط PostgreSQL
frontend/         React 18 + Vite 5 (RTL عربي)
  src/pages/      Auth, Home (المحادثات), Explore, Search, Profile, Settings
  src/components/ مكونات واجهة (Avatar, Modal, Attachment...)
.github/workflows/build.yml   بناء + اختبار حي (smoke) + أرشيف زيب
scripts/          gen-env.sh, start.sh, smoke.sh
```

**تدفق الرسالة:** الواجهة → `POST /api/conversations/:id/messages` → حفظ في PostgreSQL → نشر على `meev/{conv}/messages` عبر MQTT → جسر MQTT يوزّع على عملاء WebSocket (مع جدول إزالة تكرار).

---

## 🚀 التشغيل السريع (حزمة الإصدار)

1. جهّز PostgreSQL وMosquitto وffmpeg على جهازك.
2. فك ضغط `MEEV-release-linux-x86_64.zip` ثم:
   ```bash
   cd meev-linux-x86_64
   ./start.sh            # ينشئ .env بأسرار عشوائية + قاعدة البيانات (يتطلب psql وsudo)
   # أو يدوياً:
   ./gen-env.sh          # ثم عدّل مسار ffmpeg في .env إذا لزم
   ./meev-backend
   ```
3. افتح `http://localhost:8080`.

> في `.env`: غيّر `FFMPEG_PATH` و`FFPROBE_PATH` إلى المسار الكامل لأداة ffmpeg على جهازك
> (هذا كان مطلباً صريحاً)، و`MQTT_URL` إلى عنوان بروكر Mosquitto الخاص بك.

## 🛠️ البناء من السورس

```bash
# الواجهة
cd frontend && npm install && npm run build        # الناتج في frontend/dist
cp -r frontend/dist backend/static

# الباك اند (يتطلب Rust stable + PostgreSQL للتشغيل)
cd backend && cargo build --release
cp ../.env .env && ./target/release/meev-backend
```

## 🔐 ملف البيئة `.env`

انظر `backend/.env.example` — جميع الإعدادات: الشبكة، قاعدة البيانات، JWT، معدل الطلبات،
MQTT، مسار ffmpeg، حدود الرفع، ومسار مجلد الواجهة الثابتة. **لا ترفع ملف `.env` أبداً إلى Git.**

## ✅ اختبار شامل

```bash
bash scripts/smoke.sh   # بعد بدء PostgreSQL/Mosquitto وبناء الباك اند
```
الاختبار يغطي: التسجيل/الدخول، تخصيص الملف، المتابعة، الاقتراحات، البحث، المحادثة،
الرفع والضغط WebP، حد المعدل 120، ومحاولات حقن SQL.

## 📦 الإصدارات

- `MEEV-release-linux-x86_64.zip` — نسخة مبنية جاهزة (الـbinary + الواجهة الثابتة + سكربتات التشغيل).
- `MEEV-source.zip` — السورس الكامل.

(بُني الباك اند بواسطة GitHub Actions — انظر تبويب Actions لتنزيل الأرشيف الرسمي.)
