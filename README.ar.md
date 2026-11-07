<img src="docs/images/new-conscirpt-hero-image.png" alt="الصورة الرئيسية لمشروع VERA20K" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <strong lang="ar" dir="rtl">العربية</strong> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ja.md" lang="ja">日本語</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
</p>
<p align="center" dir="ltr">
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="آخر تشغيل لاختبارات المكتبة على macOS (تُشغّل يدويًا)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="اختبارات المكتبة على macOS" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="آخر تشغيل لاختبارات المكتبة على Linux (تُشغّل يدويًا)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="اختبارات المكتبة على Linux" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="آخر تشغيل لاختبارات المكتبة على Windows (تُشغّل يدويًا)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="اختبارات المكتبة على Windows" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/wasm.yml?query=branch%3Amain" title="آخر تشغيل لبناء WebAssembly (يُشغّل يدويًا)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/wasm.yml/badge.svg?branch=main" alt="بناء WebAssembly" height="23" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="انضم إلى Discord" height="23" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="مجموعة QQ" height="23" align="middle">
</p>

<div dir="rtl">

# VERA20K

يعيد VERA20K، وهو مشروع يقوده المجتمع، كتابة المحرك الأصلي، `gamemd.exe`. يستخدم ملفات اللعبة
الأصلية، لذا ستحتاج إلى نسختك الخاصة من Red Alert 2: Yuri's Revenge.

مشروع VERA20K من صنع اللاعبين ومن أجلهم، وللاعبين الكلمة الأخيرة في تحديد مساره.

