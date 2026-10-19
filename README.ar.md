<img src="docs/images/new-conscirpt-hero-image.png" alt="الصورة الرئيسية لمشروع VERA20K" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <strong lang="ar" dir="rtl">العربية</strong> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
  &nbsp;&nbsp;
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="آخر تشغيل لاختبارات المكتبة على macOS (تُشغّل يدويًا)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="اختبارات المكتبة على macOS" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="آخر تشغيل لاختبارات المكتبة على Linux (تُشغّل يدويًا)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="اختبارات المكتبة على Linux" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="آخر تشغيل لاختبارات المكتبة على Windows (تُشغّل يدويًا)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="اختبارات المكتبة على Windows" height="20" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="انضم إلى Discord" height="20" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ_group-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="مجموعة QQ" height="20" align="middle">
</p>

<div dir="rtl">

# VERA20K

يعيد VERA20K، وهو مشروع يقوده المجتمع، كتابة المحرك الأصلي، `gamemd.exe`. يستخدم ملفات اللعبة
الأصلية، لذا ستحتاج إلى نسختك الخاصة من Red Alert 2: Yuri's Revenge.

VERA20K من صنع اللاعبين ومن أجلهم، وللاعبين الكلمة الأخيرة في تحديد مساره.

<img src="docs/images/vera20k-screenshots.png" alt="شاشة إعداد المناوشات ومشهد من اللعب في VERA20K" width="100%">

## أهداف المشروع

1. الحفاظ على أسلوب اللعب والمظهر والأجواء في لعبة Red Alert 2: Yuri's Revenge الأصلية.
2. دعم معارك أكبر: حتى **30 لاعبًا** و**20,000 وحدة** على خرائط بحجم 500×500.
3. إضافة ميزات معروفة من ألعاب الاستراتيجية في الوقت الحقيقي (RTS)، وأخرى لم يسبق لها مثيل.
4. عميل مدمج للعب متعدد اللاعبين

## الوضع الحالي

**قيد التطوير النشط.**

- **المناوشات:** قابلة للعب على Windows وLinux وmacOS، مع خرائط اللعبة الأصلية والخرائط العشوائية،
  وبناء القواعد وجمع الموارد والقتال وحفظ اللعبة وتحميلها.
- **الحملات:** يمكن بدء المهام؛ ما زال الدعم غير مكتمل.
- **الذكاء الاصطناعي:** تتولى أجزاء من الذكاء الاصطناعي الأصلي بناء القواعد وتنفيذ هجمات مبرمجة.
- **العمل المستمر:** تحسين دقة أسلوب اللعب والرسوم والأداء.
- **اللعب الجماعي:** اللعب عبر الشبكة غير متاح بعد.
- **حجم المعارك:** لم تُعرض بعد معارك تضم 30 لاعبًا و20,000 وحدة.

## البنية المعمارية

يتكون المحرك من ثلاثة أجزاء رئيسية:

- **المحاكاة:** تتولى إدارة حالة اللعب ومنطقه.
- **التصيير:** مسار تصيير جديد تمامًا يعتمد على معالج الرسوميات ويستخدم موارد اللعبة الأصلية،
  بُني للحفاظ على المظهر الأصلي.
- **التطبيق:** يتولى الإدخال والقوائم والصوت والتكامل مع المنصة، ويربط الأنظمة ببعضها.

يدعم هذا الفصل حتمية اللعب.

توضح الأسهم تدفق البيانات:

![يرسل التطبيق الأوامر إلى المحاكاة التي تعيد حالة العالم، ويرسل بيانات الرسم إلى التصيير.](docs/images/architecture.ar.png)

## البناء والتشغيل

تحتاج إلى:

