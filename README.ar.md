<img src="docs/images/new-conscirpt-hero-image.png" alt="الصورة الرئيسية لمشروع VERA20K" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <strong lang="ar" dir="rtl">العربية</strong> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ja.md" lang="ja">日本語</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
</p>
<p align="center" dir="ltr">
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="انضم إلى Discord" height="23" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ_group-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="مجموعة QQ" height="23" align="middle">
</p>

<div dir="rtl">

# VERA20K

يعيد VERA20K، وهو مشروع يقوده المجتمع، كتابة المحرك الأصلي، `gamemd.exe`. يستخدم ملفات اللعبة
الأصلية، لذا ستحتاج إلى نسختك الخاصة من Red Alert 2: Yuri's Revenge.

مشروع VERA20K من صنع اللاعبين ومن أجلهم، وللاعبين الكلمة الأخيرة في تحديد مساره.

<img src="docs/images/vera20k-screenshots.png" alt="شاشة إعداد المناوشات ومشهد من اللعب في VERA20K" width="100%">

## أهداف المشروع

1. الحفاظ على أسلوب اللعب والمظهر والأجواء في لعبة Red Alert 2: Yuri's Revenge الأصلية.
2. دعم معارك أكبر: حتى **30 لاعبًا** و**20,000 وحدة** على خرائط بحجم 500×500.
3. إضافة ميزات معروفة من ألعاب الاستراتيجية في الوقت الحقيقي (RTS)، وأخرى لم يسبق لها مثيل.
4. عميل مدمج للعب متعدد اللاعبين