**نحتاج إلى المزيد من القائمين على المشروع.** إذا كنت ترغب في المساعدة في مراجعة طلبات PR، أو
توجيه المساهمين الجدد، أو تولي مسؤولية جزء من المحرك، فأخبرنا على [Discord](https://discord.gg/kmjRUn5m5F)
أو في مجموعة QQ (1097193563).

**لا تتردد في إرسال PR.** لا يلزم أن يكون مثاليًا، وسنساعدك في الباقي.

<img src="docs/images/vera20k-screenshots.png" alt="شاشة إعداد المناوشات ومشهد من اللعب في VERA20K" width="100%">

## أهداف المشروع

1. الحفاظ على أسلوب اللعب والمظهر والأجواء في لعبة Red Alert 2: Yuri's Revenge الأصلية.
2. دعم معارك أكبر: حتى **30 لاعبًا** و**20,000 وحدة** على خرائط بحجم 500×500.
3. إضافة ميزات معروفة من ألعاب الاستراتيجية في الوقت الحقيقي (RTS)، وأخرى لم يسبق لها مثيل.
4. عميل مدمج للعب متعدد اللاعبين

اطّلع على [خارطة الطريق](https://github.com/orgs/YuriPlanet/projects/1) لمعرفة الأولويات الحالية والخطط المستقبلية.

## التنزيل واللعب

نزّل برنامج التثبيت لنظام Windows أو macOS أو Linux من
[صفحة الإصدارات](https://github.com/YuriPlanet/vera20k/releases). يعثر البرنامج على نسختك من
Yuri's Revenge ويثبّت VERA20K في مجلد جديد.

لتشغيل مناوشات YR أو حملاتها للاعب واحد عبر عميل CnCNet، راجع
[إعداد العميل دون اتصال](integrations/cncnet/README.md).

## المساهمة

نرحب بالمساعدة في أي مجال، لكن المجالات التالية هي الأحوج إليها. يرتبط كل مجال بصفحته في خارطة
الطريق، وفيها العمل الجاري والخطوة التالية:

- **[أسلوب اللعب والقتال](https://github.com/YuriPlanet/vera20k/issues/3830):** سلوك الوحدات والضرر وثبات المناوشات.
- **[الأداء](https://github.com/YuriPlanet/vera20k/issues/3831):** معدل الإطارات وسرعة الاستجابة في المعارك الكبيرة وعلى الأجهزة القديمة.
- **[اللعب الجماعي](https://github.com/YuriPlanet/vera20k/issues/3832):** الانضمام إلى المباريات عبر الإنترنت وبدؤها واستمرارها، وموثوقية اللعب التعاوني.
- **[الواجهة والصوت](https://github.com/YuriPlanet/vera20k/issues/3833):** القوائم والنصوص والترجمات والصوت.
- **[الحملات](https://github.com/YuriPlanet/vera20k/issues/3834):** يمكن بدء المهام، لكن إكمالها حتى النهاية ما زال يحتاج إلى عمل: الأهداف والمحفزات والانتقالات المبرمجة والحفظ والتحميل والانتقال إلى المهمة التالية.
- **[التوافق مع التعديلات](https://github.com/YuriPlanet/vera20k/issues/3835):** ميزات Ares وPhobos، دون تغيير الإعدادات الافتراضية للعبة الأصلية.
- **[الاختبار](https://github.com/YuriPlanet/vera20k/issues?q=is%3Aopen%20label%3Aparity):** العب VERA20K جنبًا إلى جنب مع اللعبة الأصلية وأبلغ عمّا يختلف في اللعب.

يمكن للمساهمين تولي مسؤولية أجزاء من المحرك، مع حرية واسعة لإجراء تغييرات تدفع VERA20K إلى الأمام.

سيساعد القائمون على المشروع في المراجعة والتكامل والدمج، وسيُنسب إليك الفضل كاملًا في مساهمتك.

النقاش يجري على [Discord](https://discord.gg/kmjRUn5m5F) وفي مجموعة QQ (1097193563).

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

- **خيار للاعب في VERA**، ضمن **Options**، يُحفظ في القسم `[VERA]` من `VERA.INI`.
- **خيار بناء**، مثل `--features ares` لتضمين [إضافات Ares](docs/ares.md) المدعومة.
- **مفتاح INI**، يضبطه تعديل أو خريطة.

تُحفظ إعدادات الصوت والرسوم والتحكم في `VERA.INI`. وتأتي قواعد اللعب من إعدادات الردهة أو التعديلات أو الخرائط.

## الدعم

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="ادعم VERA20K عبر PayPal" height="23"></a>

تُوزَّع جميع التبرعات المستلمة على
المساهمين وفقًا [لسجل مساهمات TIBTRE](https://github.com/YuriPlanet/tibtre-ledger).

تُرسل الدفعات إلى المساهمين عبر:

<p><img src="https://img.shields.io/badge/PayPal-003087?style=flat&amp;logo=paypal&amp;logoColor=white" alt="PayPal" height="23"> <img src="https://img.shields.io/badge/China_UnionPay-E21836?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9Ii02IC02IDI2OCAxNzIiPjxwYXRoIGQ9Ik01MC4wNDE2MzEsMC4wMDA2Njc1NjMzMDUgTDExMy44MjAyMTEsMC4wMDA2Njc1NjMzMDUgQzEyMi43MjMxMjcsMC4wMDA2Njc1NjMzMDUgMTI4LjI2MDQzOCw3LjI1NzMxMzQ1IDEyNi4xODM1NTgsMTYuMTg5NzY1NyBMOTYuNDkwMDcxNSwxNDMuNjc1NDE2IEM5NC4zOTQ1MzY1LDE1Mi41NzY3NzcgODUuNDgwNzM5LDE1OS44Mzk2NDEgNzYuNTcxNjA1LDE1OS44Mzk2NDEgTDEyLjc5OTI0MzMsMTU5LjgzOTY0MSBDMy45MDg3NjQsMTU5LjgzOTY0MSAtMS42NDA5ODQ0NCwxNTIuNTc2Nzc3IDAuNDM1ODk2MjA4LDE0My42NzU0MTYgTDMwLjE0MTgxODYsMTYuMTg5NzY1NyBDMzIuMjE4Njk5Myw3LjI1NzMxMzQ1IDQxLjEyNzgzMzMsMC4wMDA2Njc1NjMzMDUgNTAuMDQxNjMxLDAuMDAwNjY3NTYzMzA1IiBmaWxsPSIjRTIxODM2IiBzdHJva2U9IiNmZmYiIHN0cm9rZS13aWR0aD0iOCIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCIvPjxwYXRoIGQ9Ik0xMDguNTEzMjgzLDAuMDAwNjY3NTYzMzA1IEwxODEuODU3MDE4LDAuMDAwNjY3NTYzMzA1IEMxOTAuNzU4Mzc4LDAuMDAwNjY3NTYzMzA1IDE4Ni43NDQ1MjcsNy4yNTczMTM0NSAxODQuNjUwNTQ2LDE2LjE4OTc2NTcgTDE1NC45NjE3MjQsMTQzLjY3NTQxNiBDMTUyLjg3ODYyNSwxNTIuNTc2Nzc3IDE1My41Mjg0MjcsMTU5LjgzOTY0MSAxNDQuNjA4NDExLDE1OS44Mzk2NDEgTDcxLjI2NDY3NzUsMTU5LjgzOTY0MSBDNjIuMzQ0NjYxNSwxNTkuODM5NjQxIDU2LjgyNDQ0OTUsMTUyLjU3Njc3NyA1OC45MTk5ODUsMTQzLjY3NTQxNiBMODguNjA3MjUyNSwxNi4xODk3NjU3IEM5MC43MTUyMjQsNy4yNTczMTM0NSA5OS42MDU3MDM1LDAuMDAwNjY3NTYzMzA1IDEwOC41MTMyODMsMC4wMDA2Njc1NjMzMDUiIGZpbGw9IiMwMDQ0N0MiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSI4IiBzdHJva2UtbGluZWpvaW49InJvdW5kIi8%2BPHBhdGggZD0iTTE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSBMMjQyLjcyNjQwOSwwLjAwMDY2NzU2MzMwNSBDMjUxLjY0MTc2MiwwLjAwMDY2NzU2MzMwNSAyNTcuMTc5MDc0LDcuMjU3MzEzNDUgMjU1LjA4NTA5MywxNi4xODk3NjU3IEwyMjUuMzk2MjcxLDE0My42NzU0MTYgQzIyMy4zMDA3MzYsMTUyLjU3Njc3NyAyMTQuMzgwNzE5LDE1OS44Mzk2NDEgMjA1LjQ2NjkyMiwxNTkuODM5NjQxIEwxNDEuNzE3ODc5LDE1OS44Mzk2NDEgQzEzMi43OTc4NjMsMTU5LjgzOTY0MSAxMjcuMjY1MjE1LDE1Mi41NzY3NzcgMTI5LjM1NDUzMiwxNDMuNjc1NDE2IEwxNTkuMDQ4MDE3LDE2LjE4OTc2NTcgQzE2MS4xMjQ4OTksNy4yNTczMTM0NSAxNzAuMDI3ODE0LDAuMDAwNjY3NTYzMzA1IDE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSIgZmlsbD0iIzAwN0I4NCIgc3Ryb2tlPSIjZmZmIiBzdHJva2Utd2lkdGg9IjgiIHN0cm9rZS1saW5lam9pbj0icm91bmQiLz48L3N2Zz4%3D" alt="China UnionPay" height="23"> <img src="https://img.shields.io/badge/Alipay-1677FF?style=flat&amp;logo=alipay&amp;logoColor=white" alt="Alipay" height="23"> <img src="https://img.shields.io/badge/Vipps-FF5B24?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjQzLjUgMjEuNSAyNyAyNyI%2BPHBhdGggZD0iTTU3LjMsNDAuNmMzLjcsMCw1LjgtMS44LDcuOC00LjRjMS4xLTEuNCwyLjUtMS43LDMuNS0wLjlzMS4xLDIuMywwLDMuN2MtMi45LDMuOC02LjYsNi4xLTExLjMsNi4xIGMtNS4xLDAtOS42LTIuOC0xMi43LTcuN2MtMC45LTEuMy0wLjctMi43LDAuMy0zLjRzMi41LTAuNCwzLjQsMUM1MC41LDM4LjMsNTMuNSw0MC42LDU3LjMsNDAuNnogTTY0LjIsMjguM2MwLDEuOC0xLjQsMy0zLDMgcy0zLTEuMi0zLTNzMS40LTMsMy0zQzYyLjgsMjUuMyw2NC4yLDI2LjYsNjQuMiwyOC4zeiIgZmlsbD0iI2ZmZiIvPjwvc3ZnPg%3D%3D" alt="Vipps" height="23"> <img src="https://img.shields.io/badge/SEPA-2D4B9A?style=flat&amp;logo=sepa&amp;logoColor=white" alt="SEPA" height="23"> <img src="https://img.shields.io/badge/Bitcoin-F7931A?style=flat&amp;logo=bitcoin&amp;logoColor=white" alt="Bitcoin" height="23"></p>

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