- **اللعبة:** Red Alert 2: Yuri's Revenge.
- **Rust وGit:** أحدث إصدار مستقر من [Rust](https://rustup.rs/) مثبتًا عبر rustup، و[Git](https://git-scm.com/).
- **أدوات البناء:** على Windows أدوات بناء C++ من Visual Studio، ويعرض مثبّت Rust تثبيتها؛ وعلى macOS الأمر
  `xcode-select --install`؛ وعلى Debian وUbuntu الأمر `sudo apt install build-essential libasound2-dev pkg-config`.
- **الرسوميات:** بطاقة رسوميات تدعم Vulkan أو DirectX 12 أو Metal.

وقد استُخدم VERA20K للعب على Windows وLinux وmacOS.

<div dir="ltr">

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# عدّل config.toml واضبط ra2_dir على مجلد اللعبة قبل التشغيل:
cargo run --release --bin vera20k
```

</div>

## كيف نعمل

يكتب معظم الكود وكلاء برمجة بالذكاء الاصطناعي، يستخدمون Ghidra لدراسة المحرك الأصلي،
ثم ينقلون سلوكه إلى Rust ويتحققون منه باستخدام [أدوات المقارنة](tools/native_oracle.md) واختبارات اللعب.
يتبع الوكلاء [AGENTS.md](AGENTS.md).

## التخصيص

ينبغي أن تكون الميزات الاختيارية (مثل ميزات Ares) قابلة للضبط عبر ملفات INI،
مع مفاتيح تشغيل وإيقاف في العميل حيثما كان ذلك مناسبًا.

## المساهمة

نرحب بالمساعدة. يمكنك كتابة الكود، أو إعادة هيكلة المحرك، أو اختبار اللعبة، أو مشاركة الأفكار، أو لعبها
جنبًا إلى جنب مع الأصل وإخبارنا بما يبدو غير صحيح. افتح PR وسنتولى الأمر من هناك.
النقاش يجري على [Discord](https://discord.gg/kmjRUn5m5F) وفي مجموعة QQ (1097193563).

## الدعم

<a href="https://paypal.me/vera20k"><img src="docs/images/paypal-support.svg" alt="ادعم VERA20K عبر PayPal" height="40"></a>

تُوزَّع جميع التبرعات المستلمة على
المساهمين وفقًا [لسجل مساهمات TIBTRE](https://github.com/YuriPlanet/tibtre-ledger).

تُرسل الدفعات إلى المساهمين عبر:

<p><img src="https://img.shields.io/badge/PayPal-003087?style=flat&amp;logo=paypal&amp;logoColor=white" alt="PayPal" height="20"> <img src="https://img.shields.io/badge/China_UnionPay-E21836?style=flat" alt="China UnionPay" height="20"> <img src="https://img.shields.io/badge/Alipay-1677FF?style=flat&amp;logo=alipay&amp;logoColor=white" alt="Alipay" height="20"> <img src="https://img.shields.io/badge/Vipps-FF5B24?style=flat" alt="Vipps" height="20"> <img src="https://img.shields.io/badge/SEPA-2D4B9A?style=flat&amp;logo=sepa&amp;logoColor=white" alt="SEPA" height="20"> <img src="https://img.shields.io/badge/Bitcoin-F7931A?style=flat&amp;logo=bitcoin&amp;logoColor=white" alt="Bitcoin" height="20"></p>

## شكر ومعلومات قانونية

نشكر OpenRA وXCC Mixer وويكي ModEnc وProject Perfect Mod، وشركة EA على نشر الكود المصدري
للعبتي Command & Conquer وRed Alert تحت رخصة GPL، وكذلك World-Altering Editor وFinal Alert
وYRpp وAres وPhobos وكثيرين غيرهم.

المشروع مرخّص بموجب [GPLv3](LICENSE-GPL). لا يحتوي هذا المستودع على أي ملفات للعبة. تُعد
Command & Conquer وRed Alert علامتين تجاريتين لشركة Electronic Arts Inc.، وتعرض لقطات الشاشة
رسومات من اللعبة تملكها Electronic Arts. لا يتبع VERA20K شركة Electronic Arts ولا يحظى بتأييدها.

ترجمة لـ [README.md](README.md)؛ يُرجى تحديثها مع النسخة الإنجليزية.

</div>