اطّلع على [خارطة الطريق](https://github.com/orgs/YuriPlanet/projects/1) لمعرفة الأولويات الحالية والخطط المستقبلية.

## الوضع الحالي

**قيد التطوير النشط.**

- **المناوشات:** قابلة للعب على Windows وLinux وmacOS، مع خرائط اللعبة الأصلية والخرائط العشوائية،
  وبناء القواعد وجمع الموارد والقتال وحفظ اللعبة وتحميلها.
- **الحملات:** يمكن بدء المهام؛ ما زال الدعم غير مكتمل.
- **الذكاء الاصطناعي:** تتولى أجزاء من الذكاء الاصطناعي الأصلي بناء القواعد وتنفيذ هجمات مبرمجة.
- **اللعب الجماعي:** يتوفر اللعب عبر الشبكة المحلية والإنترنت على Windows وLinux وmacOS.

## التنزيل واللعب

نزّل برنامج التثبيت لنظام Windows أو macOS أو Linux من
[صفحة الإصدارات](https://github.com/YuriPlanet/vera20k/releases). يعثر البرنامج على نسختك من
Yuri's Revenge ويثبّت VERA20K في مجلد جديد.

لتشغيل مناوشات YR أو حملاتها للاعب واحد عبر عميل CnCNet، راجع
[إعداد العميل دون اتصال](integrations/cncnet/README.md).

## المساهمة

نرحب بالمساعدة. يمكنك كتابة الكود، أو إعادة هيكلة المحرك، أو اختبار اللعبة، أو مشاركة الأفكار، أو لعبها
جنبًا إلى جنب مع الأصل وإخبارنا بما يبدو غير صحيح. افتح PR وسنتولى الأمر من هناك.

يمكن للمساهمين تولي مسؤولية أجزاء من المحرك، مع حرية واسعة لإجراء تغييرات تدفع VERA20K إلى الأمام.

سيساعد القائمون على المشروع في المراجعة والتكامل والدمج، وسيُنسب إليك الفضل كاملًا في مساهمتك.

النقاش يجري على [Discord](https://discord.gg/kmjRUn5m5F) وفي مجموعة QQ (1097193563).

## البناء والتشغيل

تحتاج إلى:

- **لغة Rust وأداة Git:** [Rust](https://rustup.rs/) 1.97 أو أحدث، مثبتًا عبر rustup، و[Git](https://git-scm.com/).
- **أدوات البناء:** على Windows أدوات بناء C++ من Visual Studio، ويعرض مثبّت Rust تثبيتها؛ وعلى macOS الأمر
  `xcode-select --install`؛ وعلى Debian وUbuntu الأمر `sudo apt install build-essential libasound2-dev libxkbcommon-x11-0 pkg-config`.

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

تُختار سعة الخريطة تلقائيًا دون خيار بناء: يمكن أن يصل مجموع العرض والارتفاع في `Size` إلى 1024.

يعمل VERA20K افتراضيًا مثل اللعبة الأصلية. يجب أن تبقى كل ميزة جديدة أخرى، بما في ذلك الإضافات القادمة
من تعديلات مثل Ares، معطّلة إلى أن تُفعَّل بإحدى الطرق التالية:

- **خيار للاعب في VERA**، ضمن **Options → Vera Options**، يُحفظ في القسم `[VERA]` من `VERA.INI`.
- **خيار بناء**، مثل `--features ares` لتضمين [إضافات Ares](docs/ares.md) المدعومة.
- **مفتاح INI**، يضبطه تعديل أو خريطة.

تُحفظ إعدادات الصوت والرسوم والتحكم في `VERA.INI`. وتأتي قواعد اللعب من إعدادات الردهة أو التعديلات أو الخرائط.

## الدعم

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="ادعم VERA20K عبر PayPal" height="23"></a>

تُوزَّع جميع التبرعات المستلمة على
المساهمين وفقًا [لسجل مساهمات TIBTRE](https://github.com/YuriPlanet/tibtre-ledger).

تُرسل الدفعات إلى المساهمين عبر PayPal وChina UnionPay وAlipay وVipps وSEPA وBitcoin.

## شكر ومعلومات قانونية

نشكر OpenRA وXCC Mixer وويكي ModEnc وProject Perfect Mod، وشركة EA على نشر الكود المصدري
للعبتي Command & Conquer وRed Alert تحت رخصة GPL، وكذلك World-Altering Editor وFinal Alert
وYRpp وAres وPhobos وكثيرين غيرهم.

حقوق النشر © 2026 Halvor Nikolai Rusten والمساهمون في VERA20K.
المشروع مرخّص بموجب [GPLv3](LICENSE-GPL) مع [شروط إضافية](LEGAL.md#additional-terms).
لا يحتوي هذا المستودع على أي ملفات للعبة. تُعد
Command & Conquer وRed Alert علامتين تجاريتين لشركة Electronic Arts Inc.، وتعرض لقطات الشاشة
وأيقونة التطبيق، أي النجمة الحمراء من Red Alert 2، رسومات من اللعبة تملكها Electronic Arts.
لا يتبع VERA20K شركة Electronic Arts ولا يحظى بتأييدها.

**اقرأ [الإشعار القانوني](LEGAL.md) قبل أن تشارك VERA20K أو تبيعه أو تنشئ نسخة متفرعة منه.**

## التوثيق

- [معاينة WebAssembly](docs/wasm.md): بنية اختيارية للعب الفردي تعمل في المتصفح.
- [ملفات تعريف INI](docs/ini-roots.md): اختيار ملفات القواعد والرسوم والذكاء الاصطناعي.
- [Ares](docs/ares.md) و[Phobos](docs/phobos.md) و[NPatch](docs/npatch.md): دعم الإضافات وحدوده.
- [نظرة عامة على المحرك](docs/index.md): تنظيم الشيفرة وكيف تترابط أجزاء المحرك.
- [أدوات المطورين](tools/README.md): البناء والاختبارات وفحص موارد اللعبة.
- [اختبار الخرائط](tools/map_observation.md): سيناريوهات مبرمجة ولقطات شاشة وملاحظات عن المباريات.
- [المقارنة مع الأصل](tools/native_oracle.md): مقارنة السلوك مع اللعبة الأصلية.
- [سير العمل في Ghidra](docs/ghidra-workflow.md): فحص المحرك الأصلي.

ترجمة لـ [README.md](README.md)؛ يُرجى تحديثها مع النسخة الإنجليزية.

</div>
